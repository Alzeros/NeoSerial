import { Channel, type InvokeArgs, type InvokeOptions } from '@tauri-apps/api/core';
import { diagnosticSummary } from './diagnosticSummary.ts';
import type { DiagnosticLevel } from './frontendDiagnosticsModel';

type RecordEvent = (kind: string, data?: unknown, level?: DiagnosticLevel, urgent?: boolean) => void;
interface Bridge {
  invoke<T>(command: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T>;
  transformCallback?(callback?: (value: unknown) => unknown, once?: boolean): number;
  unregisterCallback?(id: number): void;
  __neoserialDiagnostics?: {
    ready: boolean;
    raw: Bridge;
    connect(client: Bridge): () => void;
  };
}
const isDiagnostic = (command: string, args: unknown) => /^(append_frontend_|frontend_diagnostics_heartbeat)/.test(command)
  || (command.startsWith('plugin:event|') && typeof args === 'object' && args !== null
    && 'event' in args && String(args.event).startsWith('frontend-diagnostics'));
interface BridgeObserver {
  rawInvoke: Bridge['invoke'];
  flushTraffic(): void;
  restore(): void;
}

/** Central Tauri transport hook: also covers future commands, plugins and event listeners. */
export function observeDiagnosticBridge(bridge: Bridge, record: RecordEvent): BridgeObserver {
  if (bridge.__neoserialDiagnostics) {
    const bootstrap = bridge.__neoserialDiagnostics;
    if (!bootstrap.ready) throw new Error('Tauri 诊断入口尚未完成初始化');
    // Production methods are immutable. The native bootstrap delegates to this
    // private mutable client without changing any Tauri property descriptors.
    const client = { ...bootstrap.raw };
    const observer = observeDiagnosticBridge(client, record);
    const disconnect = bootstrap.connect(client);
    return { rawInvoke: observer.rawInvoke, flushTraffic: observer.flushTraffic,
      restore() { disconnect(); observer.restore(); } };
  }
  for (const key of ['invoke', 'transformCallback', 'unregisterCallback']) {
    const descriptor = Object.getOwnPropertyDescriptor(bridge, key);
    if (descriptor && ('value' in descriptor ? !descriptor.writable : !descriptor.set)) {
      throw new Error(`Tauri ${key} 只读；诊断初始化脚本未接入`);
    }
  }
  const invoke = bridge.invoke.bind(bridge);
  const callback = bridge.transformCallback?.bind(bridge);
  const unregister = bridge.unregisterCallback?.bind(bridge);
  const listeners = new Map<number, string>();
  const eventIds = new Map<number, number>();
  const traffic = new Map<string, { count: number; first: number; last: number; errors: number; handler_ms: number }>();
  let request = 0;
  if (callback) bridge.transformCallback = (fn, once) => {
    let id = 0;
    id = callback(value => {
      const event = listeners.get(id);
      const start = performance.now();
      let current: { count: number; first: number; last: number; errors: number; handler_ms: number } | undefined;
      if (event) {
        current = traffic.get(event) ?? { count: 0, first: Date.now(), last: Date.now(), errors: 0, handler_ms: 0 };
        current.count++; current.last = Date.now(); traffic.set(event, current);
        if (current.count <= 5) {
          const payload = value && typeof value === 'object' && 'payload' in value ? value.payload : value;
          record('event.received', { event, listener: id, schema: diagnosticSummary(payload) });
        }
      }
      try { return fn?.(value); }
      catch (error) {
        if (current) current.errors++;
        record('event.handler_error', { event, listener: id, error }, 'error', true);
        throw error;
      } finally {
        if (current) current.handler_ms += Math.round(performance.now() - start);
      }
    }, once);
    return id;
  };
  if (unregister) bridge.unregisterCallback = id => { listeners.delete(id); unregister(id); };
  bridge.invoke = async <T>(command: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> => {
    if (isDiagnostic(command, args)) return invoke<T>(command, args, options);
    const number = ++request;
    const start = performance.now();
    const object = args && typeof args === 'object' ? args as Record<string, unknown> : {};
    const event = typeof object.event === 'string' ? object.event.slice(0, 160) : undefined;
    const handler = typeof object.handler === 'number' ? object.handler : undefined;
    const channels: number[] = [];
    const trackChannels = (value: unknown, path: string, depth: number) => {
      if (value instanceof Channel) {
        listeners.set(value.id, `channel:${command}:${path}`.slice(0, 160)); channels.push(value.id); return;
      }
      if (!value || typeof value !== 'object' || depth >= 2 || Array.isArray(value)) return;
      for (const key of Object.keys(value).slice(0, 20)) {
        try {
          const descriptor=Object.getOwnPropertyDescriptor(value,key);
          if(descriptor&&'value' in descriptor) trackChannels(descriptor.value,path?`${path}.${key}`:key,depth+1);
        } catch { /* Schema-only observation. */ }
      }
    };
    trackChannels(args, '', 0);
    if (command === 'plugin:event|listen' && handler !== undefined && event) listeners.set(handler, event);
    if (command === 'plugin:event|unlisten' && typeof object.eventId === 'number') {
      const registered = eventIds.get(object.eventId);
      if (registered !== undefined) listeners.delete(registered);
      eventIds.delete(object.eventId);
    }
    record('ipc.start', { command, request: number, event, args_schema: diagnosticSummary(args) }, 'info', true);
    const pending = setTimeout(() => record('ipc.pending', { command, request: number, elapsed_ms: Math.round(performance.now() - start) }, 'warn', true), 5000);
    try {
      const result = await invoke<T>(command, args, options);
      if (command === 'plugin:event|listen' && handler !== undefined && typeof result === 'number') eventIds.set(result, handler);
      record('ipc.ok', { command, request: number, elapsed_ms: Math.round(performance.now() - start), result_schema: diagnosticSummary(result) });
      return result;
    } catch (error) {
      channels.forEach(id => listeners.delete(id));
      if (handler !== undefined) listeners.delete(handler);
      record('ipc.error', { command, request: number, elapsed_ms: Math.round(performance.now() - start), error }, 'error', true);
      throw error;
    } finally { clearTimeout(pending); }
  };
  return {
    rawInvoke: invoke,
    flushTraffic() {
      for (const [event, summary] of traffic) record('event.traffic', { event, ...summary });
      traffic.clear();
    },
    restore() {
      bridge.invoke = invoke;
      if (callback) bridge.transformCallback = callback;
      if (unregister) bridge.unregisterCallback = unregister;
    },
  };
}
