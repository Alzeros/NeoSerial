import { defaultScriptPage, type LogLine, type ScriptPage } from './types';

// ============ 连接状态 ============
export const connected = $state<{ value: boolean }>({ value: false });
export const currentPort = $state<{ value: string | null }>({ value: null });
export const availablePorts = $state<{ value: string[] }>({ value: [] });

export const connectionParams = $state<{
  port: string;
  baudRate: number;
  dataBits: 'Five' | 'Six' | 'Seven' | 'Eight';
  parity: 'None' | 'Odd' | 'Even';
  stopBits: 1 | 2;
  flowControl: 'None' | 'Software' | 'Hardware';
}>({
  port: '',
  baudRate: 115200,
  dataBits: 'Eight',
  parity: 'None',
  stopBits: 1,
  flowControl: 'None',
});

// ============ 日志数据 ============
const MAX_LOG_LINES = 10_000;
export const logLines = $state<LogLine[]>([]);
export const paused = $state<{ value: boolean }>({ value: false });
export const displayMode = $state<{ value: 'ascii' | 'hex' }>({ value: 'ascii' });
export const showTimestamp = $state<{ value: boolean }>({ value: true });
export const autoScroll = $state<{ value: boolean }>({ value: true });

export function appendLogLine(line: LogLine) {
  if (paused.value) return;
  logLines.push(line);
  if (logLines.length > MAX_LOG_LINES) {
    logLines.splice(0, logLines.length - MAX_LOG_LINES);
  }
}

export function clearLogLines() {
  logLines.length = 0;
}

// ============ 统计 ============
export const txBytes = $state<{ value: number }>({ value: 0 });
export const rxBytes = $state<{ value: number }>({ value: 0 });

// ============ 文件日志 ============
export const loggingPath = $state<{ value: string | null }>({ value: null });
export const logSendContent = $state<{ value: boolean }>({ value: true });

// ============ 手动发送 ============
export const sendText = $state<{ value: string }>({ value: '' });
export const lineEnding = $state<{ value: 'None' | 'Cr' | 'Lf' | 'Crlf' }>({ value: 'Crlf' });
export const hexSend = $state<{ value: boolean }>({ value: false });
export const sendHistory = $state<{ value: string[] }>({ value: [] });
export const sendHistoryIndex = $state<{ value: number }>({ value: -1 });

// ============ 文件发送 ============
export const fileSendPath = $state<{ value: string | null }>({ value: null });
export const fileSendProgress = $state<{ value: number }>({ value: 0 });

// ============ 工具栏 ============
export const hexDisplay = $state<{ value: boolean }>({ value: false });

// ============ 脚本序列面板 ============
export const scriptPanelOpen = $state<{ value: boolean }>({ value: true });
export const scriptPanelWidth = $state<{ value: number }>({ value: 360 });

export const scriptPages = $state<ScriptPage[]>([defaultScriptPage('Page0')]);
export const activeScriptPage = $state<{ value: number }>({ value: 0 });
export const scriptRunning = $state<{ value: boolean }>({ value: false });
export const scriptRunCount = $state<{ value: number }>({ value: 1 });
export const scriptLoopInterval = $state<{ value: number }>({ value: 500 });
export const scriptCurrentRow = $state<{ value: number }>({ value: -1 });

export function toggleScriptPanel() {
  scriptPanelOpen.value = !scriptPanelOpen.value;
}

export function addScriptPage() {
  if (scriptPages.length >= 6) return;
  scriptPages.push(defaultScriptPage(`Page${scriptPages.length}`));
}

export function removeScriptPage(index: number) {
  if (scriptPages.length <= 1) return;
  scriptPages.splice(index, 1);
  if (activeScriptPage.value >= scriptPages.length) {
    activeScriptPage.value = scriptPages.length - 1;
  }
}
