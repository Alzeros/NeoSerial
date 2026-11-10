import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ call, fresh, click, evaluate, delay }) => {
  await call('Emulation.setDeviceMetricsOverride', {
    width: 680, height: 500, deviceScaleFactor: 1, mobile: false,
  });
  await fresh('settings');
  const measure = (title) => evaluate(`(() => {
    const header = persistenceTest.button(${JSON.stringify(title)});
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

  // 通用页:全收起时一屏放得下(设计要求),底部按钮可见
  await click('通用');
  const general = await measure('预设波特率');
  assert.equal(general.overflowing, false, 'General fits without scrolling when fully collapsed');
  assert.ok(general.saveBottom <= 500, 'Footer stays visible');

  // 日志页:展开/收起一行,内容区宽度和底部按钮都不动
  await click('日志');
  const collapsed = await measure('缓存上限');
  await click('缓存上限');
  await delay(250);
  const expanded = await measure('缓存上限');
  assert.equal(expanded.overflowing, true);
  assert.equal(expanded.width, collapsed.width, 'Expanding must not narrow setting headers');
  assert.equal(expanded.left, collapsed.left);
  assert.equal(expanded.clientWidth, collapsed.clientWidth);
  assert.equal(expanded.saveTop, collapsed.saveTop);
  assert.ok(expanded.saveBottom <= 500, 'Footer stays visible');
  await click('缓存上限');
  await delay(250);
  assert.deepEqual(await measure('缓存上限'), collapsed);
  console.log('PASS settings scrollbar: general fits collapsed; log rows keep width and footer on expand/collapse');
}, { timeoutMs: 180000 });
