/** Bounded, value-safe records. Never pass IPC arguments, results, or input values here. */
export type DiagnosticLevel = 'info' | 'warn' | 'error';
export interface FrontendRecord {
  ts: string;
  session: string;
  seq: number;
  level: DiagnosticLevel;
  kind: string;
  data: unknown;
}

const SECRET_KEY = /^(?:key|api[_-]?key|token|access[_-]?token|refresh[_-]?token|authorization|password|secret|credential|cookie)$/i;
export function redactDiagnosticText(text: string): string {
  return text
    .replace(/\b((?:Bearer|Basic)\s+)[^\s"',;]+/gi, '$1[redacted]')
    .replace(/(["']?(?:api[_-]?key|token|password|secret|authorization)["']?\s*[:=]\s*)(?:"[^"\n]*"|'[^'\n]*'|[^\s,;}]+)/gi, '$1[redacted]')
    .replace(/https?:\/\/[^\s<>"')]+/gi, raw => {
      try {
        const url = new URL(raw);
        url.username = ''; url.password = ''; url.search = ''; url.hash = '';
        return url.toString();
      } catch { return '[url]'; }
    });
}

export function diagnosticValue(value: unknown, depth = 0, seen = new WeakSet<object>()): unknown {
  try {
    if (typeof value === 'string') return redactDiagnosticText(value.slice(0, 6000));
    if (value == null || typeof value === 'boolean') return value;
    if (typeof value === 'number') return Number.isFinite(value) ? value : String(value);
    if (typeof value !== 'object') return `[${typeof value}]`;
    if (seen.has(value)) return '[circular]';
    if (depth > 3) return '[truncated]';
    seen.add(value);
    if (value instanceof Error) {
      return diagnosticValue({ name: value.name, message: value.message, stack: value.stack }, depth + 1, seen);
    }
    if (Array.isArray(value)) return value.slice(0, 16).map(item => diagnosticValue(item, depth + 1, seen));
    const result: Record<string, unknown> = {};
    // Descriptors avoid executing getters while reporting an error.
    for (const key of Object.keys(value).slice(0, 20)) {
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      result[key.slice(0, 80)] = SECRET_KEY.test(key) ? '[redacted]'
        : descriptor && 'value' in descriptor ? diagnosticValue(descriptor.value, depth + 1, seen) : '[getter]';
    }
    return result;
  } catch { return '[unserializable]'; }
}

export class FrontendRecorder {
  private queue: FrontendRecord[] = [];
  private sequence = 0;
  private dropped = 0;
  private inFlight: Promise<void> | null = null;
  private bucketStart = 0;
  private bucketCount = 0;
  private readonly session: string;
  private readonly write: (records: FrontendRecord[]) => Promise<void>;
  private readonly now: () => number;
  private readonly limits: { rate: number; queue: number };
  private immediate = new Set<Promise<void>>();
  private immediateStart = 0;
  private immediateCount = 0;

  constructor(
    session: string,
    write: (records: FrontendRecord[]) => Promise<void>,
    now: () => number = Date.now,
    limits = { rate: 100, queue: 200 },
  ) { this.session = session; this.write = write; this.now = now; this.limits = limits; }

  record(kind: string, data: unknown = {}, level: DiagnosticLevel = 'info'): void {
    try {
      const now = this.now();
      if (now - this.bucketStart >= 10000) { this.bucketStart = now; this.bucketCount = 0; }
      if (++this.bucketCount > this.limits.rate) { this.dropped++; return; }
      const entry: FrontendRecord = {
        ts: new Date(now).toISOString(), session: this.session, seq: ++this.sequence,
        level, kind: kind.slice(0, 80), data: diagnosticValue(data),
      };
      if (new TextEncoder().encode(JSON.stringify(entry)).length > 12000) entry.data = '[record exceeded 12 KB]';
      this.queue.push(entry);
      if (this.queue.length > this.limits.queue) { this.queue.shift(); this.dropped++; }
    } catch { /* Diagnostics must never break the UI. */ }
  }

  /** A real input must reach Rust before its handler can block JS, even if another write awaits ACK. */
  recordImmediate(kind: string, data: unknown, level: DiagnosticLevel = 'info'): void {
    try {
      const now = this.now();
      if (now - this.immediateStart >= 10000) { this.immediateStart = now; this.immediateCount = 0; }
      if (++this.immediateCount > 100 || this.immediate.size >= 32) {
        this.record(kind, data, level); void this.flush().catch(() => {}); return;
      }
      const entry: FrontendRecord = { ts: new Date(now).toISOString(), session: this.session, seq: ++this.sequence,
        kind: kind.slice(0, 80), level, data: diagnosticValue(data) };
      if (new TextEncoder().encode(JSON.stringify(entry)).length > 12000) entry.data = '[record exceeded 12 KB]';
      // Calling write synchronously submits the native message before returning to event dispatch.
      const pending = this.write([entry]).catch(() => {
        this.queue.push(entry);
        if (this.queue.length > this.limits.queue) { this.queue.shift(); this.dropped++; }
      }).finally(() => { this.immediate.delete(pending); });
      this.immediate.add(pending);
    } catch { /* A diagnostic transport failure must never swallow the user's input. */ }
  }

  /** One IPC at a time. On failure keep a bounded queue for the next scheduled retry. */
  flush(): Promise<void> {
    if (this.inFlight) return this.inFlight;
    if (!this.queue.length && !this.dropped && !this.immediate.size) return Promise.resolve();
    const run = async () => {
      while (this.queue.length || this.dropped || this.immediate.size) {
        if (this.immediate.size) await Promise.all([...this.immediate]);
        if (!this.queue.length && !this.dropped) continue;
        const batch = this.queue.splice(0, 20);
        const dropped = this.dropped;
        this.dropped = 0;
        if (dropped) batch.unshift({
          ts: new Date(this.now()).toISOString(), session: this.session, seq: ++this.sequence,
          level: 'warn', kind: 'diagnostics.dropped', data: { count: dropped },
        });
        try { await this.write(batch); }
        catch (error) {
          this.dropped += dropped;
          this.queue = [...batch.filter(entry => entry.kind !== 'diagnostics.dropped'), ...this.queue];
          if (this.queue.length > this.limits.queue) {
            this.dropped += this.queue.length - this.limits.queue;
            this.queue = this.queue.slice(-this.limits.queue);
          }
          throw error;
        }
      }
    };
    this.inFlight = run().finally(() => { this.inFlight = null; });
    return this.inFlight;
  }
}
