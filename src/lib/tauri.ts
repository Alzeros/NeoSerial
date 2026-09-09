import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open, save } from '@tauri-apps/plugin-dialog';
import type {
  CommandIndexCache,
  CommandIndexRefreshDocResult,
  CommandIndexRefreshResult,
  ConnectionMode,
  ConnectionParams,
  ConnectionState,
  ErrorEvent,
  LogLine,
  RxUpdate,
  ScriptCommand,
  ScriptModule,
  ScriptPage,
  SequenceDone,
  SequenceProgress,
  Settings,
  SettingsPatch,
  TxUpdate,
  WindowConnState,
} from './types';
import { defaultScriptModule } from './types';

// ============ 连接管理 ============

export async function connect(params: ConnectionParams): Promise<void> {
  await invoke('connect', {
    port: params.port,
    baudRate: params.baud_rate,
    dataBits: params.data_bits,
    parity: params.parity,
    stopBits: params.stop_bits,
    flowControl: params.flow_control,
  });
}

export async function disconnect(port: string): Promise<void> {
  await invoke('disconnect', { port });
}

export async function listPorts(): Promise<string[]> {
  return await invoke<string[]>('list_ports');
}

export async function resetStats(port: string): Promise<void> {
  await invoke('reset_stats', { port });
}

/** 开一个新串口窗口(完整界面的复制品)。
 *  - 不传 port:空白未连接窗口,用户进去自己选端口连接。
 *  - 传 port:开窗后发 auto-connect-port 全局事件带 {label,port,baud},
 *    新窗口 onMount 匹配自己的 label 后自动 connect 接管该 MCP 连接。 */
export async function openPortWindow(port?: string, baud?: number): Promise<void> {
  await invoke('open_port_window', { port: port ?? null, baud: baud ?? null });
}

/** 打开自定义主题编辑器窗口(单例,已存在则聚焦) */
export async function openThemeEditor(): Promise<void> {
  await invoke('open_theme_editor');
}

/** 窗口 onMount 调:查归属本窗口的连接状态(dev 重载后恢复 UI 用)。 */
export async function getWindowConnState(): Promise<WindowConnState> {
  return await invoke<WindowConnState>('get_window_conn_state');
}

/** 本窗口挂上一个已有连接(agent 建的 / 后台留下的)后,拉该连接的收发历史回填日志区。
 *  新建的连接历史为空。去重见 App.svelte 的 backfillHistory。 */
export async function getWindowHistory(port: string): Promise<LogLine[]> {
  return await invoke<LogLine[]>('get_window_history', { port });
}

/** 没有窗口显示的连接列表:agent 建的、或后台模式下窗口关掉留在后台的(window_label 为 mcp-*)。 */
export interface McpOnlyConn {
  port: string;
  baud: number;
}
export async function getMcpOnlyConnections(): Promise<McpOnlyConn[]> {
  return await invoke<McpOnlyConn[]>('get_mcp_only_connections');
}

/** 新窗口 onMount 调:取走"本窗口被要求自动接管的端口"(openPortWindow(port) 时记的)。
 *  返回 null 表示无待接管(普通开窗)。取走即删(一次性)。 */
export async function takePendingTakeover(): Promise<McpOnlyConn | null> {
  return await invoke<McpOnlyConn | null>('take_pending_takeover');
}

/** 用系统默认浏览器打开 URL(关于页 GitHub 链接)。 */
export async function openUrl(url: string): Promise<void> {
  await invoke('open_url', { url });
}

// ============ MCP ============

export interface McpStatus {
  running: boolean;
  port: number | null;
}

/** 查询 MCP server 运行状态(是否在跑 + 实际端口)。供设置页显示连接指令。 */
export async function getMcpStatus(): Promise<McpStatus> {
  return await invoke<McpStatus>('get_mcp_status');
}

// ============ 数据收发 ============

export async function send(port: string, text: string, ending: string, isHex: boolean): Promise<number> {
  return await invoke<number>('send', { port, text, ending, isHex });
}

export async function sendFile(port: string, path: string): Promise<number> {
  return await invoke<number>('send_file', { port, path });
}

