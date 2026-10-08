import assert from 'node:assert/strict';
import { access, mkdtemp, readFile, rm } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

// No Rust, device, signed-in browser, or real settings are used. Override the
// Chromium binary with BROWSER_BINARY (Chrome/Edge/Chromium), if necessary.
const root = fileURLToPath(new URL('../../', import.meta.url));
const candidates = [
  process.env.BROWSER_BINARY,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  ...['PROGRAMFILES', 'PROGRAMFILES(X86)', 'LOCALAPPDATA'].flatMap(key => process.env[key] ? [
    join(process.env[key], 'Google/Chrome/Application/chrome.exe'),
    join(process.env[key], 'Microsoft/Edge/Application/msedge.exe'),
  ] : []),
  '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
].filter(Boolean);
let binary;
for (const candidate of candidates) {
  try { await access(candidate); binary = candidate; break; } catch { /* try next installation */ }
}
assert.ok(binary, 'Install Chromium or set BROWSER_BINARY to a Chrome/Edge executable.');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const profile = await mkdtemp(join(tmpdir(), 'neoserial-persistence-test-'));
let server, browser, socket, deadline;
try {
  server = await createServer({
    root, logLevel: 'silent', cacheDir: join(profile, 'vite-cache'),
    server: { host: '127.0.0.1', port: 0, strictPort: false },
  });
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = spawn(binary, [
    '--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
    '--disable-extensions', '--disable-sync', '--window-size=1400,900', 'about:blank',
  ], { stdio: 'ignore' });
  let launchError;
  browser.on('error', error => { launchError = error; });
  let port;
  for (let i = 0; i < 200 && !port; i++) {
    if (launchError) throw launchError;
    if (browser.exitCode !== null) throw new Error(`Browser exited: ${browser.exitCode}`);
    try { port = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0]; } catch { /* starting */ }
    if (!port) await delay(50);
  }
  assert.ok(port, 'Headless Chromium must start within 10 seconds.');
  const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket = new WebSocket(pages.find(page => page.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true });
    socket.addEventListener('error', reject, { once: true });
  });
  const pending = new Map();
  let id = 0;
  let timedOut = false;
  const errors = [];
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data);
    if (message.method === 'Runtime.exceptionThrown') errors.push(message.params.exceptionDetails);
    if (!message.id) return;
    const request = pending.get(message.id);
    pending.delete(message.id);
    if (request) message.error ? request.reject(new Error(JSON.stringify(message.error))) : request.resolve(message.result);
  });
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    if (timedOut) { reject(new Error('Persistence regression timed out')); return; }
    const next = ++id;
    pending.set(next, { resolve, reject });
    socket.send(JSON.stringify({ id: next, method, params }));
  });
  const evaluate = async expression => {
    const result = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const until = async expression => {
    for (let i = 0; i < 200; i++) {
      if (await evaluate(expression)) return;
      if (errors.length) throw new Error(JSON.stringify(errors));
      await delay(50);
    }
    assert.fail(`Timed out: ${expression}`);
  };
  const click = async text => {
    await evaluate(`persistenceTest.button(${JSON.stringify(text)}).click()`);
    await evaluate('persistenceTest.tick()');
  };
  let run = 0;
  const fresh = async label => {
    assert.deepEqual(errors, [], 'No uncaught frontend exceptions');
    const current = String(++run);
    await call('Page.navigate', { url: `${base}/tests/browser/persistence-regressions.html?window=${label}&run=${current}` });
    await until(`window.persistenceTest?.run===${JSON.stringify(current)}`);
    await delay(80); // let pending open requests and initial effects settle
  };
  await call('Runtime.enable');
  await call('Page.enable');
  // Bound a stuck CDP request, too; rejecting pending promises runs finally.
  deadline = setTimeout(() => {
    timedOut = true;
    for (const request of pending.values()) request.reject(new Error('Persistence regression timed out'));
    socket.close();
  }, 60000);

  await fresh('main');
  const quick = 'persistenceTest.stores.scriptModules[0].pages[0].commands[0]';
  await evaluate(`${quick}.command='EDITED';persistenceTest.tick()`);
  await evaluate(`document.querySelector('button[title="收起脚本面板"]').click();persistenceTest.tick()`);
  await evaluate(`document.querySelector('button[title="展开脚本面板"]').click();persistenceTest.tick()`);
  await until(`persistenceTest.sequence()[0].pages[0].commands[0].command==='EDITED'`);
  assert.equal(await evaluate(`${quick}.command`), 'EDITED');
  assert.equal(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='load_sequence_auto').length`), 1, 'showing the panel does not reload from disk');
  console.log('PASS: quick-command edits survive a hide/show cycle inside the debounce window');

  await evaluate(`persistenceTest.stores.scriptModules.find(m=>m.type==='data_processing').tools.find(t=>t.kind==='codec').config.input='UNSAVED CODEC';persistenceTest.tick()`);
  await evaluate(`document.querySelector('button[title="收起脚本面板"]').click();persistenceTest.tick()`);
  await until(`persistenceTest.sequence().find(m=>m.type==='data_processing').tools.find(t=>t.kind==='codec').config.input==='UNSAVED CODEC'`);
  console.log('PASS: data-tool edits are saved even while the panel stays hidden');

  const saves = await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='save_sequence_auto').length`);
  await evaluate(`persistenceTest.remoteCommand('REMOTE')`);
  await until(`${quick}.command==='REMOTE'`);
  await evaluate(`document.querySelector('button[title="展开脚本面板"]').click();persistenceTest.tick()`);
  await delay(950);
  assert.equal(await evaluate(`${quick}.command`), 'REMOTE');
  assert.equal(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='save_sequence_auto').length`), saves, 'remote reload does not echo-save');
  console.log('PASS: hidden panels keep cross-window synchronization without an autosave loop');

  await fresh('settings');
  await click('雾灰松');
  await evaluate(`persistenceTest.holdNext('patch_settings')`);
  await click('应用');
  await until(`persistenceTest.calls.some(c=>c.cmd==='patch_settings')`);
  assert.equal(await evaluate(`persistenceTest.button('应用').disabled && persistenceTest.button('保存').disabled`), true, 'serialize settings writes');
  await click('深海夜航');
  await evaluate('persistenceTest.release()');
  await until(`persistenceTest.saved().presets.theme==='preset-2' && !persistenceTest.button('应用').disabled`);
  assert.equal(await evaluate('persistenceTest.stores.theme.value'), 'preset-2', 'saved stores reflect the submitted value, not later edits');
  await click('应用');
  await until(`persistenceTest.saved().presets.theme==='preset-3' && persistenceTest.button('应用').disabled`);
  console.log('PASS: later edits stay dirty and are persisted by a second apply');

  await fresh('settings');
  await click('雾灰松');
  await evaluate(`persistenceTest.holdNext('patch_settings')`);
  await click('保存');
  await until(`persistenceTest.calls.some(c=>c.cmd==='patch_settings')`);
  await click('深海夜航');
  await evaluate('persistenceTest.release()');
  await until(`persistenceTest.saved().presets.theme==='preset-2' && !persistenceTest.button('应用').disabled`);
  assert.equal(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='plugin:window|close').length`), 0, 'do not close with newer unsaved edits');
  await click('保存');
  await until(`persistenceTest.calls.some(c=>c.cmd==='plugin:window|close')`);
  assert.equal(await evaluate('persistenceTest.saved().presets.theme'), 'preset-3');
  console.log('PASS: save keeps the window open when edits arrive during the write');

  await fresh('settings');
  await click('雾灰松');
  await click('扩展');
  await click('指令联想');
  await evaluate(`[...document.querySelectorAll('button[aria-expanded]')].find(b=>b.textContent.includes('知识库服务器')).click();persistenceTest.tick()`);
  const baseInput = "document.querySelector('input[placeholder*=\"http://127.0.0.1:8200\"]')";
  const keyInput = "document.querySelector('input[autocomplete=\"off\"]')";
  const input = async (selector, value) => {
    await evaluate(`(()=>{const el=${selector};el.value=${JSON.stringify(value)};el.dispatchEvent(new Event('input',{bubbles:true}));})()`);
    await evaluate('persistenceTest.tick()');
  };
  await input(baseInput, 'http://127.0.0.1:8201');
  await input(keyInput, 'isolated-test-key-1');
  await evaluate(`persistenceTest.holdNext('kb_set_api_key')`);
  await click('应用');
  await until(`persistenceTest.calls.some(c=>c.cmd==='kb_set_api_key')`);
  await input(baseInput, 'http://127.0.0.1:8202');
  await input(keyInput, 'isolated-test-key-2');
  await evaluate('persistenceTest.release()');
  await until(`persistenceTest.saved().command_index.base_url==='http://127.0.0.1:8201' && !persistenceTest.button('应用').disabled`);
  assert.equal(await evaluate(`${baseInput}.value`), 'http://127.0.0.1:8202', 'retain the newer address');
  assert.equal(await evaluate(`${keyInput}.value`), 'isolated-test-key-2', 'retain the newer credential input');
  await click('应用');
  await until(`persistenceTest.saved().command_index.base_url==='http://127.0.0.1:8202' && persistenceTest.button('应用').disabled`);
  assert.deepEqual(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='kb_set_api_key').map(c=>c.args.key)`), ['isolated-test-key-1', 'isolated-test-key-2']);
  assert.equal(await evaluate(`${baseInput}.value`), '');
  assert.equal(await evaluate(`${keyInput}.value`), '');
  console.log('PASS: snapshot precedes credential IPC and newer address/key inputs are not cleared');

  await fresh('settings');
  await click('雾灰松');
  await evaluate(`persistenceTest.failNext('patch_settings')`);
  await click('应用');
  await until(`document.body.innerText.includes('isolated save failure')`);
  assert.equal(await evaluate('persistenceTest.saved().presets.theme'), 'preset-1');
  assert.equal(await evaluate(`persistenceTest.button('应用').disabled`), false);
  await click('应用');
  await until(`persistenceTest.saved().presets.theme==='preset-2' && persistenceTest.button('应用').disabled`);
  console.log('PASS: failed saves retain the draft and can be retried');
  assert.deepEqual(errors, [], 'No uncaught frontend exceptions');
} finally {
  clearTimeout(deadline);
  socket?.close();
  if (browser && browser.exitCode === null && !browser.killed) {
    const exited = new Promise(resolve => browser.once('exit', resolve));
    browser.kill();
    await Promise.race([exited, delay(3000)]);
  }
  await server?.close();
  await rm(profile, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
}
