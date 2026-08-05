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
export const logVersion = $state<{ value: number }>({ value: 0 });

export function appendLogLine(line: LogLine) {
  if (paused.value) return;
  logLines.push(line);
  if (logLines.length > MAX_LOG_LINES) {
    logLines.splice(0, logLines.length - MAX_LOG_LINES);
  }
  logVersion.value++; // 始终递增，确保 effect 能触发
}

export function clearLogLines() {
  logLines.length = 0;
}

// ============ 统计 ============
export const txBytes = $state<{ value: number }>({ value: 0 });
export const rxBytes = $state<{ value: number }>({ value: 0 });

// ============ 预设项（设置弹窗维护，持久化到 settings.json） ============
/** 预设波特率：连接栏下拉用，默认 9600/115200/921600，用户可在设置中增删 */
export const presetBaudRates = $state<{ value: number[] }>({ value: [9600, 115200, 921600] });

/** 主题预设：4 套完整色板，默认 preset-1（暖米白 + 青绿） */
export type ThemeKey = 'preset-1' | 'preset-2' | 'preset-3' | 'preset-4';
export const themeMeta: { key: ThemeKey; label: string; bg: string; accent: string }[] = [
  { key: 'preset-1', label: '暖白青', bg: '#F4F1E9', accent: '#0F6E56' },
  { key: 'preset-2', label: '雾灰松', bg: '#EFEFEC', accent: '#3A5A50' },
  { key: 'preset-3', label: '深海夜航', bg: '#1B2430', accent: '#1D9E75' },
  { key: 'preset-4', label: '暖砂陶', bg: '#F2E9DD', accent: '#B4653F' },
];
/** 当前主题 key */
export const theme = $state<{ value: string }>({ value: 'preset-1' });

/** 应用主题到 <html>：设置 data-theme 属性，CSS 规则自动切换整套色板 */
export function applyTheme(value: string) {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-theme', value);
}

// ============ 文件日志 ============
export const loggingPath = $state<{ value: string | null }>({ value: null });
/** 是否正在记录中（与 loggingPath 解耦：停止后路径保留，此值为 false） */
export const loggingActive = $state<{ value: boolean }>({ value: false });

// ============ 日志区字体（设置中调节，持久化） ============
export const logFontSize = $state<{ value: number }>({ value: 14 });
export const logLineHeight = $state<{ value: number }>({ value: 1.6 });
/** 方向标签样式：'short'=Tx/Rx，'full'=发送/接收 */
export const logDirLabelStyle = $state<{ value: 'short' | 'full' }>({ value: 'short' });

/** 应用日志区字体到 <html>，CSS 变量覆盖即生效 */
export function applyLogFont(size: number, lineH: number) {
  if (typeof document === 'undefined') return;
  const el = document.documentElement;
  el.style.setProperty('--log-font-size', `${size}px`);
  el.style.setProperty('--log-line-height', String(lineH));
}
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
export const scriptPanelWidth = $state<{ value: number }>({ value: 500 });

/** 右栏模块列表（Page 之上的分组层）。预置功能，代码写死，用户不可增删。 */
export const scriptModules = $state(presetScriptModules());
export const activeScriptModule = $state<{ value: number }>({ value: 0 });
export const activeScriptPage = $state<{ value: number }>({ value: 0 });
export const scriptRunning = $state<{ value: boolean }>({ value: false });
export const scriptRunCount = $state<{ value: number }>({ value: 1 });
export const scriptLoopInterval = $state<{ value: number }>({ value: 500 });
export const scriptCurrentRow = $state<{ value: number }>({ value: -1 });

/** 序列运行实时状态：由后端 sequence-progress / sequence-done 事件驱动，
 *  供底部"执行状态区 + 进度条"展示。所有字段在一次运行生命周期内有效。 */
export const scriptRunState = $state<{
  /** 当前轮次，从 1 开始 */
  round: number;
  /** 本次运行累计已发送条数（每发一条 +1） */
  sent: number;
  /** 本次运行总发送条数 = 勾选行数 × 轮数，运行开始时算定 */
  total: number;
  /** 运行起始时间戳(ms)，用于显示用时；非运行中为 0 */
  startedAt: number;
  /** 结束态文案展示用：'done' | 'aborted' | ''（空闲） */
  finished: '' | 'done' | 'aborted';
}>({ round: 1, sent: 0, total: 0, startedAt: 0, finished: '' });

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
