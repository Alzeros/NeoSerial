import assert from 'node:assert/strict';
import { access, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

// No Rust, device, signed-in browser, or real settings are used. Override the
// Chromium binary with BROWSER_BINARY (Chrome/Edge/Chromium), if necessary.
const root = fileURLToPath(new URL('../../', import.meta.url));

export async function withHeadlessBrowser(test) {
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
  const profile = await mkdtemp(join(tmpdir(), 'neoserial-headless-test-'));
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
      if (timedOut) { reject(new Error('Browser regression timed out')); return; }
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
      for (const request of pending.values()) request.reject(new Error('Browser regression timed out'));
      socket.close();
    }, 90000);

    const screenshot = async path => {
      await evaluate('document.fonts.ready');
      // Let 150–200ms color/opacity transitions settle before judging a theme by eye.
      await delay(300);
      const image = await call('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
      await writeFile(path, Buffer.from(image.data, 'base64'));
    };
    await test({ call, evaluate, until, click, fresh, delay, screenshot });
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

}
