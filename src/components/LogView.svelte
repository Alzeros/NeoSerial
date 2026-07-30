<script lang="ts">
  import { autoScroll, hexDisplay, logLines, showTimestamp } from '$lib/stores';
  import type { LogLine } from '$lib/types';

  let scrollContainer: HTMLDivElement;

  $effect(() => {
    if (autoScroll.value && scrollContainer && logLines.length > 0) {
      requestAnimationFrame(() => {
        scrollContainer.scrollTop = scrollContainer.scrollHeight;
      });
    }
  });

  function renderLine(line: LogLine): string {
    if (hexDisplay.value) {
      return line.hex;
    }
    return line.ascii;
  }

  function dirColor(dir: string): string {
    switch (dir) {
      case 'rx':
        return 'text-[var(--rx)]';
      case 'tx':
        return 'text-[var(--tx)]';
      default:
        return 'text-[var(--muted-foreground)]';
    }
  }

  function dirLabel(dir: string): string {
    switch (dir) {
      case 'rx':
        return 'Rx';
      case 'tx':
        return 'Tx';
      default:
        return 'Sys';
    }
  }
</script>

<!-- 数据显示区：最干净的纸白，视觉重心 -->
<div class="h-full overflow-hidden flex flex-col" style="background: var(--background-data);">
  <div
    bind:this={scrollContainer}
    class="flex-1 overflow-y-auto overflow-x-auto font-mono px-5 py-4"
  >
    {#each logLines as line, i (i)}
      <div class="flex hover:bg-[rgba(255,255,255,0.03)] px-2 py-px gap-4">
        <!-- 方向标记 -->
        <span class="w-8 shrink-0 text-right font-bold {dirColor(line.dir)}">
          {dirLabel(line.dir)}
        </span>
        <!-- 时间戳 -->
        {#if showTimestamp.value}
          <span class="w-24 shrink-0 text-[var(--muted-foreground)]">{line.ts}</span>
        {/if}
        <!-- 内容 -->
        <span class="break-all whitespace-pre-wrap {line.is_error ? 'text-[var(--error)]' : ''}">
          {renderLine(line)}
        </span>
      </div>
    {/each}
  </div>
</div>
