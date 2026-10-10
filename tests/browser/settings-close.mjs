import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';

// Native Tauri regression (no mocked IPC). Run ONLY against a disposable app
// instance using a separate identifier and NEOSERIAL_DATA_DIR, with WebView2
// remote debugging enabled with identical additional_browser_args for both
// main and settings windows. This test changes that instance's theme.
// WEBVIEW_DEBUG_URL=http://localhost:9336 node tests/browser/settings-close.mjs
const debugUrl = process.env.WEBVIEW_DEBUG_URL ?? 'http://localhost:9336';
const sockets = [];
const timeout = setTimeout(() => { console.error('Native regression timed out'); process.exit(1); }, 45000);
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const pages = async () => (await (await fetch(`${debugUrl}/json/list`)).json()).filter(p => p.type === 'page');

async function connect(page) {
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  sockets.push(ws);
  await new Promise(resolve => ws.addEventListener('open', resolve, { once: true }));
  let id = 0;
  const pending = new Map();
  ws.addEventListener('message', e => {
    const m = JSON.parse(e.data);
    if (!m.id) return;
    const p = pending.get(m.id);
    pending.delete(m.id);
    m.error ? p.reject(new Error(JSON.stringify(m.error))) : p.resolve(m.result);
  });
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const next = ++id;
    pending.set(next, { resolve, reject });
    ws.send(JSON.stringify({ id: next, method, params }));
  });
  const evaluate = async expression => {
    const r = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
    return r.result.value;
  };
  return { call, evaluate };
}

try {
  let main;
  for (const page of await pages()) {
    const client = await connect(page);
    if (await client.evaluate(`window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label`) === 'main') {
      main = client;
      break;
    }
  }
  assert.ok(main, 'disposable native main window found');
  const dirs = await main.evaluate(`window.__TAURI_INTERNALS__.invoke('data_dirs')`);
  assert.ok(['settings-close-test-data', 'settings-perf-data'].some(name => dirs.config.replaceAll('\\', '/').endsWith('/docs/' + name)), 'refuse to modify real user settings');
  const invoke = (cmd, args = {}) => main.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`);
  const labels = () => main.evaluate(`window.__TAURI_INTERNALS__.invoke('plugin:window|get_all_windows')`);
  const theme = async () => (await invoke('get_settings')).presets.theme;
  const button = text => `[...document.querySelectorAll('button')].find(b=>b.textContent.trim()===${JSON.stringify(text)})`;
  async function click(client, expression) {
    const p = await client.evaluate(`(()=>{const el=${expression};if(!el)throw Error('button not found');const r=el.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
    await client.call('Input.dispatchMouseEvent', { type: 'mousePressed', ...p, button: 'left', clickCount: 1 });
    await client.call('Input.dispatchMouseEvent', { type: 'mouseReleased', ...p, button: 'left', clickCount: 1 });
  }
  await invoke('patch_settings', { patch: { presets: { theme: 'preset-1' } } });
  for (const action of ['cancel', 'x', 'save', 'saveUnchanged', 'escape', 'native']) {

    await invoke('open_settings_window', { section: 'appearance' });
    let settings;
    for (let i = 0; i < 80 && !settings; i++) {
      for (const p of await pages()) {
        if (p.url.startsWith('devtools:')) continue;
        const client = await connect(p);
        if (await client.evaluate(`window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label === 'settings' && Boolean(document.querySelector('button[title="关闭 (Esc)"]'))`)) settings = client;
        else sockets.pop().close();
      }
      if (!settings) await delay(100);
    }
    assert.ok(settings, 'native settings window loaded');
    await settings.evaluate(`window.closeErrors=[];window.addEventListener('unhandledrejection',e=>closeErrors.push(String(e.reason)));`);
    // Navigate through real UI; wait for the initial pending request to settle.
    await delay(200);
    await click(settings, button('外观'));
    await delay(50);
    const before = await theme();
    const desired = before === 'preset-1' ? 'preset-2' : 'preset-1';
    if (action !== 'saveUnchanged') await click(settings, `[...document.querySelectorAll('button')].find(b=>b.textContent.trim().startsWith(${JSON.stringify(desired === 'preset-2' ? '雾灰松' : '暖白青')}))`);
    await delay(50);
    if (action.startsWith('save') || action === 'cancel') await click(settings, button(action.startsWith('save') ? '保存' : '取消'));
    else if (action === 'x') await click(settings, `document.querySelector('button[title="关闭 (Esc)"]')`);
    else if (action === 'escape') {
      await settings.call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
    } else await settings.evaluate(`window.__TAURI_INTERNALS__.invoke('plugin:window|close',{label:'settings'})`);
    let gone = false;
    for (let i = 0; i < 30; i++) {
      if (await settings.evaluate(`(async()=>!document.querySelector('[data-tauri-drag-region]') && !(await window.__TAURI_INTERNALS__.invoke('plugin:window|is_visible',{label:'settings'})))()`)) { gone = true; break; }
      await delay(100);
    }
    if (!gone) console.error('Native errors:', await settings.evaluate('closeErrors'));
    assert.ok(gone, `${action}: settings window must actually disappear`);
    assert.equal(await theme(), action === 'save' ? desired : before, `${action}: persisted theme`);
    const diskSettings = JSON.parse(await readFile(join(dirs.config, 'settings.json'), 'utf8'));
    assert.equal(diskSettings.presets.theme, action === 'save' ? desired : before, `${action}: settings file`);
    assert.ok((await labels()).includes('settings'), 'settings WebView remains reusable');
    assert.ok((await labels()).includes('main'), 'main window remains open');
    console.log(`PASS ${action}: window hidden and reusable, saved/cancelled changes correct, main stays open`);
  }
} finally {
  clearTimeout(timeout);
  sockets.forEach(ws => ws.close());
}
