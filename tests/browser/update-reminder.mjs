import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, evaluate, until, delay, screenshot }) => {
  let visited = false;
  const visit = async query => {
    if (visited) assert.deepEqual(await evaluate('updateTest.unexpected'), []);
    await call('Page.navigate', { url: `${base}/tests/browser/update-reminder.html?${query}` });
    try {
      await until('window.updateTest && (updateTest.view.ready || location.search.includes("win-other"))', 30000);
    } catch (error) {
      console.error(await evaluate('({ location: location.href, text: document.body.innerText, ready: window.updateTest?.view.ready, error: window.updateTest?.view.error, unexpected: window.updateTest?.unexpected })'));
      throw error;
    }
    visited = true;
  };
  const notice = () => evaluate('Boolean(document.querySelector("[data-update-notice]"))');
  await visit('window=main');
  assert.equal(await evaluate('document.querySelector("[data-app-version]")?.textContent.trim() ?? ""'), 'v0.3.7');
  assert.equal(await evaluate('document.querySelector("[data-app-version]").hasAttribute("data-tauri-drag-region")'), true);
  await evaluate('document.querySelector("[data-app-version]").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }))');
  await until('updateTest.maximized');
  await evaluate('document.querySelector("[data-app-version]").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }))');
  await until('!updateTest.maximized');
  assert.equal(await evaluate('updateTest.checks'), 0);
  await until('Boolean(document.querySelector("[data-update-notice]"))');
  assert.equal(await evaluate('updateTest.checks'), 1);
  assert.equal(await evaluate('updateTest.closed'), 1);
  assert.equal(await evaluate('document.querySelector("[data-app-version]").textContent.trim()'), 'v0.3.7');
  assert.equal(await evaluate('document.querySelector("[data-update-notice] button").title.includes("0.3.8")'), true);
  assert.equal(await evaluate('document.querySelector("[data-update-notice] button").closest("[data-tauri-drag-region]")'), null);
  assert.ok(await evaluate('document.querySelector("[data-update-notice]").getBoundingClientRect().left < 150'));
  await evaluate('document.querySelector("[data-update-notice] button").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }))');
  await delay(50);
  assert.equal(await evaluate('updateTest.maximized'), false);
  await evaluate('updateTest.port.value = "COM8"');
  await until('document.querySelector("[data-title-port]")?.textContent.includes("COM8")');
  assert.equal(await evaluate('document.querySelector("[data-title-port]").getBoundingClientRect().left >= document.querySelector("[data-app-version]").getBoundingClientRect().right'), true);
  if (process.argv.includes('--screenshots')) await screenshot('docs/update-notice.png');
  assert.equal(await evaluate('document.querySelector("[data-update-notice]").closest("[data-theme-target]").getBoundingClientRect().height'), 32);
  await evaluate('document.querySelector("[data-update-notice] button").click()');
  await until('updateTest.opened !== null');
  await evaluate('document.querySelector("[aria-label=稍后提醒]").click()');
  await until('!document.querySelector("[data-update-notice]") && updateTest.saved.updater.snoozed_version === "0.3.8"');
  assert.equal(await evaluate('document.querySelector("[data-app-version]").textContent.trim()'), 'v0.3.7');

  await visit('window=main');
  assert.equal(await notice(), false);
  assert.equal(await evaluate('updateTest.checks'), 0);
  await evaluate('updateTest.remote({ snooze_until: 0 })');
  await until('Boolean(document.querySelector("[data-update-notice]"))');
  await evaluate('updateTest.remote({ auto_check: false })');
  await until('!document.querySelector("[data-update-notice]")');

  await visit('window=settings');
  await call('Emulation.setDeviceMetricsOverride', { width: 680, height: 500, deviceScaleFactor: 1, mobile: false });
  if (process.argv.includes('--screenshots')) await screenshot('docs/update-card.png');
  assert.equal(await evaluate('document.querySelector("input[type=checkbox]").checked'), false);
  assert.equal(await evaluate('updateTest.checks'), 0);
  await evaluate('Array.from(document.querySelectorAll("button")).find(button => button.textContent.trim() === "重新检查").click()');
  await until('updateTest.checks === 1 && updateTest.closed === 1');
  assert.equal(await evaluate('updateTest.saved.updater.auto_check'), false);
  await evaluate('Array.from(document.querySelectorAll("button")).find(button => button.textContent.trim() === "忽略此版本").click()');
  await until('updateTest.saved.updater.ignored_version === "0.3.8"');
  await evaluate('document.querySelector("input[type=checkbox]").click()');
  await until('updateTest.saved.updater.auto_check === true');
  await evaluate('updateTest.failSave = true; document.querySelector("input[type=checkbox]").click()');
  await until('Boolean(document.querySelector("[role=alert]"))');
  assert.equal(await evaluate('document.querySelector("input[type=checkbox]").checked'), true);

  await visit('window=main');
  assert.equal(await notice(), false);
  await evaluate('updateTest.remote({ available_version: "0.3.9" })');
  await until('document.querySelector("[data-update-notice] button")?.title.includes("0.3.9")');
  await visit('window=win-other');
  await delay(100);
  await until('document.querySelector("[data-app-version]")?.textContent.trim() === "v0.3.7"');
  assert.equal(await notice(), false);
  assert.equal(await evaluate('updateTest.checks'), 0);

  await evaluate('sessionStorage.removeItem("update-prefs")');
  await visit('window=main&hold=1');
  await until('updateTest.checks === 1');
  await evaluate('updateTest.remote({ auto_check: false })');
  await until('!updateTest.view.settings.auto_check');
  await evaluate('updateTest.release()');
  await until('updateTest.closed === 1');
  assert.equal(await notice(), false);
  assert.equal(await evaluate('updateTest.saved.updater.available_version'), '');
  await visit('window=main');
  await delay(5200);
  assert.equal(await evaluate('updateTest.checks'), 0);
  assert.equal(await notice(), false);
  assert.deepEqual(await evaluate('updateTest.unexpected'), []);
}, { timeoutMs: 180000 });
console.log('Update reminder browser regressions passed');
