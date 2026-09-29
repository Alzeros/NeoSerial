import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';

// Run against an isolated native app (NEOSERIAL_DATA_DIR=docs/codec-test-data)
// with WebView2 debugging on 9336 and Vite on 5173. Uses real GBK Rust IPC.
const debugUrl = process.env.WEBVIEW_DEBUG_URL ?? 'http://localhost:9336';
const baseUrl = process.env.VITE_URL ?? 'http://localhost:5173';
const pages = await (await fetch(debugUrl + '/json/list')).json();
const page = pages.find(p => p.type === 'page' && p.url.startsWith(baseUrl));
assert.ok(page, 'native test window is running');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise(resolve => ws.addEventListener('open', resolve, { once: true }));
let id = 0;
const pending = new Map();
ws.addEventListener('message', e => {
  const m = JSON.parse(e.data);
  if (!m.id) return;
  const p = pending.get(m.id); pending.delete(m.id);
  m.error ? p.reject(new Error(JSON.stringify(m.error))) : p.resolve(m.result);
});
const call = (method, params = {}) => new Promise((resolve, reject) => {
  const next = ++id; pending.set(next, { resolve, reject });
  ws.send(JSON.stringify({ id: next, method, params }));
});
async function ev(expression) {
  const r = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: true });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(expression, message) {
  for (let i = 0; i < 100; i++) { if (await ev(expression)) return; await wait(50); }
  assert.fail(message);
}
const button = text => "[...document.querySelectorAll('.codec-panel button')].find(b=>b.textContent.trim()===" + JSON.stringify(text) + ")";
const click = text => ev(button(text) + '.click()');
const config = patch => ev('codecTest.update(' + JSON.stringify(patch) + ')');
const input = text => ev("(()=>{const el=document.querySelector('#codec-input');el.value=" + JSON.stringify(text) + ";el.dispatchEvent(new Event('input',{bubbles:true}));})()");
async function run(expected) {
  await ev("document.querySelector('.codec-panel .btn-primary').click()");
  await until("!document.querySelector('.codec-panel .btn-primary').disabled", 'conversion completed');
  assert.equal(await ev("document.querySelector('#codec-result').value"), expected);
}
const timeout = setTimeout(() => { console.error('codec regression timed out'); process.exit(1); }, 60000);
try {
  const dirs = await ev("window.__TAURI_INTERNALS__.invoke('data_dirs')");
  assert.ok(dirs.config.replaceAll('\\', '/').endsWith('/docs/codec-test-data'), 'requires disposable app profile');
  await call('Page.navigate', { url: baseUrl + '/tests/browser/codec-encodings.html' });
  await until('Boolean(window.codecTest)', 'codec harness mounted');
  assert.equal(await ev("document.querySelector('#codec-input').placeholder.includes('UTF-8')"), true, 'legacy config defaults to UTF-8');
  await input('你好');
  await run('E4 BD A0 E5 A5 BD');

  // Select GBK through the actual dropdown.
  await ev("[...document.querySelectorAll('.custom-select-trigger')].find(b=>b.textContent.trim()==='UTF-8').click()");
  await ev("[...document.querySelectorAll('[role=option]')].find(e=>e.textContent.trim()==='GBK').click()");
  assert.ok(await ev("document.body.innerText.includes('已过期')"));
  assert.ok(await ev(button('填入发送框') + '.disabled'));
  await run('C4 E3 BA C3');
  await click('填入发送框');
  assert.deepEqual(await ev('codecTest.fill()'), { text: 'C4 E3 BA C3', hex: true });
  console.log('PASS: UTF-8 default, GBK selector/native encode, stale guard and fill');

  await config({ operation: 'hex_to_text', input: 'C4 E3 BA C3' });
  await run('你好');
  await click('填入发送框');
  assert.deepEqual(await ev('codecTest.fill()'), { text: 'C4 E3 BA C3', hex: true });
  await config({ operation: 'base64_encode', input: '你好', format: 'text' });
  await run('xOO6ww==');
  await click('填入发送框');
  assert.deepEqual(await ev('codecTest.fill()'), { text: 'xOO6ww==', hex: false });
  await config({ operation: 'base64_decode', input: 'T2BZfQ==', encoding: 'utf16be' });
  await run('你好');
  await click('填入发送框');
  assert.deepEqual(await ev('codecTest.fill()'), { text: '4F 60 59 7D', hex: true });
  await config({ operation: 'text_to_hex', input: 'A🙂', encoding: 'utf16le' });
  await run('41 00 3D D8 42 DE');
  console.log('PASS: GBK/Base64 and UTF-16 byte-preserving conversion/fill');

  await config({ operation: 'escaped_to_hex', input: '你\\r\\n好\\xFF', encoding: 'gbk' });
  await run('C4 E3 0D 0A BA C3 FF');
  await config({ operation: 'hex_to_escaped', input: '41 0D 0A 00 FF' });
  await run('A\\r\\n\\0\\xFF');
  assert.ok(!(await ev("document.body.innerText.includes('文本编码')")), 'byte display hides unused encoding');
  await click('填入发送框');
  assert.deepEqual(await ev('codecTest.fill()'), { text: '41 0D 0A 00 FF', hex: true });
  await call('Emulation.setFocusEmulationEnabled', { enabled: true });
  await click('复制结果');
  await until("document.body.innerText.includes('已复制')", 'copy completed');
  assert.equal(await ev('navigator.clipboard.readText()'), 'A\\r\\n\\0\\xFF', 'copy preserves escaped display');
  console.log('PASS: GBK ordinary text mixed with escaped bytes, escaped copy and raw fill');

  await config({ operation: 'text_to_hex', input: '🙂', encoding: 'gbk' });
  await ev("document.querySelector('.codec-panel .btn-primary').click()");
  await until("document.querySelector('[role=alert]')?.textContent.includes('GBK')", 'unencodable character is rejected');
  assert.ok(await ev(button('填入发送框') + '.disabled'));
  await config({ operation: 'escaped_to_hex', input: '\\xG1' });
  await ev("document.querySelector('.codec-panel .btn-primary').click()");
  await until("document.querySelector('[role=alert]')?.textContent.includes('转义')", 'invalid escape is rejected');

  // Delay only the first native GBK reply; the new input must win.
  await config({ operation: 'text_to_hex', input: '你', encoding: 'gbk' });
  await ev("(()=>{const native=window.fetch;let delayed=false;window.fetch=async(url,options)=>{if(String(url).endsWith('/encode_gbk_texts')&&!delayed){delayed=true;await new Promise(r=>setTimeout(r,500));}return native(url,options);};})()");
  await ev("document.querySelector('.codec-panel .btn-primary').click()");
  assert.ok(await ev(button('填入发送框') + '.disabled'));
  await input('好');
  await run('BA C3');
  await wait(700);
  assert.equal(await ev("document.querySelector('#codec-result').value"), 'BA C3', 'late result cannot overwrite');
  console.log('PASS: strict errors and out-of-order native replies');
  await config({ operation: 'escaped_to_hex', input: 'AT\\r\\n\\x00', encoding: 'utf8' });
  await run('41 54 0D 0A 00');
  const shot = await call('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: 510, height: 630, scale: 1 } });
  await writeFile('docs/codec-encodings.png', Buffer.from(shot.data, 'base64'));
} finally { clearTimeout(timeout); ws.close(); }
