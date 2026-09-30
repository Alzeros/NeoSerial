import type { LogLine } from './types';

export const DEFAULT_MAX_LOG_LINES = 5000;
export const MIN_LOG_LINES = 1000;
export const MAX_LOG_LINES_LIMIT = 10_000;
export const LOG_MIB = 1024 * 1024;
export const DEFAULT_MAX_LOG_BYTES = 4 * LOG_MIB;
export const MIN_LOG_BYTES = LOG_MIB;
export const MAX_LOG_BYTES_LIMIT = 16 * LOG_MIB;

export interface LogLimits { lines: number; bytes: number }

function bounded(value: unknown, fallback: number, min: number, max: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(max, Math.max(min, Math.floor(value))) : fallback;
}

export function resolveLogLimits(ui?: {ring_buffer_capacity?: unknown; log_buffer_max_bytes?: unknown}): LogLimits {
  return {
    lines: bounded(ui?.ring_buffer_capacity, DEFAULT_MAX_LOG_LINES, MIN_LOG_LINES, MAX_LOG_LINES_LIMIT),
    bytes: bounded(ui?.log_buffer_max_bytes, DEFAULT_MAX_LOG_BYTES, MIN_LOG_BYTES, MAX_LOG_BYTES_LIMIT),
  };
}

function omitOversized(line: LogLine): LogLine {
  return {
    ...line, raw: [], omitted_bytes: line.raw.length,
    ascii: `日志内容过大，已省略 ${line.raw.length.toLocaleString()} 字节。`,
  };
}

/** 按到达时的原始字节数计量；生产日志的 raw payload 到达后不再编辑。
 * 只在追加/淘汰时增减计数，不在每条到达时扫描全部缓存。 */
export class LogRetentionBuffer {
  readonly lines: LogLine[] = [];
  private costs = new WeakMap<LogLine, number>();
  private bytes = 0;
  private wrap: (line: LogLine) => LogLine;
  constructor(wrap: (line: LogLine) => LogLine = line => line) { this.wrap = wrap; }

  get rawBytes(): number { return this.bytes; }

  private prepare(line: LogLine, limits: LogLimits): LogLine {
    const record = this.wrap(line.raw.length > limits.bytes ? omitOversized(line) : line);
    const size = record.raw.length;
    this.costs.set(record, size);
    this.bytes += size;
    return record;
  }

  private enforce(limits: LogLimits): boolean {
    let drop = 0;
    while (drop < this.lines.length &&
      (this.lines.length - drop > limits.lines || this.bytes > limits.bytes)) {
      this.bytes -= this.costs.get(this.lines[drop])!;
      drop++;
    }
    if (drop === 0) return false;
    this.lines.splice(0, drop);
    return true;
  }

  append(batch: LogLine[], limits: LogLimits): void {
    for (const line of batch) this.lines.push(this.prepare(line, limits));
    this.enforce(limits);
  }

  insert(at: number, batch: LogLine[], limits: LogLimits): void {
    const records = batch.map(line => this.prepare(line, limits));
    this.lines.splice(Math.max(0, Math.min(at, this.lines.length)), 0, ...records);
    this.enforce(limits);
  }

  trim(limits: LogLimits): boolean {
    let changed = false;
    // 调低大小上限时，已有超大记录同样改成省略提示，避免它挤掉全部历史。
    for (let i = 0; i < this.lines.length; i++) {
      const line = this.lines[i];
      if (this.costs.get(line)! <= limits.bytes) continue;
      this.bytes -= this.costs.get(line)!;
      this.lines[i] = this.prepare(omitOversized(line), limits);
      changed = true;
    }
    return this.enforce(limits) || changed;
  }

  clear(): void {
    this.lines.length = 0;
    this.bytes = 0;
    this.costs = new WeakMap();
  }
}
