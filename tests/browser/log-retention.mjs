import assert from 'node:assert/strict';
import test from 'node:test';

const baseUrl = process.env.VITE_URL ?? 'http://localhost:5173';
const debugUrl = process.env.CHROME_DEBUG_URL ?? 'http://localhost:9335';
const pages = await (await fetch(`${debugUrl}/json/list`)).json();
const ws = new WebSocket(pages.find(p => p.type === 'page').webSocketDebuggerUrl);
await new Promise(resolve => ws.addEventListener('open', resolve, {once: true}));
let id = 0;
const pending = new Map();
ws.addEventListener('message', e => {
  const m = JSON.parse(e.data);
  if (!m.id) return;
  const p = pending.get(m.id); pending.delete(m.id);
  m.error ? p.reject(m.error) : p.resolve(m.result);
});
const call = (method, params = {}) => new Promise((resolve, reject) => {
  const next = ++id; pending.set(next, {resolve, reject});
  ws.send(JSON.stringify({id: next, method, params}));
});
async function ev(expression) {
  const r = await call('Runtime.evaluate', {expression, awaitPromise: true, returnByValue: true});
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
const timeout = setTimeout(() => {console.error('log retention regression timed out'); process.exit(1);}, 60000);
try {
  await call('Page.navigate', {url: `${baseUrl}/tests/browser/log-copy.html`});
  for (let i = 0; i < 100; i++) {
    if (await ev('Boolean(window.logTest)')) break;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  assert.ok(await ev('Boolean(window.logTest)'));
  await ev(`window.makeRetainedLog=(n,size)=>({raw:Array(size).fill(65),ascii:'A'.repeat(size),
    ts:String(n),ts_ms:0,dir:'rx',line_index:n,is_error:false});`);

  await test('old 100000-row setting is clamped to 10000 in the actual store', async () => {
    await ev(`(async()=>{
      logTest.stores.clearLogLines();
      logTest.stores.cachedSettings.value={ui:{ring_buffer_capacity:100000,log_buffer_max_bytes:4194304}};
      logTest.stores.appendLogLines(Array.from({length:10005},(_,i)=>makeRetainedLog(i+1,1)));
      await logTest.tick();
    })()`);
    assert.equal(await ev('logTest.stores.logLines.length'), 10000);
    assert.equal(await ev("document.querySelectorAll('[data-log-row]').length"), 10000);
    assert.equal(await ev('logTest.stores.logLines[0].line_index'), 6);
  });

  await test('raw byte budget evicts oldest records before the row limit', async () => {
    await ev(`(async()=>{
      logTest.stores.clearLogLines();
      logTest.stores.cachedSettings.value={ui:{ring_buffer_capacity:5000,log_buffer_max_bytes:1048576}};
      logTest.stores.appendLogLines(Array.from({length:1030},(_,i)=>makeRetainedLog(i+1,1024)));
      await logTest.tick();
    })()`);
    assert.equal(await ev('logTest.stores.logLines.length'), 1024);
    assert.equal(await ev('logTest.stores.logLines[0].line_index'), 7);
    assert.equal(await ev('logTest.stores.logLines.reduce((n,l)=>n+l.raw.length,0)'), 1048576);
  });

  await test('oversized record remains visible in HEX with no retained raw payload', async () => {
    await ev(`(async()=>{
      logTest.stores.clearLogLines();
      logTest.stores.displayMode.value='hex';
      logTest.stores.appendLogLines([makeRetainedLog(1,3),makeRetainedLog(2,1048577)]);
      await logTest.tick();
    })()`);
    assert.equal(await ev('logTest.stores.logLines.length'), 2);
    assert.equal(await ev('logTest.stores.logLines[1].raw.length'), 0);
    assert.equal(await ev('logTest.stores.logLines[1].omitted_bytes'), 1048577);
    assert.ok(await ev("document.querySelector('[role=log]').textContent.includes('已省略')"));
    await ev(`(async()=>{logTest.stores.displayMode.value='ascii';await logTest.tick();})()`);
    assert.ok(await ev("document.querySelector('[role=log]').textContent.includes('已省略')"));
  });
} finally {
  clearTimeout(timeout); ws.close();
}
