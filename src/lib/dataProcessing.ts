// 帧构造器:类型 + 默认值 + 预置合并 + 模板操作。全部纯函数,供 node 测试。
// FrameConfig 与后端 dataproc/frame.rs 对齐(snake_case)。

export type Endian = 'le' | 'be';
export interface LengthFieldSpec { size: 'none' | 'one' | 'two'; endian: Endian; coverage: 'data' | 'data_plus_checksum' | 'whole_frame' }
export interface RandomTextSpec {
  upper: boolean; lower: boolean; digits: boolean; special: boolean;
  special_chars: string; min_digits: number; min_special: number;
}
export interface DataSpec {
  mode: 'content' | 'fill'; format: 'hex' | 'text'; value: string;
  pattern: 'zeros' | 'ff' | 'random_bytes' | 'random_text' | 'increment' | 'custom_loop';
  /** 旧字段,保留兼容 */
  charset: string;
  random_text: RandomTextSpec;
  start: number; custom_format: 'hex' | 'text'; custom_value: string;
  /** whole_frame 仅用于识别和迁移旧配置；新界面的 length 始终表示数据字节数。 */
  length_basis: 'data_field' | 'whole_frame'; length: number;
}
export interface ChecksumSpec {
  algo: 'none' | 'crc16_modbus' | 'crc16_ccitt_false' | 'crc16_xmodem' | 'crc16_arc' | 'crc32' | 'sum8' | 'xor8';
  endian: Endian; include_header: boolean; include_length: boolean;
}
export interface FrameConfig { header_hex: string; length: LengthFieldSpec; data: DataSpec; checksum: ChecksumSpec }

export type DataSourceValue = 'content' | DataSpec['pattern'];
export type ChecksumAlgorithm = ChecksumSpec['algo'];
export interface DataProcessingOption<T extends string> { label: string; value: T }

export const DATA_SOURCE_OPTIONS: DataProcessingOption<DataSourceValue>[] = [
  { label: '随机字符', value: 'random_text' },
  { label: '随机字节', value: 'random_bytes' },
  { label: '手动输入', value: 'content' },
  { label: '全 00', value: 'zeros' },
  { label: '全 FF', value: 'ff' },
  { label: '递增', value: 'increment' },
  { label: '自定义循环', value: 'custom_loop' },
];

export const CHECKSUM_OPTIONS: DataProcessingOption<ChecksumAlgorithm>[] = [
  { label: '无', value: 'none' },
  { label: 'CRC16 Modbus', value: 'crc16_modbus' },
  { label: 'CRC16 CCITT-FALSE', value: 'crc16_ccitt_false' },
  { label: 'CRC16 XModem', value: 'crc16_xmodem' },
  { label: 'CRC16 ARC', value: 'crc16_arc' },
  { label: 'CRC32', value: 'crc32' },
  { label: 'SUM8', value: 'sum8' },
  { label: 'XOR8', value: 'xor8' },
];

export interface VisibleOptions<T extends string> {
  options: DataProcessingOption<T>[];
  currentLabel?: string;
}

function visibleOptions<T extends string>(
  catalog: DataProcessingOption<T>[],
  hidden: readonly string[],
  current: T,
  fixed: T,
): VisibleOptions<T> {
  const hiddenSet = new Set(hidden);
  const options = catalog.filter(({ value }) => value === fixed || !hiddenSet.has(value));
  const currentOption = catalog.find(({ value }) => value === current);
  const currentLabel = current !== fixed && hiddenSet.has(current) && currentOption
    ? `${currentOption.label}（已隐藏）`
    : undefined;
  return { options, currentLabel };
}

export function visibleDataSourceOptions(
  hidden: readonly string[],
  current: DataSourceValue,
): VisibleOptions<DataSourceValue> {
  return visibleOptions(DATA_SOURCE_OPTIONS, hidden, current, 'content');
}

