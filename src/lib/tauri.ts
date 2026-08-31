import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open, save } from '@tauri-apps/plugin-dialog';
import type {
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

/** 副窗口 onMount 调:按本窗口 label 反推 port,查连接状态。 */
export async function getWindowConnState(): Promise<WindowConnState> {
  return await invoke<WindowConnState>('get_window_conn_state');
}

/** agent 连了但还没 GUI 接管的端口列表(window_label 仍是 mcp- 前缀)。 */
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

/** 查活跃会话(其他窗口数 + 连接数),供 main 关闭二次确认。 */
export interface ActiveSessions {
  other_windows: number;
  connections: number;
}
export async function hasActiveSessions(): Promise<ActiveSessions> {
  return await invoke<ActiveSessions>('has_active_sessions');
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

export async function isLogging(): Promise<boolean> {
  return await invoke<boolean>('is_logging');
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

/** 加载序列配置。返回 ScriptModule[]；旧格式（裸 ScriptPage[]）自动迁移为默认模块。 */
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
