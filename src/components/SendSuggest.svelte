<script lang="ts">
  import { tick } from 'svelte';
  import { cachedSettings } from '$lib/stores';
  import { commandIndex } from '$lib/commandIndex';
  import {
    buildManualEntries,
    displayName,
    docTitle,
    exampleLines,
    matchSuggestions,
    shortTitle,
    splitSyntax,
    type Suggestion,
  } from '$lib/suggest';
  import type { ManualCommand } from '$lib/types';

  let {
    query,
    hexMode,
    enabled,
    onAccept,
  }: {
    /** 输入框当前文本 */
    query: string;
    /** HEX发送开着 → 不联想 */
    hexMode: boolean;
    /** 设置里的联想总开关 */
    enabled: boolean;
    /** 用户接受一条候选(键盘/点击/示例块):把 text 填进输入框 */
    onAccept: (text: string) => void;
  } = $props();

  // 键盘高亮项;-1 = 无高亮(此时回车照旧发送)。只由键盘改,鼠标悬停不改。
  let highlight = $state(-1);
  // 详情卡当前看的来源:0 = 主记录,n = alsoIn[n-1]
  let sourceIndex = $state(0);
  // 空输入按 ↑ 弹出的纯历史列表
  let forcedHistory = $state(false);
  // 收起态记的是"针对哪段文本收起":Esc/发送/失焦时记当前文本,接受候选时记将要填入的文本;
  // 文本一变化就自然解除。不用 $effect 监听 query 复位,否则接受候选后会被自己触发重开。
  let dismissedFor = $state<string | null>(null);
  let listEl: HTMLDivElement | undefined;

  const disabledDocIds = $derived(cachedSettings.value?.command_index?.disabled_doc_ids ?? []);
  const manualEntries = $derived(buildManualEntries(commandIndex.documents, commandIndex.commands, disabledDocIds));

  const items = $derived.by((): Suggestion[] => {
    if (!enabled || hexMode) return [];
    const q = query.trim();
    if (forcedHistory && q.length === 0) {
      return commandIndex.history.slice(0, 50).map((text) => ({ kind: 'history' as const, text }));
    }
    return matchSuggestions(q, manualEntries, commandIndex.history);
  });

  // 列表自下而上展示:rank 0(最佳匹配/最近发送的历史)贴着输入框渲染在最下,↑ 从输入框
  // 进入后向上走。内部逻辑(高亮/预览/Tab 兜底)全用 rank 下标,只在渲染这一处反转。
  const rows = $derived(items.map((item, i) => ({ item, i })).reverse());

  const open = $derived(items.length > 0 && query !== dismissedFor);
  const hasManual = $derived(items.some((i) => i.kind === 'manual'));
  // 详情卡显示项:有高亮看高亮,否则预览第 0 项(列表不画高亮条)
  const previewIndex = $derived(highlight >= 0 && highlight < items.length ? highlight : 0);
  const preview = $derived(open ? items[previewIndex] : undefined);
  const record = $derived.by((): ManualCommand | null => {
    if (!preview || preview.kind !== 'manual') return null;
    const e = preview.entry;
    return sourceIndex === 0 ? e.primary : (e.alsoIn[sourceIndex - 1] ?? e.primary);
  });

  /** 父组件在输入框 oninput 时调:文本变了,高亮与来源复位。 */
  export function onInput() {
    highlight = -1;
    sourceIndex = 0;
    forcedHistory = false;
  }

  /** 收起,直到文本再变化。父组件在发送成功、输入框失焦时调。 */
  export function dismiss() {
    dismissedFor = query;
    forcedHistory = false;
    highlight = -1;
  }

  function accept(text: string) {
    dismissedFor = text;
    forcedHistory = false;
    highlight = -1;
    sourceIndex = 0;
    onAccept(text);
  }

  function acceptItem(item: Suggestion | undefined) {
    if (!item) return;
    // 手册项填 key(规范大写):primary.command 是原始串,可能带小写/尾随空格
    accept(item.kind === 'history' ? item.text : item.entry.key);
  }

  async function scrollHighlightIntoView() {
    await tick();
    listEl?.querySelector<HTMLElement>(`[data-idx="${highlight}"]`)?.scrollIntoView({ block: 'nearest' });
  }

  // 列表反转后 rank 0(最佳候选)在 DOM 末尾:候选多到溢出时,滚动默认停在 DOM 顶部=最旧的
  // 候选,贴输入框的最佳候选反而看不见。弹层刚开、还没高亮时把滚动钉到底部;之后有高亮
  // 就交给 scrollHighlightIntoView,这里不再动。
  $effect(() => {
    if (open && highlight < 0) {
      void tick().then(() => {
        if (listEl && highlight < 0) listEl.scrollTop = listEl.scrollHeight;
      });
    }
  });

  /** 父组件 handleKeydown 先调这里;返回 true 表示该键已被弹层处理。
   *  无高亮时的回车返回 false → 父组件照旧发送。IME 组合中一律不处理。 */
  export function handleKey(e: KeyboardEvent): boolean {
    if (e.isComposing) return false;
    if (!open) {
      // 输入为空时按 ↑:弹出纯历史列表(最近在前)
      if (e.key === 'ArrowUp' && enabled && !hexMode && query.trim().length === 0 && commandIndex.history.length > 0) {
        e.preventDefault();
        forcedHistory = true;
        dismissedFor = null;
        highlight = -1;
        return true;
      }
      return false;
    }
    switch (e.key) {
      // 弹层在输入框上方(bottom-full)且列表反转(最佳候选贴着输入框):
      // ↑ 是唯一"进入列表"的键,进入即选中贴着输入框的最佳候选,继续 ↑ 向列表上方翻;
      // ↓ 只在高亮列表内向下走(回向输入框方向),无高亮时放行给输入框。
      case 'ArrowDown':
        if (highlight < 0) return false;
        e.preventDefault();
        // 视觉下方 = rank 减小(向贴输入框的最佳候选回);高亮可能因后台刷新越界,先夹回再移
        highlight = Math.max(Math.min(highlight, items.length) - 1, 0);
        sourceIndex = 0;
        scrollHighlightIntoView();
        return true;
      case 'ArrowUp':
        e.preventDefault();
        // 无高亮 → 最佳候选(rank 0,视觉上在最下贴着输入框);继续 ↑ 向视觉上方移动
        highlight = Math.min(highlight + 1, items.length - 1);
        sourceIndex = 0;
        scrollHighlightIntoView();
        return true;
      case 'Enter':
        if (highlight < 0 || highlight >= items.length) return false;
        e.preventDefault();
        acceptItem(items[highlight]);
        return true;
      case 'Tab':
        // Shift+Tab 是反向焦点导航,不劫持
        if (e.shiftKey) return false;
        e.preventDefault();
        acceptItem(items[highlight >= 0 && highlight < items.length ? highlight : 0]);
        return true;
      case 'Escape':
        e.preventDefault();
        dismiss();
        return true;
    }
    return false;
  }

  /** 把候选文本按当前输入切成 [前, 命中, 后],命中段加粗。 */
  function splitMatch(text: string): [string, string, string] {
    const q = query.trim().toUpperCase();
    if (q.length < 2) return [text, '', ''];
    const pos = text.toUpperCase().indexOf(q);
    if (pos < 0) return [text, '', ''];
    return [text.slice(0, pos), text.slice(pos, pos + q.length), text.slice(pos + q.length)];
  }
