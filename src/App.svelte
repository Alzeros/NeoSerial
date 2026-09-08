<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import TitleBar from '$components/TitleBar.svelte';
  import ConnectionBar from '$components/ConnectionBar.svelte';
  import LogView from '$components/LogView.svelte';
  import BottomPanel from '$components/BottomPanel.svelte';
  import StatusBar from '$components/StatusBar.svelte';
  import ScriptSequencer from '$components/ScriptSequencer.svelte';
  import ThemeEditor from '$components/ThemeEditor.svelte';
  import {
    appendLogLines,
    insertLogLines,
    applySharedSettings,
    cachedSettings,
    connected,
    connectionParams,
    currentPort,
    displayMode,
    fileSendProgress,
    lineEnding,
    logLines,
    logSendContent,
    theme,
    customTheme,
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
  import { normalizeCustomTheme } from '$lib/customTheme';
  import { initCommandIndex } from '$lib/commandIndex';
  import { loadSettingsOnce } from '$lib/startup';
  import {
    getSettings,
    getWindowConnState,
    getWindowHistory,
    patchSettings,
    takePendingTakeover,
    onConnectionMode,
    onConnectionState,
    onError,
    onRxLines,
    onRxUpdate,
    onSequenceDone,
    onSequenceProgress,
    onSettingsChanged,
    onCloseGuard,
    resolveLastClose,
    onTxLine,
    onTxUpdate,
    onThemeChanged,
    onThemePreview,
    onThemeHighlight,
  } from '$lib/tauri';
  import { connect as connectPort } from '$lib/tauri';

  async function handleRxLines(lines: LogLine[]) {
    // 自动滚动由 LogView 内部 $effect + requestAnimationFrame 处理
    // 这里只负责把数据塞进 logLines，无需再 tick + 读 scrollHeight
    // 回填历史后 reader 再发来的批次里若含历史已覆盖的行(line_index <= backfillMaxIndex),丢弃
    const fresh = backfillMaxIndex > 0
      ? lines.filter((l) => !(l.line_index > 0 && l.line_index <= backfillMaxIndex))
      : lines;
    appendLogLines(fresh);
  }

  // ============ 挂上已有连接时回填历史 ============
  // 后台连接(agent 建的 / 窗口关掉留下的)被本窗口挂上时,把后端 rx_history 灌进日志区,
  // 让"重新打开"名副其实。新建的连接历史为空,调用无害。
  // 去重靠 line_index(每个连接内单调唯一,tx+rx 共用):
  //  - insertAt:收到 connected 时记下的位置,历史插在这里,之后到的实时行接在后面;
  //  - 接管瞬间 reader 可能已往新 label 发了一批 → 插入前过滤掉 insertAt 之后已有的 index;
  //  - 历史拉回后 reader 再发的批次里含 <= backfillMaxIndex 的行 → 历史里已有,handleRxLines 丢弃。
  let backfillMaxIndex = 0;
  async function backfillHistory(port: string, insertAt: number) {
    try {
      const hist = await getWindowHistory(port);
      if (hist.length > 0) {
        const at = Math.min(insertAt, logLines.length);
        const seen = new Set(logLines.slice(at).map((l) => l.line_index));
        insertLogLines(at, hist.filter((l) => !seen.has(l.line_index)));
        for (const l of hist) {
          if (l.line_index > backfillMaxIndex) backfillMaxIndex = l.line_index;
        }
      }
      // 收发字节数也从后端同步,不然要等下一次收发事件才从 0 跳到真实值
      const st = await getWindowConnState();
      if (st.connected && st.port === port) {
        if (st.tx_bytes != null) txBytes.value = st.tx_bytes;
        if (st.rx_bytes != null) rxBytes.value = st.rx_bytes;
      }
    } catch (e) {
      console.error('回填连接历史失败:', e);
    }
  }

  // 主题编辑器窗口(label=theme-editor)只渲染 ThemeEditor,不跑串口逻辑
  const isThemeEditorWindow = getCurrentWebview().label === 'theme-editor';

  // ============ 主界面开关的持久化 ============
  // 主界面上直接拨的开关(HEX 显示/时间戳/行号/回车换行/记录发送)改了就回写,防抖 200ms:
  // 原先只在"断开连接"时整份落盘,关窗口、托盘退出都不经过它,后台模式下用户几乎不点断开,
  // 这些设置等于永不保存;而"记录发送"/"时间戳"后端还要拿来决定文件日志格式,拨了得立刻同步。
  // 只回写**本窗口自己改过**的字段(与上次回写的快照逐项比对):多个窗口共用一份 settings,
  // 整份覆盖会把别的窗口刚改的字段冲回旧值,只写差异就互不干扰(同一字段两边都改,后改的赢)。
  type OwnedToggles = {
    display_mode: 'Hex' | 'Ascii';
    line_ending: Settings['ui']['line_ending'];
    show_timestamp: boolean;
    show_line_index: boolean;
    log_send: boolean;
  };
  function ownedToggles(): OwnedToggles {
    return {
      display_mode: displayMode.value === 'hex' ? 'Hex' : 'Ascii',
      line_ending: lineEnding.value,
      show_timestamp: showTimestamp.value,
      show_line_index: showLineIndex.value,
      log_send: logSendContent.value,
    };
  }
  // 上次回写(或从设置同步进来)时各开关的值;null = 设置还没加载,先不比
  let lastPersistedToggles: OwnedToggles | null = null;
  let toggleFlushTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    if (isThemeEditorWindow) return;
    const now = ownedToggles();
    // 只依赖开关 store 本身:cachedSettings 变(别的窗口保存)不该触发回写,不然两个窗口来回覆盖
    untrack(() => {
      if (!lastPersistedToggles) {
        if (cachedSettings.value) lastPersistedToggles = now;
        return;
      }
      const changed = (Object.keys(now) as (keyof OwnedToggles)[]).filter((k) => now[k] !== lastPersistedToggles![k]);
      if (changed.length === 0) return;
      if (toggleFlushTimer) clearTimeout(toggleFlushTimer);
      toggleFlushTimer = setTimeout(() => {
        toggleFlushTimer = null;
        const cur = ownedToggles();
        const base = lastPersistedToggles!;
        const ui: Partial<OwnedToggles> = {};
        for (const k of Object.keys(cur) as (keyof OwnedToggles)[]) {
          if (cur[k] !== base[k]) (ui as Record<string, unknown>)[k] = cur[k];
        }
        if (Object.keys(ui).length === 0) return;
        lastPersistedToggles = cur;
        patchSettings({ ui }).then((s) => {
          cachedSettings.value = s;
        }).catch((e) => console.error('保存界面开关失败:', e));
      }, 200);
    });
  });

  /** settings-changed 到达时同步三个全局开关(时间戳/行号/记录发送:文件日志格式是全局的,
   *  窗口间必须一致)。只跟"与本窗口上次回写值不同"的字段——相同的是本窗口自己那次回写的回声,
   *  跟了会把用户在这 200ms 里刚拨回去的新值又翻回来;跟进来的值同时记进快照,不然下次比对
   *  会把它当成本窗口的改动再写一遍。 */
  function syncGlobalToggles(s: Settings) {
    if (!lastPersistedToggles) return;
    const incoming = {
      show_timestamp: s.ui.show_timestamp,
      show_line_index: s.ui.show_line_index ?? false,
      log_send: s.ui.log_send,
    };
    if (incoming.show_timestamp !== lastPersistedToggles.show_timestamp) showTimestamp.value = incoming.show_timestamp;
    if (incoming.show_line_index !== lastPersistedToggles.show_line_index) showLineIndex.value = incoming.show_line_index;
    if (incoming.log_send !== lastPersistedToggles.log_send) logSendContent.value = incoming.log_send;
    lastPersistedToggles = { ...lastPersistedToggles, ...incoming };
  }

  /** 连接成功时把端口与串口参数记为下次启动的默认值。只在连接成功时记(不在下拉框每次变化时):
   *  端口轮询会在模组还没枚举出来时把下拉框临时换成别的口,那不是用户的选择,不该被记下来。 */
  function persistConnectionDefaults(port: string, baud: number) {
    patchSettings({
      last_port: port,
      serial_defaults: {
        baud_rate: baud,
        data_bits: connectionParams.dataBits,
        parity: connectionParams.parity,
        stop_bits: connectionParams.stopBits === 2 ? 'Two' : 'One',
        flow_control: connectionParams.flowControl,
      },
    }).then((s) => {
      cachedSettings.value = s;
    }).catch((e) => console.error('保存连接参数失败:', e));
  }

  let connectionMode = $state<{ mode: string | null }>({ mode: null });
  let showModeNotification = $state<{ value: boolean }>({ value: false });

  // 轻量模式下关最后一个窗口、agent 建的连接还活着:退出会掐断 agent,后端拦下关窗
  // 发 close-guard 过来,这里弹确认。三选一:取消 / 开启后台运行并收起 / 仍然退出。
  let closeGuard = $state<{ open: boolean; ports: string[] }>({ open: false, ports: [] });
  async function handleCloseGuard(action: 'exit' | 'background') {
    closeGuard.open = false;
    try {
      await resolveLastClose(action);
    } catch (e) {
      console.error('处理关闭确认失败:', e);
    }
  }

  onMount(() => {
    // 主题编辑器窗口:不加载串口连接等设置,ThemeEditor 组件自行加载主题
    if (isThemeEditorWindow) return;

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

    // 持久化设置:main.ts 在挂载前已发起加载并回填 store(首帧就是按它画的),单飞,这里只是兜底。
    // 设置到位后记下开关快照,之后只回写相对它的改动(预取超时、设置晚到的情况也能接上)
    loadSettingsOnce().then(() => {
      if (!lastPersistedToggles && cachedSettings.value) lastPersistedToggles = ownedToggles();
    });

    // 指令联想:订阅手册索引缓存 / 发送历史的变化广播,并载入初值
    const cleanupCommandIndex = initCommandIndex();

    const unlistenRxLine = onRxLines(handleRxLines);
    const unlistenTxLine = onTxLine((line) => handleRxLines([line]));
    const unlistenTx = onTxUpdate((u) => (txBytes.value = u.total));
    const unlistenRx = onRxUpdate((u) => (rxBytes.value = u.total));
    const unlistenState = onConnectionState((s) => {
      connected.value = s.connected;
      currentPort.value = s.port;
      // 每次连接成功都尝试回填(挂上已有连接才有内容);断开时清掉去重水位
      backfillMaxIndex = 0;
      if (s.connected && s.port) {
        backfillHistory(s.port, logLines.length);
      }
      // windowPort 跟随当前连接的 port:连接成功时锁定,供所有 invoke(send/disconnect/sequence)定位连接。
      // 断开时清空,避免下次连接用残留 windowPort 连错端口(用户会重新选 port)。
      if (s.connected && s.port) {
        windowPort.value = s.port;
      } else {
        windowPort.value = null;
      }
      // 连接成功时回填端口/波特率下拉框——MCP connect 走后端,顶部的 connectionParams
      // 不会自动更新,这里同步避免"连了 COM2 但下拉框还显 COM1";并记为下次启动的默认值。
      if (s.connected) {
        if (s.port) connectionParams.port = s.port;
        if (s.baud_rate) connectionParams.baudRate = s.baud_rate;
        if (s.port) persistConnectionDefaults(s.port, s.baud_rate ?? connectionParams.baudRate);
      }
      // 断开时文件发送进度随连接作废(中途断开的那次不会再有结果)
      if (!s.connected) {
        fileSendProgress.value = 0;
        // 正在跑的序列随连接一起没了。后端被动断开时也会发 sequence-done{aborted},
        // 这里不等它:窗口不该有"未连接却还在运行"的状态,停止按钮此时也已无处可停。
        if (scriptRunning.value) {
          scriptRunning.value = false;
          scriptCurrentRow.value = -1;
          scriptRunState.finished = 'aborted';
        }
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
    // 主题变更:主题编辑器保存后广播,本窗口重新加载主题设置
    const unlistenTheme = onThemeChanged(() => {
      getSettings()
        .then((s) => {
          customTheme.value = normalizeCustomTheme(s.presets?.custom_theme);
          theme.value = s.presets?.theme || 'preset-1';
          applyTheme(theme.value, customTheme.value);
        })
        .catch((e) => console.error('重载主题失败:', e));
    });
    // 主题编辑器实时预览：改色时广播到主窗口，直接 applyTheme 预览（不改 store）
    const unlistenPreview = onThemePreview((data) => {
      if (data.custom) {
        applyTheme('custom', data.custom);
      } else {
        // custom 为 null：编辑器关闭，从 settings 重载已保存的主题
        getSettings()
          .then((s) => {
            customTheme.value = normalizeCustomTheme(s.presets?.custom_theme);
            theme.value = s.presets?.theme || 'preset-1';
            applyTheme(theme.value, customTheme.value);
          })
          .catch((e) => console.error('重载主题失败:', e));
      }
    });
    // 主题编辑器悬停高亮：主窗口给用到该色的元素加虚线框
    const unlistenHighlight = onThemeHighlight((data) => {
      if (data.field) {
        document.documentElement.dataset.hl = data.field;
      } else {
        delete document.documentElement.dataset.hl;
      }
    });

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    window.addEventListener('resize', handleResize);

    const unlistenGuard = onCloseGuard((g) => {
      closeGuard = { open: true, ports: g.ports };
    });

    // 任一窗口/主题编辑器/agent 保存了设置 → 本窗口跟上所有共享项(主题、字体、编码、预设波特率、
    // 三个全局开关),只有端口/波特率下拉、HEX 显示、回车换行这些"本窗口自己的"不动。
    const unlistenSettings = onSettingsChanged(() => {
      getSettings()
        .then((s) => {
          applySharedSettings(s);
          syncGlobalToggles(s);
        })
        .catch((e) => console.error('刷新设置快照失败:', e));
    });

    return () => {
      cleanupCommandIndex();
      unlistenGuard.then((f) => f());
      unlistenSettings.then((f) => f());
      unlistenRxLine.then((f) => f());
      unlistenTxLine.then((f) => f());
      unlistenTx.then((f) => f());
      unlistenRx.then((f) => f());
      unlistenState.then((f) => f());
      unlistenSeqDone.then((f) => f());
      unlistenSeqProgress.then((f) => f());
      unlistenError.then((f) => f());
      unlistenMode.then((f) => f());
      unlistenTheme.then((f) => f());
      unlistenPreview.then((f) => f());
      unlistenHighlight.then((f) => f());
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

  // 全局右键菜单：输入框/文本域 → 自定义菜单（复制/剪切/粘贴/全选）；
  // 日志区有自己的 oncontextmenu（stopPropagation 不走到这里）；其余区域 → 禁用。
  let inputMenu = $state<{
    x: number; y: number; show: boolean;
    el: HTMLInputElement | HTMLTextAreaElement | null;
    hasSelection: boolean; canCut: boolean;
    selStart: number; selEnd: number;
  }>({ x: 0, y: 0, show: false, el: null, hasSelection: false, canCut: false, selStart: 0, selEnd: 0 });

  function handleContextMenu(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') {
      const el = target as HTMLInputElement | HTMLTextAreaElement;
      e.preventDefault();
      if (el.disabled) return;
      const start = el.selectionStart ?? 0;
      const end = el.selectionEnd ?? 0;
      const hasSel = start !== end;
      inputMenu.el = el;
      inputMenu.selStart = start;
      inputMenu.selEnd = end;
      inputMenu.hasSelection = hasSel;
      inputMenu.canCut = hasSel && !el.readOnly;
      // 边界检测：靠近底部翻到上方，靠近右边左移
      const MW = 120, MH = 140;
      const vw = window.innerWidth, vh = window.innerHeight;
      inputMenu.x = e.clientX + MW > vw ? Math.max(4, vw - MW - 4) : e.clientX;
      inputMenu.y = e.clientY + MH > vh ? Math.max(4, e.clientY - MH) : e.clientY;
      inputMenu.show = true;
      return;
    }
    e.preventDefault();
  }

  function closeInputMenu() {
    inputMenu.show = false;
  }

  async function inputCopy() {
    const el = inputMenu.el;
    if (!el) return;
    const text = el.value.substring(inputMenu.selStart, inputMenu.selEnd);
    try { await navigator.clipboard.writeText(text); } catch { /* 剪贴板不可用 */ }
    closeInputMenu();
  }

  async function inputCut() {
    const el = inputMenu.el;
    if (!el) return;
    const start = inputMenu.selStart;
    const end = inputMenu.selEnd;
    const text = el.value.substring(start, end);
    try { await navigator.clipboard.writeText(text); } catch { /* 剪贴板不可用 */ }
    el.focus();
    el.setSelectionRange(start, end);
    document.execCommand('insertText', false, '');
    closeInputMenu();
  }

  async function inputPaste() {
    const el = inputMenu.el;
    if (!el) return;
    el.focus();
    try {
      const text = await navigator.clipboard.readText();
      document.execCommand('insertText', false, text);
    } catch { /* 剪贴板权限拒绝 */ }
    closeInputMenu();
  }

  function inputSelectAll() {
    const el = inputMenu.el;
    if (!el) return;
    el.focus();
    el.select();
    closeInputMenu();
  }

  /** Ctrl+A 的选中范围:只圈"数据/正文"那一块,不碰整个界面。
   *  webview 默认是全选文档:app.css 已把标签/按钮/开关那些字设成不可选(选了也没用),
   *  但默认全选仍会把所有可选块(日志区 + MCP 记录 + 指令详情)一起点亮,同时还会把
   *  空输入框的占位文字刷蓝,看着像"整个软件被选中"。
   *  规则:
   *   - 焦点在输入框/文本域 → 不插手,浏览器默认就是"只选自己";
   *   - 焦点落在某个 .select-text 块里(日志区、MCP 记录、指令详情,这些容器带
   *     tabindex="-1",点一下即可聚焦)→ 只选那一块;
   *   - 焦点不在任何可选块(刚启动、点在空白处)→ 清掉选区,什么都不选。 */
  function handleSelectAll(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || (e.key !== 'a' && e.key !== 'A')) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    e.preventDefault();
    const sel = window.getSelection();
    if (!sel) return;
    sel.removeAllRanges();
    const scope = (document.activeElement as HTMLElement | null)?.closest('.select-text');
    if (!scope) return;
    const range = document.createRange();
    range.selectNodeContents(scope);
    sel.addRange(range);
  }

  $effect(() => {
    if (!inputMenu.show) return;
    const close = () => { inputMenu.show = false; };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') inputMenu.show = false; };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', onKey);
    };
  });
</script>

<!-- Ctrl+A 全局兜底:限定选中范围,见 handleSelectAll -->
<svelte:window onkeydown={handleSelectAll} />

{#if isThemeEditorWindow}
  <ThemeEditor />
{:else}
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

<div class="flex h-screen w-screen flex-col overflow-hidden" data-theme-target="background" oncontextmenu={handleContextMenu}>
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

  {#if inputMenu.show}
    <div
      class="fixed z-[200] py-1 rounded-md border shadow-lg min-w-[100px]"
      style="background: var(--background-elevated); border-color: var(--border); left: {inputMenu.x}px; top: {inputMenu.y}px;"
      onmousedown={(e) => e.stopPropagation()}
    >
      <button
        class="block w-full text-left px-3 py-1.5 text-[13px] transition-colors cursor-pointer {inputMenu.hasSelection ? 'hover:bg-[var(--border-subtle)] text-[var(--foreground)]' : 'text-[var(--muted-foreground)] opacity-40 cursor-default'}"
        onclick={inputCopy}
      >复制</button>
      <button
        class="block w-full text-left px-3 py-1.5 text-[13px] transition-colors cursor-pointer {inputMenu.canCut ? 'hover:bg-[var(--border-subtle)] text-[var(--foreground)]' : 'text-[var(--muted-foreground)] opacity-40 cursor-default'}"
        onclick={inputCut}
      >剪切</button>
      <button
        class="block w-full text-left px-3 py-1.5 text-[13px] hover:bg-[var(--border-subtle)] text-[var(--foreground)] transition-colors cursor-pointer"
        onclick={inputPaste}
      >粘贴</button>
      <button
        class="block w-full text-left px-3 py-1.5 text-[13px] hover:bg-[var(--border-subtle)] text-[var(--foreground)] transition-colors cursor-pointer"
        onclick={inputSelectAll}
      >全选</button>
    </div>
  {/if}

  <!-- 关闭确认:仅轻量模式关最后一个窗口且 agent 还连着时出现(后端 close-guard 触发) -->
  {#if closeGuard.open}
    <div
      class="fixed inset-0 z-[100] flex items-center justify-center"
      style="background: rgba(0,0,0,0.35);"
      onclick={() => (closeGuard.open = false)}
      onkeydown={(e) => { if (e.key === 'Escape') closeGuard.open = false; }}
      role="presentation"
    >
      <div
        class="rounded-lg shadow-xl w-[400px] border"
        style="background: var(--background-elevated); border-color: var(--border);"
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="close-guard-title"
        tabindex="-1"
      >
        <div class="px-6 py-5">
          <div id="close-guard-title" class="text-[14px] font-medium text-[var(--foreground)] mb-2">
            agent 正在使用 {closeGuard.ports.join('、')}
          </div>
          <div class="text-[13px] text-[var(--muted-foreground)] leading-relaxed">
            这是最后一个窗口。退出会断开 agent 的连接并停止 MCP 服务；<br />
            开启"后台运行"则关窗后连接和 MCP 继续运行，应用留在系统托盘。
          </div>
        </div>
        <div class="flex justify-end gap-2 px-4 pb-4">
          <button class="btn btn-ghost" style="padding: 6px 14px;" onclick={() => (closeGuard.open = false)}>取消</button>
          <button class="btn btn-secondary" style="padding: 6px 14px;" onclick={() => handleCloseGuard('exit')}>仍然退出</button>
          <button class="btn btn-primary" style="padding: 6px 14px;" onclick={() => handleCloseGuard('background')}>开启后台运行并收起</button>
        </div>
      </div>
    </div>
  {/if}
</div>
{/if}
