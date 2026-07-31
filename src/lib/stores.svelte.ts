import { defaultScriptCommand, defaultScriptPage, defaultScriptModule, presetScriptModules, type LogLine, type ScriptPage, type Settings } from './types';

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
export const scriptPanelWidth = $state<{ value: number }>({ value: 420 });

/** 右栏模块列表（Page 之上的分组层）。预置功能，代码写死，用户不可增删。 */
export const scriptModules = $state(presetScriptModules());
export const activeScriptModule = $state<{ value: number }>({ value: 0 });
export const activeScriptPage = $state<{ value: number }>({ value: 0 });
export const scriptRunning = $state<{ value: boolean }>({ value: false });
export const scriptRunCount = $state<{ value: number }>({ value: 1 });
export const scriptLoopInterval = $state<{ value: number }>({ value: 500 });
export const scriptCurrentRow = $state<{ value: number }>({ value: -1 });

/** 当前激活模块的 pages（便捷访问，源数据在 scriptModules[activeScriptModule].pages） */
export function currentModulePages(): ScriptPage[] {
  return scriptModules[activeScriptModule.value]?.pages ?? [];
}

export function toggleScriptPanel() {
  scriptPanelOpen.value = !scriptPanelOpen.value;
}

// ============ 设置缓存（用于断开时回写） ============
export const cachedSettings = $state<{ value: Settings | null }>({ value: null });

// ===== 模块级操作（仅切换；模块为预置功能，用户不可增删） =====
export function switchScriptModule(index: number) {
  if (index < 0 || index >= scriptModules.length) return;
  activeScriptModule.value = index;
  activeScriptPage.value = 0;
}

// ===== 页签级操作（作用于当前模块） =====
export function addScriptPage() {
  const pages = currentModulePages();
  if (pages.length >= 6) return;
  pages.push(defaultScriptPage(`Page${pages.length}`));
}

export function removeScriptPage(index: number) {
  const pages = currentModulePages();
  if (pages.length <= 1) return;
  pages.splice(index, 1);
  if (activeScriptPage.value >= pages.length) {
    activeScriptPage.value = pages.length - 1;
  }
}

// ===== 命令行级操作（作用于当前模块的当前页） =====
/** 在当前页末尾追加一个空命令行 */
export function addScriptRow() {
  const page = currentModulePages()[activeScriptPage.value];
  if (!page) return;
  page.commands.push(defaultScriptCommand(page.commands.length + 1));
}

/** 删除指定行（至少保留 1 行） */
export function removeScriptRow(rowIndex: number) {
  const page = currentModulePages()[activeScriptPage.value];
  if (!page || page.commands.length <= 1) return;
  page.commands.splice(rowIndex, 1);
}

/** 交换两行顺序（越界自动忽略） */
export function moveScriptRow(from: number, to: number) {
  const page = currentModulePages()[activeScriptPage.value];
  if (!page) return;
  if (to < 0 || to >= page.commands.length || from === to) return;
  const tmp = page.commands[from];
  page.commands[from] = page.commands[to];
  page.commands[to] = tmp;
}

/** 拖拽排序：把 from 行移到 to 位置（插入式，先删后插） */
export function reorderScriptRow(from: number, to: number) {
  const page = currentModulePages()[activeScriptPage.value];
  if (!page) return;
  if (from < 0 || from >= page.commands.length) return;
  if (to < 0 || to >= page.commands.length || from === to) return;
  const [item] = page.commands.splice(from, 1);
  page.commands.splice(to, 0, item);
}
