import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ call, fresh, evaluate, until, click, delay, screenshot }) => {
  await call('Emulation.setDeviceMetricsOverride', {
    width: 680, height: 650, deviceScaleFactor: 1, mobile: false,
  });
  await fresh('settings');
  await evaluate(`persistenceTest.reopenSettings({section:'extensions',ext_module:'data',anchor:null})`);
  await until('document.body.innerText.includes("帧构造器选项")');
  const labels = `Array.from(document.querySelectorAll('[data-settings-group-label]')).map(label => label.textContent.trim())`;
  assert.deepEqual(await evaluate(labels), ['显示模块', '帧构造器选项', '编解码选项', '短信选项']);
  assert.equal(await evaluate(`document.querySelectorAll('[data-settings-group] [aria-expanded="true"]').length`), 0);
  await screenshot('docs/settings-groups-collapsed.png');
  await click('数据域生成方式');
  await delay(250);
  const geometry = await evaluate(`(() => {
    const boxes = [...document.querySelectorAll('[data-settings-group-box]')];
    const frame = boxes[1].getBoundingClientRect();
    const next = boxes[2].getBoundingClientRect();
    const content = boxes[1].querySelector('[data-collapse-content]').getBoundingClientRect();
    return { bottom: frame.bottom, nextTop: next.top, contentBottom: content.bottom,
      width: frame.width, nextWidth: next.width };
  })()`);
  assert.ok(geometry.contentBottom <= geometry.bottom, 'Expanded content stays inside its own group');
  assert.ok(geometry.nextTop - geometry.bottom >= 12, 'Separate groups retain a visible gap');
  assert.equal(geometry.width, geometry.nextWidth);
  await screenshot('docs/settings-groups-expanded.png');
  await call('Emulation.setDeviceMetricsOverride', {
    width: 680, height: 500, deviceScaleFactor: 1, mobile: false,
  });
  assert.ok(await evaluate(`persistenceTest.button('保存').getBoundingClientRect().bottom <= 500`));
  await click('通用');
  assert.deepEqual(await evaluate(labels), ['串口与发送', '应用']);
  assert.deepEqual(await evaluate(`Array.from(document.querySelectorAll('[data-settings-group-box]')).map(box => box.querySelectorAll(':scope > [data-settings-collapse]').length)`), [2, 3]);
  assert.equal(await evaluate(`document.querySelectorAll('[aria-expanded="true"]').length`), 0, 'General opens fully collapsed');
  await click('日志');
  assert.deepEqual(await evaluate(labels), ['内容', '显示']);
  assert.deepEqual(await evaluate(`Array.from(document.querySelectorAll('[data-settings-group-box]')).map(box => box.querySelectorAll(':scope > [data-settings-collapse]').length)`), [3, 0]);
  assert.equal(await evaluate(`document.querySelectorAll('[aria-expanded="true"]').length`), 0, 'Logs opens fully collapsed');
  await click('外观');
  assert.deepEqual(await evaluate(labels), ['主题']);
  console.log('PASS settings groups: labels, rows, expansion containment, spacing and footer');
}, { timeoutMs: 180000 });
