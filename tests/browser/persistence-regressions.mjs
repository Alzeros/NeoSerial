import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ evaluate, until, click, fresh, delay }) => {
  await fresh('main');
  const quick = 'persistenceTest.stores.scriptModules[0].pages[0].commands[0]';
  await evaluate(`${quick}.command='EDITED';persistenceTest.tick()`);
  await evaluate(`document.querySelector('button[title="收起脚本面板"]').click();persistenceTest.tick()`);
  await evaluate(`document.querySelector('button[title="展开脚本面板"]').click();persistenceTest.tick()`);
  await until(`persistenceTest.sequence()[0].pages[0].commands[0].command==='EDITED'`);
  assert.equal(await evaluate(`${quick}.command`), 'EDITED');
  assert.equal(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='load_sequence_auto').length`), 1, 'showing the panel does not reload from disk');
  console.log('PASS: quick-command edits survive a hide/show cycle inside the debounce window');

  await evaluate(`persistenceTest.stores.scriptModules.find(m=>m.type==='data_processing').tools.find(t=>t.kind==='codec').config.input='UNSAVED CODEC';persistenceTest.tick()`);
  await evaluate(`document.querySelector('button[title="收起脚本面板"]').click();persistenceTest.tick()`);
  await until(`persistenceTest.sequence().find(m=>m.type==='data_processing').tools.find(t=>t.kind==='codec').config.input==='UNSAVED CODEC'`);
  console.log('PASS: data-tool edits are saved even while the panel stays hidden');

  const saves = await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='save_sequence_auto').length`);
  await evaluate(`persistenceTest.remoteCommand('REMOTE')`);
  await until(`${quick}.command==='REMOTE'`);
  await evaluate(`document.querySelector('button[title="展开脚本面板"]').click();persistenceTest.tick()`);
  await delay(950);
  assert.equal(await evaluate(`${quick}.command`), 'REMOTE');
  assert.equal(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='save_sequence_auto').length`), saves, 'remote reload does not echo-save');
  console.log('PASS: hidden panels keep cross-window synchronization without an autosave loop');

  await fresh('settings');
  await click('雾灰松');
  await evaluate(`persistenceTest.holdNext('patch_settings')`);
  await click('应用');
  await until(`persistenceTest.calls.some(c=>c.cmd==='patch_settings')`);
  assert.equal(await evaluate(`persistenceTest.button('应用').disabled && persistenceTest.button('保存').disabled`), true, 'serialize settings writes');
  await click('深海夜航');
  await evaluate('persistenceTest.release()');
  await until(`persistenceTest.saved().presets.theme==='preset-2' && !persistenceTest.button('应用').disabled`);
  assert.equal(await evaluate('persistenceTest.stores.theme.value'), 'preset-2', 'saved stores reflect the submitted value, not later edits');
  await click('应用');
  await until(`persistenceTest.saved().presets.theme==='preset-3' && persistenceTest.button('应用').disabled`);
  console.log('PASS: later edits stay dirty and are persisted by a second apply');

  await fresh('settings');
  await click('雾灰松');
  await evaluate(`persistenceTest.holdNext('patch_settings')`);
  await click('保存');
  await until(`persistenceTest.calls.some(c=>c.cmd==='patch_settings')`);
  await click('深海夜航');
  await evaluate('persistenceTest.release()');
  await until(`persistenceTest.saved().presets.theme==='preset-2' && !persistenceTest.button('应用').disabled`);
  assert.equal(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='plugin:window|hide').length`), 0, 'do not close with newer unsaved edits');
  await click('保存');
  await until(`persistenceTest.calls.some(c=>c.cmd==='plugin:window|hide')`);
  assert.equal(await evaluate('persistenceTest.saved().presets.theme'), 'preset-3');
  console.log('PASS: save keeps the window open when edits arrive during the write');

  await fresh('settings');
  await click('雾灰松');
  await click('扩展');
  await click('指令联想');
  await evaluate(`[...document.querySelectorAll('button[aria-expanded]')].find(b=>b.textContent.includes('知识库服务器')).click();persistenceTest.tick()`);
  const baseInput = "document.querySelector('input[placeholder*=\"http://127.0.0.1:8200\"]')";
  const keyInput = "document.querySelector('input[autocomplete=\"off\"]')";
  const input = async (selector, value) => {
    await evaluate(`(()=>{const el=${selector};el.value=${JSON.stringify(value)};el.dispatchEvent(new Event('input',{bubbles:true}));})()`);
    await evaluate('persistenceTest.tick()');
  };
  await input(baseInput, 'http://127.0.0.1:8201');
  await input(keyInput, 'isolated-test-key-1');
  await evaluate(`persistenceTest.holdNext('kb_set_api_key')`);
  await click('应用');
  await until(`persistenceTest.calls.some(c=>c.cmd==='kb_set_api_key')`);
  await input(baseInput, 'http://127.0.0.1:8202');
  await input(keyInput, 'isolated-test-key-2');
  await evaluate('persistenceTest.release()');
  await until(`persistenceTest.saved().command_index.base_url==='http://127.0.0.1:8201' && !persistenceTest.button('应用').disabled`);
  assert.equal(await evaluate(`${baseInput}.value`), 'http://127.0.0.1:8202', 'retain the newer address');
  assert.equal(await evaluate(`${keyInput}.value`), 'isolated-test-key-2', 'retain the newer credential input');
  await click('应用');
  await until(`persistenceTest.saved().command_index.base_url==='http://127.0.0.1:8202' && persistenceTest.button('应用').disabled`);
  assert.deepEqual(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='kb_set_api_key').map(c=>c.args.key)`), ['isolated-test-key-1', 'isolated-test-key-2']);
  assert.equal(await evaluate(`${baseInput}.value`), '');
  assert.equal(await evaluate(`${keyInput}.value`), '');
  console.log('PASS: snapshot precedes credential IPC and newer address/key inputs are not cleared');

  await fresh('settings');
  await click('雾灰松');
  await evaluate(`persistenceTest.failNext('patch_settings')`);
  await click('应用');
  await until(`document.body.innerText.includes('isolated save failure')`);
  assert.equal(await evaluate('persistenceTest.saved().presets.theme'), 'preset-1');
  assert.equal(await evaluate(`persistenceTest.button('应用').disabled`), false);
  await click('应用');
  await until(`persistenceTest.saved().presets.theme==='preset-2' && persistenceTest.button('应用').disabled`);
  console.log('PASS: failed saves retain the draft and can be retried');
});
