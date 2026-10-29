import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, evaluate, until, delay, screenshot }) => {
  await call('Page.addScriptToEvaluateOnNewDocument', { source: `
    window.activityTimerDelays=[];
    window.activityTimerIds=new Set();
    const nativeSetInterval=window.setInterval;
    const nativeClearInterval=window.clearInterval;
    window.setInterval=function(callback,interval,...args){
      const activityTimer=new Error().stack?.includes('/src/components/McpActivityStatus.svelte');
      const timer=nativeSetInterval.call(window,callback,interval,...args);
      if(activityTimer){window.activityTimerDelays.push(interval);window.activityTimerIds.add(timer);}
      return timer;
    };
    window.clearInterval=function(timer){
      window.activityTimerIds.delete(timer);
      return nativeClearInterval.call(window,timer);
    };
  ` });
  const navigate = async scenario => {
    await call('Page.navigate', { url: `${base}/tests/browser/notifications.html?scenario=${scenario}` });
    await until('Boolean(window.notificationTest)', 30000);
  };
  const action = expression => evaluate(`(async()=>{${expression};await notificationTest.tick();})()`);
  const text = () => evaluate('document.querySelector("[data-mcp-activity]").textContent');
  const expect = async pattern => {
    await until(`new RegExp(${JSON.stringify(pattern.source)}).test(document.querySelector('[data-mcp-activity]')?.textContent ?? '')`);
  };
  const expectHiddenActivity = async () => {
    assert.equal(await evaluate('document.querySelector("[data-mcp-activity]")'), null, 'disabled MCP service hides the entire activity summary');
    assert.equal(await evaluate('activityTimerIds.size'), 0, 'disabled MCP service has no activity clock or reconciliation timer');
    const readsBefore = await evaluate('notificationTest.calls.filter(entry=>entry.cmd==="get_mcp_activity").length');
    await action('window.dispatchEvent(new Event("focus"))');
    assert.equal(await evaluate('notificationTest.calls.filter(entry=>entry.cmd==="get_mcp_activity").length'), readsBefore, 'disabled MCP service does not refresh activity on focus');
  };
  const active = (id, port, tool = 'send_and_read') => ({ id, port, tool, started_at_ms: Date.now() - 3000, finished_at_ms: null, ok: null });
  const complete = (id, port, tool = 'send', ok = true) => ({ ...active(id, port, tool), finished_at_ms: Date.now() - 10000, ok });
  const publish = async (revision, activeCalls = [], recent = [], running = true) => {
    const snapshot = { revision, active: activeCalls, recent, running };
    await action(`await notificationTest.emit('mcp-activity',${JSON.stringify(snapshot)})`);
  };
  const pointerClick = async selector => {
    const point = await evaluate(`(()=>{const element=document.querySelector(${JSON.stringify(selector)}),rect=element.getBoundingClientRect(),x=rect.left+rect.width/2,y=rect.top+rect.height/2;return {x,y,reachable:element.contains(document.elementFromPoint(x,y))}})()`);
    assert.equal(point.reachable, true, `${selector} is pointer accessible`);
    await call('Input.dispatchMouseEvent', { type: 'mousePressed', button: 'left', clickCount: 1, x: point.x, y: point.y });
    await call('Input.dispatchMouseEvent', { type: 'mouseReleased', button: 'left', clickCount: 1, x: point.x, y: point.y });
    await action('');
  };

  await navigate('activity-disabled');
  await expectHiddenActivity();
  assert.equal(await evaluate('notificationTest.calls.filter(entry=>entry.cmd==="get_mcp_activity").length'), 0, 'a disabled startup never queries MCP activity');
  await publish(1, [active(1, 'COM_TEST')]);
  await expectHiddenActivity();
  await action('notificationTest.stores.cachedSettings.value.mcp.auto_start=true');
  await expect(/未启用/);
  assert.equal(await evaluate('activityTimerIds.size'), 2, 'enabling MCP mounts the activity summary and its refresh timers');
  await action('notificationTest.stores.cachedSettings.value.mcp.auto_start=false');
  await expectHiddenActivity();
  console.log('PASS: disabled MCP hides activity on startup and settings changes, releases timers/listeners, and ignores runtime activity events.');

  await navigate('activity');
  await expect(/未启用/);
  assert.deepEqual(await evaluate('activityTimerDelays'),[1000,5000],'activity clock refreshes once per second; snapshot reconciliation stays unchanged');
  await publish(1, [], [], false);
  await expect(/未启用/);
  await navigate('activity');
  await expect(/未启用/);
  await action(`notificationTest.stores.currentPort.value='COM_TEST';notificationTest.stores.connectionParams.port='COM_TEST';notificationTest.stores.windowPort.value='COM_TEST';notificationTest.stores.connected.value=true`);
  await publish(1, [active(1, 'COM_OTHER')]);
  await expect(/暂无执行中的调用/);
  assert.doesNotMatch(await text(), /COM_OTHER|等待串口响应/);
  await publish(2, [active(2, 'COM_TEST')]);
  await expect(/等待串口响应/);
  const before = await text();
  assert.match(before,/ · \d+s$/);
  assert.doesNotMatch(before,/\d+\.\d+s/);
  await until(`document.querySelector('[data-mcp-activity]')?.textContent !== ${JSON.stringify(before)}`, 3000);
  assert.notEqual(await text(), before, 'Elapsed time progresses while the tool is active');
  assert.match(await text(),/ · \d+s$/);
  await publish(3, [active(2, 'COM_TEST'), active(3, 'COM_TEST', 'send_file'), active(4, 'COM_OTHER')]);
  await expect(/2 项调用/);
  assert.doesNotMatch(await evaluate('document.querySelector("[data-mcp-activity]").title'), /COM_OTHER/);
  await publish(4, [active(2, 'COM_TEST')], [complete(3, 'COM_TEST', 'send_file')]);
  await expect(/等待串口响应/);
  await publish(5, [], [complete(2, 'COM_TEST')]);
  await expect(/发送已提交/);
  assert.doesNotMatch(await text(), /设备.*成功|控制中|思考/);
  await publish(3, [active(2, 'COM_TEST')]);
  await expect(/发送已提交/);
  await publish(6, [], [complete(5, 'COM_TEST', 'sequence_run')]);
  await expect(/启动序列调用已完成/);
  assert.match(await evaluate('document.querySelector("[data-mcp-activity]").title'), /后台序列可能继续运行/);
  await publish(7, [], [complete(6, 'COM_TEST', 'send', false)]);
  await expect(/失败/);
  assert.equal(await evaluate('document.querySelector("[data-mcp-activity]").dataset.tone'), 'error');
  assert.equal(await evaluate('document.querySelectorAll("[data-notification]").length'), 0, 'MCP activity failures do not produce repeated toasts');
  console.log('PASS: port isolation, real lifecycle summaries, concurrency, timers, stale events and honest completion/failure wording.');

  await publish(8, [active(7, 'COM_TEST')]);
  await publish(9, [active(7, 'COM_TEST')], []);
  await expect(/等待串口响应/);
  await publish(10);
  await expect(/暂无执行中的调用/);
  await action(`notificationTest.stores.currentPort.value=null;notificationTest.stores.windowPort.value=null;notificationTest.stores.connectionParams.port=''`);
  await publish(11, [active(8, 'COM_OTHER'), active(9, null, 'get_settings')]);
  await expect(/应用级.*读取配置/);
  await action(`notificationTest.stores.currentPort.value='COM_TEST';notificationTest.stores.connected.value=true;notificationTest.stores.windowPort.value='COM_TEST';notificationTest.stores.connectionParams.port='COM_TEST'`);
  await expect(/暂无执行中的调用/);
  await action(`notificationTest.stores.currentPort.value='COM_OTHER';notificationTest.stores.windowPort.value='COM_OTHER'`);
  await expect(/等待串口响应/);
  await publish(12, [active(10, 'COM_TEST'), active(11, null, 'get_settings')], [complete(12, 'COM_OTHER', 'disconnect')]);
  await action(`await notificationTest.emit('connection-state',{connected:false,port:'COM_OTHER',baud_rate:null});notificationTest.stores.connectionParams.port='COM_TEST'`);
  assert.equal(await evaluate('notificationTest.stores.currentPort.value'), 'COM_OTHER', 'The real disconnect handler retains the previous port');
  assert.equal(await evaluate('notificationTest.stores.connected.value'), false);
  await expect(/等待串口响应/);
  assert.match(await evaluate('document.querySelector("[data-mcp-activity]").title'), /COM_TEST/);
  await action(`notificationTest.stores.connectionParams.port=''`);
  await expect(/应用级.*读取配置/);
  await action(`notificationTest.stores.currentPort.value='COM_OTHER';notificationTest.stores.windowPort.value='COM_OTHER';notificationTest.stores.connectionParams.port='COM_OTHER';notificationTest.stores.connected.value=true`);
  await publish(13, [active(13, 'COM_OTHER')]);
  await expect(/等待串口响应/);
  console.log('PASS: clear/finish transitions, application scope and window port changes do not leak activity across ports.');

  await action(`notificationTest.reply('get_mcp_call_log',[{ts:new Date().toISOString(),tool:'send',args:'{"port":"COM_OTHER"}',ok:false,error:'isolated write rejected',duration_ms:1}]);notificationTest.stores.cachedSettings.value.mcp.auto_start=true;notificationTest.stores.cachedSettings.value.ui.show_mcp_tab=true;notificationTest.stores.scriptPanelOpen.value=false`);
  const savesBefore = await evaluate('notificationTest.calls.filter(entry=>/save|patch/.test(entry.cmd)).length');
  await pointerClick('[data-mcp-activity]');
  await until('document.querySelector("[data-module-tab=mcp]")?.getAttribute("aria-current")==="page"');
  await until('document.body.textContent.includes("isolated write rejected")');
  assert.equal(await evaluate('notificationTest.stores.scriptPanelOpen.value'), true);
  assert.equal(await evaluate('notificationTest.stores.cachedSettings.value.ui.show_mcp_tab'), true);
  await action('notificationTest.stores.cachedSettings.value.ui.show_mcp_tab=false');
  assert.equal(await evaluate('document.querySelector("[data-module-tab=mcp]")'), null, 'disabling the log tab removes it even after activity navigation');
  assert.equal(await evaluate('document.querySelector("[data-module-tab=scripts]")?.getAttribute("aria-current")'), 'page');
  assert.equal(await evaluate('document.body.textContent.includes("isolated write rejected")'), false, 'disabling the log tab also removes the open log content');

  const settingsBefore = await evaluate('notificationTest.calls.filter(entry=>entry.cmd==="open_settings_window").length');
  await action('notificationTest.stores.scriptPanelOpen.value=false');
  await pointerClick('[data-mcp-activity]');
  await until(`notificationTest.calls.filter(entry=>entry.cmd==="open_settings_window").length>${settingsBefore}`);
  assert.deepEqual(await evaluate('notificationTest.calls.filter(entry=>entry.cmd==="open_settings_window").at(-1).args'), { section: 'extensions', extModule: 'mcp', anchor: null });
  assert.equal(await evaluate('notificationTest.stores.scriptPanelOpen.value'), false, 'a disabled log shortcut must not force the sidebar open');
  assert.equal(await evaluate('notificationTest.stores.mcpLogRequest.pending'), false);
  assert.equal(await evaluate('notificationTest.stores.cachedSettings.value.ui.show_mcp_tab'), false);
  assert.match(await evaluate('document.querySelector("[data-mcp-activity]").title'), /设置.*扩展.*MCP 服务/);
  assert.match(await evaluate('document.querySelector("[data-mcp-activity]").getAttribute("aria-label")'), /设置/);
  await publish(14, [active(14, 'COM_OTHER')]);
  assert.equal(await evaluate('document.querySelector("[data-module-tab=mcp]")'), null, 'activity updates never reopen a disabled tab');

  await action('notificationTest.stores.cachedSettings.value.ui.show_mcp_tab=true');
  await pointerClick('[data-mcp-activity]');
  await until('document.querySelector("[data-module-tab=mcp]")?.getAttribute("aria-current")==="page"');
  await action('notificationTest.stores.cachedSettings.value.mcp.auto_start=false');
  assert.equal(await evaluate('document.querySelector("[data-module-tab=mcp]")'), null, 'disabling MCP service also hides a log opened from activity');
  await expectHiddenActivity();
  assert.equal(await evaluate('notificationTest.stores.cachedSettings.value.ui.show_mcp_tab'), true, 'disabling MCP preserves the saved tab preference');
  await action('notificationTest.stores.mcpLogRequest.pending=true');
  assert.equal(await evaluate('notificationTest.stores.mcpLogRequest.pending'), false, 'stale navigation requests are consumed without overriding settings');
  assert.equal(await evaluate('document.querySelector("[data-module-tab=scripts]")?.getAttribute("aria-current")'), 'page');
  await action('notificationTest.stores.scriptPanelOpen.value=false');
  await publish(15, [active(15, 'COM_OTHER')]);
  await expectHiddenActivity();
  assert.equal(await evaluate('notificationTest.stores.scriptPanelOpen.value'), false, 'activity events never reopen a sidebar while MCP is disabled');
  assert.equal(await evaluate('notificationTest.stores.cachedSettings.value.mcp.auto_start'), false, 'navigation never enables MCP service');

  await action('notificationTest.stores.cachedSettings.value.mcp.auto_start=true;delete notificationTest.stores.cachedSettings.value.ui.show_mcp_tab;notificationTest.stores.scriptPanelOpen.value=true');
  assert.equal(await evaluate('document.querySelector("[data-module-tab=mcp]")'), null, 'a missing tab setting uses the backend default of hidden');
  await action('notificationTest.stores.cachedSettings.value.ui.show_mcp_tab=true');
  await pointerClick('[data-mcp-activity]');
  await until('document.querySelector("[data-module-tab=mcp]")?.getAttribute("aria-current")==="page"');
  assert.equal(await evaluate('notificationTest.calls.filter(entry=>/save|patch/.test(entry.cmd)).length'), savesBefore, 'Navigation never changes persisted settings');
  console.log('PASS: activity navigation respects tab/service settings, removes disabled log content, guides hidden logs to settings, and preserves saved preferences.');

  await call('Emulation.setDeviceMetricsOverride', { width: 800, height: 640, deviceScaleFactor: 1, mobile: false });
  await action(`notificationTest.stores.rxBytes.value=99999999;notificationTest.stores.txBytes.value=88888888`);
  for (const theme of ['preset-1', 'preset-2', 'preset-3', 'preset-4']) {
    await action(`document.documentElement.dataset.theme=${JSON.stringify(theme)}`);
    assert.deepEqual(await evaluate(`(()=>{const element=document.querySelector('[data-mcp-activity]'),rect=element.getBoundingClientRect(),style=getComputedStyle(element);return [rect.width>80,rect.left>=0,rect.right<=innerWidth+1,rect.bottom<=innerHeight+1,document.documentElement.scrollWidth<=innerWidth+1,style.color!==style.backgroundColor]})()`), [true, true, true, true, true, true]);
    await pointerClick('[data-mcp-activity]');
  }
  if (process.env.SCREENSHOT_PATH) {
    await call('Emulation.setDeviceMetricsOverride', { width: 1400, height: 860, deviceScaleFactor: 1, mobile: false });
    await action(`document.documentElement.dataset.theme='preset-1'`);
    await screenshot(process.env.SCREENSHOT_PATH);
  }
  console.log('PASS: minimum-size layouts, long summaries, themes and pointer access remain usable.');

  await navigate('activity-race');
  await action(`notificationTest.stores.currentPort.value='COM_TEST';notificationTest.stores.connectionParams.port='COM_TEST';notificationTest.stores.connected.value=true`);
  await publish(20, [], [complete(10, 'COM_TEST')]);
  await action(`notificationTest.resolveActivity({running:false,revision:1,active:[${JSON.stringify(active(10, 'COM_TEST'))}],recent:[]})`);
  await expect(/发送已提交/);
  await delay(150);
  await expect(/发送已提交/);
  await action(`notificationTest.failNext('get_mcp_activity','isolated query failure');window.dispatchEvent(new Event('focus'))`);
  await expect(/暂不可用/);
  await action(`notificationTest.reply('get_mcp_activity',{running:true,revision:21,active:[],recent:[]});window.dispatchEvent(new Event('focus'))`);
  await expect(/暂无执行中的调用/);
  assert.equal(await evaluate('notificationTest.calls.some(entry=>["connect","disconnect","send","sequence_run","sequence_stop"].includes(entry.cmd))'), false, 'Activity display never controls the serial connection');
  console.log('PASS: a slow initial snapshot cannot overwrite new events; passive refresh failures recover without serial operations.');
}, { timeoutMs: 150000 });