export function visibleChecksumOptions(
  hidden: readonly string[],
  current: ChecksumAlgorithm,
): VisibleOptions<ChecksumAlgorithm> {
  return visibleOptions(CHECKSUM_OPTIONS, hidden, current, 'none');
}

export interface FramePreviewSegment {
  kind: string;
  offset: number;
  len: number;
}

/** 从完整帧中只取数据域，并按 ASCII 展示；不可打印字节显示为中点。 */
export function dataTextPreview(
  hex: string,
  breakdown: readonly FramePreviewSegment[],
  maxBytes?: number,
): { text: string; totalBytes: number; truncated: boolean } {
  const data = breakdown.find((segment) => segment.kind === 'data');
  if (!data) return { text: '', totalBytes: 0, truncated: false };

  const shownBytes = Math.min(data.len, maxBytes === undefined ? data.len : Math.max(0, Math.floor(maxBytes)));
  const bytes = hex.trim() === '' ? [] : hex.trim().split(/\s+/u);
  const text = bytes
    .slice(data.offset, data.offset + shownBytes)
    .map((value) => {
      const byte = Number.parseInt(value, 16);
      return byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : '·';
    })
    .join('');
  return { text, totalBytes: data.len, truncated: shownBytes < data.len };
}

export interface FrameTemplate { name: string; config: FrameConfig }
export interface FrameBuilderTool { id: string; kind: 'frame_builder'; config: FrameConfig; templates: FrameTemplate[] }
export type DataTool = FrameBuilderTool;

export interface FrameSectionState {
  header: boolean;
  length: boolean;
  checksum: boolean;
}

/**
 * 普通字段编辑保持用户当前的展开状态；只有初始加载或模板替换了整个 config
 * 引用时，才从新配置重新推导各段是否启用。
 */
export function resolveFrameSectionState(
  current: FrameSectionState,
  previousConfig: FrameConfig | null,
  nextConfig: FrameConfig,
): FrameSectionState {
  if (previousConfig === nextConfig) return current;
  return {
    header: nextConfig.header_hex !== '',
    length: nextConfig.length.size !== 'none',
    checksum: nextConfig.checksum.algo !== 'none',
  };
}

export function defaultRandomTextSpec(): RandomTextSpec {
  return { upper: true, lower: true, digits: true, special: false, special_chars: '!@#$%^&*', min_digits: 0, min_special: 0 };
}

export function defaultFrameConfig(): FrameConfig {
  return {
    header_hex: '',
    length: { size: 'none', endian: 'le', coverage: 'data' },
    data: { mode: 'content', format: 'hex', value: '', pattern: 'zeros', charset: '', random_text: defaultRandomTextSpec(), start: 0, custom_format: 'hex', custom_value: '', length_basis: 'data_field', length: 0 },
    checksum: { algo: 'none', endian: 'le', include_header: false, include_length: false },
  };
}

export function defaultFrameBuilderTool(): FrameBuilderTool {
  return { id: 'frame_builder', kind: 'frame_builder', config: defaultFrameConfig(), templates: [] };
}

/** 就地迁移旧配置；basis 写回后再次加载不会重复扣减固定段。 */
export function normalizeFrameConfig(config: FrameConfig): FrameConfig {
  const data = config.data;
  const charset = data.charset;
  if (!data.random_text) {
    data.random_text = charset && charset !== 'A-Za-z0-9'
      ? { upper: false, lower: false, digits: false, special: true, special_chars: charset, min_digits: 0, min_special: 0 }
      : defaultRandomTextSpec();
  } else if (charset && charset !== 'A-Za-z0-9') {
    // 过渡版本可能已经写入 random_text 默认对象,但还没清理旧 charset。
    // 只有仍处于默认特殊字符设置时才用旧值覆盖,避免覆盖用户新配置。
    if (!data.random_text.special && data.random_text.special_chars === defaultRandomTextSpec().special_chars) {
      data.random_text.special = true;
      data.random_text.special_chars = charset;
    }
    data.charset = '';
  }

  if (data.length_basis === 'whole_frame') {
    const headerHex = config.header_hex.replace(/\s/gu, '');
    const lengthWidths = { none: 0, one: 1, two: 2 };
    const checksumWidths: Record<ChecksumSpec['algo'], number> = {
      none: 0, sum8: 1, xor8: 1, crc16_modbus: 2, crc16_ccitt_false: 2,
      crc16_xmodem: 2, crc16_arc: 2, crc32: 4,
    };
    const fixed = headerHex.length / 2 + lengthWidths[config.length.size] + checksumWidths[config.checksum.algo];
    // 非法帧头或不足固定段的旧长度保留给后端报错，不静默裁成 0。
    if (/^(?:[0-9a-f]{2})*$/i.test(headerHex) && Number.isSafeInteger(data.length) && Number.isSafeInteger(fixed) && data.length >= fixed) {
      data.length -= fixed;
      data.length_basis = 'data_field';
    }
  }
  return config;
}

