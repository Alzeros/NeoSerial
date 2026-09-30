import assert from 'node:assert/strict';

// Run with Vite and an isolated headless Chrome (--remote-debugging-port=9335).
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
  const p = pending.get(m.id);
  pending.delete(m.id);
  m.error ? p.reject(m.error) : p.resolve(m.result);
});
function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const requestId = ++id;
    pending.set(requestId, {resolve, reject});
    ws.send(JSON.stringify({id: requestId, method, params}));
  });
}
async function evaluate(expression) {
  const r = await call('Runtime.evaluate', {expression, awaitPromise: true, returnByValue: true});
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
const timeout = setTimeout(() => {console.error('log performance regression timed out'); process.exit(1);}, 60000);
try {
  await call('Page.navigate', {url: `${baseUrl}/tests/browser/log-copy.html`});
  for (let i = 0; i < 100; i++) {
    if (await evaluate('Boolean(window.logTest)')) break;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  assert.ok(await evaluate('Boolean(window.logTest)'), 'actual LogView harness mounted');
  await evaluate(`(async () => {
    logTest.stores.clearLogLines();
    logTest.stores.displayMode.value = 'hex';
    logTest.stores.showLineIndex.value = false;
    window.makeLog = n => ({ascii:'same payload',raw:Array.from({length:42},(_,i)=>i),
      ts:String(n),ts_ms:0,dir:'tx',line_index:n,is_error:false});
    logTest.stores.appendLogLines(Array.from({length:5000},(_,i)=>makeLog(i+1)));
    await logTest.tick();
    const rows = document.querySelectorAll('[data-log-row]');
    window.survivor = rows[1];
    window.survivorContent = survivor.lastElementChild;
    window.survivorText = [...survivorContent.childNodes].find(node => node.nodeType === Node.TEXT_NODE && node.textContent.length >= 5);
    const selection = getSelection();
    selection.removeAllRanges();
    const range = document.createRange();
    range.setStart(survivorText,0); range.setEnd(survivorText,5);
    selection.addRange(range);
    window.changes = 0;
    window.attributeChanges = 0;
    window.observer = new MutationObserver(records => {
      changes += records.filter(r => r.type === 'characterData').length;
      attributeChanges += records.filter(r => r.type === 'attributes').length;
    });
    observer.observe(document.querySelector('[role=log]'), {subtree:true,characterData:true,attributes:true});
    logTest.stores.appendLogLine(makeLog(5001));
    await logTest.tick();
    observer.disconnect();
  })()`);
  assert.ok(await evaluate("document.querySelector('[data-log-row]') === survivor"),
    'evicting the oldest log must retain the DOM for the surviving record');
  assert.ok(await evaluate('survivorText.parentNode === survivorContent'), 'surviving HEX text node retained');
  assert.equal(await evaluate('getSelection().toString()'), '00 01', 'selection survives oldest-row eviction');
  assert.equal(await evaluate('changes'), 0, 'append must not rewrite thousands of retained timestamps');
  assert.equal(await evaluate('attributeChanges'), 0, 'append must not rewrite retained row indices when search is closed');
  assert.equal(await evaluate('logTest.stores.logLines.length'), 5000);
  assert.equal(await evaluate("document.querySelector('[data-log-row] [data-log-field]').textContent"), 'Tx');
  assert.equal(await evaluate('survivor.querySelectorAll("[data-log-field]")[1].textContent'), '2');
  console.log('PASS: full 5000-line HEX buffer evicts one row without rewriting retained rows.');

  // Legacy logs and reconnected ports may reuse backend line_index values.
  await evaluate(`(async () => {
    logTest.stores.clearLogLines();
    getSelection().removeAllRanges();
    logTest.stores.appendLogLines([makeLog(0),makeLog(0),makeLog(1),makeLog(1)]);
    await logTest.tick();
    window.duplicateSurvivor=document.querySelectorAll('[data-log-row]')[2];
    logTest.stores.insertLogLines(0,[makeLog(0)]);
    await logTest.tick();
  })()`);
  assert.equal(await evaluate("document.querySelectorAll('[data-log-row]').length"), 5);
  assert.ok(await evaluate("document.querySelectorAll('[data-log-row]')[3] === duplicateSurvivor"),
    'history insertion preserves row identity even with duplicate backend indices');
  console.log('PASS: legacy/reconnected indices and history insertion retain the correct row identities.');

  await evaluate(`(async () => {
    window.dispatchEvent(new KeyboardEvent('keydown',{key:'f',ctrlKey:true}));
    await logTest.tick();
    const input=document.querySelector('input');
    input.value='01 02'; input.dispatchEvent(new Event('input',{bubbles:true}));
    await logTest.tick(); await new Promise(requestAnimationFrame);
  })()`);
  assert.equal(await evaluate('duplicateSurvivor.dataset.idx'), '3', 'search tracks the current row position');
  await evaluate(`(async () => {
    logTest.stores.insertLogLines(0,[makeLog(9)]); await logTest.tick();
  })()`);
  assert.equal(await evaluate('duplicateSurvivor.dataset.idx'), '4', 'search position updates after historical insertion');
  assert.equal(await evaluate("document.querySelectorAll('[data-log-row] mark').length"), 6);
  console.log('PASS: search indices and HEX highlights follow history insertion.');

  await evaluate(`(async () => {
    window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));
    logTest.stores.displayMode.value='ascii'; await logTest.tick();
    logTest.stores.logLines[0].ascii='updated record'; await logTest.tick();
  })()`);
  assert.equal(await evaluate("document.querySelector('[data-log-row]').lastElementChild.textContent"), 'updated record',
    'record edits remain reactive');
  await evaluate(`(async () => {
    logTest.stores.paused.value=true; logTest.stores.appendLogLine(makeLog(10)); await logTest.tick();
  })()`);
  assert.equal(await evaluate("document.querySelectorAll('[data-log-row]').length"), 6, 'paused logs do not append');
  await evaluate(`(async () => {
    logTest.stores.paused.value=false; logTest.stores.clearLogLines(); await logTest.tick();
  })()`);
  assert.equal(await evaluate("document.querySelectorAll('[data-log-row]').length"), 0, 'clear notifies the rendered collection');
  console.log('PASS: record edits, pause and standalone clear stay reactive.');

  await evaluate(`(async () => {
    window.originalSettings=logTest.stores.cachedSettings.value;
    logTest.stores.appendLogLines(Array.from({length:1200},(_,i)=>makeLog(i+1)));
    await logTest.tick();
    logTest.stores.cachedSettings.value={ui:{ring_buffer_capacity:1000}};
    logTest.stores.trimLogLines(); await logTest.tick();
  })()`);
  assert.equal(await evaluate("document.querySelectorAll('[data-log-row]').length"), 1000, 'lowering capacity immediately trims the DOM');
  assert.equal(await evaluate('logTest.stores.logLines[0].line_index'), 201);
  await evaluate(`(async () => {
    logTest.stores.appendLogLine(makeLog(1201)); await logTest.tick();
  })()`);
  assert.equal(await evaluate("document.querySelectorAll('[data-log-row]').length"), 1000);
  assert.equal(await evaluate('logTest.stores.logLines[0].line_index'), 202);
  await evaluate('logTest.stores.cachedSettings.value=originalSettings');
  console.log('PASS: changing retention capacity and subsequent rolling append stay consistent.');
} finally {
  clearTimeout(timeout);
  ws.close();
}
