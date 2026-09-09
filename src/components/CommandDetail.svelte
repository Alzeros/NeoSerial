<script lang="ts">
  import { tick, onMount } from 'svelte';
  import { commandIndex } from '$lib/commandIndex.svelte';
  import { cachedSettings, requestSuggestFill } from '$lib/stores';
  import {
    buildManualEntries,
    displayName,
    docTitle,
    exampleLines,
    searchCommands,
    shortTitle,
    splitSyntax,
    type ManualEntry,
  } from '$lib/suggest';
  import type { ManualCommand } from '$lib/types';

  let searchQuery = $state('');
  let selectedIndex = $state(0);
  // 0=primary,n>0=alsoIn[n-1];选中项变化时复位为 0
  let sourceIndex = $state(0);
  let searchEl: HTMLInputElement | undefined;
  let listEl: HTMLDivElement | undefined;

  const disabledDocIds = $derived(cachedSettings.value?.command_index?.disabled_doc_ids ?? []);
  const manualEntries = $derived(
    buildManualEntries(commandIndex.documents, commandIndex.commands, disabledDocIds),
  );
  const results = $derived(searchCommands(searchQuery, manualEntries));
  const selected = $derived(results[selectedIndex] ?? null);

  // 结果区没东西时的一行提示:只在真的有问题时出(没索引/手册全关/搜不到)。
  // 空手进来不出——搜索框的 placeholder 已经把用法说了,底下再摆一句是重复。
  const emptyHint = $derived.by((): string | null => {
    if (results.length) return null;
    if (!commandIndex.commands.length) return '还没有指令索引。点右上角齿轮填知识库地址,再刷新指令库。';
    if (!manualEntries.length) return '参与联想的手册都关掉了。点右上角齿轮勾选要用的手册。';
    return searchQuery.trim() ? '无匹配指令。换个更短或更通用的词试试。' : null;
  });
  /** 搜索框下面是否有内容(决定要不要画那条分隔线) */
  const hasBody = $derived(results.length > 0 || emptyHint !== null);

  // 结果集变化(新搜索/输入)时选回第一条,来源复位。读 results.length 触发。
  let prevLen = -1;
  $effect(() => {
    const len = results.length;
    if (len !== prevLen) {
      prevLen = len;
      selectedIndex = 0;
      sourceIndex = 0;
    }
  });

  const record = $derived.by((): ManualCommand | null => {
    const s = selected;
    if (!s) return null;
    return sourceIndex === 0 ? s.primary : s.alsoIn[sourceIndex - 1] ?? s.primary;
  });

  async function scrollSelectedIntoView() {
    await tick();
    listEl?.querySelector<HTMLElement>(`[data-idx="${selectedIndex}"]`)?.scrollIntoView({ block: 'nearest' });
  }

  function handleSearchKey(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (!results.length) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, results.length - 1);
      sourceIndex = 0;
      void scrollSelectedIntoView();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
      sourceIndex = 0;
      void scrollSelectedIntoView();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (selected) requestSuggestFill(selected.key);
    }
  }

  function selectRow(i: number) {
    selectedIndex = i;
    sourceIndex = 0;
  }

  // 切到本 tab 时自动聚焦搜索框(便于直接开搜)。
  onMount(() => {
    searchEl?.focus();
  });
</script>

