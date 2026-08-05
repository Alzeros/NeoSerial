import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
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

export async function disconnect(): Promise<void> {
  await invoke('disconnect');
}

export async function listPorts(): Promise<string[]> {
  return await invoke<string[]>('list_ports');
}

export async function resetStats(): Promise<void> {
  await invoke('reset_stats');
}

// ============ 数据收发 ============

export async function send(text: string, ending: string, isHex: boolean): Promise<number> {
  return await invoke<number>('send', { text, ending, isHex });
}

export async function sendFile(path: string): Promise<number> {
  return await invoke<number>('send_file', { path });
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
  commands: ScriptCommand[],
  runCount: number,
  loopInterval: number,
): Promise<void> {
  await invoke('sequence_run', { commands, runCount, loopInterval });
}

export async function sequenceStop(): Promise<void> {
  await invoke('sequence_stop');
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

export function onRxLine(cb: (line: LogLine) => void) {
  return listen<LogLine>('rx-line', (e) => cb(e.payload));
}

export interface FileSendProgress {
  sent: number;
  total: number;
}

export function onFileSendProgress(cb: (progress: FileSendProgress) => void) {
  return listen<FileSendProgress>('file-send-progress', (e) => cb(e.payload));
}

export function onTxLine(cb: (line: LogLine) => void) {
  return listen<LogLine>('tx-line', (e) => cb(e.payload));
}

export function onTxUpdate(cb: (update: TxUpdate) => void) {
  return listen<TxUpdate>('tx-update', (e) => cb(e.payload));
}

export function onRxUpdate(cb: (update: RxUpdate) => void) {
  return listen<RxUpdate>('rx-update', (e) => cb(e.payload));
}

export function onConnectionState(cb: (state: ConnectionState) => void) {
  return listen<ConnectionState>('connection-state', (e) => cb(e.payload));
}

export function onSequenceProgress(cb: (progress: SequenceProgress) => void) {
  return listen<SequenceProgress>('sequence-progress', (e) => cb(e.payload));
}

export function onSequenceDone(cb: (done: SequenceDone) => void) {
  return listen<SequenceDone>('sequence-done', (e) => cb(e.payload));
}

export function onError(cb: (error: ErrorEvent) => void) {
  return listen<ErrorEvent>('error', (e) => cb(e.payload));
}

export function onConnectionMode(cb: (mode: ConnectionMode) => void) {
  return listen<ConnectionMode>('connection-mode', (e) => cb(e.payload));
}
