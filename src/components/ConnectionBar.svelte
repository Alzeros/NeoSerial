<script lang="ts">
  import { availablePorts, connectionParams, connected, presetBaudRates, windowPort } from '$lib/stores';
  import { connect, disconnect, listPorts } from '$lib/tauri';

  // 波特率下拉项 = 预设波特率（来自设置，持久化）
  const baudRates = $derived(presetBaudRates.value.map((b) => String(b)));
  const dataBitsOpts = [{ l: '5', v: 'Five' }, { l: '6', v: 'Six' }, { l: '7', v: 'Seven' }, { l: '8', v: 'Eight' }];
  const parityOpts = [{ l: 'None', v: 'None' }, { l: 'Odd', v: 'Odd' }, { l: 'Even', v: 'Even' }];
  const stopBitsOpts = ['1', '2'];
  const flowOpts = [{ l: 'None', v: 'None' }, { l: 'Soft', v: 'Software' }, { l: 'Hard', v: 'Hardware' }];

  let baudRateStr = $state('115200');
  let stopBitsStr = $state('1');

  async function refreshPorts() {
    try {
      const ports = await listPorts();
      availablePorts.value = ports;
      if (ports.length > 0 && !connectionParams.port) {
        connectionParams.port = ports[0]!;
      }
    } catch (e) {
      console.error('获取端口列表失败:', e);
    }
  }

  async function handleConnect() {
    const baud = Number(baudRateStr);
    // 波特率为空/非法时拒绝连接，避免 0 波特率下发到后端
    if (!baud || baud <= 0) return;
    // 副窗口的 port 锁定为本窗口 label 反推的 windowPort,不允许连到别的 port
    // (事件 emit_to(win-{windowPort}),连别的 port 会收不到该窗口事件)。
    if (!windowPort.value) return;
    try {
      await connect({
        port: windowPort.value,
        baud_rate: baud,
        data_bits: connectionParams.dataBits,
        parity: connectionParams.parity,
        stop_bits: Number(stopBitsStr) as 1 | 2,
        flow_control: connectionParams.flowControl,
      });
    } catch (e) {
      console.error('连接失败:', e);
    }
  }

  async function handleDisconnect() {
    try {
      if (windowPort.value) await disconnect(windowPort.value);
    } catch (e) {
      console.error('断开失败:', e);
    }
  }

  $effect(() => {
    refreshPorts();
  });

  // 未连接时定时轮询串口列表（每 2 秒），检测热插拔
  $effect(() => {
    if (connected.value) return;
    const timer = setInterval(refreshPorts, 2000);
    return () => clearInterval(timer);
  });
</script>

<div class="layout-fixed flex items-end border-b px-5 py-4" style="background: var(--background-elevated); border-color: var(--border);">
  <!-- 左侧：所有配置项分组（紧凑间距，左对齐紧凑排列，不拉伸） -->
  <div class="config-group" style="margin-right: 24px; gap: 12px;">
    <!-- 端口号(副窗口锁定为本窗口 port,不可改) -->
    <label class="control-group">
      <span>端口号</span>
      <select style="width: 100px;" value={windowPort.value ?? connectionParams.port} disabled>
        <option value={windowPort.value ?? connectionParams.port}>{windowPort.value ?? connectionParams.port}</option>
      </select>
    </label>

    <!-- 波特率 -->
    <label class="control-group">
      <span>波特率</span>
      <select style="width: 96px;" bind:value={baudRateStr} disabled={connected.value}>
        {#each baudRates as b}
          <option value={b}>{b}</option>
        {/each}
      </select>
    </label>

    <!-- 数据位 -->
    <label class="control-group">
      <span>数据位</span>
      <select style="width: 64px;" bind:value={connectionParams.dataBits} disabled={connected.value}>
        {#each dataBitsOpts as o}
          <option value={o.v}>{o.l}</option>
        {/each}
      </select>
    </label>

    <!-- 校验位 -->
    <label class="control-group">
      <span>校验位</span>
      <select style="width: 80px;" bind:value={connectionParams.parity} disabled={connected.value}>
        {#each parityOpts as o}
          <option value={o.v}>{o.l}</option>
        {/each}
      </select>
    </label>

    <!-- 停止位 -->
    <label class="control-group">
      <span>停止位</span>
      <select style="width: 60px;" bind:value={stopBitsStr} disabled={connected.value}>
        {#each stopBitsOpts as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </label>

    <!-- 流控制 -->
    <label class="control-group">
      <span>流控制</span>
      <select style="width: 84px;" bind:value={connectionParams.flowControl} disabled={connected.value}>
        {#each flowOpts as o}
          <option value={o.v}>{o.l}</option>
        {/each}
      </select>
    </label>
  </div>

  <!-- 右侧：连接/断开按钮（ml-auto 推至最右，与下拉框底部对齐） -->
  <div class="flex-shrink-0 ml-auto">
    {#if connected.value}
      <button class="btn btn-danger" style="width: 76px;" onclick={handleDisconnect}>断开</button>
    {:else}
      <button class="btn btn-primary" style="width: 76px;" onclick={handleConnect}>连接</button>
    {/if}
  </div>
</div>
