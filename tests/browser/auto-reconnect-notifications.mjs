import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, evaluate, until, delay }) => {
  await call('Page.navigate', { url: `${base}/tests/browser/notifications.html` });
  await until('Boolean(window.notificationTest)', 30000);
  await delay(150);
  const action = expression => evaluate(`(async()=>{${expression};await notificationTest.tick();})()`);
  const clear = () => evaluate('notificationTest.clear()');
  const emitError = error => action(`await notificationTest.emit('error',${JSON.stringify(error)})`);
  const errorCount = () => evaluate('document.querySelectorAll("[data-notification][data-kind=error]").length');
  const message = '串口读取错误: 拒绝访问。 (os error 5)';

  await action(`await notificationTest.connect();await notificationTest.emit('auto-connect-changed',{
    id:31,params:{port:'COM_TEST',baud_rate:115200,data_bits:'Eight',parity:'None',stop_bits:1,flow_control:'None'}
  })`);
  await until('notificationTest.stores.autoConnection.binding?.id === 31');
  await clear();
  for (let count = 0; count < 3; count++) await emitError({ message, port: 'COM_TEST', source: 'read' });
  assert.equal(await errorCount(), 0, 'Read interruption during active automatic reconnection must not produce repeated error banners');
  assert.equal(await evaluate('notificationTest.stores.autoConnection.error'), message, 'The cause remains available for recovery status/diagnostics');

  await action(`notificationTest.reply('list_ports',[]);await notificationTest.emit('connection-state',{connected:false,port:null,baud_rate:null})`);
  await until('document.body.textContent.includes("等待 COM_TEST")');
  assert.equal(await errorCount(), 0, 'A missing port is represented by the waiting state');
  console.log('PASS: repeated read interruptions remain recovery information and show waiting status');

  for (const error of [
    { message: 'Send failed after a partial write', port: 'COM_TEST', source: 'write' },
    { message: 'Read lock failure', port: 'COM_TEST', source: 'read_lock' },
    { message: 'Another port read failed', port: 'COM_OTHER', source: 'read' },
    { message: 'Legacy unclassified serial failure', port: 'COM_TEST' },
    { message: 'Read failure without port identity', source: 'read' },
  ]) {
    await clear();
    await emitError(error);
    assert.equal(await errorCount(), 1, `${error.message} must remain visible`);
  }

  // Removing the active binding leaves the next-connection checkbox selected.
  // A selected option alone must not suppress failures in a manual session.
  await action(`await notificationTest.emit('auto-connect-changed',null);await notificationTest.connect()`);
  await clear();
  await emitError({ message, port: 'COM_TEST', source: 'read' });
  assert.equal(await errorCount(), 1, 'A selected checkbox without an active reconnection binding must not hide read failures');
  console.log('PASS: send/lock errors, unrelated ports, legacy events and inactive sessions still report errors');
});