// ============ 配置管理 ============

export async function getSettings(): Promise<Settings> {
  return await invoke<Settings>('get_settings');
}

export async function saveSettings(settings: Settings): Promise<void> {
  await invoke('save_settings', { settings });
}

/** 局部更新设置:只传自己改动的字段,后端以内存态为底深合并、落盘、广播 settings-changed,
 *  返回合并后的完整 Settings(调用方据此刷新 cachedSettings)。窗口/主题编辑器都走这条,
 *  不再整份回写——持旧快照整份保存会把别处刚改的字段冲回去。值无效时 reject,后端不动。 */
export async function patchSettings(patch: SettingsPatch): Promise<Settings> {
  return await invoke<Settings>('patch_settings', { patch });
}

export async function saveCommands(groups: Settings['command_groups']): Promise<void> {
  await invoke('save_commands', { groups });
}

/** 导出自定义主题 JSON 到指定路径 */
export async function exportThemeFile(path: string, data: unknown): Promise<void> {
  await invoke('export_theme_file', { path, data });
}

/** 从指定路径读入主题 JSON（字段校验在前端 parseThemeFile 做） */
export async function importThemeFile(path: string): Promise<unknown> {
  return await invoke('import_theme_file', { path });
}

// ============ 文件日志 ============

export async function startLogging(path?: string): Promise<string> {
  return await invoke<string>('start_logging', { path });
}

export async function stopLogging(): Promise<void> {
  await invoke('stop_logging');
}

/** 文件日志的全局状态(logger 是进程级单份)。path:记录中是正在写的文件,停止后是上次路径。 */
export interface LoggingStatus {
  active: boolean;
  path: string | null;
  /** 写盘失败原因,只在失败广播里带;此时 logger 已被后端摘掉,active 必为 false */
  error: string | null;
}

export async function getLoggingStatus(): Promise<LoggingStatus> {
  return await invoke<LoggingStatus>('get_logging_status');
}

/** logging-changed 全局事件:任一窗口/agent 开始或停止了记录、存盘线程写失败。各窗口据此刷新按钮与路径。 */
export function onLoggingChanged(cb: (status: LoggingStatus) => void) {
  return getCurrentWebview().listen<LoggingStatus>('logging-changed', (e) => cb(e.payload));
}

// ============ 脚本序列 ============

export async function sequenceRun(
  port: string,
  commands: ScriptCommand[],
  runCount: number,
  loopInterval: number,
): Promise<void> {
  await invoke('sequence_run', { port, commands, runCount, loopInterval });
}

export async function sequenceStop(port: string): Promise<void> {
  await invoke('sequence_stop', { port });
}

export async function saveSequenceConfig(path: string, data: ScriptModule[]): Promise<void> {
  await invoke('save_sequence_config', { path, data });
}

/** 加载序列配置。支持 JSON(本工具导出)与 INI(旧串口工具导出,自动转换)。
 *  返回 ScriptModule[]；旧 JSON 格式(裸 ScriptPage[])自动迁移为默认模块。 */
export async function loadSequenceConfig(path: string): Promise<ScriptModule[]> {
  const raw = await invoke<unknown[]>('load_sequence_config', { path });
  // 迁移：元素无 pages 字段 → 视为旧裸 ScriptPage[]，包进默认"快捷指令"模块
  const isModule = (r: unknown): r is ScriptModule =>
    typeof r === 'object' && r !== null && 'pages' in r && 'id' in r;
  if (raw.length > 0 && isModule(raw[0])) {
    return raw as ScriptModule[];
  }
  // 旧格式：整包作为默认模块的 pages
  return [defaultScriptModule('快捷指令')].map((m) => ({
    ...m,
    pages: raw as unknown as ScriptPage[],
  }));
}

/** 自动保存序列配置到默认路径（%APPDATA%/neoserial/sequence.json）。 */
export async function saveSequenceAuto(data: ScriptModule[]): Promise<void> {
  await invoke('save_sequence_auto', { data });
}

