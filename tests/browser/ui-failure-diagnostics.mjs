import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

// These failures deliberately throw no JS exception. Logs must explain what actually happened.
await withHeadlessBrowser(async ({ base, call, evaluate, until, delay }) => {
  const trigger = '[data-diagnostic-control="serial-port"]';
  const freshPort = async suffix => {
    await call('Page.navigate', { url: `${base}/tests/browser/com-selection.html?diagnostics=1&case=${suffix}` });
    await until('window.comTest?.connectionParams.port === "COM3"');
    await evaluate('comTest.flushFrontendDiagnostics()');
  };
  const realClick = async selector => {
    const point = await evaluate(`(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return {x:r.x+r.width/2, y:r.y+r.height/2}; })()`);
    await call('Input.dispatchMouseEvent', { type: 'mousePressed', ...point, button: 'left', clickCount: 1 });
    await call('Input.dispatchMouseEvent', { type: 'mouseReleased', ...point, button: 'left', clickCount: 1 });
  };
  const portRecords = () => evaluate('comTest.flushFrontendDiagnostics().then(() => comTest.records)');

  await freshPort('normal');
  await realClick(trigger);
  await until('comTest.records.some(r => r.kind === "select.dom")');
  let records = await portRecords();
  const normal = records.find(r => r.kind === 'select.dom');
  assert.equal(normal.data.control, 'serial-port');
  assert.equal(normal.data.options_count, 2);
  assert.equal(normal.data.menu.present, true);
  assert.equal(normal.data.menu.css_hidden, false);
  assert.ok(normal.data.menu.hit_matches > 0);
  assert.ok(records.some(r => r.kind === 'ui.pointerdown' && r.data.trusted && r.data.control === 'serial-port'));
  await realClick('[role="option"]:nth-child(2)');
  await until('comTest.connectionParams.port === "COM8"');
  records = await portRecords();
  assert.ok(records.some(r => r.kind === 'select.close' && r.data.reason === 'option_selected'));
  assert.equal(records.filter(r => r.kind === 'select.opened').length, 1, 'Label activation must not reopen the menu');

  await freshPort('disabled');
  await evaluate('comTest.connected.value = true; comTest.tick()');
  await realClick(trigger);
  records = await portRecords();
  assert.ok(records.some(r => r.kind === 'ports.controls' && r.data.locked && r.data.connected));
  assert.ok(records.some(r => r.kind === 'ui.pointerdown' && r.data.control === 'serial-port' && r.data.disabled));
  assert.equal(records.some(r => r.kind === 'select.opened'), false);

  await freshPort('overlay');
  await evaluate(`(() => {
    const r = document.querySelector('${trigger}').getBoundingClientRect();
    const overlay = document.createElement('div'); overlay.dataset.diagnosticControl='test-overlay';
    Object.assign(overlay.style, {position:'fixed',left:r.x+'px',top:r.y+'px',width:r.width+'px',height:r.height+'px',zIndex:9999});
    document.body.append(overlay);
  })()`);
  await realClick(trigger);
  records = await portRecords();
  const blocked = records.find(r => r.kind === 'ui.pointerdown');
  assert.equal(blocked.data.control, 'test-overlay');
  assert.ok(blocked.data.controls_under_point.some(control => control.control === 'serial-port'));
  assert.equal(records.some(r => r.kind === 'select.toggle_received'), false, 'Overlay steals the input before the dropdown handler');

  await freshPort('hidden-menu');
  await evaluate(`(() => { const style=document.createElement('style'); style.textContent='[data-diagnostic-control="serial-port-menu"] { display:none!important }'; document.head.append(style); })()`);
  await realClick(trigger);
  await until('comTest.records.some(r => r.kind === "select.dom")');
  records = await portRecords();
  const hidden = records.find(r => r.kind === 'select.dom');
  assert.equal(hidden.data.open, true);
  assert.equal(hidden.data.menu.present, true);
  assert.equal(hidden.data.menu.css_hidden, true);
  assert.equal(hidden.data.menu.hit_matches, 0);
  assert.deepEqual(await evaluate('window.errors'), []);

  await freshPort('immediate-close');
  await evaluate(`document.addEventListener('click', () => setTimeout(() => document.body.dispatchEvent(new MouseEvent('mousedown', {bubbles:true})), 0), {once:true})`);
  await realClick(trigger);
  await until('comTest.records.some(r => r.kind === "select.close")');
  records = await portRecords();
  assert.ok(records.some(r => r.kind === 'select.opened'));
  assert.ok(records.some(r => r.kind === 'select.close' && r.data.reason === 'outside_pointer'));

  await call('Page.navigate', { url: `${base}/tests/browser/persistence-regressions.html?window=settings&entry=1&requestId=501` });
  await until('window.persistenceTest && persistenceTest.button("保存")');
  const settingsRecords = `persistenceTest.calls.filter(call => call.cmd === 'append_frontend_diagnostics').flatMap(call => call.args.records)`;
  await until(`${settingsRecords}.some(r => r.kind === 'settings.frontend' && r.data.stage === 'dom_probe' && r.data.request_id === 501)`);
  let settings = await evaluate(settingsRecords);
  for (const stage of ['request_consumed', 'prepare_started', 'settings_loaded', 'dom_committed', 'native_show_returned', 'acknowledged', 'dom_probe']) {
    assert.ok(settings.some(r => r.kind === 'settings.frontend' && r.data.stage === stage && r.data.request_id === 501), stage);
  }

  await evaluate(`(() => { const style=document.createElement('style'); style.textContent='[data-diagnostic-control="settings-page"] {display:none!important}'; document.head.append(style); })()`);
  await evaluate(`persistenceTest.reopenSettings({section:'about',ext_module:null,anchor:null,request_id:502})`);
  await until(`${settingsRecords}.some(r => r.kind === 'settings.frontend' && r.data.stage === 'dom_probe' && r.data.request_id === 502)`);
  settings = await evaluate(settingsRecords);
  const invisible = settings.find(r => r.kind === 'settings.frontend' && r.data.stage === 'dom_probe' && r.data.request_id === 502);
  assert.equal(invisible.data.root.present, true);
  assert.equal(invisible.data.root.css_hidden, true);
  assert.ok(settings.some(r => r.kind === 'settings.frontend' && r.data.stage === 'acknowledged' && r.data.request_id === 502 && !r.data.failure));

  // A different page keeps the rate limiter independent of the above clicks and probes.
  await call('Page.navigate', { url: `${base}/tests/browser/persistence-regressions.html?window=settings&entry=1&requestId=600` });
  await until('window.persistenceTest && persistenceTest.button("保存")');
  await evaluate(`persistenceTest.button('取消').click(); persistenceTest.tick()`);
  await until('!persistenceTest.button("保存")');
  await evaluate(`persistenceTest.holdNext('get_settings'); persistenceTest.reopenSettings({section:'general',ext_module:null,anchor:null,request_id:601})`);
  await until(`${settingsRecords}.some(r => r.kind === 'settings.frontend' && r.data.stage === 'prepare_failed' && r.data.request_id === 601)`, 10000);
  settings = await evaluate(settingsRecords);
  assert.ok(settings.some(r => r.kind === 'settings.frontend' && r.data.stage === 'loading_settings' && r.data.request_id === 601));
  assert.ok(!settings.some(r => r.kind === 'settings.frontend' && r.data.stage === 'settings_loaded' && r.data.request_id === 601));
  assert.ok(await evaluate(`persistenceTest.calls.some(call => call.cmd === 'complete_settings_open' && call.args.requestId === 601 && call.args.error?.includes('超时'))`));
  await evaluate('persistenceTest.release()');
  await delay(50);
  console.log('PASS silent UI failures: real clicks, disabled controls, transparent overlay, hidden menu, immediate close, hidden settings DOM and stalled settings load');
}, { timeoutMs: 120000 });
