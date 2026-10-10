import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ call, fresh, evaluate, until, click, delay, screenshot }) => {
  await call('Emulation.setDeviceMetricsOverride', {
    width: 680, height: 650, deviceScaleFactor: 1, mobile: false,
  });
  await fresh('settings');
  await evaluate(`persistenceTest.reopenSettings({section:'extensions',ext_module:'data',anchor:null})`);
  await until('document.body.innerText.includes("帧构造器选项")');
  assert.deepEqual(await evaluate(`Array.from(document.querySelectorAll('.extension-settings-group > h3')).map(header => header.textContent)`),
    ['帧构造器选项', '编解码选项', '短信选项']);
  assert.equal(await evaluate(`document.querySelectorAll('.extension-settings-group [aria-expanded="true"]').length`), 0);
  await screenshot('docs/settings-groups-collapsed.png');
  await click('数据域生成方式');
  await delay(250);
  const geometry = await evaluate(`(() => {
    const groups = [...document.querySelectorAll('.extension-settings-group')];
    const first = groups[0].getBoundingClientRect();
    const second = groups[1].getBoundingClientRect();
    const content = groups[0].querySelector('[data-collapse-content]').getBoundingClientRect();
    return { bottom: first.bottom, nextTop: second.top, contentBottom: content.bottom,
      width: first.width, nextWidth: second.width };
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
  assert.equal(await evaluate(`document.querySelectorAll('.extension-settings-group').length`), 0);
  console.log('PASS extension settings groups: grouping, expansion containment, spacing and footer');
}, { timeoutMs: 180000 });