</script>

{#if open}
  <!-- mousedown 阻止默认:点弹层不让输入框失焦(失焦会收起),点完焦点仍在输入框 -->
  <div
    class="absolute left-0 right-0 bottom-full mb-1 z-[300] flex overflow-hidden"
    style="background: var(--background-elevated); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg); max-height: 320px;"
    tabindex="-1"
    onmousedown={(e) => e.preventDefault()}
  >
    <!-- 左:候选列表(只有历史项时占满);role="listbox" 放这里使 option 行是其直接子元素 -->
    <div bind:this={listEl} class="overflow-y-auto py-1" role="listbox" tabindex="-1" style="flex: {hasManual ? '0 0 40%' : '1 1 auto'}; min-width: 220px;">
      {#each rows as { item, i } (item.kind === 'history' ? 'h:' + item.text : 'm:' + item.entry.key)}
        {@const text = item.kind === 'history' ? item.text : item.entry.key}
        {@const [before, hit, after] = splitMatch(text)}
        <div
          role="option"
          aria-selected={i === highlight}
          tabindex="-1"
          data-idx={i}
          class="suggest-row flex items-center gap-2 px-3 cursor-pointer select-none"
          style="height: 30px; font-size: 13px; {i === highlight ? 'background: var(--overlay-hover); box-shadow: inset 2px 0 0 var(--primary);' : ''}"
          onclick={() => acceptItem(item)}
        >
          <span
            class="shrink-0 rounded px-1.5 text-[10px] leading-[16px] whitespace-nowrap overflow-hidden text-ellipsis"
            style="background: var(--border-subtle); color: var(--muted-foreground); max-width: 72px;"
          >{item.kind === 'history' ? '历史' : shortTitle(docTitle(commandIndex.documents, item.entry.primary.document_id))}</span>
          <span class="truncate" style="font-family: var(--font-mono); color: var(--foreground);">{before}<b>{hit}</b>{after}</span>
          {#if item.kind === 'manual'}
            <span class="truncate ml-auto text-[12px]" style="color: var(--muted-foreground); max-width: 45%;">{displayName(item.entry.primary)}</span>
          {/if}
        </div>
      {/each}
    </div>

    <!-- 右:详情卡(只有历史项时不显示) -->
    {#if hasManual && preview}
      <div class="overflow-y-auto px-4 py-3 text-[12px] leading-relaxed" style="flex: 1 1 60%; border-left: 1px solid var(--border); color: var(--foreground);">
        {#if preview.kind === 'history'}
          <div class="text-[13px] break-all" style="font-family: var(--font-mono);">{preview.text}</div>
          <div class="mt-1" style="color: var(--muted-foreground);">来自发送历史</div>
        {:else if preview.kind === 'manual' && record}
          {@const entry = preview.entry}
          <div class="flex items-baseline gap-2 flex-wrap">
            <span class="text-[13px] font-semibold" style="font-family: var(--font-mono);">{record.command.trim()}</span>
            <span style="color: var(--muted-foreground);">{displayName(record, 60)}</span>
          </div>

          {#if record.syntax.trim()}
            <div class="mt-2 font-medium" style="color: var(--muted-foreground);">语法</div>
            {#each splitSyntax(record.syntax) as line}
              <div class="break-all" style="font-family: var(--font-mono);">{line}</div>
            {/each}
          {/if}

          {#if record.parameters.length}
            <div class="mt-2 font-medium" style="color: var(--muted-foreground);">参数 <span class="font-normal">(* 必选)</span></div>
            <!-- 参数多的指令(AT+MQTTCFG 23 个)在区域内滚动,不撑高弹层 -->
            <div class="overflow-y-auto" style="max-height: 120px;">
              {#each record.parameters as p}
                <div class="flex gap-2 py-0.5" style="border-top: 1px solid var(--border-subtle);">
                  <span class="shrink-0" style="font-family: var(--font-mono); min-width: 96px;">{p.name}{p.required ? ' *' : ''}</span>
                  <span class="break-all" style="color: var(--muted-foreground);">{p.description}</span>
                </div>
              {/each}
            </div>
          {/if}

          {#if record.example.trim()}
            <div class="mt-2 font-medium" style="color: var(--muted-foreground);">示例 <span class="font-normal">(点 AT 行填入)</span></div>
            <div class="flex flex-col gap-0.5 items-start">
              {#each exampleLines(record.example) as ex}
                {#if ex.fillable}
                  <button
                    type="button"
                    class="suggest-example text-left rounded px-1.5 py-0.5 break-all"
                    style="font-family: var(--font-mono); background: var(--border-subtle); color: var(--foreground);"
                    onclick={() => accept(ex.text)}
                  >{ex.text}</button>
                {:else}
                  <div class="px-1.5 break-all" style="font-family: var(--font-mono); color: var(--muted-foreground);">{ex.text}</div>
                {/if}
              {/each}
            </div>
          {/if}

          {#if record.summary.trim()}
            <div class="mt-2" style="color: var(--muted-foreground);">{record.summary}</div>
          {/if}

          <div class="mt-2 flex flex-wrap items-center gap-x-2 gap-y-0.5" style="color: var(--muted-foreground);">
            <span>来源:{docTitle(commandIndex.documents, record.document_id)}{record.page_no != null ? ` · 第 ${record.page_no} 页` : ''}</span>
            {#if entry.alsoIn.length}
              <span>也见于:</span>
              {#each [entry.primary, ...entry.alsoIn] as rec, si}
                {#if si !== sourceIndex}
                  <button
                    type="button"
                    class="underline decoration-dotted hover:opacity-80"
                    style="color: var(--primary);"
                    onclick={() => (sourceIndex = si)}
                  >{docTitle(commandIndex.documents, rec.document_id)}</button>
                {/if}
              {/each}
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .suggest-row:hover {
    background: var(--overlay-hover);
  }
  .suggest-example:hover {
    background: var(--overlay-hover);
  }
</style>
