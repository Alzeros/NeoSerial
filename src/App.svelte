<script lang="ts">
  import { onMount } from 'svelte';
  import TitleBar from '$components/TitleBar.svelte';
  import ConnectionBar from '$components/ConnectionBar.svelte';
  import LogView from '$components/LogView.svelte';
  import BottomPanel from '$components/BottomPanel.svelte';
  import StatusBar from '$components/StatusBar.svelte';
  import ScriptSequencer from '$components/ScriptSequencer.svelte';
  import {
    appendLogLines,
    autoScroll,
    cachedSettings,
    connected,
    connectionParams,
    currentPort,
    displayMode,
    textEncoding,
    lineEnding,
    logLines,
    logSendContent,
    presetBaudRates,
    logFontSize,
    logLineHeight,
    logDirLabelStyle,
    logFontLatin,
    logFontCJK,
    applyLogFont,
    theme,
    applyTheme,
    rxBytes,
    scriptPanelOpen,
    scriptPanelWidth,
    scriptCurrentRow,
    scriptRunning,
    scriptRunCount,
    scriptRunState,
    showTimestamp,
    showLineIndex,
    txBytes,
    windowPort,
  } from '$lib/stores';
  import type { LogLine, Settings } from '$lib/types';
  import {
    getSettings,
    getWindowConnState,
    saveSettings,
    takePendingTakeover,
    onConnectionMode,
    onConnectionState,
    onError,
    onRxLines,
    onRxUpdate,
    onSequenceDone,
    onSequenceProgress,
    onTxLine,
    onTxUpdate,
  } from '$lib/tauri';
  import { connect as connectPort } from '$lib/tauri';

  async function handleRxLines(lines: LogLine[]) {
    // 自动滚动由 LogView 内部 $effect + requestAnimationFrame 处理
    // 这里只负责把数据塞进 logLines，无需再 tick + 读 scrollHeight
    appendLogLines(lines);
  }

  // 把加载到的 Settings 回填到各响应式 store
  function applySettings(s: Settings) {
    cachedSettings.value = s;
    connectionParams.port = s.last_port || connectionParams.port;
    connectionParams.baudRate = s.serial_defaults.baud_rate;
    connectionParams.dataBits = s.serial_defaults.data_bits;
    connectionParams.parity = s.serial_defaults.parity;
    connectionParams.stopBits = s.serial_defaults.stop_bits;
    connectionParams.flowControl = s.serial_defaults.flow_control;
    lineEnding.value = s.ui.line_ending;
    showTimestamp.value = s.ui.show_timestamp;
    showLineIndex.value = s.ui.show_line_index ?? false;
    autoScroll.value = s.ui.auto_scroll;
    displayMode.value = s.ui.display_mode === 'Hex' ? 'hex' : 'ascii';
    textEncoding.value = (s.ui?.text_encoding || 'Ascii') === 'Utf8' ? 'utf8' : (s.ui?.text_encoding || 'Ascii') === 'Gbk' ? 'gbk' : 'ascii';
    logSendContent.value = s.ui.log_send;
    // 日志区字体：兜底默认 14px / 1.6
    const fs = s.ui?.log_font_size ?? 14;
    const lh = s.ui?.log_line_height ?? 1.6;
    const fLatin = s.ui?.log_font_latin ?? 'default';
    const fCjk = s.ui?.log_font_cjk ?? 'default';
    logFontSize.value = fs;
    logLineHeight.value = lh;
    logFontLatin.value = fLatin;
    logFontCJK.value = fCjk;
    applyLogFont(fs, lh, fLatin, fCjk);
    logDirLabelStyle.value = (s.ui?.log_dir_label === 'full') ? 'full' : 'short';
    // 预设波特率：兜底为默认三项
    presetBaudRates.value =
      s.presets?.baud_rates?.length ? s.presets.baud_rates : [9600, 115200, 921600];
    // 主题：兜底为 preset-1，并应用到 <html>
    const tk = s.presets?.theme || 'preset-1';
    theme.value = tk;
    applyTheme(tk);
  }

  // 从当前 UI 状态构建可保存的 Settings（基于缓存，避免丢字段）
  function buildSettingsFromUi(): Settings | null {
    const base = cachedSettings.value;
    if (!base) return null;
    return {
      ...base,
      last_port: connectionParams.port,
      serial_defaults: {
        baud_rate: connectionParams.baudRate,
        data_bits: connectionParams.dataBits,
        parity: connectionParams.parity,
        stop_bits: connectionParams.stopBits,
        flow_control: connectionParams.flowControl,
      },
      ui: {
        ...base.ui,
        display_mode: displayMode.value === 'hex' ? 'Hex' : 'Ascii',
        text_encoding: textEncoding.value === 'utf8' ? 'Utf8' : textEncoding.value === 'gbk' ? 'Gbk' : 'Ascii',
        line_ending: lineEnding.value,
        auto_scroll: autoScroll.value,
        show_timestamp: showTimestamp.value,
        show_line_index: showLineIndex.value,
        log_send: logSendContent.value,
        log_font_size: logFontSize.value,
        log_line_height: logLineHeight.value,
        log_dir_label: logDirLabelStyle.value,
        log_font_latin: logFontLatin.value,
        log_font_cjk: logFontCJK.value,
      },
      presets: {
        baud_rates: presetBaudRates.value,
        theme: theme.value,
      },
    };
  }

  async function persistSettings() {
    const s = buildSettingsFromUi();
    if (!s) return;
    try {
      await saveSettings(s);
    } catch (e) {
      console.error('保存设置失败:', e);
    }
  }

  // "记录发送"开关变化时即时同步后端 state，让 writer 线程立刻按新值决定是否 emit tx-line。
  // 否则只有断开连接时 persistSettings 才同步，拨开关后日志区仍会显示 Tx。
  $effect(() => {
    const v = logSendContent.value;
    const base = cachedSettings.value;
    if (!base) return;
    // 仅更新 log_send 字段并同步后端内存 state + 落盘
    if (base.ui.log_send === v) return;
    base.ui.log_send = v;
    saveSettings(base).catch((e) => console.error('同步记录发送开关失败:', e));
  });

  // 行号开关:即时同步后端内存 state + 落盘(开关切换立即持久化,不丢设置)
  $effect(() => {
    const v = showLineIndex.value;
    const base = cachedSettings.value;
    if (!base) return;
    if (base.ui.show_line_index === v) return;
    base.ui.show_line_index = v;
    saveSettings(base).catch((e) => console.error('同步行号开关失败:', e));
  });

  let connectionMode = $state<{ mode: string | null }>({ mode: null });
  let showModeNotification = $state<{ value: boolean }>({ value: false });

  onMount(() => {
    // 所有窗口(main + 副窗口)都是完整串口界面。
    // 副窗口(win-{port})按 label 反推 port 存 windowPort;main 的 label="main" 后端返回 port=None,
    // windowPort 保持 null,等连接成功后从 connection-state 事件同步。
    // 若 MCP 已先连了该 port(副窗口场景),回填已连状态。
    getWindowConnState()
      .then((s) => {
        if (s.port) windowPort.value = s.port;
        connected.value = s.connected;
        if (s.connected) {
          currentPort.value = s.port;
          if (s.baud) connectionParams.baudRate = s.baud;
        }
      })
      .catch((e) => console.error('获取窗口连接状态失败:', e));

    // 若本窗口是被"快捷打开接管某 MCP 端口"创建的(openPortWindow(port) 记了 pending),
    // 取走后自动 connect 接管。用 pending 而非事件:窗口 JS 加载有先后,事件可能早于监听丢失。
    // 接管走 connect 的"已存在 mcp 连接"分支,只改 window_label 不重开串口,参数不实际用。
    takePendingTakeover()
      .then((t) => {
        if (t) {
          // 先把下拉框选成目标 port,避免接管后下拉框还显别的端口
          connectionParams.port = t.port;
          connectionParams.baudRate = t.baud;
          connectPort({
            port: t.port,
            baud_rate: t.baud,
            data_bits: connectionParams.dataBits,
            parity: connectionParams.parity,
            stop_bits: connectionParams.stopBits,
            flow_control: connectionParams.flowControl,
          }).catch((e) => console.error('自动接管失败:', e));
        }
      })
      .catch((e) => console.error('查询待接管失败:', e));

    // 启动时加载持久化设置
    getSettings()
      .then(applySettings)
      .catch((e) => console.error('加载设置失败:', e));

    const unlistenRxLine = onRxLines(handleRxLines);
    const unlistenTxLine = onTxLine((line) => handleRxLines([line]));
    const unlistenTx = onTxUpdate((u) => (txBytes.value = u.total));
    const unlistenRx = onRxUpdate((u) => (rxBytes.value = u.total));
    const unlistenState = onConnectionState((s) => {
      connected.value = s.connected;
      currentPort.value = s.port;
      // windowPort 跟随当前连接的 port:连接成功时锁定,供所有 invoke(send/disconnect/sequence)定位连接。
      // 断开时清空,避免下次连接用残留 windowPort 连错端口(用户会重新选 port)。
      if (s.connected && s.port) {
        windowPort.value = s.port;
      } else {
        windowPort.value = null;
      }
      // 连接成功时回填端口/波特率下拉框——MCP connect 走后端,顶部的 connectionParams
      // 不会自动更新,这里同步避免"连了 COM2 但下拉框还显 COM1"。
      if (s.connected) {
        if (s.port) connectionParams.port = s.port;
        if (s.baud_rate) connectionParams.baudRate = s.baud_rate;
      }
      // 断开时把当前 UI 设置落盘
      if (!s.connected) {
        persistSettings();
      }
    });
    const unlistenSeqDone = onSequenceDone((d) => {
      scriptRunning.value = false;
      scriptCurrentRow.value = -1;
      // 结束态：aborted=用户中断，否则完成
      scriptRunState.finished = d.aborted ? 'aborted' : 'done';
    });
    const unlistenSeqProgress = onSequenceProgress((p) => {
      scriptCurrentRow.value = p.row;
      // 每条实际发送（progress 仅在 enabled 行发送后触发）计数 +1
      scriptRunState.sent += 1;
      // 轮次：已发送数对单轮勾选数取整 +1（单轮 0 条时兜底 1）
      const perRound = scriptRunState.total > 0 && scriptRunCount.value > 0
        ? scriptRunState.total / scriptRunCount.value
        : 0;
      scriptRunState.round = perRound > 0
        ? Math.min(scriptRunCount.value, Math.floor((scriptRunState.sent - 1) / perRound) + 1)
        : 1;
    });
    const unlistenError = onError((e) => {
      console.error('[Serial Error]', e.message);
    });
    const unlistenMode = onConnectionMode((mode) => {
      connectionMode.mode = mode.mode;
      if (mode.mode === 'shared') {
        showModeNotification.value = true;
        setTimeout(() => {
          showModeNotification.value = false;
        }, 5000);
      }
    });

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    window.addEventListener('resize', handleResize);

    return () => {
      unlistenRxLine.then((f) => f());
      unlistenTxLine.then((f) => f());
      unlistenTx.then((f) => f());
      unlistenRx.then((f) => f());
      unlistenState.then((f) => f());
      unlistenSeqDone.then((f) => f());
      unlistenSeqProgress.then((f) => f());
      unlistenError.then((f) => f());
      unlistenMode.then((f) => f());
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
      window.removeEventListener('resize', handleResize);
    };
  });

  // 窗口缩小时若右栏超出合法范围（左栏已触底），自动收缩右栏，
  // 避免输入框卡在拉宽后的长度、需拖分隔条才刷新。
  function handleResize() {
    if (!leftPanel) return;
    const containerWidth = leftPanel.parentElement?.clientWidth ?? 1200;
    const maxByLeftFloor = containerWidth - 700 - 1;
    if (scriptPanelWidth.value > maxByLeftFloor) {
      scriptPanelWidth.value = Math.max(400, maxByLeftFloor);
    }
  }

  let isDragging = false;
  let leftPanel: HTMLDivElement;

  function handleMouseDown() {
    isDragging = true;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isDragging || !leftPanel) return;
    const containerWidth = leftPanel.parentElement?.clientWidth ?? 1200;
    const newWidth = containerWidth - e.clientX;
    // 左栏有 700px 下限：右栏最大不能超过 容器宽 - 左栏下限 - 分隔条(1)，
    // 否则左栏触底后继续拖会把右栏撑大（总宽固定时左栏已不能缩，右栏变大无意义）。
    const maxByLeftFloor = containerWidth - 700 - 1;
    scriptPanelWidth.value = Math.max(400, Math.min(600, newWidth, maxByLeftFloor));
  }

  function handleMouseUp() {
    isDragging = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  }
