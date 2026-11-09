<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { autoConnection, availablePorts, connectionParams, connected, presetBaudRates, windowPort, settingsRequest } from '$lib/stores';
  import { autoConnectOnce, connect, disconnect, getAutoConnect, listPorts, onAutoConnectChanged, setAutoConnect } from '$lib/tauri';
  import type { AutoConnectBinding, ConnectionParams } from '$lib/types';
  import { normalizePorts, shouldPollPorts } from '$lib/connectionPorts';
  import { notifyError } from '$lib/notifications.svelte';
  import CustomSelect from '$components/ui/CustomSelect.svelte';
  import ConnectionButton from '$components/ui/ConnectionButton.svelte';
  import { recordFrontend } from '$lib/frontendDiagnostics';

  let manualBusy = $state(false);
  let autoChanging = $state(false);
  let autoSelected = $state(false);
  let disposed = false;
  let refreshing = false;
  let ignoreEnabled = false;
  let controlVersion = 0;
  let bindingRevision = 0;
  let controlQueue: Promise<unknown> = Promise.resolve();
  const controlsLocked = $derived(connected.value || autoConnection.binding !== null || manualBusy || autoChanging);
  let lastControlState = '';
  $effect(() => {
    const state = { locked: controlsLocked, connected: connected.value, auto_bound: autoConnection.binding !== null, manual_busy: manualBusy, auto_changing: autoChanging };
    const serialized = JSON.stringify(state);
    untrack(() => {
      if (serialized !== lastControlState) {
        lastControlState = serialized;
        recordFrontend('ports.controls', state, 'info', true);
      }
    });
  });
  const connectionTitle = $derived(autoConnection.binding
    ? `自动重连 ${autoConnection.binding.params.port}，点击断开并停止自动重连`
    : connected.value ? '断开串口' : autoSelected ? '连接串口并启用自动重连' : '连接串口');

  function queueControl<Result>(operation: () => Promise<Result>): Promise<Result> {
    const result = controlQueue.then(operation, operation);
    controlQueue = result.then(() => undefined, () => undefined);
    return result;
  }

  function captureParams(): ConnectionParams {
    return {
      port: connectionParams.port,
      baud_rate: connectionParams.baudRate,
      data_bits: connectionParams.dataBits,
      parity: connectionParams.parity,
      stop_bits: connectionParams.stopBits,
      flow_control: connectionParams.flowControl,
    };
  }

  function applyBinding(binding: AutoConnectBinding | null) {
    if (disposed || (ignoreEnabled && binding)) return;
    if (autoConnection.binding?.id !== binding?.id) autoConnection.connecting = false;
    autoConnection.binding = binding;
    autoConnection.error = '';
    if (binding) {
      autoSelected = true;
      const params = binding.params;
      connectionParams.port = params.port;
      connectionParams.baudRate = params.baud_rate;
      connectionParams.dataBits = params.data_bits;
      connectionParams.parity = params.parity;
      connectionParams.stopBits = params.stop_bits;
      connectionParams.flowControl = params.flow_control;
    }
  }

  async function attemptAutoConnect() {
    const binding = autoConnection.binding;
    if (!binding || disposed || connected.value || manualBusy || autoChanging || autoConnection.connecting
      || !availablePorts.value.some(port => port.port === binding.params.port)) return;
    autoConnection.connecting = true;
    try {
      await autoConnectOnce(binding.id);
      if (autoConnection.binding?.id === binding.id) autoConnection.error = '';
    } catch (error) {
      if (!disposed && autoConnection.binding?.id === binding.id) autoConnection.error = String(error);
    } finally {
      if (!disposed && autoConnection.binding?.id === binding.id) autoConnection.connecting = false;
    }
  }

  async function toggleAutoConnect(checked: boolean) {
    if (manualBusy || autoChanging) return;
    // Selection configures the next Connect; only an active binding may retry.
    autoSelected = checked;
    if (checked) return;
    const previous = autoConnection.binding;
    const version = ++controlVersion;
    const revision = bindingRevision;
    ignoreEnabled = true;
    if (!previous) return;
    applyBinding(null);
    autoChanging = true;
    try {
      const binding = await queueControl(() => setAutoConnect(null));
      if (!disposed && controlVersion === version && bindingRevision === revision) applyBinding(binding);
    } catch (error) {
      if (!disposed && controlVersion === version) {
        if (bindingRevision === revision) {
          ignoreEnabled = false;
          applyBinding(previous);
        }
        notifyError('自动重连设置失败', error);
      }
    } finally {
      if (!disposed && controlVersion === version) autoChanging = false;
    }
  }

  async function startAutoConnect(params: ConnectionParams) {
    const version = ++controlVersion;
    const revision = bindingRevision;
    ignoreEnabled = false;
    autoChanging = true;
    try {
      const binding = await queueControl(() => setAutoConnect(params));
      if (!disposed && controlVersion === version && bindingRevision === revision) applyBinding(binding);
    } catch (error) {
      if (!disposed && controlVersion === version) notifyError('自动重连启动失败', error);
    } finally {
      if (!disposed && controlVersion === version) autoChanging = false;
    }
  }

  onMount(() => {
    const listener = onAutoConnectChanged(binding => { bindingRevision++; applyBinding(binding); });
    listener.then(async () => {
      const before = bindingRevision;
      const binding = await getAutoConnect();
      if (!disposed && bindingRevision === before) applyBinding(binding);
    }).catch(error => console.error('读取自动重连状态失败:', error));
    return () => {
      disposed = true;
      listener.then(unlisten => unlisten()).catch(() => {});
      autoConnection.binding = null;
      autoConnection.connecting = false;
      autoConnection.error = '';
    };
  });

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

  // CustomSelect 需要 {label, value} 格式的选项;tip = 设备名,下拉里 hover 选项时弹出,
  // 选项本身仍只显示 COM 号,显示结构不变。
  const portOptions = $derived(availablePorts.value.map((p) => ({ label: p.port, value: p.port, tip: p.device })));
  const baudOptions = $derived(baudRates.map((b) => ({ label: b, value: b })));
  const dataBitOptions = $derived(dataBitsOpts.map((o) => ({ label: o.l, value: o.v })));
  const parityOptions = $derived(parityOpts.map((o) => ({ label: o.l, value: o.v })));
  const stopBitOptions = $derived(stopBitsOpts.map((s) => ({ label: s, value: s })));
  const flowControlOptions = $derived(flowOpts.map((o) => ({ label: o.l, value: o.v })));

  function openBaudSettings() {
    settingsRequest.section = 'general';
    // 通用页各节默认收起,不带这个就只是跳到页面、波特率那节还是关着
    settingsRequest.anchor = 'baud';
  }

  async function refreshPorts() {
    if (refreshing || disposed) return;
    refreshing = true;
    try {
      const ports = normalizePorts(await listPorts());
      if (disposed) return;
      availablePorts.value = ports;
      // 默认显示一个端口:优先保持当前选的(上次连的 last_port),不在可用列表则取第一个
      if (ports.length > 0 && !autoConnection.binding && !autoChanging && !connected.value && !manualBusy) {
        if (!connectionParams.port || !ports.some((p) => p.port === connectionParams.port)) {
          connectionParams.port = ports[0]!.port;
        }
      }
      void attemptAutoConnect();
    } catch (e) {
      console.error('获取端口列表失败:', e);
    } finally {
      refreshing = false;
    }
  }

  async function handleConnect() {
    if (manualBusy || autoChanging) return;
    const baud = connectionParams.baudRate;
    // 波特率为空/非法时拒绝连接，避免 0 波特率下发到后端
    if (!Number.isFinite(baud) || baud <= 0) return;
    // 始终用用户在下拉框选的 port(窗口不绑 port,每次连接前重新选)。
    // windowPort 只用于"已连接后"定位 send/disconnect,连接前用 connectionParams.port。
    const targetPort = connectionParams.port;
    if (!targetPort) return;
    if (autoSelected) {
      await startAutoConnect(captureParams());
      return;
    }
    ignoreEnabled = true;
    manualBusy = true;
    try {
      await queueControl(() => connect({
        port: targetPort,
        baud_rate: baud,
        data_bits: connectionParams.dataBits,
        parity: connectionParams.parity,
        stop_bits: connectionParams.stopBits,
        flow_control: connectionParams.flowControl,
      }));
    } catch (e) {
      console.error('[connect] 失败:', e);
      notifyError('连接失败', e);
    } finally {
      manualBusy = false;
    }
  }

  async function handleDisconnect() {
    if (manualBusy || autoChanging) return;
    const targetPort = windowPort.value ?? autoConnection.binding?.params.port ?? connectionParams.port;
    ++controlVersion;
    ignoreEnabled = true;
    applyBinding(null);
    manualBusy = true;
    try {
      if (targetPort) await queueControl(() => disconnect(targetPort));
    } catch (e) {
      console.error('断开失败:', e);
      notifyError('断开失败', e);
    } finally {
      manualBusy = false;
    }
  }

  $effect(() => {
    untrack(() => { void refreshPorts(); });
  });

  $effect(() => {
    if (autoConnection.binding && !connected.value && !autoChanging && !manualBusy) {
      untrack(() => { void refreshPorts(); });
    }
  });

  // 未连接时每 2 秒检测热插拔；已连接但启动时列表尚为空也继续补拉，避免 MCP
  // 先恢复连接后轮询停掉，COM 下拉永久无选项。
  $effect(() => {
    if (!shouldPollPorts(connected.value, availablePorts.value)) return;
    const timer = setInterval(refreshPorts, 2000);
    return () => clearInterval(timer);
  });
