/** Content-free schemas for IPC, events and application state. Shared by every command/event. */
const PRIVATE = /key|secret|password|token|authorization|cookie|credential|text|content|command|payload|raw|ascii|hex|body/i;
const CREDENTIAL = /key|secret|password|token|authorization|cookie|credential/i;
const stringIds = new Map<string, number>();
let nextStringId = 0;
const publicEnums = new Set(['ascii','hex','utf8','gbk','Ascii','Utf8','Gbk','None','Cr','Lf','Crlf','Five','Six','Seven','Eight','One','Two','Odd','Even','Hardware','Software','visible','hidden']);
export function diagnosticSummary(value: unknown, depth = 0, field = '', seen = new WeakSet<object>()): unknown {
  try {
    if (CREDENTIAL.test(field) || (PRIVATE.test(field) && (typeof value === 'string' || Array.isArray(value)))) return { redacted: true, type: typeof value, ...(Array.isArray(value) ? { count: value.length } : {}) };
    if (value == null || typeof value === 'boolean') return value;
    if (typeof value === 'number') return Number.isFinite(value) ? value : String(value);
    if (typeof value === 'string') {
      if (value.length <= 256 && !stringIds.has(value) && stringIds.size < 4096) stringIds.set(value, ++nextStringId);
      return { type: 'string', length: value.length, token: stringIds.get(value), ...(publicEnums.has(value) ? { value } : {}) };
    }
    if (typeof value !== 'object') return { type: typeof value };
    if (typeof Element !== 'undefined' && value instanceof Element) return { type: 'element', tag: value.tagName };
    if (seen.has(value)) return { type: 'reference' };
    if (depth >= 3) return { type: Array.isArray(value) ? 'array' : 'object', ...(Array.isArray(value) ? { count: value.length } : {}) };
    seen.add(value);
    if (Array.isArray(value)) return { type: 'array', count: value.length,
      first: value.length ? diagnosticSummary(value[0], depth + 1, '', seen) : null,
      last: value.length > 1 ? diagnosticSummary(value.at(-1), depth + 1, '', seen) : null };
    if (ArrayBuffer.isView(value)) return { type: 'binary', bytes: value.byteLength };
    const result: Record<string, unknown> = {};
    for (const key of Object.keys(value).slice(0, 40)) {
      try {
        const descriptor=Object.getOwnPropertyDescriptor(value,key);
        result[key]=descriptor&&'value' in descriptor ? diagnosticSummary(descriptor.value,depth+1,key,seen) : {type:'getter'};
      }
      catch { result[key] = { type: 'unreadable' }; }
    }
    return result;
  } catch { return { type: 'unreadable' }; }
}
