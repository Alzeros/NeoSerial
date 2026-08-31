// 自定义主题：用户只填 14 个基础色 + 圆角，其余 CSS 变量
// （hover/active、覆盖层、阴影、Tailwind HSL 映射等）由这里推导，
// 保证自定义主题与 4 套预设覆盖同一套完整变量、不破坏任何组件样式。

/** 用户可编辑的颜色字段（key 即 CSS 变量名去掉 -- 前缀） */
export const customThemeFields: { key: string; label: string }[] = [
  { key: 'background', label: '主背景' },
  { key: 'background-elevated', label: '卡片背景' },
  { key: 'background-input', label: '输入框背景' },
  { key: 'background-data', label: '日志区背景' },
  { key: 'background-deep', label: '深层背景' },
  { key: 'foreground', label: '正文文字' },
  { key: 'foreground-secondary', label: '次要文字' },
  { key: 'muted-foreground', label: '弱化文字' },
  { key: 'primary', label: '主色调' },
  { key: 'rx', label: '接收 Rx' },
  { key: 'tx', label: '发送 Tx' },
  { key: 'error', label: '错误色' },
  { key: 'warning', label: '警告色' },
  { key: 'border', label: '边框' },
];

/** 首次启用自定义主题时的底稿：预设 1（暖米白 + 青绿）的取值 */
export function defaultCustomTheme(): Record<string, string> {
  return {
    'background': '#F4F1E9',
    'background-elevated': '#FBFAF6',
    'background-input': '#FFFFFF',
    'background-data': '#FBFAF6',
    'background-deep': '#EFEBE0',
    'foreground': '#3A3A32',
    'foreground-secondary': '#6A665A',
    'muted-foreground': '#8A8676',
    'primary': '#0F6E56',
    'rx': '#3F8F5C',
    'tx': '#3B5B8A',
    'error': '#B5473A',
    'warning': '#C48A2E',
    'border': '#E4E0D4',
    'radius': '10',
  };
}

/** 4 套预设色板的可编辑字段取值（与 app.css 各预设一致）。
 *  "从预设起稿"：一键灌入自定义面板作为微调底稿，用户不必从零配色。 */
export const presetPalettes: Record<string, Record<string, string>> = {
  'preset-1': defaultCustomTheme(),
  'preset-2': {
    'background': '#EFEFEC',
    'background-elevated': '#FFFFFF',
    'background-input': '#FFFFFF',
    'background-data': '#FFFFFF',
    'background-deep': '#E8E8E4',
    'foreground': '#4A4A46',
    'foreground-secondary': '#6B6B65',
    'muted-foreground': '#8B8B85',
    'primary': '#3A5A50',
    'rx': '#3F8F5C',
    'tx': '#4A5A6A',
    'error': '#A8483C',
    'warning': '#B8862E',
    'border': '#DCDCD7',
    'radius': '10',
  },
  'preset-3': {
    'background': '#1B2430',
    'background-elevated': '#232E3C',
    'background-input': '#283445',
    'background-data': '#232E3C',
    'background-deep': '#161E2A',
    'foreground': '#E5EAEE',
    'foreground-secondary': '#D0D8E0',
    'muted-foreground': '#B8C2CC',
    'primary': '#1D9E75',
    'rx': '#3FB37A',
    'tx': '#6B9FD9',
    'error': '#E0655A',
    'warning': '#E0A83E',
    'border': '#344150',
    'radius': '10',
  },
  'preset-4': {
    'background': '#F2E9DD',
    'background-elevated': '#FBF6EE',
    'background-input': '#FFFFFF',
    'background-data': '#FBF6EE',
    'background-deep': '#EDE3D3',
    'foreground': '#5C5142',
    'foreground-secondary': '#7A6F5E',
    'muted-foreground': '#9C8F78',
    'primary': '#B4653F',
    'rx': '#5C9A5E',
    'tx': '#5A6B8A',
    'error': '#B5473A',
    'warning': '#C99A3E',
    'border': '#E0D2BC',
    'radius': '10',
  },
};

