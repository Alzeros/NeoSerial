// 帧构造器:类型 + 默认值 + 预置合并 + 模板操作。全部纯函数,供 node 测试。
// FrameConfig 与后端 dataproc/frame.rs 对齐(snake_case)。

export type Endian = 'le' | 'be';
export interface LengthFieldSpec { size: 'none' | 'one' | 'two'; endian: Endian; coverage: 'data' | 'data_plus_checksum' | 'whole_frame' }
export interface DataSpec {
  mode: 'content' | 'fill'; format: 'hex' | 'text'; value: string;
  pattern: 'zeros' | 'ff' | 'random_bytes' | 'random_text' | 'increment' | 'custom_loop';
  charset: string; start: number; custom_format: 'hex' | 'text'; custom_value: string;
  length_basis: 'data_field' | 'whole_frame'; length: number;
}
export interface ChecksumSpec {
  algo: 'none' | 'crc16_modbus' | 'crc16_ccitt_false' | 'crc16_xmodem' | 'crc16_arc' | 'crc32' | 'sum8' | 'xor8';
  endian: Endian; include_header: boolean; include_length: boolean;
}
export interface FrameConfig { header_hex: string; length: LengthFieldSpec; data: DataSpec; checksum: ChecksumSpec }

export interface FrameTemplate { name: string; config: FrameConfig }
export interface FrameBuilderTool { id: string; kind: 'frame_builder'; config: FrameConfig; templates: FrameTemplate[] }
export type DataTool = FrameBuilderTool;

export function defaultFrameConfig(): FrameConfig {
  return {
    header_hex: '',
    length: { size: 'none', endian: 'le', coverage: 'data' },
    data: { mode: 'content', format: 'hex', value: '', pattern: 'zeros', charset: 'A-Za-z0-9', start: 0, custom_format: 'hex', custom_value: '', length_basis: 'data_field', length: 0 },
    checksum: { algo: 'none', endian: 'le', include_header: false, include_length: false },
  };
}

export function defaultFrameBuilderTool(): FrameBuilderTool {
  return { id: 'frame_builder', kind: 'frame_builder', config: defaultFrameConfig(), templates: [] };
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
  if (t) tool.config = JSON.parse(JSON.stringify(t.config)) as FrameConfig;
}
