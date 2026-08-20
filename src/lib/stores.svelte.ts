import { defaultScriptCommand, defaultScriptPage, defaultScriptModule, presetScriptModules, type LogLine, type ScriptPage, type Settings } from './types';

// ============ 连接状态 ============
export const connected = $state<{ value: boolean }>({ value: false });
export const currentPort = $state<{ value: string | null }>({ value: null });
export const availablePorts = $state<{ value: string[] }>({ value: [] });
/** agent 连了但还没 GUI 窗口接管的端口(window_label 仍是 mcp- 前缀)。
 *  供 main 窗口 + 号旁渲染快捷 chip:点击 = 开窗口并接管该连接。 */
export const mcpOnlyConnections = $state<{ value: { port: string; baud: number }[] }>({ value: [] });
/** 本窗口绑定的 port:副窗口(win-{port})从 label 反推得到;main 窗口为 null。
 *  所有 invoke 调用(send/disconnect/sequence 等)用它定位目标连接。 */
export const windowPort = $state<{ value: string | null }>({ value: null });

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
/** 文本模式的编码方式：'ascii' | 'utf8' | 'gbk'，默认 ascii */
export const textEncoding = $state<{ value: 'ascii' | 'utf8' | 'gbk' }>({ value: 'ascii' });
export const showTimestamp = $state<{ value: boolean }>({ value: true });
/** 日志区最左侧行号(本次连接期间 index)开关 */
export const showLineIndex = $state<{ value: boolean }>({ value: false });
export const autoScroll = $state<{ value: boolean }>({ value: true });
export const logVersion = $state<{ value: number }>({ value: 0 });
/** LogView 滚动容器的 DOM 引用，供 App.svelte 的回调函数使用 */
export const scrollContainerRef = $state<{ el: HTMLDivElement | null }>({ el: null });

export function appendLogLine(line: LogLine) {
  if (paused.value) return;
  logLines.push(line);
  if (logLines.length > MAX_LOG_LINES) {
    logLines.splice(0, logLines.length - MAX_LOG_LINES);
  }
  logVersion.value++;
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
/** 日志区英文字体族。'default' = 用默认 monospace 栈 */
export const logFontLatin = $state<{ value: string }>({ value: 'default' });
/** 日志区中文字体族。'default' = 不单独指定，沿用英文字体栈回退 */
export const logFontCJK = $state<{ value: string }>({ value: 'default' });

/** 英文字体预设：等宽字体，用于 ASCII/HEX 字节对齐 */
export const logFontLatinPresets: { key: string; label: string; value: string }[] = [
  { key: 'default', label: '默认', value: 'default' },
  { key: 'consolas', label: 'Consolas', value: 'Consolas' },
  { key: 'cascadia', label: 'Cascadia Code', value: '"Cascadia Code"' },
  { key: 'jetbrains', label: 'JetBrains Mono', value: '"JetBrains Mono"' },
  { key: 'courier', label: 'Courier New', value: '"Courier New"' },
];
/** 中文字体预设：渲染中文内容，不影响英文/HEX 对齐 */
export const logFontCJKPresets: { key: string; label: string; value: string }[] = [
  { key: 'default', label: '跟随英文', value: 'default' },
  { key: 'yahei', label: '微软雅黑', value: '"Microsoft YaHei", "微软雅黑"' },
  { key: 'dengxian', label: '等线', value: '"DengXian", "等线"' },
  { key: 'simsun', label: '宋体', value: 'SimSun, "宋体"' },
  { key: 'kaiti', label: '楷体', value: 'KaiTi, "楷体"' },
  { key: 'sarasa', label: '更纱黑体', value: '"Sarasa Mono SC", "Sarasa Gothic SC"' },
];

/** 把英文/中文字体值拼成 CSS font-family 回退栈。
 *  - latin='default' 时用内置等宽字体列表
 *  - cjk='default' 时不追加中文回退（沿用 latin 栈的中文能力）
 *  - monospace 始终作为最终兜底放最末尾：通用关键字会"吃掉"任何字符，
 *    若放在 cjk 前面会导致中文字体回退被截断（英文选默认时中文改了不生效） */
function buildFontStack(latin: string, cjk: string): string {
  const parts: string[] = [];
  if (latin && latin !== 'default') {
    parts.push(latin);
  } else {
    parts.push('"SF Mono"', '"JetBrains Mono"', '"Cascadia Code"', 'Consolas', '"Courier New"');
  }
  if (cjk && cjk !== 'default') {
    parts.push(cjk);
  }
  parts.push('monospace');
  return parts.join(', ');
}

/** 应用日志区字体到 <html>：字号/行高/字体族，CSS 变量覆盖即生效。
 *  latin/cjk 为 'default' 或缺省时取默认值，中文不单独指定则跟随英文栈。 */
export function applyLogFont(size: number, lineH: number, latin?: string, cjk?: string) {
  if (typeof document === 'undefined') return;
  const el = document.documentElement;
  el.style.setProperty('--log-font-size', `${size}px`);
  el.style.setProperty('--log-line-height', String(lineH));
  // 始终写入完整栈：default 时也用默认栈覆盖，保证取消/切换时回到干净状态
  const stack = buildFontStack(latin ?? 'default', cjk ?? 'default');
  el.style.setProperty('--log-font-family', stack);
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
