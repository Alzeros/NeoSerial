<script lang="ts">
  import { tick } from 'svelte';
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
  // 调用方(ScriptSequencer 切 view)触发 mount,这里 onMount 聚焦一次。
  import { onMount } from 'svelte';
  onMount(() => {
    searchEl?.focus();
  });
</script>

<div class="flex flex-col h-full min-h-0">
  <!-- 搜索框:模糊搜 command/name/summary,与输入框联想的精确前缀分开 -->
  <div class="p-2 shrink-0" style="border-bottom: 1px solid var(--border);">
    <input
      bind:this={searchEl}
      type="text"
      class="w-full"
      style="height: 32px; padding: 4px 10px; font-size: 13px; background: var(--background); border: 1px solid var(--border); border-radius: var(--radius); color: var(--foreground);"
      placeholder="搜索指令或功能(如 mqtt、信号、CSQ)…"
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
            <span
              class="shrink-0 rounded px-1.5 text-[10px] leading-[16px] whitespace-nowrap overflow-hidden text-ellipsis"
              style="background: var(--border-subtle); color: var(--muted-foreground); max-width: 72px;"
            >{shortTitle(docTitle(commandIndex.documents, entry.primary.document_id))}</span>
            <span class="truncate" style="font-family: var(--font-mono); color: var(--foreground);">{entry.key}</span>
            <span class="truncate ml-auto text-[12px]" style="color: var(--muted-foreground); max-width: 45%;">{displayName(entry.primary)}</span>
          </div>
        {/each}
      </div>
    {:else if searchQuery.trim()}
      <div class="px-4 py-3 text-[12px]" style="color: var(--muted-foreground);">无匹配指令。试试更短或更通用的词(mqtt、信号、CSQ)。</div>
    {/if}

    {#if record}
      {@const entry = (selected as ManualEntry)}
      <div class="overflow-y-auto px-4 py-3 text-[12px] leading-relaxed" style="flex: 1 1 0%; min-height: 0; border-top: 1px solid var(--border); color: var(--foreground);">
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
          <div class="mt-2 font-medium" style="color: var(--muted-foreground);">示例 <span class="font-normal">(点 AT 行填入输入框)</span></div>
          <div class="flex flex-col gap-0.5 items-start">
            {#each exampleLines(record.example) as ex}
              {#if ex.fillable}
                <button
                  type="button"
                  class="suggest-example text-left rounded px-1.5 py-0.5 break-all"
                  style="font-family: var(--font-mono); background: var(--border-subtle); color: var(--foreground);"
                  onclick={() => requestSuggestFill(ex.text)}
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
      </div>
    {:else if !results.length && !searchQuery.trim()}
      <div class="px-4 py-3 text-[12px]" style="color: var(--muted-foreground);">输入关键词搜索指令(mqtt、信号、CSQ…),↑↓ 选中,Enter 填入输入框。</div>
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