</script>

<div class="layout-fixed relative flex items-end border-b px-5 py-2" data-diagnostic-region="connection" data-theme-target="background-elevated" style="background: var(--background-elevated); border-color: var(--border);">
  <!-- 左侧：所有配置项分组（紧凑间距，左对齐紧凑排列，不拉伸） -->
  <div class="config-group" style="margin-right: 24px; gap: 12px;">
    <!-- 端口号(所有窗口都可选,连不同 port) -->
    <label class="control-group">
      <span>端口号</span>
      <CustomSelect diagnosticId="serial-port" bind:value={connectionParams.port} options={portOptions} width="100px" disabled={controlsLocked} emptyLabel="暂无可用端口" />
    </label>

    <!-- 波特率 -->
    <label class="control-group">
      <span>波特率</span>
      <CustomSelect diagnosticId="serial-baud" bind:value={getBaudStr, setBaudStr} options={baudOptions} width="96px" disabled={controlsLocked} onAddOption={openBaudSettings} />
    </label>

    <!-- 数据位 -->
    <label class="control-group">
      <span>数据位</span>
      <CustomSelect diagnosticId="serial-data-bits" bind:value={connectionParams.dataBits} options={dataBitOptions} width="64px" disabled={controlsLocked} />
    </label>

    <!-- 校验位 -->
    <label class="control-group">
      <span>校验位</span>
      <CustomSelect diagnosticId="serial-parity" bind:value={connectionParams.parity} options={parityOptions} width="80px" disabled={controlsLocked} />
    </label>

    <!-- 停止位 -->
    <label class="control-group">
      <span>停止位</span>
      <CustomSelect diagnosticId="serial-stop-bits" bind:value={getStopBitsStr, setStopBitsStr} options={stopBitOptions} width="60px" disabled={controlsLocked} />
    </label>

    <!-- 流控制 -->
    <label class="control-group">
      <span>流控制</span>
      <CustomSelect diagnosticId="serial-flow-control" bind:value={connectionParams.flowControl} options={flowControlOptions} width="84px" disabled={controlsLocked} />
    </label>
  </div>

  <!-- 右侧：连接/断开按钮（ml-auto 推至最右，与下拉框底部对齐） -->
  <div class="flex-shrink-0 ml-auto">
    <ConnectionButton
      connected={connected.value}
      autoEnabled={autoConnection.binding !== null}
      {autoSelected}
      busy={manualBusy || autoChanging}
      title={connectionTitle}
      onPrimary={() => connected.value || autoConnection.binding ? void handleDisconnect() : void handleConnect()}
      onAutoToggle={checked => { void toggleAutoConnect(checked); }}
    />
  </div>
</div>