<div class="flex flex-col h-full min-h-0">
  <!-- 搜索框:模糊搜 command/name/summary。去设置的齿轮在面板右上角(ScriptSequencer 的
       tab 条里,三个 tab 共用),这里不再重复摆一个。
       输入框只压高度,底色/圆角/聚焦描边都沿用全局 input 样式(白底 + 6px 圆角),
       跟窗口里其他输入框一致;分隔线只在下面真有内容时画,不留一根悬着的线。 -->
  <div class="flex items-center px-3 py-2 shrink-0" style={hasBody ? 'border-bottom: 1px solid var(--border);' : ''}>
    <input
      bind:this={searchEl}
      type="text"
      class="flex-1 min-w-0"
      style="height: 32px; padding: 0 10px;"
      placeholder="搜索指令或功能(如 mqtt、信号、CSQ)…"
      title="↑↓ 选中,Enter 把指令填入发送框"
      spellcheck="false"
      bind:value={searchQuery}
      onkeydown={handleSearchKey}
    />
  </div>

  <!-- 结果列表 + 详情:上下堆叠(避免左右分栏在窄面板里挤压),各自独立滚动 -->
  <div class="flex flex-col min-h-0" style="flex: 1 1 0%;">
    {#if results.length}
      <!-- 结果列表:command + 来源徽标 + 中文名,点击选中(不直接填,Enter 或详情里示例才填) -->
      <div bind:this={listEl} class="overflow-y-auto py-1 shrink-0" style="max-height: 42%;" role="listbox" tabindex="-1">
        {#each results as entry, i (entry.key)}
          <div
            role="option"
            aria-selected={i === selectedIndex}
            tabindex="-1"
            data-idx={i}
            class="suggest-row flex items-center gap-2 px-3 cursor-pointer select-none"
            style="height: 30px; font-size: 13px; {i === selectedIndex ? 'background: var(--overlay-hover); box-shadow: inset 2px 0 0 var(--primary);' : ''}"
            onclick={() => selectRow(i)}
          >
            <span class="shrink-0 flex items-center gap-1">
              <span
                class="rounded px-1.5 text-[10px] leading-[16px] whitespace-nowrap overflow-hidden text-ellipsis"
                style="background: var(--border-subtle); color: var(--muted-foreground); max-width: 72px;"
              >{shortTitle(docTitle(commandIndex.documents, entry.primary.document_id))}</span>
              {#if entry.alsoIn.length}
                <!-- 选中前就看得出这条在多本手册里有(选中后详情底部的"也见于"可切换来源) -->
                <span
                  class="text-[10px] leading-[16px]"
                  style="color: var(--muted-foreground);"
                  title="另见 {entry.alsoIn.length} 本手册"
                >+{entry.alsoIn.length}</span>
              {/if}
            </span>
            <span class="truncate" style="font-family: var(--font-mono); color: var(--foreground);">{entry.key}</span>
            <span class="truncate ml-auto text-[12px]" style="color: var(--muted-foreground); max-width: 45%;">{displayName(entry.primary)}</span>
          </div>
        {/each}
      </div>
    {:else if emptyHint}
      <div class="px-3 py-3 text-[12px]" style="color: var(--muted-foreground);">{emptyHint}</div>
    {/if}

    {#if record}
      {@const entry = (selected as ManualEntry)}
      <!-- select-text:语法/参数/示例是要往别处抄的正文(全局默认不可选,见 app.css"文本选中范围")。
           tabindex="-1":点一下后 Ctrl+A 只圈这块(范围逻辑在 App.svelte 的 handleSelectAll) -->
      <div tabindex="-1" class="overflow-y-auto outline-none px-3 py-3 text-[12px] leading-relaxed select-text" style="flex: 1 1 0%; min-height: 0; border-top: 1px solid var(--border); color: var(--foreground);">
        <div class="flex items-baseline gap-2 flex-wrap">
          <span class="text-[14px] font-semibold" style="font-family: var(--font-mono);">{record.command.trim()}</span>
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
          <div class="overflow-y-auto" style="max-height: 240px;">
            {#each record.parameters as p}
              <div class="flex gap-2 py-0.5" style="border-top: 1px solid var(--border-subtle);">
                <span class="shrink-0" style="font-family: var(--font-mono); min-width: 96px;">{p.name}{p.required ? ' *' : ''}</span>
                <span class="break-all" style="color: var(--muted-foreground);">{p.description}</span>
              </div>
            {/each}
          </div>
        {/if}

        {#if record.example.trim()}
          <div class="mt-2 font-medium" style="color: var(--muted-foreground);">示例 <span class="font-normal">(点指令填入输入框)</span></div>
          <div class="flex flex-col gap-0.5 items-start">
            {#each exampleLines(record.example) as ex}
              {#if ex.fillable}
                <!-- 按钮只包会被填入的那段,行首标签("Test Command:")留在按钮外做纯文字:
                     可点范围 = 填入内容,点什么进去什么 -->
                <div class="flex items-baseline gap-1 break-all">
                  {#if ex.prefix}
                    <span class="pl-1.5 shrink-0" style="font-family: var(--font-mono); color: var(--muted-foreground);">{ex.prefix}</span>
                  {/if}
                  <button
                    type="button"
                    class="suggest-example text-left rounded px-1.5 py-0.5 break-all"
                    style="font-family: var(--font-mono); background: var(--border-subtle); color: var(--foreground);"
                    title="填入输入框"
                    onclick={() => {
                      // 详情区可选中(复制语法/示例),拖选恰好落在这个按钮里时不该顺手填进输入框。
                      // 普通单击会先把选区折叠掉,isCollapsed 为真(无 range 也算真),照常填。
                      if (!window.getSelection()?.isCollapsed) return;
                      requestSuggestFill(ex.fill);
                    }}
                  >{ex.fill}</button>
                </div>
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
      </div>
    {/if}
  </div>
</div>

<style>
  .suggest-row:hover {
    background: var(--overlay-hover);
  }
  .suggest-example:hover {
    background: var(--overlay-hover);
  }
</style>
