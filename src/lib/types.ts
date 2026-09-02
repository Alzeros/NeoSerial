// 与 Rust 后端对齐的前端类型定义

export type Dir = 'rx' | 'tx';

export interface LogLine {
  ts: string;
  dir: Dir;
  raw: number[];
  ascii: string;
  is_error: boolean;
  /** 本次连接期间的行号(tx+rx 共用,开端口=1)。0 表示旧数据(前端显示为空)。 */
  line_index: number;
}

export interface ConnectionState {
  connected: boolean;
  port: string | null;
  /** 波特率(连接成功时带;断开为 null)。供回填波特率下拉框。 */
  baud_rate: number | null;
}

export interface TxUpdate {
  total: number;
  port: string;
}

export interface RxUpdate {
  total: number;
  port: string;
}

export interface ErrorEvent {
  message: string;
}

export interface SequenceProgress {
  row: number;
  total: number;
}

export interface SequenceDone {
  aborted: boolean;
}

export interface ConnectionMode {
  mode: 'independent' | 'shared';
}

/** 副窗口 onMount 调 get_window_conn_state 的返回:本窗口对应 port 的连接状态。
 *  main 窗口(port=null)返回 connected:false,由它自己 connect 流程管理。 */
export interface WindowConnState {
  ok: boolean;
  port: string | null;
  connected: boolean;
  baud: number | null;
  tx_bytes: number | null;
  rx_bytes: number | null;
}

export type Parity = 'None' | 'Odd' | 'Even';
export type DataBits = 'Five' | 'Six' | 'Seven' | 'Eight';
export type StopBits = 1 | 2;
export type FlowControl = 'None' | 'Software' | 'Hardware';
export type LineEnding = 'None' | 'Cr' | 'Lf' | 'Crlf';

export interface ConnectionParams {
  port: string;
  baud_rate: number;
  data_bits: DataBits;
  parity: Parity;
  stop_bits: StopBits;
  flow_control: FlowControl;
}

export interface CommandItem {
  name: string;
  command: string;
  hex: boolean;
  enter: boolean;
}

export interface CommandGroup {
  name: string;
  items: CommandItem[];
}

export interface Settings {
  version: number;
  window: { width: number; height: number; x: number; y: number };
  serial_defaults: {
    baud_rate: number;
    data_bits: DataBits;
    parity: Parity;
    stop_bits: StopBits;
    flow_control: FlowControl;
  };
  last_port: string;
  ui: {
    display_mode: 'Ascii' | 'Hex';
    line_ending: LineEnding;
    auto_scroll: boolean;
    ring_buffer_capacity: number;
    show_timestamp: boolean;
    /** 日志区最左侧行号(本次连接期间 index)开关 */
    show_line_index: boolean;
    log_send: boolean;
    log_font_size: number;
    log_line_height: number;
    log_dir_label: string;
    /** 日志区英文字体族：'default' 或 CSS font-family 值 */
    log_font_latin: string;
    /** 日志区中文字体族：'default'=跟随英文，或 CSS font-family 值 */
    log_font_cjk: string;
    text_encoding: 'Ascii' | 'Utf8' | 'Gbk';
    /** 关闭主窗口时最小化到系统托盘（保持 MCP 服务运行） */
    minimize_to_tray: boolean;
    /** 是否已弹过首次关闭提示（true=不再弹） */
    close_prompted: boolean;
  };
  command_groups: CommandGroup[];
  error_keywords: string[];
  presets: {
    baud_rates: number[];
    theme: string;
    /** 自定义主题色板：变量名 → 颜色值/数值。空对象 = 未配置过 */
    custom_theme: Record<string, string>;
  };
  mcp: {
    /** 打开软件时是否自动启动 MCP server,改后重启生效 */
    auto_start: boolean;
    /** MCP server 监听端口(默认 23333),改后需重新 claude mcp add */
    port: number;
  };
}

export interface ScriptCommand {
  enabled: boolean;
  command: string;
  hex: boolean;
  enter: boolean;
  delay_ms: number;
  note: string;
}

export interface ScriptPage {
  name: string;
  commands: ScriptCommand[];
}

/** 脚本模块：右栏一个独立功能区，是 Page 之上的分组层。
 *  当前仅快捷指令一种类型，后续可扩展自动化测试等。 */
export interface ScriptModule {
  id: string;
  name: string;
  type: 'quick_commands';
  pages: ScriptPage[];
}

export function defaultScriptCommand(id: number): ScriptCommand {
  return {
    enabled: true,
    command: '',
    hex: false,
    enter: true,
    delay_ms: 0,
    note: '',
  };
}

export function defaultScriptPage(name: string): ScriptPage {
  return {
    name,
    commands: Array.from({ length: 10 }, (_, i) => defaultScriptCommand(i + 1)),
  };
}

let moduleIdSeq = 0;
export function defaultScriptModule(name = '快捷指令'): ScriptModule {
  moduleIdSeq += 1;
  return {
    id: `module_${moduleIdSeq}`,
    name,
    type: 'quick_commands',
    pages: [defaultScriptPage('Page0')],
  };
}

/** 预置模块列表（代码写死，用户不可增删）。
 *  快捷指令是初始功能；后续扩展新功能模块在此追加，
 *  并在 ScriptSequencer 按 type 分发到对应子组件。 */
export function presetScriptModules(): ScriptModule[] {
  return [
    {
      id: 'quick_commands',
      name: '快捷指令',
      type: 'quick_commands',
      pages: [defaultScriptPage('Page0')],
    },
  ];
}
