import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ call, fresh, click, evaluate, delay }) => {
  await call('Emulation.setDeviceMetricsOverride', {
    width: 680, height: 500, deviceScaleFactor: 1, mobile: false,
  });
  await fresh('settings');
  await click('通用');
  const measure = () => evaluate(`(() => {
    const header = persistenceTest.button('日志显示');
    let container = header.parentElement;
    while (container && getComputedStyle(container).overflowY !== 'auto') {
      container = container.parentElement;
    }
    const rect = header.getBoundingClientRect();
    const save = persistenceTest.button('保存').getBoundingClientRect();
    return {
      width: rect.width, left: rect.left,
      clientWidth: container.clientWidth,
      overflowing: container.scrollHeight > container.clientHeight,
      saveTop: save.top, saveBottom: save.bottom,
    };
  })()`);
  const collapsed = await measure();
  assert.equal(collapsed.overflowing, false);
  await click('日志显示');
  await delay(250);
  const expanded = await measure();
  assert.equal(expanded.overflowing, true);
  assert.equal(expanded.width, collapsed.width, 'Expanding must not narrow setting headers');
  assert.equal(expanded.left, collapsed.left);
  assert.equal(expanded.clientWidth, collapsed.clientWidth);
  assert.equal(expanded.saveTop, collapsed.saveTop);
  assert.ok(expanded.saveBottom <= 500, 'Footer stays visible');
  await click('日志显示');
  await delay(250);
  assert.deepEqual(await measure(), collapsed);
  console.log('PASS settings scrollbar: stable content width and footer on expand/collapse');
}, { timeoutMs: 180000 });
