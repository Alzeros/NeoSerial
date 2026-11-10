import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { Channel } from '@tauri-apps/api/core';
import { observeDiagnosticBridge } from '../src/lib/diagnosticBridge.ts';

// Exact upstream initialization scripts, including immutable property descriptors.
// Updating the locked Tauri version requires refreshing and testing these fixtures.
const fixtureVersion = '2.11.5';
const load = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const bootstrap = load('../src-tauri/src/logging/bridge-bootstrap.js');
const core = load(`./fixtures/tauri-core-${fixtureVersion}.js`);
const transport = load(`./fixtures/tauri-ipc-protocol-${fixtureVersion}.js`);

for (const os of ['windows', 'macos', 'linux']) test(`real Tauri ${os} bootstrap preserves readonly methods, calls, callbacks, channels and fallback transport`, async t => {
  assert.equal(load('../src-tauri/Cargo.lock').match(/\[\[package\]\]\r?\nname = "tauri"\r?\nversion = "([^"]+)"/)?.[1], fixtureVersion);
  const records: { kind: string; data: any }[] = [];
  const nativeCalls: string[] = [];
  let failFetch = false;
  const answer = (command: string) => command === 'plugin:event|listen' ? 99 : { ok: true, count: 42 };
  const context = vm.createContext({ crypto: globalThis.crypto, Headers, Uint32Array, setTimeout, clearTimeout,
    console: { warn() {} },
    fetch: async (url: string) => {
      if (failFetch) throw new Error('simulated custom protocol failure');
      const command = decodeURIComponent(new URL(url).pathname.slice(1));
      nativeCalls.push(command);
      return new Response(JSON.stringify(command === 'fails' ? { failure: true } : answer(command)), {
        headers: { 'Tauri-Response': command === 'fails' ? 'error' : 'ok', 'content-type': 'application/json' },
      });
    },
  });
  vm.runInContext(`globalThis.window=globalThis;Object.defineProperty(window,'__TAURI_INTERNALS__',{value:{plugins:{}}});window.originalDefine=Object.defineProperty;`, context);
  const render = (script: string) => script.replaceAll('__TEMPLATE_os_name__', JSON.stringify(os))
    .replaceAll('__TEMPLATE_protocol_scheme__', '"http"').replaceAll('__TEMPLATE_invoke_key__', '"test-only-key"')
    .replaceAll('__TEMPLATE_fetch_channel_data_command__', '"plugin:__TAURI_CHANNEL__|fetch"')
    .replaceAll('__RAW_process_ipc_message_fn__', `(payload)=>({contentType:'application/json',data:JSON.stringify(payload)})`);
  // This order matches Tauri manager/webview.rs: default transport, our append,
  // frozen Object prototype and core's read-only method definitions.
  vm.runInContext(render(transport), context);
  vm.runInContext(bootstrap, context);
  vm.runInContext('Object.freeze(Object.prototype)', context);
  vm.runInContext(render(core), context);
  vm.runInContext(`Object.defineProperty(window.__TAURI_INTERNALS__,'ipc',{value:Object.freeze(message=>window.__TAURI_INTERNALS__.postMessage(message))})`, context);
  const bridge = context.__TAURI_INTERNALS__;
  Object.defineProperty(context, 'ipc', { value: Object.freeze({ postMessage(serialized: string) {
    const message = JSON.parse(serialized); nativeCalls.push(message.cmd);
    queueMicrotask(() => bridge.runCallback(message.callback, answer(message.cmd)));
  } }) });
  assert.equal(vm.runInContext('Object.defineProperty===originalDefine', context), true);
  for (const key of ['invoke', 'transformCallback', 'unregisterCallback']) {
    assert.equal(Object.getOwnPropertyDescriptor(bridge, key)!.writable, false);
    assert.equal(Object.getOwnPropertyDescriptor(bridge, key)!.configurable, false);
  }
  const observer = observeDiagnosticBridge(bridge, (kind, data) => records.push({ kind, data }));
  t.after(() => observer.restore());
  assert.deepEqual(await bridge.invoke('future_command', { text: 'PRIVATE_CONTENT' }), { ok: true, count: 42 });
  await assert.rejects(bridge.invoke('fails'), error => Boolean((error as {failure:boolean}).failure));
  let delivered = 0;
  const id = bridge.transformCallback(() => delivered++);
  await bridge.invoke('plugin:event|listen', { event: 'future-event', handler: id });
  bridge.runCallback(id, { payload: { enabled: true } });
  assert.equal(delivered, 1);
  assert.ok(records.some(row => row.kind === 'event.received' && row.data.event === 'future-event'));
  bridge.unregisterCallback(id); assert.equal(bridge.callbacks.has(id), false);
  const once = bridge.transformCallback(() => delivered++, true);
  bridge.runCallback(once, {}); assert.equal(bridge.callbacks.has(once), false);

  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  Object.defineProperty(globalThis, 'window', { configurable: true, value: context });
  try {
    const channel = new Channel(() => delivered++);
    await bridge.invoke('plugin:future|stream', { onEvent: channel });
    bridge.runCallback(channel.id, { index: 0, message: { progress: 75 } });
    assert.ok(records.some(row => row.kind === 'event.received' && row.data.event === 'channel:plugin:future|stream:onEvent'));
    bridge.runCallback(channel.id, { index: 1, end: true });
    assert.equal(bridge.callbacks.has(channel.id), false);
  } finally {
    if (previousWindow) Object.defineProperty(globalThis, 'window', previousWindow); else Reflect.deleteProperty(globalThis, 'window');
  }
  failFetch = true;
  assert.deepEqual(await bridge.invoke('fallback_command'), { ok: true, count: 42 });
  assert.ok(nativeCalls.includes('fallback_command'));
  assert.ok(records.some(row => row.kind === 'ipc.ok' && row.data.command === 'fallback_command'));
  assert.doesNotMatch(JSON.stringify(records), /PRIVATE_CONTENT|test-only-key/);
  const count = records.length;
  await bridge.invoke('append_frontend_diagnostics', { records: [] });
  assert.equal(records.length, count);
  observer.restore();
  await bridge.invoke('after_disconnect');
  assert.equal(records.length, count);
});

test('incomplete initialization restores Object and unsupported locked methods leave the bridge intact', async () => {
  const context = vm.createContext({ setTimeout, clearTimeout });
  vm.runInContext('globalThis.window=globalThis;window.__TAURI_INTERNALS__={};window.originalDefine=Object.defineProperty;', context);
  vm.runInContext(bootstrap, context);
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(vm.runInContext('Object.defineProperty===originalDefine', context), true);
  const bridge = { invoke: async<T>() => true as T, transformCallback: () => 1 };
  const callback = bridge.transformCallback;
  Object.defineProperty(bridge, 'invoke', { writable: false });
  assert.throws(() => observeDiagnosticBridge(bridge, () => {}), /只读/);
  assert.equal(bridge.transformCallback, callback);
  assert.equal(await bridge.invoke(), true);
});
