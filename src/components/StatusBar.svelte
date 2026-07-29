<script lang="ts">
  import { connected, currentPort, rxBytes, scriptPanelOpen, toggleScriptPanel, txBytes } from '$lib/stores';

  function formatBytes(n: number): string {
    if (n < 1024) return n.toString();
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}K`;
    return `${(n / (1024 * 1024)).toFixed(1)}M`;
  }
</script>

<div class="flex items-center gap-6 border-t border-[var(--border)] px-5 py-2" style="background: var(--background-elevated);">
  <!-- 连接状态 -->
  <span class="flex items-center gap-2 text-[13px]">
    <span class="h-2 w-2 rounded-full {connected.value ? 'bg-[var(--rx)]' : 'bg-[var(--muted-foreground)]'}"></span>
    <span class="text-[var(--muted-foreground)]">{connected.value ? `已连接 ${currentPort.value || ''}` : '未连接'}</span>
  </span>

  <div class="w-px h-4 bg-[var(--border)]"></div>

  <!-- 统计 -->
  <span class="text-[13px] font-medium text-[var(--tx)]">Tx: {formatBytes(txBytes.value)}</span>
  <span class="text-[13px] font-medium text-[var(--rx)]">Rx: {formatBytes(rxBytes.value)}</span>

  <!-- 右侧: 脚本面板切换 -->
  <button
    class="ml-auto text-[13px] text-[var(--muted-foreground)] hover:text-[var(--foreground)] px-2 py-1 rounded hover:bg-[var(--background-elevated)] cursor-pointer transition-colors"
    onclick={toggleScriptPanel}
  >
    脚本 {scriptPanelOpen.value ? '▾' : '▸'}
  </button>
</div>
