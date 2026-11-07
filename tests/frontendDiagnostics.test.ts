import test from 'node:test';
import assert from 'node:assert/strict';
import { diagnosticValue, FrontendRecorder, type FrontendRecord } from '../src/lib/frontendDiagnosticsModel.ts';

test('preserves error stacks and handles circular objects, getters and hostile proxies', () => {
  const error = new Error('render failed');
  const circular: Record<string, unknown> = { error, bigint: 1n };
  circular.self = circular;
  Object.defineProperty(circular, 'dangerous', { enumerable: true, get() { throw new Error('getter executed'); } });
  const value = diagnosticValue(circular) as Record<string, unknown>;
  assert.match(JSON.stringify(value.error), /render failed/);
  assert.match(JSON.stringify(value.error), /frontendDiagnostics.test/);
  assert.equal(value.self, '[circular]');
  assert.equal(value.dangerous, '[getter]');
  assert.equal(value.bigint, '[bigint]');
  assert.equal(diagnosticValue(new Proxy({}, { ownKeys() { throw new Error('hostile'); } })), '[unserializable]');
});

test('removes credential fields, bearer tokens and URL credentials/query strings', () => {
  const value = diagnosticValue({
    apiKey: 'PRIVATE_KEY', password: 'PRIVATE_PASSWORD', nested: { token: 'PRIVATE_TOKEN' },
    error: new Error('GET https://user:PRIVATE_USERPASS@example.com/data?api_key=PRIVATE_QUERY#PRIVATE_HASH failed; Authorization: Bearer PRIVATE_BEARER'),
    message: 'api_key="PRIVATE_QUOTED" token=PRIVATE_PLAIN',
  });
  assert.doesNotMatch(JSON.stringify(value), /PRIVATE_/);
  assert.match(JSON.stringify(value), /example.com\/data/);
  assert.match(JSON.stringify(value), /redacted/);
});

test('queues are bounded during transport failure and report dropped records on recovery', async () => {
  let now = 100000;
  let failing = true;
  const written: FrontendRecord[] = [];
  const recorder = new FrontendRecorder('session', async entries => {
    if (failing) throw new Error('disk unavailable');
    written.push(...entries);
  }, () => now);
  for (let batch = 0; batch < 6; batch++) {
    for (let index = 0; index < 100; index++) recorder.record('ui.click', { batch, index });
    await assert.rejects(recorder.flush(), /disk unavailable/);
    now += 10001;
  }
  failing = false;
  await recorder.flush();
  assert.equal(written.filter(entry => entry.kind === 'ui.click').length, 200);
  assert.equal((written.find(entry => entry.kind === 'diagnostics.dropped')!.data as { count: number }).count, 400);
  assert.equal((written.at(-1)!.data as { batch: number }).batch, 5);
});

test('an error storm is rate limited and oversized records do not poison later batches', async () => {
  const written: FrontendRecord[] = [];
  const recorder = new FrontendRecorder('session', async entries => { written.push(...entries); }, () => 100000);
  recorder.record('console.error', Array.from({ length: 16 }, () => 'x'.repeat(6000)), 'error');
  for (let i = 0; i < 2000; i++) recorder.record('console.error', { message: 'repeat' }, 'error');
  await recorder.flush();
  assert.equal(written.filter(entry => entry.kind === 'console.error').length, 100);
  assert.equal(written.find(entry => entry.kind === 'console.error')!.data, '[record exceeded 12 KB]');
  assert.equal((written.find(entry => entry.kind === 'diagnostics.dropped')!.data as { count: number }).count, 1901);
});

test('serializes concurrent flushes and drains records arriving while IPC is pending', async () => {
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const batches: FrontendRecord[][] = [];
  const recorder = new FrontendRecorder('session', async entries => { batches.push(entries); await gate; });
  recorder.record('frontend.start');
  const first = recorder.flush();
  const second = recorder.flush();
  assert.equal(first, second);
  recorder.record('frontend.mounted');
  assert.equal(batches.length, 1);
  release();
  await first;
  assert.deepEqual(batches.flat().map(entry => entry.kind), ['frontend.start', 'frontend.mounted']);
  assert.deepEqual(batches.flat().map(entry => entry.seq), [1, 2]);
});

test('a real input is sent before a potentially blocking handler while a previous write still awaits ACK',async()=>{
  let release!:()=>void;
  const gate=new Promise<void>(resolve=>{release=resolve;});
  const batches:FrontendRecord[][]=[];
  const recorder=new FrontendRecorder('session',async entries=>{batches.push(entries);await gate;});
  recorder.record('state.changed');
  const regular=recorder.flush();
  recorder.recordImmediate('ui.pointerdown',{control:'unknown-future-button'});
  assert.equal(batches.length,2,'The input is submitted without waiting for the stalled JS ACK continuation');
  assert.equal(batches[1][0].kind,'ui.pointerdown');
  release();await regular;await recorder.flush();
  assert.equal(batches.flat().filter(entry=>entry.kind==='ui.pointerdown').length,1);
});