/** 自动加载序列配置（从 %APPDATA%/neoserial/sequence.json）。文件不存在返回空数组。 */
export async function loadSequenceAuto(): Promise<ScriptModule[]> {
  const raw = await invoke<unknown[]>('load_sequence_auto');
  if (raw.length === 0) return [];
  const isModule = (r: unknown): r is ScriptModule =>
    typeof r === 'object' && r !== null && 'pages' in r && 'id' in r;
  if (isModule(raw[0])) {
    return raw as ScriptModule[];
  }
  // 旧格式迁移
  return [defaultScriptModule('快捷指令')].map((m) => ({
    ...m,
    pages: raw as unknown as ScriptPage[],
  }));
}

// ============ 指令联想(知识库手册索引 + 发送历史) ============

/** 设置页"刷新指令库":用编辑框里的地址/Key 按手册拉全量并写本地缓存;成功后后端广播 command-index-changed。
 *  地址/Key 传空串 = 用后端的生效值(用户已保存的,或编译期内置的)。 */
export async function commandIndexRefresh(baseUrl: string, apiKey: string): Promise<CommandIndexRefreshResult> {
  return await invoke<CommandIndexRefreshResult>('command_index_refresh', { baseUrl, apiKey });
}

/** 手册列表每行的"刷新这一本":只拉这本的指令,其余沿用缓存。与全量刷新互斥(后端同一把标志),
 *  撞上正在进行的刷新会 reject "正在刷新,请稍候"。成功后后端广播 command-index-changed。 */
export async function commandIndexRefreshDoc(baseUrl: string, apiKey: string, documentId: number): Promise<CommandIndexRefreshDocResult> {
  return await invoke<CommandIndexRefreshDocResult>('command_index_refresh_doc', { baseUrl, apiKey, documentId });
}

/** 读本地缓存(%LOCALAPPDATA%/neoserial/command-index.json)。从未刷新过返回空 documents/commands。 */
export async function commandIndexLoad(): Promise<CommandIndexCache> {
  return await invoke<CommandIndexCache>('command_index_load');
}

/** 连通性测试:用传入的地址/Key(传空串则用生效值)请求手册列表接口探活,不落盘。
 *  返回一句话(如"连通正常 · 3 本手册")或失败原因。 */
export async function commandIndexTestConnection(baseUrl: string, apiKey: string): Promise<string> {
  return await invoke<string>('command_index_test_connection', { baseUrl, apiKey });
}

/** 知识库凭据状态。只有布尔与枚举,拿不到 Key 本身——前端不需要知道它是什么。 */
export interface KbCredentialStatus {
  /** 二进制里注入了内置地址:地址框留空即用它 */
  builtin_base_url: boolean;
  /** 二进制里注入了内置 Key */
  builtin_api_key: boolean;
  /** 用户自己填的 Key:set=已存 / unset=没填 / undecryptable=存了但解不开(换了 Windows 用户或机器) */
  user_key: 'set' | 'unset' | 'undecryptable';
}

export async function kbCredentialStatus(): Promise<KbCredentialStatus> {
  return await invoke<KbCredentialStatus>('kb_credential_status');
}

/** 写入/清除用户填的知识库 API Key(空串 = 清除,回到内置值)。
 *  Key 不走 patchSettings:它不在 Settings 结构里,见 config/secret.rs。 */
export async function kbSetApiKey(key: string): Promise<void> {
  await invoke('kb_set_api_key', { key });
}

// ============ 数据目录 ============

/** 当前使用的数据目录。standard = 系统标准位置;portable = exe 旁的便携目录;custom = NEOSERIAL_DATA_DIR */
export interface DataDirs {
  config: string;
  local: string;
  mode: 'standard' | 'portable' | 'custom';
}

export async function dataDirs(): Promise<DataDirs> {
  return await invoke<DataDirs>('data_dirs');
}

/** 在资源管理器里打开数据目录(cache=true 打开缓存目录)。路径由后端解析,不传路径。 */
export async function openDataDir(cache: boolean): Promise<void> {
  await invoke('open_data_dir', { cache });
}

/** 输入框手动发送成功后记一条历史;后端去重挪前、按设置的留存上限截尾,变化时广播 send-history-changed。返回最新全量列表。 */
export async function sendHistoryPush(text: string): Promise<string[]> {
  return await invoke<string[]>('send_history_push', { text });
}

