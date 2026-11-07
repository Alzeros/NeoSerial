import { invoke as nativeInvoke, type InvokeArgs, type InvokeOptions } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { emit } from '@tauri-apps/api/event';
import { FrontendRecorder, type DiagnosticLevel } from './frontendDiagnosticsModel';
import { describeDiagnosticElement } from './frontendDomDiagnostics';
import { observeDiagnosticBridge } from './diagnosticBridge';
import { observeDiagnosticNetwork } from './diagnosticNetwork';
import { observeDiagnosticState } from './diagnosticState';

let recorder: FrontendRecorder | null = null;
let bridgeObserver: ReturnType<typeof observeDiagnosticBridge> | null = null;
let replay: { flush(): Promise<void>; nodeId(node: Node): number } | null = null;
let replayReady: Promise<void> = Promise.resolve();
const states: ReturnType<typeof observeDiagnosticState>[] = [];

export function registerFrontendState(name: string, module: Record<string, unknown>): void {
  if (recorder) states.push(observeDiagnosticState(name, module, recordFrontend));
}

export function recordFrontend(kind: string, data: unknown = {}, level: DiagnosticLevel = 'info', urgent = false, direct = false): void {
  if (direct) { recorder?.recordImmediate(kind, data, level); return; }
  recorder?.record(kind, data, level);
  if (urgent) void recorder?.flush().catch(() => {});
}

/** Record names, timing and failure only. Arguments/results may contain credentials or serial data. */
export async function diagnosticInvoke<T>(command: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  return nativeInvoke<T>(command, args, options);
}

export async function flushFrontendDiagnostics(): Promise<void> {
  if (!recorder) return;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      (async () => {
        await replayReady;
        states.forEach(state => state.sample());
        bridgeObserver?.flushTraffic();
        await replay?.flush();
        await recorder!.flush();
      })(),
      new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error('前端日志保存超时，请稍后重试')), 5000); }),
    ]);
  } finally { clearTimeout(timer); }
}

function viewport() {
  return {
    width: innerWidth, height: innerHeight, scale: devicePixelRatio,
    screen_width: screen.width, screen_height: screen.height,
    visibility: document.visibilityState, focused: document.hasFocus(),
  };
}

