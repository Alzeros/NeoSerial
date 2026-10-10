import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, evaluate, until }) => {
  await call('Emulation.setDeviceMetricsOverride', { width: 700, height: 90, deviceScaleFactor: 2, mobile: false });
  await call('Page.navigate', { url: `${base}/tests/browser/update-reminder.html?window=main&hold` });
  await until('window.updateTest?.view.ready', 30000);
  assert.equal(await evaluate('Boolean(document.querySelector("[data-title-separator]"))'), false);
  await evaluate('updateTest.port.value = "COM5"; updateTest.tick()');
  assert.equal(await evaluate('document.querySelector("[data-title-port]").textContent.trim()'), 'COM5');
  assert.equal(await evaluate('getComputedStyle(document.querySelector("[data-app-version]")).fontSize'), '12px');
  assert.equal(await evaluate('document.querySelector("[data-title-separator]").getBoundingClientRect().height'), 12);
  assert.equal(await evaluate('getComputedStyle(document.querySelector("[data-title-separator]")).borderLeftWidth'), '1px');
  assert.equal(await evaluate('document.querySelector("[data-title-port]").closest("[data-theme-target]").getBoundingClientRect().height'), 32);
  const checkBaselines = async () => {
    const baselines = await evaluate(`(() => {
      const name = [...document.querySelectorAll('[data-tauri-drag-region]')].find(element => element.textContent === 'NeoSerial');
      return [name, document.querySelector('[data-app-version]'), document.querySelector('[data-title-port]')].map(element => {
        const marker = document.createElement('span');
        marker.style.cssText = 'display:inline-block;width:0;height:0;vertical-align:baseline';
        element.append(marker);
        const baseline = marker.getBoundingClientRect().top;
        marker.remove();
        return baseline;
      });
    })()`);
    assert.ok(Math.max(...baselines) - Math.min(...baselines) < 0.5, `Text baselines must align: ${baselines}`);
  };
  await checkBaselines();
  const capture = async name => {
    if (!process.argv.includes('--screenshots')) return;
    await evaluate('document.fonts.ready');
    const image = await call('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: 700, height: 32, scale: 1 } });
    await writeFile(`docs/${name}.png`, Buffer.from(image.data, 'base64'));
  };
  await capture('titlebar-light-group');
  await evaluate('updateTest.release()');
  await until('Boolean(document.querySelector("[data-update-notice]"))');
  assert.equal(await evaluate('document.querySelector("[data-update-notice] button").closest("[data-tauri-drag-region]")'), null);
  assert.ok(await evaluate('document.querySelector("[data-title-port]").getBoundingClientRect().left > document.querySelector("[data-update-notice]").getBoundingClientRect().right'));
  await capture('titlebar-light-group-update');
  await checkBaselines();
  await evaluate('updateTest.port.value = ""; updateTest.tick()');
  assert.equal(await evaluate('Boolean(document.querySelector("[data-title-separator]"))'), false);
  console.log('PASS: grouped title, version sizing, conditional separator and update layout');
}, { timeoutMs: 180000 });
