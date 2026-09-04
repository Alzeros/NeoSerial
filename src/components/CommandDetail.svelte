<script lang="ts">
  import { previewedCommand, requestSuggestFill } from '$lib/stores';
  import { commandIndex } from '$lib/commandIndex.svelte';
  import { displayName, docTitle, exampleLines, splitSyntax } from '$lib/suggest';
  import type { ManualCommand } from '$lib/types';

  const suggestion = $derived(previewedCommand.suggestion);
  const sourceIndex = $derived(previewedCommand.sourceIndex);
  // 0=primary,n>0=alsoIn[n-1];越界兜底回 primary
  const record = $derived.by((): ManualCommand | null => {
    const s = suggestion;
    if (!s || s.kind !== 'manual') return null;
    const e = s.entry;
    return sourceIndex === 0 ? e.primary : e.alsoIn[sourceIndex - 1] ?? e.primary;
  });
</script>

<div class="flex flex-col h-full min-h-0">
  <div class="overflow-y-auto px-4 py-3 text-[12px] leading-relaxed" style="color: var(--foreground);">
    {#if suggestion?.kind === 'history'}
      <div class="text-[13px] break-all" style="font-family: var(--font-mono);">{suggestion.text}</div>
      <div class="mt-1" style="color: var(--muted-foreground);">来自发送历史</div>
    {:else if suggestion?.kind === 'manual' && record}
      {@const entry = suggestion.entry}
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
                onclick={() => (previewedCommand.sourceIndex = si)}
              >{docTitle(commandIndex.documents, rec.document_id)}</button>
            {/if}
          {/each}
        {/if}
      </div>
    {:else}
      <div class="text-[12px]" style="color: var(--muted-foreground);">输入框输入指令时,此处显示其语法、参数与示例;↑ 高亮某条即更新。</div>
    {/if}
  </div>
</div>

<style>
  .suggest-example:hover {
    background: var(--overlay-hover);
  }
</style>
