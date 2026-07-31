// 与 Rust 后端对齐的前端类型定义

export type Dir = 'rx' | 'tx';

export interface LogLine {
  ts: string;
  dir: Dir;
  raw: number[];
  ascii: string;
  hex: string;
  is_error: boolean;
}

export interface ConnectionState {
  connected: boolean;
  port: string | null;
}

export interface TxUpdate {
  total: number;
}

export interface RxUpdate {
  total: number;
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
    log_send: boolean;
  };
  command_groups: CommandGroup[];
  error_keywords: string[];
}

export interface ScriptCommand {
  enabled: boolean;
  command: string;
  hex: boolean;
  enter: boolean;
  delay_ms: number;
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
    delay_ms: id === 1 ? 2000 : 0,
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
