<script lang="ts">
  import { send } from '$lib/tauri';
  import { autoScroll, clearLogLines, hexDisplay, hexSend, lineEnding, paused, showTimestamp } from '$lib/stores';

  async function handleSendCtrlZ() {
    try {
      await send('1A', 'None', true);
    } catch (e) {
      console.error('发送 Ctrl-Z 失败:', e);
    }
  }
</script>

<div class="flex items-center gap-0.5 border-t border-border px-2 py-1 bg-muted/20 text-xs">
  <label class="flex items-center gap-0.5 px-1 cursor-pointer select-none hover:text-foreground text-muted-foreground">
    <input type="checkbox" class="h-3 w-3 rounded accent-primary" bind:checked={hexDisplay.value} />
    HEX显示
  </label>
  <label class="flex items-center gap-0.5 px-1 cursor-pointer select-none hover:text-foreground text-muted-foreground">
    <input type="checkbox" class="h-3 w-3 rounded accent-primary" bind:checked={hexSend.value} />
    HEX发送
  </label>
  <label class="flex items-center gap-0.5 px-1 cursor-pointer select-none hover:text-foreground text-muted-foreground">
    <input type="checkbox" class="h-3 w-3 rounded accent-primary" bind:checked={showTimestamp.value} />
    时间戳
  </label>
  <label class="flex items-center gap-0.5 px-1 cursor-pointer select-none hover:text-foreground text-muted-foreground">
    <input
      type="checkbox"
      class="h-3 w-3 rounded accent-primary"
      checked={lineEnding.value === 'Crlf'}
      onchange={(e) => lineEnding.value = (e.target as HTMLInputElement).checked ? 'Crlf' : 'None'}
    />
    回车换行
  </label>
  <label class="flex items-center gap-0.5 px-1 cursor-pointer select-none hover:text-foreground text-muted-foreground">
    <input type="checkbox" class="h-3 w-3 rounded accent-primary" bind:checked={autoScroll.value} />
    自动滚动
  </label>

  <div class="ml-auto flex items-center gap-1">
    {#if paused.value}
      <button
        class="h-6 px-2 rounded bg-primary/20 text-primary hover:bg-primary/30 cursor-pointer"
        onclick={() => (paused.value = false)}
      >继续</button>
    {:else}
      <button
        class="h-6 px-2 rounded hover:bg-accent text-muted-foreground cursor-pointer"
        onclick={() => (paused.value = true)}
      >暂停</button>
    {/if}
    <button
      class="h-6 px-2 rounded hover:bg-accent text-muted-foreground cursor-pointer"
      onclick={clearLogLines}
    >清空</button>
    <button
      class="h-6 px-2 rounded hover:bg-accent text-muted-foreground cursor-pointer"
      onclick={handleSendCtrlZ}
    >发送Ctrl-Z</button>
  </div>
</div>
