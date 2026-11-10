import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ fresh, evaluate, until, click, delay }) => {
  const preference = '[data-settings-pause-follow-on-scroll]';
  const following = 'persistenceTest.stores.autoScroll.value';
  const atBottom = "(()=>{const element=document.querySelector('[role=log]');return Math.abs(element.scrollHeight-element.clientHeight-element.scrollTop)<5;})()";
  const scrollUp = async gesture => {
    await evaluate(`(()=>{
      const element=document.querySelector('[role=log]');
      ${gesture}
      element.scrollTop=Math.max(0,element.scrollTop-100);
      element.dispatchEvent(new Event('scroll'));
      return persistenceTest.tick();
    })()`);
  };
  const gestures = [
    "element.dispatchEvent(new WheelEvent('wheel',{deltaY:-100,bubbles:true}));",
    ...['ArrowUp', 'PageUp', 'Home'].map(key => `element.dispatchEvent(new KeyboardEvent('keydown',{key:${JSON.stringify(key)},bubbles:true}));`),
    "element.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));",
  ];
  const resume = async () => {
    await evaluate("document.querySelector('[data-log-follow]').click();persistenceTest.tick()");
    await until(atBottom);
    await delay(80);
  };
  const append = async () => {
    await evaluate('persistenceTest.stores.appendLogLine(makeFollowRow(++followIndex));persistenceTest.tick()');
    await delay(80);
  };

  await fresh('main');
  await evaluate(`(()=>{
    const stores=persistenceTest.stores;
    window.followIndex=200;
    window.makeFollowRow=index=>({ts:'12:00:00',ts_ms:10000+index,dir:'rx',ascii:'record '+index,raw:[65],line_index:index,is_error:false});
    stores.clearLogLines();
    stores.appendLogLines(Array.from({length:followIndex},(_,index)=>makeFollowRow(index+1)));
    return persistenceTest.tick();
  })()`);
  await until(atBottom);
  await delay(80);
  assert.equal(await evaluate('persistenceTest.stores.cachedSettings.value.ui.pause_follow_on_scroll'), undefined);
  for (const gesture of gestures) {
    await scrollUp(gesture);
    assert.equal(await evaluate(following), true, 'old settings default to keeping following after upward navigation');
    await append();
    assert.equal(await evaluate(atBottom), true, 'new records restore the bottom while following stays enabled');
  }
  console.log('PASS: missing preference keeps following across wheel, keyboard and scrollbar movement.');

  await evaluate('persistenceTest.remoteSettings({ui:{pause_follow_on_scroll:true}})');
  for (const gesture of gestures) {
    await scrollUp(gesture);
    assert.equal(await evaluate(following), false, 'enabled preference stops following for upward navigation');
    const position = await evaluate("document.querySelector('[role=log]').scrollTop");
    await append();
    assert.ok(Math.abs((await evaluate("document.querySelector('[role=log]').scrollTop")) - position) < 3);
    await resume();
  }
  await evaluate("document.querySelector('[role=log]').dispatchEvent(new WheelEvent('wheel',{deltaY:-1,bubbles:true}));persistenceTest.tick()");
  assert.equal(await evaluate(following), false, 'enabled preference still handles a tiny upward wheel gesture');
  await evaluate('persistenceTest.remoteSettings({ui:{pause_follow_on_scroll:false}})');
  assert.equal(await evaluate(following), false, 'disabling the preference does not resume previously stopped following');
  await append();
  assert.equal(await evaluate(following), false);
  await evaluate('persistenceTest.stores.paused.value=true;persistenceTest.tick()');
  await resume();
  assert.equal(await evaluate('persistenceTest.stores.paused.value'), true, 'return to latest does not resume paused display');
  await evaluate('persistenceTest.stores.paused.value=false;persistenceTest.tick()');
  for (const gesture of gestures) {
    await scrollUp(gesture);
    assert.equal(await evaluate(following), true, 'explicitly disabled preference keeps following');
    await append();
    assert.equal(await evaluate(atBottom), true);
  }
  await evaluate('persistenceTest.remoteSettings({ui:{show_log_tools:true}})');
  await evaluate("document.querySelector('[data-log-follow-toggle]').click();persistenceTest.tick()");
  assert.equal(await evaluate(following), false, 'manual follow toggle still works with automatic suspension disabled');
  await append();
  assert.equal(await evaluate(following), false);
  await resume();
  assert.equal(await evaluate('persistenceTest.stores.paused.value'), false);
  await evaluate("window.dispatchEvent(new KeyboardEvent('keydown',{key:'f',ctrlKey:true}));persistenceTest.tick()");
  assert.equal(await evaluate(following), false, 'search still stops following with automatic suspension disabled');
  await evaluate(`(()=>{
    const input=document.querySelector('input[placeholder="搜索日志..."]');
    input.value='record 100';
    input.dispatchEvent(new Event('input',{bubbles:true}));
    return persistenceTest.tick();
  })()`);
  await until("!document.querySelector('button[title=\"下一个 (Enter)\"]').disabled");
  await evaluate("document.querySelector('button[title=\"下一个 (Enter)\"]').click();persistenceTest.tick()");
  await delay(80);
  assert.equal(await evaluate(atBottom), false, 'search navigation reveals an earlier matching record');
  const searchPosition = await evaluate("document.querySelector('[role=log]').scrollTop");
  await append();
  assert.equal(await evaluate(following), false);
  assert.ok(Math.abs((await evaluate("document.querySelector('[role=log]').scrollTop")) - searchPosition) < 3, 'new records preserve the search reading position');
  await evaluate("window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));persistenceTest.tick()");
  assert.equal(await evaluate(following), false, 'closing search does not resume following');
  await resume();
  assert.equal(await evaluate("persistenceTest.calls.filter(record=>record.cmd==='patch_settings').length"), 0, 'scrolling and following never persist preference changes');
  console.log('PASS: enabled/disabled changes apply live and leave manual following and display pause independent.');

  const openLogSettings = async () => {
    await click('日志');
    if ((await evaluate("persistenceTest.button('编码与跟随').getAttribute('aria-expanded')")) === 'false') await click('编码与跟随');
    await until(`Boolean(document.querySelector(${JSON.stringify(preference)}))`);
  };
  const toggle = async () => {
    await evaluate(`document.querySelector(${JSON.stringify(preference)}).click();persistenceTest.tick()`);
  };
  await fresh('settings');
  await openLogSettings();
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(preference)}).checked`), false);
  assert.equal(await evaluate("persistenceTest.button('应用').disabled"), true, 'missing preference does not dirty old settings');
  await toggle();
  assert.equal(await evaluate("persistenceTest.button('应用').disabled"), false);
  assert.equal(await evaluate('persistenceTest.stores.cachedSettings.value.ui.pause_follow_on_scroll'), undefined, 'draft changes do not preview or persist');
  await click('取消');
  assert.equal(await evaluate("persistenceTest.calls.filter(record=>record.cmd==='patch_settings').length"), 0);
  await evaluate("persistenceTest.reopenSettings({section:'general',ext_module:null,anchor:null})");
  await openLogSettings();
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(preference)}).checked`), false);
  await toggle();
  await evaluate('persistenceTest.remoteSettings({ui:{log_send:false}})');
  await click('应用');
  await until("persistenceTest.saved().ui.pause_follow_on_scroll===true&&persistenceTest.button('应用').disabled");
  assert.deepEqual(await evaluate("persistenceTest.calls.find(record=>record.cmd==='patch_settings').args.patch"), { ui: { pause_follow_on_scroll: true } });
  assert.equal(await evaluate('persistenceTest.saved().ui.log_send'), false, 'differential patch preserves concurrent changes');
  assert.equal(await evaluate('persistenceTest.stores.cachedSettings.value.ui.pause_follow_on_scroll'), true);
  await click('取消');
  await evaluate("persistenceTest.reopenSettings({section:'general',ext_module:null,anchor:null})");
  await openLogSettings();
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(preference)}).checked`), true, 'saved preference survives reopening');
  await toggle();
  await evaluate("persistenceTest.failNext('patch_settings')");
  await click('应用');
  await until("document.body.textContent.includes('isolated save failure')");
  assert.equal(await evaluate('persistenceTest.saved().ui.pause_follow_on_scroll'), true, 'failed writes do not replace saved preference');
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(preference)}).checked`), false, 'failed writes retain the edited preference');
  assert.equal(await evaluate("persistenceTest.button('应用').disabled"), false);
  const hideCount = await evaluate("persistenceTest.calls.filter(record=>record.cmd==='plugin:window|hide').length");
  await click('保存');
  await until(`persistenceTest.saved().ui.pause_follow_on_scroll===false&&persistenceTest.calls.filter(record=>record.cmd==='plugin:window|hide').length>${hideCount}`);
  console.log('PASS: General switch defaults, draft/cancel, differential Apply/Save, reopen and write recovery.');
});
