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
/** connect 命令的 stopBits 参数：后端 u8（1|2）。 */
export type StopBits = 1 | 2;
/** settings.json 里 serial_defaults.stop_bits 的形态：后端 StopBits 枚举 serde PascalCase。
 *  与 connect 命令的数字形态不同，两边转换见 App.svelte 的 applySettings / buildSettingsFromUi。 */
export type SettingsStopBits = 'One' | 'Two';
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

/** settings.json 的 command_index 段。与后端 CommandIndexSettings 对齐。 */
export interface CommandIndexSettings {
  /** 知识库服务器地址,如 http://10.12.16.11:8200;空 = 未配置,联想只用发送历史 */
  base_url: string;
  api_key: string;
  /** 手册 id 排除名单:不在此列的手册都参与候选 */
  disabled_doc_ids: number[];
  /** 启动时后台刷新一次缓存 */
  auto_refresh: boolean;
  /** 联想总开关 */
  suggest_enabled: boolean;
}

export interface Settings {
  version: number;
  window: { width: number; height: number; x: number; y: number };
  serial_defaults: {
    baud_rate: number;
    data_bits: DataBits;
    parity: Parity;
    stop_bits: SettingsStopBits;
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
    /** 后台运行(托盘常驻):关窗口连接不断、关最后一个窗口应用仍在;
     *  关 = 无托盘,关窗口断开自己连的,关最后一个窗口即退出 */
    background_mode: boolean;
    /** 首次收进后台的系统通知是否已发过。后端写,前端只透传 */
    tray_hint_shown: boolean;
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
  command_index: CommandIndexSettings;
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
