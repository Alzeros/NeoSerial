/// <reference types="svelte" />
/// <reference types="vite/client" />

declare module '$lib/stores' {
  export const connected: any;
  export const currentPort: any;
  export const availablePorts: any;
  export const connectionParams: any;
  export const logLines: any;
  export const paused: any;
  export const displayMode: any;
  export const showTimestamp: any;
  export const autoScroll: any;
  export const txBytes: any;
  export const rxBytes: any;
  export const loggingPath: any;
  export const logSendContent: any;
  export const sendText: any;
  export const lineEnding: any;
  export const hexSend: any;
  export const sendHistory: any;
  export const sendHistoryIndex: any;
  export const fileSendPath: any;
  export const fileSendProgress: any;
  export const hexDisplay: any;
  export const scriptPanelOpen: any;
  export const scriptPanelWidth: any;
  export const scriptPages: any;
  export const activeScriptPage: any;
  export const scriptRunning: any;
  export const scriptRunCount: any;
  export const scriptLoopInterval: any;
  export const scriptCurrentRow: any;
  export function appendLogLine(line: any): void;
  export function clearLogLines(): void;
  export function toggleScriptPanel(): void;
  export function addScriptPage(): void;
  export function removeScriptPage(index: number): void;
}

declare module '$lib/tauri' {
  export function connect(params: any): Promise<void>;
  export function disconnect(): Promise<void>;
  export function listPorts(): Promise<string[]>;
  export function send(text: string, ending: string, isHex: boolean): Promise<number>;
  export function sendFile(path: string): Promise<number>;
  export function getSettings(): Promise<any>;
  export function saveSettings(settings: any): Promise<void>;
  export function saveCommands(groups: any): Promise<void>;
  export function startLogging(path?: string): Promise<string>;
  export function stopLogging(): Promise<void>;
  export function isLogging(): Promise<boolean>;
  export function sequenceRun(commands: any[], runCount: number, loopInterval: number): Promise<void>;
  export function sequenceStop(): Promise<void>;
  export function saveSequenceConfig(path: string, pages: any[]): Promise<void>;
  export function loadSequenceConfig(path: string): Promise<any[]>;
  export function openFileDialog(title?: string, filters?: any[]): Promise<string | null>;
  export function saveFileDialog(title?: string, defaultName?: string, filters?: any[]): Promise<string | null>;
  export function onRxLine(cb: (line: any) => void): Promise<() => void>;
  export function onTxUpdate(cb: (update: any) => void): Promise<() => void>;
  export function onRxUpdate(cb: (update: any) => void): Promise<() => void>;
  export function onConnectionState(cb: (state: any) => void): Promise<() => void>;
  export function onSequenceProgress(cb: (progress: any) => void): Promise<() => void>;
  export function onSequenceDone(cb: (done: any) => void): Promise<() => void>;
  export function onError(cb: (error: any) => void): Promise<() => void>;
  export function onConnectionMode(cb: (mode: any) => void): Promise<() => void>;
}

declare module '$lib/types' {
  export type Dir = 'rx' | 'tx';
  export interface LogLine { ts: string; dir: Dir; raw: number[]; ascii: string; hex: string; is_error: boolean; }
  export type ConnectionParams = any;
  export type Settings = any;
  export type ScriptCommand = any;
  export type ScriptPage = any;
  export function defaultScriptCommand(id: number): any;
  export function defaultScriptPage(name: string): any;
}
