import assert from 'node:assert/strict';

// Native regression against a disposable Tauri app. All three windows need
// identical WebView2 debugging args. Use a separate app identifier and set
// NEOSERIAL_DATA_DIR to docs/theme-window-test-data, never the real user profile.
const debugUrl = process.env.WEBVIEW_DEBUG_URL ?? 'http://localhost:9336';
const sockets = [];
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
const pages = async () => (await (await fetch(`${debugUrl}/json/list`)).json())
  .filter(page => page.type === 'page' && !page.url.startsWith('devtools:'));
const timeout = setTimeout(() => { console.error('Native theme test timed out'); process.exit(1); }, 60000);

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

async function windowByLabel(label) {
  for (let i = 0; i < 70; i++) {
    for (const page of await pages()) {
      const client = await connect(page);
      if (await client.evaluate(`window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label === ${JSON.stringify(label)} && document.querySelectorAll('button').length > 0`)) return client;
      sockets.pop().close();
    }
    await wait(100);
  }
  throw Error(`Missing native window: ${label}`);
}
async function until(check, message) {
  for (let i = 0; i < 60; i++) { if (await check()) return; await wait(100); }
  assert.fail(message);
}
const button = text => `[...document.querySelectorAll('button')].find(b=>b.textContent.trim()===${JSON.stringify(text)})`;
async function click(client, element) {
  const p = await client.evaluate(`(()=>{const el=${element};if(!el)throw Error('missing button');el.scrollIntoView({block:'nearest'});const r=el.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  await client.call('Input.dispatchMouseEvent', { type: 'mousePressed', ...p, button: 'left', clickCount: 1 });
  await client.call('Input.dispatchMouseEvent', { type: 'mouseReleased', ...p, button: 'left', clickCount: 1 });
}

try {
  const main = await windowByLabel('main');
  const invoke = (cmd, args = {}) => main.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)},${JSON.stringify(args)})`);
  const dirs = await invoke('data_dirs');
  assert.ok(dirs.config.replaceAll('\\', '/').endsWith('/docs/theme-window-test-data'), 'test requires a disposable profile');
  const labels = () => invoke('plugin:window|get_all_windows');
  await invoke('patch_settings', { patch: { presets: { theme: 'preset-1' } } });
  async function settingsPage() {
    await invoke('open_settings_window', { section: 'appearance' });
    const settings = await windowByLabel('settings');
    await wait(200);
    await click(settings, button('外观'));
    await click(settings, `[...document.querySelectorAll('button')].find(b=>b.textContent.includes('自己调配色板'))`);
    return settings;
  }

  // A slow IPC exposes the destroyed-caller race without mocking native opening.
  let settings = await settingsPage();
  await settings.evaluate(`(()=>{const fetchNative=window.fetch;window.fetch=async(url,options)=>{if(String(url).endsWith('/open_theme_editor'))await new Promise(r=>setTimeout(r,250));return fetchNative(url,options);};})()`);
  await click(settings, button('打开 →'));
  await wait(1000);
  assert.ok((await labels()).includes('theme-editor'), 'theme editor must open before caller is destroyed');
  const editor = await windowByLabel('theme-editor');
  await until(async () => !(await labels()).includes('settings'), 'settings closes after editor opens');
  console.log('PASS: actual native editor opens even with delayed IPC; settings closes afterward');

  assert.ok(await editor.evaluate(`Boolean(document.querySelector('[data-tauri-drag-region]'))`), 'editor retains its native drag region');

  const oldPrimary = await editor.evaluate(`document.querySelector('input[type=color]').value`);
  const color = oldPrimary === '#123456' ? '#654321' : '#123456';
  await editor.evaluate(`(()=>{const input=document.querySelector('input[type=color]');input.value=${JSON.stringify(color)};input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  await until(async () => await main.evaluate(`getComputedStyle(document.documentElement).getPropertyValue('--background').trim().toLowerCase()`) === color, 'color change previews in main window');
  assert.equal((await invoke('get_settings')).presets.theme, 'preset-1', 'preview does not save automatically');
  console.log('PASS: color edits preview in main without persisting');
  await click(editor, `document.querySelector('button[title="关闭 (Esc)"]')`);
  await until(async () => !(await labels()).includes('theme-editor'), 'editor X really closes');
  await until(async () => await main.evaluate(`document.documentElement.dataset.theme`) === 'preset-1', 'closing restores saved theme');
  console.log('PASS: editor closes and restores saved theme');

  // An opening error must keep the settings draft and show a visible error.
  settings = await settingsPage();
  await settings.evaluate(`(()=>{const fetchNative=window.fetch;window.fetch=(url,options)=>String(url).endsWith('/open_theme_editor')?Promise.resolve(new Response('theme-open-test-failure',{headers:{'Tauri-Response':'error','Content-Type':'text/plain'}})):fetchNative(url,options);})()`);
  await click(settings, button('打开 →'));
  await wait(500);
  assert.ok((await labels()).includes('settings'), 'failed editor open keeps settings window');
  assert.ok(await settings.evaluate(`document.body.innerText.includes('theme-open-test-failure')`), 'opening error is visible');
  await click(settings, button('取消'));
  await until(async () => !(await labels()).includes('settings'), 'cancel remains functional after failed open');
  assert.equal((await invoke('get_settings')).presets.theme, 'preset-1', 'failure did not save draft');
  console.log('PASS: failure leaves settings usable, displays error, and preserves saved values');

  settings = await settingsPage();
  await click(settings, button('打开 →'));
  const saveEditor = await windowByLabel('theme-editor');
  await saveEditor.evaluate(`(()=>{const input=document.querySelector('input[type=color]');input.value='#abcdef';input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  await click(saveEditor, button('保存并应用'));
  await until(async () => (await invoke('get_settings')).presets.custom_theme.background.toLowerCase() === '#abcdef', 'theme saves to backend');
  await click(saveEditor, `document.querySelector('button[title="关闭 (Esc)"]')`);
  await until(async () => !(await labels()).includes('theme-editor'), 'saved editor closes');
  await until(async () => await main.evaluate(`getComputedStyle(document.documentElement).getPropertyValue('--background').trim().toLowerCase()`) === '#abcdef', 'saved theme stays applied after closing');
  console.log('PASS: theme can be saved, and remains applied after editor closes');
} finally {
  clearTimeout(timeout);
  sockets.forEach(ws => ws.close());
}
