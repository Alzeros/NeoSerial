import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { withHeadlessBrowser } from './headless.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
await withHeadlessBrowser(async ({ base, debugUrl, call, until }) => {
  await call('Page.navigate', { url: `${base}/tests/browser/log-copy.html` });
  await until('Boolean(window.logTest)', 30000);
  for (const file of ['log-copy.mjs', 'log-retention.mjs', 'log-performance.mjs']) {
    console.log(`=== ${file} ===`);
    const exitCode = await new Promise((resolve, reject) => {
      const process = spawn(globalThis.process.execPath, [`tests/browser/${file}`], {
        cwd: root, windowsHide: true, stdio: 'inherit',
        env: { ...globalThis.process.env, VITE_URL: base, CHROME_DEBUG_URL: debugUrl },
      });
      process.once('error', reject);
      process.once('exit', resolve);
    });
    assert.equal(exitCode, 0, `${file} passes in an isolated browser`);
  }
}, { timeoutMs: 180000 });
