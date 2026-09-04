<script lang="ts">
  import { tick } from 'svelte';
  import { cachedSettings, previewedCommand, suggestFillRequest } from '$lib/stores';
  import { commandIndex } from '$lib/commandIndex';
  import {
    buildManualEntries,
    displayName,
    docTitle,
    matchSuggestions,
    shortTitle,
    type Suggestion,
  } from '$lib/suggest';

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
  // 详情卡显示项:有高亮看高亮,否则预览第 0 项(列表不画高亮条)。供右栏参考面板展示
  const previewIndex = $derived(highlight >= 0 && highlight < items.length ? highlight : 0);
  const preview = $derived(open ? items[previewIndex] : undefined);

  // 广播当前预览项给右栏参考面板:popup 开着就发(高亮项或默认 rank 0),关闭后不清(保持上次)。
  // 预览项变化即复位 sourceIndex=0(右栏"也见于"切换改 sourceIndex 不触发本 effect,故不会回退)。
  $effect(() => {
    const p = preview;
    if (p) {
      previewedCommand.suggestion = p;
      previewedCommand.sourceIndex = 0;
    }
  });

  // 右栏"点示例填入":nonce 变化即走自己的 accept(填输入框 + 设 dismissedFor 防弹层重开)。
  // 读 nonce 不读 text:accept 内部会改 query 相关态,不与 text 形成环。
  let lastFillNonce = 0;
  $effect(() => {
    const n = suggestFillRequest.nonce;
    if (n !== lastFillNonce) {
      lastFillNonce = n;
      accept(suggestFillRequest.text);
    }
  });

  /** 父组件在输入框 oninput 时调:文本变了,高亮复位。 */
  export function onInput() {
    highlight = -1;
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

  // 列表反转后 rank 0(最佳候选)在 DOM 末尾。候选多到溢出时浏览器默认 scrollTop=0 停在顶部
  // (最旧候选),贴输入框的最佳候选被滚出视野。无高亮时把滚动钉到底部:open、候选数
  // (items.length)、高亮清除都会触发重跑;有高亮交给 scrollHighlightIntoView。tick 等 DOM 渲染完再读 scrollHeight。
  $effect(() => {
    void items.length;
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
        previewedCommand.sourceIndex = 0;
        scrollHighlightIntoView();
        return true;
      case 'ArrowUp':
        e.preventDefault();
        // 无高亮 → 最佳候选(rank 0,视觉上在最下贴着输入框);继续 ↑ 向视觉上方移动
        highlight = Math.min(highlight + 1, items.length - 1);
        previewedCommand.sourceIndex = 0;
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
    class="absolute left-0 right-0 bottom-full mb-1 z-[300] overflow-hidden"
    style="background: var(--background-elevated); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg);"
    tabindex="-1"
    onmousedown={(e) => e.preventDefault()}
  >
    <!-- 候选列表:弹层只放列表,详情在右栏 CommandDetail。max-height 放内层使它成为真正的
         滚动容器(外层 overflow:hidden 只管圆角裁剪,不滚);role="listbox" 放这里使 option 行是其直接子元素 -->
    <div bind:this={listEl} class="overflow-y-auto py-1" role="listbox" tabindex="-1" style="min-width: 0; max-height: 320px;">
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
  </div>
{/if}

<style>
  .suggest-row:hover {
    background: var(--overlay-hover);
  }
</style>