// ===== 颜色工具 =====

function hexToRgb(hex: string): [number, number, number] {
  let h = hex.replace('#', '');
  if (h.length === 3) h = h.split('').map((c) => c + c).join('');
  const n = parseInt(h, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** #rrggbb 校验（input[type=color] 只认这个格式） */
export function isHexColor(v: unknown): v is string {
  return typeof v === 'string' && /^#[0-9a-fA-F]{6}$/.test(v);
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l * 100];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
  else if (max === g) h = ((b - r) / d + 2) / 6;
  else h = ((r - g) / d + 4) / 6;
  return [h * 360, s * 100, l * 100];
}

function hslToHex(h: number, s: number, l: number): string {
  s /= 100; l /= 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  const to = (x: number) => Math.round(x * 255).toString(16).padStart(2, '0');
  return `#${to(f(0))}${to(f(8))}${to(f(4))}`.toUpperCase();
}

/** Tailwind 变量格式："H S% L%" */
function hslStr(hex: string): string {
  const [h, s, l] = rgbToHsl(...hexToRgb(hex));
  return `${Math.round(h)} ${Math.round(s)}% ${Math.round(l)}%`;
}

/** 明度偏移（HSL 的 L 加 delta 个百分点），用于推导 hover/active 色 */
function shiftL(hex: string, delta: number): string {
  const [h, s, l] = rgbToHsl(...hexToRgb(hex));
  return hslToHex(h, s, Math.max(0, Math.min(100, l + delta)));
}

/** YIQ 感知亮度 0-255，用于判断深浅背景 / 主色上配黑字还是白字 */
function yiq(hex: string): number {
  const [r, g, b] = hexToRgb(hex);
  return (r * 299 + g * 587 + b * 114) / 1000;
}

function rgba(hex: string, alpha: number): string {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/** 背景是否偏暗（决定中性覆盖层用白还是黑、是否禁用纸张噪点） */
export function isCustomDark(c: Record<string, string>): boolean {
  return isHexColor(c['background']) && yiq(c['background']) < 128;
}

/** 圆角取值（存储为纯数字字符串，越界钳制） */
export function customRadius(c: Record<string, string>): number {
  const r = parseInt(c['radius'] ?? '10', 10);
  return Number.isFinite(r) ? Math.max(0, Math.min(24, r)) : 10;
}

/** 把用户配置合并到底稿上：缺失/非法的颜色回退默认值（旧配置、导入的残缺主题都走这里） */
export function normalizeCustomTheme(raw: Record<string, string> | null | undefined): Record<string, string> {
  const base = defaultCustomTheme();
  if (!raw) return base;
  for (const f of customThemeFields) {
    if (isHexColor(raw[f.key])) base[f.key] = raw[f.key].toUpperCase();
  }
  if (raw['radius'] !== undefined) base['radius'] = String(customRadius(raw));
  return base;
}

/** 从 14 个基础色 + 圆角推导出完整变量表（与预设主题同一套变量名） */
export function computeCustomVars(c: Record<string, string>): Record<string, string> {
  const dark = isCustomDark(c);
  const primary = c['primary'];
  const error = c['error'];
  const vars: Record<string, string> = {};

  for (const f of customThemeFields) {
    vars[`--${f.key}`] = c[f.key];
  }

  vars['--primary-hover'] = shiftL(primary, -6);
  vars['--primary-active'] = shiftL(primary, -11);
  vars['--primary-foreground'] = yiq(primary) > 160 ? '#1F2328' : '#FFFFFF';
  vars['--error-hover'] = shiftL(error, -6);

  // 中性覆盖层：深色背景用白色系、浅色背景用黑色系（取值对齐预设 1/3）
  const ov = dark ? '255, 255, 255' : '0, 0, 0';
  vars['--border-subtle'] = `rgba(${ov}, 0.05)`;
  vars['--border-strong'] = `rgba(${ov}, 0.14)`;
  vars['--overlay-hover'] = `rgba(${ov}, ${dark ? 0.06 : 0.04})`;
  vars['--overlay-track'] = `rgba(${ov}, 0.16)`;
  vars['--overlay-scrollbar'] = `rgba(${ov}, 0.14)`;
  vars['--overlay-scrollbar-hover'] = `rgba(${ov}, 0.24)`;

  vars['--shadow-sm'] = dark ? '0 1px 2px rgba(0, 0, 0, 0.3)' : '0 1px 2px rgba(0, 0, 0, 0.04)';
  vars['--shadow-md'] = dark ? '0 2px 8px rgba(0, 0, 0, 0.4)' : '0 2px 8px rgba(0, 0, 0, 0.06)';
  vars['--shadow-lg'] = dark ? '0 4px 16px rgba(0, 0, 0, 0.5)' : '0 4px 16px rgba(0, 0, 0, 0.08)';

  vars['--focus-ring'] = rgba(primary, dark ? 0.2 : 0.15);
  vars['--danger-overlay'] = rgba(error, dark ? 0.1 : 0.08);
  vars['--danger-overlay-hover'] = rgba(error, dark ? 0.18 : 0.15);
  vars['--noise-opacity'] = dark ? '0' : '0.025';

  const r = customRadius(c);
  vars['--radius'] = `${r}px`;
  vars['--radius-sm'] = `${Math.max(3, Math.round(r * 0.6))}px`;

  // Tailwind HSL 映射（对应关系与 app.css 各预设一致）
  vars['--background-hsl'] = hslStr(c['background']);
  vars['--foreground-hsl'] = hslStr(c['foreground']);
  vars['--primary-hsl'] = hslStr(primary);
  vars['--primary-foreground-hsl'] = hslStr(vars['--primary-foreground']);
  vars['--muted-foreground-hsl'] = hslStr(c['muted-foreground']);
  vars['--border-hsl'] = hslStr(c['border']);
  vars['--input-hsl'] = hslStr(c['background-input']);
  vars['--ring-hsl'] = hslStr(primary);
  vars['--secondary-hsl'] = hslStr(c['background-elevated']);
  vars['--secondary-foreground-hsl'] = hslStr(c['foreground']);
  vars['--destructive-hsl'] = hslStr(error);
  vars['--destructive-foreground-hsl'] = '0 0% 100%';
  vars['--muted-hsl'] = hslStr(c['background-deep']);
  vars['--accent-hsl'] = hslStr(primary);
  vars['--accent-foreground-hsl'] = hslStr(vars['--primary-foreground']);
  vars['--popover-hsl'] = hslStr(c['background-elevated']);
  vars['--popover-foreground-hsl'] = hslStr(c['foreground']);
  vars['--card-hsl'] = hslStr(c['background-elevated']);
  vars['--card-foreground-hsl'] = hslStr(c['foreground']);
  vars['--rx-hsl'] = hslStr(c['rx']);
  vars['--tx-hsl'] = hslStr(c['tx']);
  vars['--system-hsl'] = hslStr(c['muted-foreground']);

  return vars;
}

/** 导出的主题文件结构 */
export interface ThemeFile {
  neoserialTheme: number;
  name?: string;
  vars: Record<string, string>;
}

export function buildThemeFile(c: Record<string, string>, name?: string): ThemeFile {
  return { neoserialTheme: 1, name: name || '自定义主题', vars: { ...c } };
}

/** 解析导入的主题 JSON。兼容裸 vars 对象；非法结构返回 null */
export function parseThemeFile(raw: unknown): Record<string, string> | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const obj = raw as Record<string, unknown>;
  const vars = (typeof obj.vars === 'object' && obj.vars !== null ? obj.vars : obj) as Record<string, string>;
  // 至少要有一个可识别的颜色字段，否则视为无效文件
  const hasAny = customThemeFields.some((f) => isHexColor(vars[f.key]));
  return hasAny ? normalizeCustomTheme(vars) : null;
}
