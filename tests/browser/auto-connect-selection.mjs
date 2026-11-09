import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, evaluate, until, delay }) => {
  let page = 0;
  const fresh = async () => {
    await call('Page.navigate', { url: `${base}/tests/browser/com-selection.html?case=${++page}` });
    await until('window.comTest?.connectionParams.port === "COM3"');
  };
  const toggle = async () => {
    await evaluate(`(() => {
      const trigger=document.querySelector('[data-connection-action] button[aria-haspopup="menu"]');
      if(trigger.getAttribute('aria-expanded')!=='true') trigger.click();
      return comTest.tick();
    })()`);
    await evaluate(`document.querySelector('[data-auto-connect-toggle]').click();comTest.tick()`);
  };
  const primary = () => evaluate(`document.querySelector('[data-connection-action] > button').click();comTest.tick()`);
  const commands = () => evaluate(`comTest.calls.filter(call=>['connect','set_auto_connect','auto_connect_once','disconnect'].includes(call.command))`);
  const checked = () => evaluate(`document.querySelector('[data-auto-connect-toggle]')?.checked`);

  await fresh();
  await toggle();
  await delay(120);
  assert.deepEqual(await commands(), [], 'Checking the option must not bind or open a serial port');
  assert.equal(await checked(), true);
  assert.equal(await evaluate(`document.querySelector('[data-auto-connect-toggle]').type`), 'checkbox');
  assert.equal(await evaluate(`document.querySelector('[data-connection-action] > button').dataset.diagnosticAction`), 'connect');
  assert.equal(await evaluate(`comTest.autoConnection.binding`), null);
  assert.ok(await evaluate(`Array.from(document.querySelectorAll('.custom-select-trigger')).every(control=>!control.disabled)`));

  await evaluate(`comTest.connectionParams.port='COM8';comTest.connectionParams.baudRate=9600;comTest.tick()`);
  await primary();
  await until('comTest.connected.value');
  const started = await commands();
  assert.equal(started[0].command, 'set_auto_connect');
  assert.equal(started[0].args.params.port, 'COM8');
  assert.equal(started[0].args.params.baud_rate, 9600);
  assert.ok(started.some(call=>call.command==='auto_connect_once'));
  assert.ok(!started.some(call=>call.command==='connect'), 'Automatic mode retains the native ownership checks');
  console.log('PASS: selecting is inert; Connect activates using the latest parameters');

  await primary();
  await until('!comTest.connected.value && comTest.binding === null');
  const attempts = (await commands()).filter(call=>call.command==='auto_connect_once').length;
  await delay(2200);
  assert.equal((await commands()).filter(call=>call.command==='auto_connect_once').length, attempts);
  await evaluate(`document.querySelector('[data-connection-action] button[aria-haspopup="menu"]').click();comTest.tick()`);
  assert.equal(await checked(), true, 'Manual disconnect keeps the next-connection option selected');
  await primary();
  await until('comTest.connected.value');
  assert.equal((await commands()).filter(call=>call.command==='set_auto_connect'&&call.args.params).length, 2);

  await toggle();
  await until('comTest.binding === null && comTest.autoConnection.binding === null');
  assert.equal(await checked(), false);
  assert.equal(await evaluate('comTest.connected.value'), true, 'Unchecking keeps the established connection');
  console.log('PASS: manual disconnect stops retries and preserves selection; unchecking keeps a live connection');

  await fresh();
  await primary();
  await until('comTest.connected.value');
  assert.deepEqual((await commands()).map(call=>call.command), ['connect'], 'Default remains a manual connection');
  await toggle();
  assert.equal(await checked(), true);
  assert.deepEqual((await commands()).map(call=>call.command), ['connect'], 'Checking while connected only configures the next connection');

  await fresh();
  await toggle();
  await evaluate('comTest.deferAttempt=true');
  await primary();
  await until(`comTest.calls.some(call=>call.command==='auto_connect_once')`);
  await toggle();
  await until('comTest.binding === null && comTest.autoConnection.binding === null');
  await evaluate('comTest.releaseAttempt();comTest.tick()');
  await until('!comTest.autoConnection.connecting');
  assert.equal(await evaluate('comTest.connected.value'), false, 'A late open cannot restore a cancelled session');
  assert.equal(await checked(), false);

  await fresh();
  await toggle();
  await evaluate('comTest.ports=[]');
  await primary();
  await until('comTest.autoConnection.binding !== null');
  assert.equal(await evaluate('comTest.connected.value'), false);
  assert.equal(await evaluate(`document.querySelector('[data-connection-action] > button').dataset.diagnosticAction`), 'disconnect');
  await primary();
  await until('comTest.binding === null && comTest.autoConnection.binding === null');
  await evaluate('comTest.ports=[{port:"COM3",device:null}]');
  await delay(2200);
  assert.equal(await evaluate('comTest.connected.value'), false, 'Restoring the port after manual cancellation must not reconnect');
  assert.deepEqual(await evaluate('window.errors'), []);
  console.log('PASS: defaults and connected selection stay inert; cancellation covers pending opens and absent ports');
});