export async function sendHistoryLoad(): Promise<string[]> {
  return await invoke<string[]>('send_history_load');
}

export async function sendHistoryClear(): Promise<void> {
  await invoke('send_history_clear');
}

// ============ 文件对话框 ============

export async function openFileDialog(title?: string, filters?: { name: string; extensions: string[] }[]): Promise<string | null> {
  const result = await open({
    title: title || '选择文件',
    filters: filters || [
      { name: '所有文件', extensions: ['*'] },
    ],
  });
  return result as string | null;
}

export async function saveFileDialog(title?: string, defaultName?: string, filters?: { name: string; extensions: string[] }[]): Promise<string | null> {
  const result = await save({
    title: title || '保存文件',
    defaultPath: defaultName,
    filters: filters || [
      { name: '所有文件', extensions: ['*'] },
    ],
  });
  return result as string | null;
}

// ============ 事件监听 ============
// 用 getCurrentWebview().listen 定向:只收 emit_to 给本窗口(label)的事件。
// 后端 emit_to(win-{port}),每个副窗口只收自己 port 的事件,多窗口不串流。

/** rx 批量事件:后端每次 flush 发一个(≤16 行或 5ms),payload 为 LogLine[]。
 * 旧版逐行 rx-line 已废弃;批量后行内容/顺序/时间戳语义不变。 */
export function onRxLines(cb: (lines: LogLine[]) => void) {
  return getCurrentWebview().listen<LogLine[]>('rx-lines', (e) => cb(e.payload));
}

export interface FileSendProgress {
  sent: number;
  total: number;
}

export function onFileSendProgress(cb: (progress: FileSendProgress) => void) {
  return getCurrentWebview().listen<FileSendProgress>('file-send-progress', (e) => cb(e.payload));
}

export function onTxLine(cb: (line: LogLine) => void) {
  return getCurrentWebview().listen<LogLine>('tx-line', (e) => cb(e.payload));
}

export function onTxUpdate(cb: (update: TxUpdate) => void) {
  return getCurrentWebview().listen<TxUpdate>('tx-update', (e) => cb(e.payload));
}

export function onRxUpdate(cb: (update: RxUpdate) => void) {
  return getCurrentWebview().listen<RxUpdate>('rx-update', (e) => cb(e.payload));
}

export function onConnectionState(cb: (state: ConnectionState) => void) {
  return getCurrentWebview().listen<ConnectionState>('connection-state', (e) => cb(e.payload));
}

export function onSequenceProgress(cb: (progress: SequenceProgress) => void) {
  return getCurrentWebview().listen<SequenceProgress>('sequence-progress', (e) => cb(e.payload));
}

export function onSequenceDone(cb: (done: SequenceDone) => void) {
  return getCurrentWebview().listen<SequenceDone>('sequence-done', (e) => cb(e.payload));
}

export function onError(cb: (error: ErrorEvent) => void) {
  return getCurrentWebview().listen<ErrorEvent>('error', (e) => cb(e.payload));
}

export function onConnectionMode(cb: (mode: ConnectionMode) => void) {
  return getCurrentWebview().listen<ConnectionMode>('connection-mode', (e) => cb(e.payload));
}

/** theme-changed 全局事件:主题编辑器保存后广播,其他窗口收到重新加载主题。
 *  emit(全局) 发给所有 webview,getCurrentWebview().listen 能收到。 */
export function onThemeChanged(cb: () => void) {
  return getCurrentWebview().listen('theme-changed', () => cb());
}

/** theme-preview 全局事件:主题编辑器改色时实时广播到主窗口预览(未保存)。
 *  custom 为完整色板对象,主窗口收到后 applyTheme('custom', custom)。
 *  custom 为 null 时主窗口从 settings 重载已保存的主题(编辑器关闭时恢复)。 */
export interface ThemePreviewEvent {
  custom: Record<string, string> | null;
}
export function onThemePreview(cb: (e: ThemePreviewEvent) => void) {
  return getCurrentWebview().listen<ThemePreviewEvent>('theme-preview', (e) => cb(e.payload));
}

