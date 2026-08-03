<script lang="ts">
  import { connected, currentPort, rxBytes, txBytes, logDirLabelStyle } from '$lib/stores';

  function formatBytes(n: number): string {
    if (n < 1024) return n.toString();
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}K`;
    return `${(n / (1024 * 1024)).toFixed(1)}M`;
  }

  // 方向标签跟随日志区设置：short=Tx/Rx，full=发送/接收
  const txLabel = $derived(logDirLabelStyle.value === 'full' ? '发送' : 'Tx');
  const rxLabel = $derived(logDirLabelStyle.value === 'full' ? '接收' : 'Rx');
</script>

<div class="flex items-center gap-6 border-t border-[var(--border)] px-5 py-2" style="background: var(--background-elevated);">
  <!-- 连接状态 -->
  <span class="flex items-center gap-2 text-[13px]">
    <span class="h-2 w-2 rounded-full {connected.value ? 'bg-[var(--rx)]' : 'bg-[var(--muted-foreground)]'}"></span>
    <span class="text-[var(--muted-foreground)]">{connected.value ? `已连接 ${currentPort.value || ''}` : '未连接'}</span>
  </span>

  <div class="w-px h-4 bg-[var(--border)]"></div>

  <!-- 统计：本次会话累计收发字节数（不随日志清空重置） -->
  <span class="text-[13px] font-medium text-[var(--tx)]" title="本次会话累计发送字节数">{txLabel}: {formatBytes(txBytes.value)}</span>
  <span class="text-[13px] font-medium text-[var(--rx)]" title="本次会话累计接收字节数">{rxLabel}: {formatBytes(rxBytes.value)}</span>
</div>
