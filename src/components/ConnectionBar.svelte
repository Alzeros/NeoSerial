<script lang="ts">
  import { availablePorts, connectionParams, connected, presetBaudRates, windowPort, settingsRequest } from '$lib/stores';
  import { connect, disconnect, listPorts } from '$lib/tauri';
  import CustomSelect from '$components/ui/CustomSelect.svelte';

  // 局部错误提示(端口已连接/被占用),4s 自动消失。不依赖全局 store 避免跨窗口响应性问题。
  let connErr = $state<string | null>(null);
  function showErr(msg: string) {
    connErr = msg;
    setTimeout(() => { connErr = null; }, 4000);
  }

  // 波特率下拉项 = 预设波特率（来自设置，持久化）
  const baudRates = $derived(presetBaudRates.value.map((b) => String(b)));
  const dataBitsOpts = [{ l: '5', v: 'Five' }, { l: '6', v: 'Six' }, { l: '7', v: 'Seven' }, { l: '8', v: 'Eight' }];
  const parityOpts = [{ l: 'None', v: 'None' }, { l: 'Odd', v: 'Odd' }, { l: 'Even', v: 'Even' }];
  const stopBitsOpts = ['1', '2'];
  const flowOpts = [{ l: 'None', v: 'None' }, { l: 'Soft', v: 'Software' }, { l: 'Hard', v: 'Hardware' }];

  // 波特率/停止位下拉直接绑 connectionParams(CustomSelect 只认 string,用函数绑定 get/set 转型),
  // 而不是本地 string state:设置里的 serial_defaults、MCP 接管/connection-state 回填写的
  // 都是 connectionParams.baudRate/stopBits,本地副本永远停在 115200/1——上次用的波特率
  // 既显示不出来也连不上,接管 9600 的连接后下拉还显 115200。
  function getBaudStr() { return String(connectionParams.baudRate); }
  function setBaudStr(v: string) {
    const n = Number(v);
    if (Number.isFinite(n) && n > 0) connectionParams.baudRate = n;
  }
  function getStopBitsStr() { return String(connectionParams.stopBits); }
  function setStopBitsStr(v: string) { connectionParams.stopBits = v === '2' ? 2 : 1; }

  // CustomSelect 需要 {label, value} 格式的选项
  const portOptions = $derived(availablePorts.value.map((p) => ({ label: p, value: p })));
  const baudOptions = $derived(baudRates.map((b) => ({ label: b, value: b })));
  const dataBitOptions = $derived(dataBitsOpts.map((o) => ({ label: o.l, value: o.v })));
  const parityOptions = $derived(parityOpts.map((o) => ({ label: o.l, value: o.v })));
  const stopBitOptions = $derived(stopBitsOpts.map((s) => ({ label: s, value: s })));
  const flowControlOptions = $derived(flowOpts.map((o) => ({ label: o.l, value: o.v })));

  function openBaudSettings() {
    settingsRequest.section = 'general';
  }

  async function refreshPorts() {
    try {
      const ports = await listPorts();
      availablePorts.value = ports;
      // 默认显示一个端口:优先保持当前选的(上次连的 last_port),不在可用列表则取第一个
      if (ports.length > 0) {
        if (!connectionParams.port || !ports.includes(connectionParams.port)) {
          connectionParams.port = ports[0]!;
        }
      }
    } catch (e) {
      console.error('获取端口列表失败:', e);
    }
  }

  async function handleConnect() {
    const baud = connectionParams.baudRate;
    // 波特率为空/非法时拒绝连接，避免 0 波特率下发到后端
    if (!Number.isFinite(baud) || baud <= 0) return;
    // 始终用用户在下拉框选的 port(窗口不绑 port,每次连接前重新选)。
    // windowPort 只用于"已连接后"定位 send/disconnect,连接前用 connectionParams.port。
    const targetPort = connectionParams.port;
    if (!targetPort) return;
    connErr = null;
    try {
      await connect({
        port: targetPort,
        baud_rate: baud,
        data_bits: connectionParams.dataBits,
        parity: connectionParams.parity,
        stop_bits: connectionParams.stopBits,
        flow_control: connectionParams.flowControl,
      });
    } catch (e) {
      console.error('[connect] 失败:', e);
      const msg = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
      showErr(msg);
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

<div class="layout-fixed relative flex items-end border-b px-5 py-2" data-theme-target="background-elevated" style="background: var(--background-elevated); border-color: var(--border);">
  <!-- 左侧：所有配置项分组（紧凑间距，左对齐紧凑排列，不拉伸） -->
  <div class="config-group" style="margin-right: 24px; gap: 12px;">
    <!-- 端口号(所有窗口都可选,连不同 port) -->
    <label class="control-group">
      <span>端口号</span>
      <CustomSelect bind:value={connectionParams.port} options={portOptions} width="100px" disabled={connected.value} />
    </label>

    <!-- 波特率 -->
    <label class="control-group">
      <span>波特率</span>
      <CustomSelect bind:value={getBaudStr, setBaudStr} options={baudOptions} width="96px" disabled={connected.value} onAddOption={openBaudSettings} />
    </label>

    <!-- 数据位 -->
    <label class="control-group">
      <span>数据位</span>
      <CustomSelect bind:value={connectionParams.dataBits} options={dataBitOptions} width="64px" disabled={connected.value} />
    </label>

    <!-- 校验位 -->
    <label class="control-group">
      <span>校验位</span>
      <CustomSelect bind:value={connectionParams.parity} options={parityOptions} width="80px" disabled={connected.value} />
    </label>

    <!-- 停止位 -->
    <label class="control-group">
      <span>停止位</span>
      <CustomSelect bind:value={getStopBitsStr, setStopBitsStr} options={stopBitOptions} width="60px" disabled={connected.value} />
    </label>

    <!-- 流控制 -->
    <label class="control-group">
      <span>流控制</span>
      <CustomSelect bind:value={connectionParams.flowControl} options={flowControlOptions} width="84px" disabled={connected.value} />
    </label>
  </div>

  <!-- 连接错误提示(端口已连接/被占用等):absolute 定位在连接按钮正下方,淡红底深红字 -->
  {#if connErr}
    <div class="absolute right-5 top-full mt-1 z-[200] px-3 py-1.5 rounded-md text-[12px] shadow-lg flex items-center gap-1.5 whitespace-nowrap"
         style="background: #fee2e2; color: #b91c1c;">
      <span>⚠</span>
      <span>{connErr}</span>
    </div>
  {/if}

  <!-- 右侧：连接/断开按钮（ml-auto 推至最右，与下拉框底部对齐） -->
  <div class="flex-shrink-0 ml-auto">
    {#if connected.value}
      <button class="btn btn-danger" style="width: 76px;" onclick={handleDisconnect}>断开</button>
    {:else}
      <button class="btn btn-primary" style="width: 76px;" onclick={handleConnect}>连接</button>
    {/if}
  </div>
</div>
