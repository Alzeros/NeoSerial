import test from 'node:test';
import assert from 'node:assert/strict';
import { observeDiagnosticNetwork } from '../src/lib/diagnosticNetwork.ts';

test('Tauri native fetch/XHR transports do not record their own diagnostic writes', async t => {
  const saved = new Map(['window', 'location', 'XMLHttpRequest'].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  let restore = () => {};
  t.after(() => { restore(); for (const [key, descriptor] of saved) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); } });
  const records: { kind: string; data: any }[] = [];
  const failure = new Error('network failure');
  let fetches = 0, sends = 0;
  const page = { fetch: async (input: RequestInfo | URL) => {
    fetches++; if (String(input).endsWith('/failure')) throw failure;
    return new Response('ok', { status: 200 });
  } } as unknown as Window;
  class XHR extends EventTarget { status = 200; open(..._args: unknown[]) {} send() { sends++; this.dispatchEvent(new Event('loadend')); } }
  for (const [key, value] of Object.entries({ window: page, location: { href: 'http://localhost/' }, XMLHttpRequest: XHR })) {
    Object.defineProperty(globalThis, key, { configurable: true, value });
  }
  restore = observeDiagnosticNetwork((kind, data) => records.push({ kind, data }));
  for (const url of ['ipc://localhost/append_frontend_diagnostics', 'http://ipc.localhost/append_frontend_replay', 'https://ipc.localhost/frontend_diagnostics_heartbeat']) {
    assert.equal(await (await page.fetch(url, { method: 'POST' })).text(), 'ok');
    const xhr = new XHR(); xhr.open('POST', url); xhr.send();
  }
  assert.equal(fetches, 3); assert.equal(sends, 3);
  assert.equal(records.length, 0, 'Recorder transport never feeds the network journal');
  assert.equal(await (await page.fetch('https://example.com/data?token=PRIVATE')).text(), 'ok');
  assert.deepEqual(records.map(row => row.kind), ['network.start', 'network.end']);
  assert.doesNotMatch(JSON.stringify(records), /PRIVATE/);
  await assert.rejects(page.fetch('https://example.com/failure'), error => error === failure);
  assert.equal(records.at(-1)!.kind, 'network.error');
  const xhr = new XHR(); xhr.open('GET', 'https://example.com/data'); xhr.send();
  assert.equal(records.at(-1)!.kind, 'network.xhr_end');
});