</script>

{#if showModeNotification.value && connectionMode.mode === 'shared'}
  <div class="fixed top-3 left-1/2 -translate-x-1/2 z-50 px-4 py-2 rounded-md text-[13px] font-medium shadow-lg flex items-center gap-2"
       style="background: var(--warning); color: var(--primary-foreground);">
    <span>⚠️</span>
    <span>串口克隆失败，已降级为共享端口模式（性能受限）</span>
    <button
      class="ml-2 opacity-70 hover:opacity-100 cursor-pointer"
      onclick={() => (showModeNotification.value = false)}
    >×</button>
  </div>
{/if}

<div class="flex h-screen w-screen flex-col overflow-hidden">
  <!-- 自定义标题栏（含脚本折叠按钮 + 窗口控制） -->
  <TitleBar />

  <!-- 主体区域：左栏 + 分隔条 + 右栏 -->
  <div class="flex flex-1 min-h-0 overflow-hidden">
    <!-- 左侧主区域：flex:1 占剩余空间；min-width 700px 锁定配置区一行 + 底部开关行不换行，
         右栏朝左拖时优先压缩右侧、到左侧下限即停，保证排版一致 -->
    <div bind:this={leftPanel} class="flex flex-col" style="flex: 1 1 0%; min-width: 700px; min-height: 0;">
    <!-- 1. 会话配置区（顶部，固定不收缩） -->
    <div class="layout-fixed">
      <ConnectionBar />
    </div>
    <!-- 2. 数据显示区（中部，内部独立滚动） -->
    <div class="flex flex-col min-h-0" style="flex: 1 1 0%;">
      <LogView />
    </div>
    <!-- 3. 底部功能区（固定不收缩） -->
    <div class="layout-fixed">
      <BottomPanel />
    </div>
    <!-- 状态栏（固定不收缩，z-index 防止被中间长文本遮挡） -->
    <div class="layout-fixed" style="position: relative; z-index: 10;">
      <StatusBar />
    </div>
  </div>

  <!-- 分隔条 + 右侧脚本面板 -->
  {#if scriptPanelOpen.value}
    <div
      role="separator"
      aria-orientation="vertical"
      tabindex="0"
      class="w-1 cursor-col-resize shrink-0 transition-colors"
      style="background: var(--border);"
      onmousedown={handleMouseDown}
    ></div>
    <!-- 右栏：flex 不收缩，宽度由拖拽控制；min-width 覆盖全部表格列防截断。
         窗口缩小时由左栏(flex:1)先吃压缩，右栏保持自身宽度直至触底。 -->
    <div class="flex flex-col" style="width: {scriptPanelWidth.value}px; min-width: 500px; max-width: 600px; flex: 0 0 auto; min-height: 0; height: 100%; overflow: hidden;">
      <ScriptSequencer />
    </div>
  {/if}
  </div>
</div>