/** theme-highlight 全局事件:编辑器鼠标悬停颜色项时广播到主窗口,
 *  主窗口给用到该色的元素加虚线高亮。field 为 null 时清除。 */
export interface ThemeHighlightEvent {
  field: string | null;
}
export function onThemeHighlight(cb: (e: ThemeHighlightEvent) => void) {
  return getCurrentWebview().listen<ThemeHighlightEvent>('theme-highlight', (e) => cb(e.payload));
}

/** sequence-changed 事件:其他窗口改了快捷指令,本窗口收到后 reload 同步。
 *  payload.source 是改动来源窗口 label,前端用它跳过自己触发的更新。 */
export interface SequenceChangedEvent {
  source: string;
}
export function onSequenceChanged(cb: (e: SequenceChangedEvent) => void) {
  return getCurrentWebview().listen<SequenceChangedEvent>('sequence-changed', (e) => cb(e.payload));
}

/** mcp-connections-changed 事件:agent 连接/断开或 GUI 接管后,全局触发,
 *  让所有窗口刷新"待接管"chip 列表。payload 为空。 */
export function onMcpConnectionsChanged(cb: () => void) {
  return getCurrentWebview().listen('mcp-connections-changed', () => cb());
}

/** settings-changed:任一窗口/agent 保存了设置。各窗口据此刷新 cachedSettings 快照,
 *  否则下次整份回写会把别处改过的字段(如 background_mode)冲回旧值。payload 为空。 */
export function onSettingsChanged(cb: () => void) {
  return getCurrentWebview().listen('settings-changed', () => cb());
}

/** command-index-changed 全局事件:任一窗口/启动任务刷新了指令库缓存,收到后重新 commandIndexLoad。payload 为空。 */
export function onCommandIndexChanged(cb: () => void) {
  return getCurrentWebview().listen('command-index-changed', () => cb());
}

/** send-history-changed 全局事件:任一窗口记了/清了发送历史,payload 带全量列表(最近在前)。 */
export function onSendHistoryChanged(cb: (items: string[]) => void) {
  return getCurrentWebview().listen<{ items: string[] }>('send-history-changed', (e) => cb(e.payload.items));
}

// ============ 系统托盘 ============

/** 真正退出应用(断开所有连接、停 MCP、注销 registry)。设置页"退出 NeoSerial"按钮用,
 *  与托盘菜单"退出"同一后端路径。 */
export async function exitApp(): Promise<void> {
  await invoke('exit_app');
}

/** close-guard:轻量模式下关最后一个窗口时 agent 建的连接还活着——退出会掐断它,
 *  后端拦下关窗、把这些端口发给本窗口弹确认。选择经 resolveLastClose 回传(取消则什么都不调)。 */
export interface CloseGuard {
  ports: string[];
}
export function onCloseGuard(cb: (guard: CloseGuard) => void) {
  return getCurrentWebview().listen<CloseGuard>('close-guard', (e) => cb(e.payload));
}

/** close-guard 确认框的选择:exit=仍然退出;background=开启后台运行并收起(连接留给 agent)。 */
export async function resolveLastClose(action: 'exit' | 'background'): Promise<void> {
  await invoke('resolve_last_close', { action });
}

// ============ MCP 工具调用记录(侧栏"MCP 日志"tab,实时显示 agent 的实际操作) ============
export interface McpCallRecord {
  /** RFC 3339 本地时间 */
  ts: string;
  tool: string;
  /** 入参 JSON(截断到 ~500 字符) */
  args: string;
  ok: boolean;
  /** 失败原因,成功为空 */
  error: string;
  /** 耗时(ms) */
  duration_ms: number;
}

/** 开 tab 时拉一次历史(环形上限 200,最旧在前)。 */
export async function getMcpCallLog(): Promise<McpCallRecord[]> {
  return await invoke<McpCallRecord[]>('get_mcp_call_log');
}

/** 每次工具调用后后端 emit,前端实时 append。 */
export function onMcpCall(cb: (rec: McpCallRecord) => void) {
  return getCurrentWebview().listen<McpCallRecord>('mcp-call', (e) => cb(e.payload));
}
