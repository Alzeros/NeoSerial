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
  ScriptPage,
  SequenceDone,
  SequenceProgress,
  Settings,
  TxUpdate,
} from './types';

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

export async function saveSequenceConfig(path: string, pages: ScriptPage[]): Promise<void> {
  await invoke('save_sequence_config', { path, pages });
}

export async function loadSequenceConfig(path: string): Promise<ScriptPage[]> {
  return await invoke<ScriptPage[]>('load_sequence_config', { path });
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
