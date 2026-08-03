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

// ============ 预设项（设置弹窗维护，持久化到 settings.json） ============
/** 预设波特率：连接栏下拉用，默认 9600/115200/921600，用户可在设置中增删 */
export const presetBaudRates = $state<{ value: number[] }>({ value: [9600, 115200, 921600] });

/** 主题色：5 种预设，默认 blue(#4A5FE8)。切换时通过 html[data-theme-color] 覆盖 --primary */
export type ThemeColorKey = 'green' | 'orange' | 'teal' | 'slate' | 'blue';
export const themeColorMeta: { key: ThemeColorKey; color: string; label: string }[] = [
  { key: 'blue', color: '#3F51C5', label: '靛蓝' },
  { key: 'green', color: '#0F6E56', label: '松绿' },
  { key: 'orange', color: '#B4653F', label: '橙棕' },
  { key: 'teal', color: '#3A5A50', label: '青墨' },
  { key: 'slate', color: '#4C5A73', label: '灰蓝' },
];
/** 主题色值：预设 key（如 'blue'）或自定义 'custom:#RRGGBB' */
export const themeColor = $state<{ value: string }>({ value: 'blue' });

/** hex → "H S% L%" 字符串，用于 --primary-hsl 等 */
function hexToHsl(hex: string): string {
  const m = hex.replace('#', '');
  const r = parseInt(m.slice(0, 2), 16) / 255;
  const g = parseInt(m.slice(2, 4), 16) / 255;
  const b = parseInt(m.slice(4, 6), 16) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  let h = 0, s = 0;
  const l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = ((g - b) / d + (g < b ? 6 : 0));
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h /= 6;
  }
  return `${Math.round(h * 360)} ${Math.round(s * 100)}% ${Math.round(l * 100)}%`;
}

/** hex 略深一点作为 hover */
function darkenHex(hex: string, amount = 0.12): string {
  const m = hex.replace('#', '');
  let r = parseInt(m.slice(0, 2), 16);
  let g = parseInt(m.slice(2, 4), 16);
  let b = parseInt(m.slice(4, 6), 16);
  r = Math.round(r * (1 - amount));
  g = Math.round(g * (1 - amount));
  b = Math.round(b * (1 - amount));
  return '#' + [r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('');
}

/** 应用主题色到 <html>。
 *  预设 key → 设 data-theme-color 属性，CSS 规则覆盖变量。
 *  custom:hex → 设 data-theme-color="custom" + 内联覆盖 --primary 等变量。 */
export function applyThemeColor(value: string) {
  if (typeof document === 'undefined') return;
  const el = document.documentElement;
  if (value.startsWith('custom:')) {
    const hex = value.slice(7);
    const hsl = hexToHsl(hex);
    el.setAttribute('data-theme-color', 'custom');
    el.style.setProperty('--primary', hex);
    el.style.setProperty('--primary-hover', darkenHex(hex));
    el.style.setProperty('--primary-hsl', hsl);
    el.style.setProperty('--ring-hsl', hsl);
    el.style.setProperty('--accent-hsl', hsl);
  } else {
    // 预设：清除可能的自定义内联变量，靠 CSS 规则
    el.style.removeProperty('--primary');
    el.style.removeProperty('--primary-hover');
    el.style.removeProperty('--primary-hsl');
    el.style.removeProperty('--ring-hsl');
    el.style.removeProperty('--accent-hsl');
    el.setAttribute('data-theme-color', value);
  }
}

/** 取主题色当前实际 hex（用于色块高亮/预览） */
export function themeColorHex(value: string): string {
  if (value.startsWith('custom:')) return value.slice(7);
  const meta = themeColorMeta.find((t) => t.key === value);
  return meta?.color ?? '#3F51C5';
}

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
export const scriptPanelWidth = $state<{ value: number }>({ value: 500 });

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
