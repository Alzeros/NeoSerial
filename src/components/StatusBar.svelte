<script lang="ts">
  import { connected, currentPort, rxBytes, txBytes, logDirLabelStyle, windowPort } from '$lib/stores';
  import { resetStats } from '$lib/tauri';

  function formatBytes(n: number): string {
    if (n < 1024) return n.toString();
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}K`;
    return `${(n / (1024 * 1024)).toFixed(1)}M`;
  }

  // 方向标签跟随日志区设置：short=Tx/Rx，full=发送/接收
  const txLabel = $derived(logDirLabelStyle.value === 'full' ? '发送' : 'Tx');
  const rxLabel = $derived(logDirLabelStyle.value === 'full' ? '接收' : 'Rx');

  // 收发活跃：字节数变化后置 true，停 ~1.5s 自动落回。
  // 简单节流：只在数值变化时重置定时器，不每次重渲染。
  let ioActive = $state(false);
  let ioTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    // 依赖订阅
    const sum = txBytes.value + rxBytes.value + (connected.value ? 1 : 0);
    if (!connected.value) {
      ioActive = false;
      if (ioTimer) { clearTimeout(ioTimer); ioTimer = null; }
      return;
    }
    if (sum === 0) { ioActive = false; return; }
    ioActive = true;
    if (ioTimer) clearTimeout(ioTimer);
    ioTimer = setTimeout(() => { ioActive = false; }, 1500);
    return () => { if (ioTimer) { clearTimeout(ioTimer); ioTimer = null; } };
  });

  async function handleResetStats() {
    try {
      if (windowPort.value) await resetStats(windowPort.value);
    } catch (e) {
      console.error('清零统计失败:', e);
    }
  }
</script>

<div class="flex items-center gap-6 border-t border-[var(--border)] px-5 py-2" style="background: var(--background-elevated);">
  <!-- 连接状态：已连接 + 正在收发时圆点呼吸 -->
  <span class="flex items-center gap-2 text-[13px]">
    <span
      class="h-2 w-2 rounded-full {connected.value ? 'bg-[var(--rx)]' : 'bg-[var(--muted-foreground)]'} {ioActive ? 'status-dot-pulse' : ''}"
    ></span>
    <span class="text-[var(--muted-foreground)]">{connected.value ? `已连接 ${currentPort.value || ''}` : '未连接'}</span>
  </span>

  <div class="w-px h-4 bg-[var(--border)]"></div>

  <!-- 统计：本次会话累计收发字节数（不随日志清空重置）。tnum 等宽数字防宽度抖动，mono 字体显仪表感 -->
  <span class="text-[13px] font-medium text-[var(--tx)] tnum" style="font-family: var(--font-mono);" title="本次会话累计发送字节数">{txLabel}: {formatBytes(txBytes.value)}</span>
  <span class="text-[13px] font-medium text-[var(--rx)] tnum" style="font-family: var(--font-mono);" title="本次会话累计接收字节数">{rxLabel}: {formatBytes(rxBytes.value)}</span>
  <button
    class="text-[12px] text-[var(--muted-foreground)] hover:text-[var(--error)] cursor-pointer transition-colors"
    title="清零收发统计"
    onclick={handleResetStats}
  >清空</button>
</div>
