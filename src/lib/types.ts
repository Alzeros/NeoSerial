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
    commands: Array.from({ length: 25 }, (_, i) => defaultScriptCommand(i + 1)),
  };
}