// ---- 类型守卫(结构化,不依赖运行时类) ----
export function isQuickCommandsModule(m: unknown): boolean {
  return typeof m === 'object' && m !== null && (m as any).type === 'quick_commands';
}
export function isDataProcessingModule(m: unknown): boolean {
  return typeof m === 'object' && m !== null && (m as any).type === 'data_processing';
}
export function isFrameBuilderTool(t: unknown): t is FrameBuilderTool {
  return typeof t === 'object' && t !== null && (t as any).kind === 'frame_builder';
}

// ---- 预置合并 ----
// loaded: 从磁盘/文件读到的模块数组;fallback: 补预置的来源(启动/同步 reload 传 [] 表示
// 以磁盘为准从 presets 补;「加载」按钮传当前 store,保住用户的帧模板不被空预置顶掉);
// presets: 调用方注入的预置列表(本文件不 import ./types,避免环)。
// 返回:按 presets 顺序排列、按 type 补齐预置的数组;loaded 里未知 type 的条目原样附在末尾。
export function mergePresetModules(loaded: any[], fallback: any[], presets: any[]): any[] {
  const result: any[] = [];
  for (const p of presets) {
    // 优先 loaded(用户文件),再 fallback(当前 store,保住模板),最后才用空预置
    let m = loaded.find((x) => x?.type === p.type) ?? fallback.find((x) => x?.type === p.type) ?? p;
    if (isDataProcessingModule(m) && !(m.tools ?? []).some(isFrameBuilderTool)) {
      m = { ...m, tools: [defaultFrameBuilderTool(), ...(m.tools ?? [])] };
    }
    if (isDataProcessingModule(m)) {
      for (const tool of m.tools ?? []) {
        if (!isFrameBuilderTool(tool)) continue;
        normalizeFrameConfig(tool.config);
        for (const template of tool.templates) normalizeFrameConfig(template.config);
      }
    }
    result.push(m);
  }
  // 未知 type 的条目原样附在末尾(不渲染、照常写回,降级再升级不丢数据)
  for (const m of loaded) {
    if (m && !presets.some((p: any) => p.type === m.type)) result.push(m);
  }
  return result;
}

// ---- 模板操作(就地改 tool.templates,调用方持有的是 $state 里的对象) ----
export function addTemplate(tool: FrameBuilderTool, name: string): void {
  const i = tool.templates.findIndex((t) => t.name === name);
  const t = { name, config: JSON.parse(JSON.stringify(tool.config)) as FrameConfig };
  if (i >= 0) tool.templates[i] = t; else tool.templates.push(t);
}
export function removeTemplate(tool: FrameBuilderTool, name: string): void {
  tool.templates = tool.templates.filter((t) => t.name !== name);
}
export function loadTemplate(tool: FrameBuilderTool, name: string): void {
  const t = tool.templates.find((x) => x.name === name);
  if (t) tool.config = normalizeFrameConfig(JSON.parse(JSON.stringify(t.config)) as FrameConfig);
}
