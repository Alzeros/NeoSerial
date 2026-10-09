import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, evaluate, until, delay }) => {
  await call('Page.navigate', { url: `${base}/tests/browser/com-selection.html` });
  await until('window.comTest?.connectionParams.port === "COM3"');
  await evaluate('document.querySelector(".custom-select-trigger").click()');
  await delay(100);
  assert.deepEqual(await evaluate('window.errors'), [], 'Opening COM menu must not throw');
  assert.deepEqual(await evaluate('Array.from(document.querySelectorAll("[role=option]"), option => option.textContent.trim())'), ['COM3', 'COM8']);
  await evaluate('document.querySelectorAll("[role=option]")[1].click()');
  await until('comTest.connectionParams.port === "COM8"');
  await evaluate('document.querySelector(".custom-select-trigger").click()');
  await delay(2200);
  assert.equal(await evaluate('document.querySelectorAll("[role=option]").length'), 2);
  assert.equal(await evaluate('comTest.connectionParams.port'), 'COM8');
  await evaluate('document.querySelectorAll(".custom-select-trigger")[1].click()');
  await delay(50);
  assert.ok(await evaluate('Array.from(document.querySelectorAll("[role=option]"), option => option.textContent.trim()).includes("115200")'));
});
console.log('COM selection regression passed');
