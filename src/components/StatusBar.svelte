<script lang="ts">
  import { onMount } from 'svelte';
  import { connected, currentPort, rxBytes, txBytes, logDirLabelStyle, windowPort } from '$lib/stores';
  import { getModemStatus, onModemStatus, resetStats } from '$lib/tauri';

  function formatBytes(n: number): string {
    if (n < 1024) return n.toString();
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}K`;
    return `${(n / (1024 * 1024)).toFixed(1)}M`;
  }

  // 方向标签跟随日志区设置：short=Tx/Rx，full=发送/接收
  const txLabel = $derived(logDirLabelStyle.value === 'full' ? '发送' : 'Tx');
  const rxLabel = $derived(logDirLabelStyle.value === 'full' ? '接收' : 'Rx');

  // 收发活跃：字节数增长时置 true，停 ~1.5s 自动落回。
  // 只看字节增量，不把"是否连接"混进比较——否则连接瞬间会误判为活跃而空转。
  let ioActive = $state(false);
  let ioTimer: ReturnType<typeof setTimeout> | null = null;
  let lastBytes = 0; // 上次的累计字节数；连接但静默时 total===lastBytes，不呼吸
  $effect(() => {
    // 读取 connected 让 effect 订阅连接状态变化（断开时落回静止）
    const conn = connected.value;
    const total = txBytes.value + rxBytes.value;
    if (!conn) {
      ioActive = false;
      lastBytes = 0;
      if (ioTimer) { clearTimeout(ioTimer); ioTimer = null; }
      return;
    }
    // 字节数没变（连接但静默）→ 不改变呼吸状态
    if (total === lastBytes) return;
    lastBytes = total;
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

  // ============ CTS/DSR 指示灯 ============
  // null=未知(未连接/非 Windows):不显示。事件驱动更新 + 连接建立/换端口时主动拉一次初始态。
  let cts = $state<boolean | null>(null);
  let dsr = $state<boolean | null>(null);

  onMount(() => {
    const un = onModemStatus((s) => {
      if (s.port === currentPort.value) { cts = s.cts; dsr = s.dsr; }
    });
    return () => un.then((f) => f());
  });

  $effect(() => {
    const conn = connected.value;
    const port = currentPort.value;
    if (!conn || !port) { cts = null; dsr = null; return; }
    getModemStatus(port).then((r) => {
      // 拉取期间可能已断开/换端口:过期结果丢弃
      if (r && connected.value && currentPort.value === port) { cts = r.cts; dsr = r.dsr; }
    }).catch(() => { /* 查询失败保持未知态 */ });
  });
</script>

<div class="flex items-center gap-6 border-t border-[var(--border)] px-5 py-1.5" data-theme-target="background-elevated" style="background: var(--background-elevated);">
  <!-- 连接状态：已连接 + 正在收发时圆点呼吸 -->
  <span class="flex items-center gap-2 text-[13px] leading-none">
    <span
      class="h-2 w-2 rounded-full {connected.value ? 'bg-[var(--rx)]' : 'bg-[var(--muted-foreground)]'} {ioActive ? 'status-dot-pulse' : ''}"
    ></span>
    <span class="text-[var(--muted-foreground)]">{connected.value ? `已连接 ${currentPort.value || ''}` : '未连接'}</span>
  </span>

  <div class="w-px h-4 bg-[var(--border)]"></div>

  <!-- CTS/DSR:对端流入本机的控制线电平(事件驱动,变化才发)。未连接/读不到不显示 -->
  {#if connected.value && cts !== null}
    <span class="flex items-center gap-1.5 text-[13px] leading-none text-[var(--muted-foreground)]" style="font-family: var(--font-mono);"
      title="CTS:对端驱动本机的输入线(通常接对端 RTS)。高=对端允许本机发送;硬件流控下对端拉低会停发">
      <span class="h-2 w-2 rounded-full {cts ? 'bg-[var(--rx)]' : 'bg-[var(--muted-foreground)] opacity-40'}"></span>CTS
    </span>
    <span class="flex items-center gap-1.5 text-[13px] leading-none text-[var(--muted-foreground)]" style="font-family: var(--font-mono);"
      title="DSR:对端驱动本机的输入线。高=对端设备就绪">
      <span class="h-2 w-2 rounded-full {dsr ? 'bg-[var(--rx)]' : 'bg-[var(--muted-foreground)] opacity-40'}"></span>DSR
    </span>
  {/if}

  <!-- 统计：本次会话累计收发字节数（不随日志清空重置）。tnum 等宽数字防宽度抖动，mono 字体显仪表感 -->
  <span class="text-[13px] leading-none font-medium text-[var(--muted-foreground)] tnum" style="font-family: var(--font-mono);" title="本次会话累计发送字节数">{txLabel}: {formatBytes(txBytes.value)}</span>
  <span class="text-[13px] leading-none font-medium text-[var(--rx)] tnum" style="font-family: var(--font-mono);" title="本次会话累计接收字节数">{rxLabel}: {formatBytes(rxBytes.value)}</span>
  <button
    class="text-[13px] leading-none font-medium text-[var(--muted-foreground)] hover:text-[var(--error)] cursor-pointer transition-colors"
    style="font-family: var(--font-mono);"
    title="清零收发统计"
    onclick={handleResetStats}
  >清空</button>
</div>