/** Runs before importing Svelte/components, in every WebView including settings/theme editor. */
export function installFrontendDiagnostics(): void {
  if (recorder) return;
  const label = getCurrentWebview().label;
  const session = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  recorder = new FrontendRecorder(session, records => nativeInvoke('append_frontend_diagnostics', { records }), Date.now, { rate: 1000, queue: 1000 });
  const bridge = (window as unknown as { __TAURI_INTERNALS__: Parameters<typeof observeDiagnosticBridge>[0] }).__TAURI_INTERNALS__;
  try { bridgeObserver = observeDiagnosticBridge(bridge, recordFrontend); }
  catch (error) { recordFrontend('diagnostics.capability_failed', { capability: 'central_ipc', error }, 'error', true); }
  try { observeDiagnosticNetwork(recordFrontend); }
  catch (error) { recordFrontend('diagnostics.capability_failed', { capability: 'network_observer', error }, 'error', true); }
  replayReady = import('./frontendReplay').then(({ startFrontendReplay }) => {
    replay = startFrontendReplay(session, (segment, events) => nativeInvoke('append_frontend_replay', { session, segment, events }), recordFrontend);
  }).catch(error => recordFrontend('replay.start_failed', { error }, 'error', true));
  void getCurrentWebview().listen<{ reply_event: string }>('frontend-diagnostics-flush', event => {
    void flushFrontendDiagnostics().then(
      () => emit(event.payload.reply_event, { window: label, ok: true }),
      () => emit(event.payload.reply_event, { window: label, ok: false }),
    ).catch(() => {});
  }).catch(error => recordFrontend('diagnostics.flush_listener_failed', { error }, 'warn'));

  for (const method of ['log', 'info', 'warn', 'error', 'debug'] as const) {
    const original = console[method].bind(console);
    console[method] = (...args: unknown[]) => {
      original(...args);
      recordFrontend(`console.${method}`, args, method === 'error' ? 'error' : method === 'warn' ? 'warn' : 'info', method === 'error');
    };
  }
  window.addEventListener('error', (event: Event) => {
    if (event instanceof ErrorEvent) {
      recordFrontend('window.error', {
        message: event.message, error: event.error, filename: event.filename, line: event.lineno, column: event.colno,
      }, 'error', true);
    } else {
      const target = event.target;
      if (target instanceof HTMLScriptElement || target instanceof HTMLLinkElement || target instanceof HTMLImageElement) {
        recordFrontend('resource.error', { tag: target.tagName, source: target instanceof HTMLLinkElement ? target.href : target.src }, 'error', true);
      }
    }
  }, true);
  window.addEventListener('unhandledrejection', event => recordFrontend('promise.unhandled', { error: event.reason }, 'error', true));
  for (const event of ['focus', 'blur', 'pageshow', 'pagehide'] as const) {
    window.addEventListener(event, () => recordFrontend(`window.${event}`, viewport(), 'info', true));
  }
  document.addEventListener('visibilitychange', () => recordFrontend('window.visibility', viewport(), 'info', true));
  let resizeTimer: ReturnType<typeof setTimeout>;
  window.addEventListener('resize', () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => recordFrontend('window.resize', viewport()), 300);
  });

  // Structural breadcrumbs only: no textContent, title, input values, selected options, or typed keys.
  const interaction = (event: Event) => {
    try {
      if (!(event.target instanceof Element)) return;
      const target = event.target.closest('button, input, textarea, select, [role="button"], [role="tab"], [role="option"]') ?? event.target;
      const mouse = event instanceof MouseEvent ? event : null;
      const hasPoint = mouse && (event.type === 'pointerdown' || mouse.detail > 0);
      const underPoint = hasPoint ? Array.from(document.querySelectorAll('[data-diagnostic-control]')).filter(element => {
        const rect = element.getBoundingClientRect();
        return mouse.clientX >= rect.left && mouse.clientX < rect.right && mouse.clientY >= rect.top && mouse.clientY < rect.bottom;
      }).slice(0, 3).map(element => {
        const rect = element.getBoundingClientRect();
        return { ...describeDiagnosticElement(element), rect: { x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height) } };
      }) : [];
      recordFrontend(`ui.${event.type}`, {
        ...describeDiagnosticElement(target),
        replay_node: replay?.nodeId(target) ?? null,
        tab: target.getAttribute('data-module-tab'),
        trusted: event.isTrusted, detail: mouse?.detail,
        point: hasPoint ? { x: mouse.clientX, y: mouse.clientY } : null,
        hit_stack: hasPoint ? document.elementsFromPoint(mouse.clientX, mouse.clientY).slice(0, 4).map(describeDiagnosticElement) : [],
        controls_under_point: underPoint,
        active_element: describeDiagnosticElement(document.activeElement),
        type: target instanceof HTMLInputElement ? target.type : undefined,
        checked: target instanceof HTMLInputElement && ['checkbox', 'radio'].includes(target.type) ? target.checked : undefined,
      }, 'info', true, event.isTrusted);
    } catch (error) { recordFrontend('diagnostics.input_probe_failed', { error }, 'warn'); }
  };
  // Capture even non-interactive overlays and disabled controls, before component handlers run.
  window.addEventListener('pointerdown', interaction, true);
  document.addEventListener('click', interaction, true);
  document.addEventListener('change', interaction, true);
  document.addEventListener('keydown', event => {
    if (['Escape', 'Enter', 'F3', 'F11'].includes(event.key) || ((event.ctrlKey || event.metaKey) && ['f', ',', 's'].includes(event.key.toLowerCase()))) {
      recordFrontend('ui.shortcut', { key: event.key, ctrl: event.ctrlKey, meta: event.metaKey, shift: event.shiftKey, alt: event.altKey }, 'info', true, event.isTrusted);
    }
  }, true);

  const supportsLongTasks = typeof PerformanceObserver !== 'undefined' && PerformanceObserver.supportedEntryTypes?.includes('longtask');
  if (supportsLongTasks) {
    try {
      const observer = new PerformanceObserver(list => {
        for (const entry of list.getEntries()) if (entry.duration >= 200) {
          recordFrontend('performance.longtask', { start_ms: Math.round(entry.startTime), duration_ms: Math.round(entry.duration) }, 'warn');
        }
      });
      observer.observe({ type: 'longtask', buffered: true });
    } catch { /* Not all WebView runtimes implement the same entry types. */ }
  }
  let lastBeat = performance.now();
  let lastVisibility = document.visibilityState;
  setInterval(() => {
    const now = performance.now();
    const gap_ms = Math.round(now - lastBeat);
    const delayed = gap_ms > 20000 && lastVisibility === 'visible' && document.visibilityState === 'visible';
    const memory = (performance as Performance & {memory?: {usedJSHeapSize:number;totalJSHeapSize:number;jsHeapSizeLimit:number}}).memory;
    recordFrontend('window.heartbeat', { ...viewport(), gap_ms, delayed,
      heap: memory ? { used: memory.usedJSHeapSize, total: memory.totalJSHeapSize, limit: memory.jsHeapSizeLimit } : null,
      note: delayed ? '可能为主线程停顿、系统休眠或调度延迟' : undefined }, delayed ? 'warn' : 'info');
    lastBeat = now; lastVisibility = document.visibilityState;
  }, 15000);
  setInterval(() => { void recorder?.flush().catch(() => {}); }, 1000);
  setInterval(() => bridgeObserver?.flushTraffic(), 5000);
  let heartbeatPending = false;
  const heartbeat = () => {
    if (heartbeatPending) return;
    heartbeatPending = true;
    void nativeInvoke('frontend_diagnostics_heartbeat', { session, visibility: document.visibilityState })
      .catch(() => {}).finally(() => { heartbeatPending = false; });
  };
  heartbeat(); setInterval(heartbeat, 2000);
  document.addEventListener('visibilitychange', heartbeat);
  recordFrontend('frontend.start', {
    build: __NEOSERIAL_BUILD__, window: label, user_agent: navigator.userAgent,
    language: navigator.language, platform: navigator.platform, timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    long_tasks_supported: Boolean(supportsLongTasks), central_ipc: Boolean(bridgeObserver), automatic_dom_replay: true, network_observer: true, native_heartbeat: true, ...viewport(),
  }, 'info', true);
}
