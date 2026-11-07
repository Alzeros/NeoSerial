import { redactDiagnosticText, type DiagnosticLevel } from './frontendDiagnosticsModel.ts';
type RecordEvent = (kind: string, data?: unknown, level?: DiagnosticLevel, urgent?: boolean) => void;
const safeUrl = (input: string) => {
  try {
    const url = new URL(input, location.href);
    if (url.protocol === 'data:') return 'data:[redacted]';
    url.username = ''; url.password = ''; url.search = ''; url.hash = '';
    return url.toString();
  } catch { return redactDiagnosticText(input); }
};
const byteLength = (value: unknown) => typeof value === 'string' ? value.length
  : value instanceof Blob ? value.size : value instanceof ArrayBuffer ? value.byteLength
  : ArrayBuffer.isView(value) ? value.byteLength : null;

// Tauri's injected IPC transport uses the global fetch too. Observing it would
// make each diagnostic write generate more writes; commands are covered by the bridge.
const nativeIpc = (url: string) => {
  try { const parsed = new URL(url, location.href); return parsed.protocol === 'ipc:' || parsed.hostname === 'ipc.localhost'; }
  catch { return false; }
};

export function observeDiagnosticNetwork(record: RecordEvent) {
  const fetch = window.fetch;
  window.fetch = async function(input, init) {
    const start = performance.now();
    const url = safeUrl(input instanceof Request ? input.url : String(input));
    if (nativeIpc(url)) return fetch.call(this, input, init);
    const method = init?.method ?? (input instanceof Request ? input.method : 'GET');
    record('network.start', { url, method });
    try {
      const response = await fetch.call(this, input, init);
      record('network.end', { url, method, status: response.status, elapsed_ms: Math.round(performance.now() - start) }, response.ok ? 'info' : 'warn');
      return response;
    } catch (error) {
      record('network.error', { url, method, elapsed_ms: Math.round(performance.now() - start), error }, 'error', true);
      throw error;
    }
  };
  const open = XMLHttpRequest.prototype.open;
  const send = XMLHttpRequest.prototype.send;
  const requests = new WeakMap<XMLHttpRequest, { method: string; url: string }>();
  XMLHttpRequest.prototype.open = function(this: XMLHttpRequest, method: string, url: string | URL, ...rest: unknown[]) {
    const args = [method, url, ...rest];
    requests.set(this, { method, url: safeUrl(String(url)) });
    return Reflect.apply(open, this, args);
  };
  XMLHttpRequest.prototype.send = function(body) {
    const info = requests.get(this);
    if (info && nativeIpc(info.url)) return send.call(this, body);
    const start = performance.now();
    record('network.xhr_start', info);
    this.addEventListener('loadend', () => record('network.xhr_end', { ...info, status: this.status, elapsed_ms: Math.round(performance.now() - start) }, this.status >= 200 && this.status < 400 ? 'info' : 'warn'), { once: true });
    return send.call(this, body);
  };
  const WebSocketClass = window.WebSocket;
  const EventSourceClass = window.EventSource;
  const WorkerClass = window.Worker;
  let connection = 0;
  if (WebSocketClass) window.WebSocket = new Proxy(WebSocketClass, {
    construct(target, args, newTarget) {
      const socket = Reflect.construct(target, args, newTarget) as WebSocket;
      const id = ++connection, url = safeUrl(String(args[0]));
      let received = 0, sent = 0, received_bytes = 0;
      record('network.websocket_start', { id, url });
      const timer = setInterval(() => {
        record('network.websocket_traffic', { id, ready_state: socket.readyState, received, sent, received_bytes });
        received = 0; sent = 0; received_bytes = 0;
      }, 5000);
      socket.addEventListener('open', () => record('network.websocket_open', { id }));
      socket.addEventListener('message', event => { received++; received_bytes += byteLength(event.data) ?? 0; });
      socket.addEventListener('error', () => record('network.websocket_error', { id, ready_state: socket.readyState }, 'warn'));
      socket.addEventListener('close', event => { clearInterval(timer); record('network.websocket_close', { id, code: event.code, clean: event.wasClean, received, sent, received_bytes }); });
      const send = socket.send;
      socket.send = function(data) { sent++; record('network.websocket_send', { id, bytes: byteLength(data) }); return send.call(this, data); };
      return socket;
    },
  });
  if (EventSourceClass) window.EventSource = new Proxy(EventSourceClass, {
    construct(target, args, newTarget) {
      const source = Reflect.construct(target, args, newTarget) as EventSource;
      const id = ++connection, url = safeUrl(String(args[0]));
      let received = 0;
      record('network.eventsource_start', { id, url });
      const timer = setInterval(() => { record('network.eventsource_traffic', { id, received, ready_state: source.readyState }); received = 0; }, 5000);
      source.addEventListener('open', () => record('network.eventsource_open', { id }));
      source.addEventListener('message', () => received++);
      source.addEventListener('error', () => { record('network.eventsource_error', { id, ready_state: source.readyState }, 'warn'); if (source.readyState === 2) clearInterval(timer); });
      const close = source.close;
      source.close = function() { clearInterval(timer); record('network.eventsource_close', { id, received }); return close.call(this); };
      return source;
    },
  });
  if (WorkerClass) window.Worker = new Proxy(WorkerClass, {
    construct(target, args, newTarget) {
      const worker = Reflect.construct(target, args, newTarget) as Worker;
      const id = ++connection;
      record('worker.started', { id, url: safeUrl(String(args[0])) });
      worker.addEventListener('error', event => record('worker.error', { id, message: event.message, line: event.lineno, column: event.colno }, 'error', true));
      worker.addEventListener('messageerror', () => record('worker.message_error', { id }, 'error', true));
      const terminate = worker.terminate;
      worker.terminate = function() { record('worker.terminated', { id }); return terminate.call(this); };
      return worker;
    },
  });
  return () => {
    window.fetch = fetch; XMLHttpRequest.prototype.open = open; XMLHttpRequest.prototype.send = send;
    window.WebSocket = WebSocketClass; window.EventSource = EventSourceClass; window.Worker = WorkerClass;
  };
}
