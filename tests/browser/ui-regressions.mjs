import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { withHeadlessBrowser } from './headless.mjs';

// Optional screenshots are written outside the browser's temporary profile.
const screenshotFlag = process.argv.indexOf('--screenshots');
const output = screenshotFlag >= 0 ? resolve(process.argv[screenshotFlag + 1] ?? 'artifacts/ui') : null;
if (output) await mkdir(output, { recursive: true });

await withHeadlessBrowser(async ({ call, evaluate, until, click, fresh, delay, screenshot }) => {
  const viewport = async (width, height) => {
    await call('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 2, mobile: false });
    await delay(40);
  };
  const input = async (selector, value, event = 'input') => {
    await evaluate(`(()=>{const el=document.querySelector(${JSON.stringify(selector)});el.value=${JSON.stringify(value)};el.dispatchEvent(new Event(${JSON.stringify(event)},{bubbles:true}));})()`);
    await evaluate('persistenceTest.tick()');
  };
  const press = async selector => {
    await evaluate(`document.querySelector(${JSON.stringify(selector)}).click();persistenceTest.tick()`);
  };
  const connectMock = async () => evaluate(`(()=>{
    const s=persistenceTest.stores,ports=[{port:'COM3',device:'UI preview — simulated device'}];
    persistenceTest.reply('list_ports',ports);
    s.availablePorts.value=ports;s.connectionParams.port='COM3';s.windowPort.value='COM3';s.currentPort.value='COM3';s.connected.value=true;
    return persistenceTest.tick();
  })()`);
  const fits = async () => {
    const problems = await evaluate(`(()=>{
      const issues=[];
      const panel=document.querySelector('[data-m-clear]').closest('[data-theme-target]');
      for(const el of [panel,document.querySelector('.script-list')]){
        const r=el.getBoundingClientRect();
        if(el.scrollWidth>el.clientWidth+1)issues.push(el.className+' overflows');
        if(r.left<0||r.right>innerWidth+1||r.bottom>innerHeight+1)issues.push(el.className+' outside viewport');
      }
      for(const el of panel.querySelectorAll('button,select,input')){
        if(el.type==='checkbox')continue;
        const a=el.getBoundingClientRect(),b=panel.getBoundingClientRect();
        if(a.width&&a.height&&(a.left<b.left-1||a.right>b.right+1))issues.push((el.textContent.trim()||el.placeholder)+' exceeds panel');
      }
      return issues;
    })()`);
    assert.deepEqual(problems, [], 'Controls fit without clipping');
  };
  const aligned = async () => {
    const centers = await evaluate(`(()=>{
      const row=document.querySelector('tr[data-row]');
      return [...row.querySelectorAll('[data-row-toggle],td input,td:last-child button')]
        .map(element=>{const rect=element.getBoundingClientRect();return rect.y+rect.height/2;});
    })()`);
    assert.ok(Math.max(...centers) - Math.min(...centers) <= 4, 'Row controls stay vertically aligned');
  };
  const singleLineRows = async () => {
    const layout = await evaluate(`(()=>{
      const style=getComputedStyle(document.querySelector('.script-list'));
      return {
        heights:[...document.querySelectorAll('tr[data-row]')].map(row=>row.getBoundingClientRect().height),
        expected:parseFloat(style.getPropertyValue('--qc-input-height'))+2*parseFloat(style.getPropertyValue('--qc-row-gap'))+1
      };
    })()`);
    assert.ok(Math.max(...layout.heights) - Math.min(...layout.heights) <= 1, 'Rows with and without notes have the same height');
    assert.ok(Math.max(...layout.heights) <= layout.expected + 1, 'Notes never add another line to a command row');
  };

  await viewport(1216, 800);
  await fresh('main');
  assert.equal(await evaluate(`document.querySelectorAll('tr[data-row] td:last-child button:not([data-add-row]):not(:disabled)').length`), 0, 'row sends stay disabled while disconnected');
  assert.equal(await evaluate(`Boolean(document.querySelector('.send-panel,[data-send-panel]'))`), false, 'no leftover regrouped send panel');

  const switches = await evaluate(`(()=>{
    const panel=document.querySelector('[data-m-clear]').closest('[data-theme-target]');
    const grid=panel.querySelector('.ml-auto');
    return [...grid.querySelectorAll(':scope > div')].map(row=>
      [...row.querySelectorAll('label.switch')].map(label=>({
        text:label.querySelector('.switch-label').textContent.trim(),
        paired:label.querySelectorAll('input').length===1&&label.querySelector('input').labels[0]===label,
      }))
    );
  })()`);
  assert.deepEqual(switches, [
    [{ text: '行号', paired: true }, { text: 'HEX显示', paired: true }, { text: 'HEX发送', paired: true }],
    [{ text: '时间戳', paired: true }, { text: '回车换行', paired: true }, { text: '记录发送', paired: true }],
  ], 'original 2×3 switch grid keeps one explicit label per switch');
  console.log('PASS: bottom toolbar keeps the original switch grid, every switch paired with its own label');

  const initialFlags = await evaluate(`({index:persistenceTest.stores.showLineIndex.value,ts:persistenceTest.stores.showTimestamp.value,logSend:persistenceTest.stores.logSendContent.value,ending:persistenceTest.stores.lineEnding.value})`);
  assert.equal(initialFlags.ending, 'Crlf');
  await press('[data-m-lineidx] .switch-label');
  await press('[data-m-ts] .switch-label');
  await press('[data-m-logsend] .switch-label');
  assert.deepEqual(await evaluate(`({index:persistenceTest.stores.showLineIndex.value,ts:persistenceTest.stores.showTimestamp.value,logSend:persistenceTest.stores.logSendContent.value})`), { index: !initialFlags.index, ts: !initialFlags.ts, logSend: !initialFlags.logSend });
  await press('[data-m-hexdisp] .switch-label');
  assert.equal(await evaluate('persistenceTest.stores.displayMode.value'), 'hex');
  assert.equal(await evaluate('persistenceTest.stores.hexSend.value'), false, 'display mode does not change sending mode');
  await press('[data-m-hexdisp] .switch-label');
  await press('[data-m-crlf] .switch-label');
  assert.equal(await evaluate('persistenceTest.stores.lineEnding.value'), 'None');
  await press('[data-m-crlf] .switch-label');
  assert.equal(await evaluate('persistenceTest.stores.lineEnding.value'), 'Crlf');
  console.log('PASS: restored switches toggle their original settings and nothing else');

  await press('tr[data-row] [data-row-toggle]');
  assert.equal(await evaluate(`persistenceTest.stores.scriptModules[0].pages[0].commands[0].enabled`), false);
  await press('.script-list thead th:nth-child(1) button');
  assert.equal(await evaluate(`persistenceTest.stores.scriptModules[0].pages[0].commands.every(c=>c.enabled)`), true);
  await press('.script-list thead th:nth-child(1) button');
  assert.equal(await evaluate(`persistenceTest.stores.scriptModules[0].pages[0].commands.every(c=>!c.enabled)`), true);
  await press('.script-list thead th:nth-child(1) button');
  for (const [column, property, initial] of [[3, 'hex', false], [4, 'enter', true]]) {
    await press(`.script-list thead th:nth-child(${column}) button`);
    assert.equal(await evaluate(`persistenceTest.stores.scriptModules[0].pages[0].commands.every(command=>command.${property}===${!initial})`), true);
    await press(`.script-list thead th:nth-child(${column}) button`);
    assert.equal(await evaluate(`persistenceTest.stores.scriptModules[0].pages[0].commands.every(command=>command.${property}===${initial})`), true);
  }
  console.log('PASS: number chips and header buttons still toggle row, HEX, and line-ending flags');

  const sendText = () => evaluate(`document.querySelector('tr[data-row] td:last-child button').textContent.trim()`);
  await singleLineRows();
  await aligned();
  await evaluate(`document.querySelector('tr[data-row]').dispatchEvent(new MouseEvent('contextmenu',{bubbles:true}));persistenceTest.tick()`);
  await click('编辑注释');
  await input('input[placeholder="为该行写点说明..."]', '查询设备信息；这段注释不会发送');
  await click('确定');
  assert.equal(await sendText(), '查询设备信…', 'display caps notes at five characters plus an ellipsis');
  assert.equal(await evaluate(`persistenceTest.stores.scriptModules[0].pages[0].commands[0].note`), '查询设备信息；这段注释不会发送', 'full note stays in the data');
  assert.equal(await evaluate(`document.querySelector('tr[data-row] td:last-child button').title`), '发送：查询设备信息；这段注释不会发送', 'full note stays on the tooltip');
  assert.equal(await evaluate(`document.querySelectorAll('tr[data-row] td:last-child button:not([data-add-row])')[1].textContent.trim()`), '发送');
  assert.equal(await evaluate(`persistenceTest.calls.some(c=>c.cmd==='send')`), false, 'editing a note never sends');
  assert.equal(await evaluate(`document.querySelector('.script-list thead th:last-child').textContent.trim()`), '注释/发送');
  await singleLineRows();
  await aligned();
  await evaluate(`document.querySelector('tr[data-row]').dispatchEvent(new MouseEvent('contextmenu',{bubbles:true}));persistenceTest.tick()`);
  await click('编辑注释');
  await input('input[placeholder="为该行写点说明..."]', '取消的修改');
  await call('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
  await call('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
  await evaluate('persistenceTest.tick()');
  assert.equal(await sendText(), '查询设备信…', 'Escape cancels the note editor');
  await evaluate(`(()=>{persistenceTest.stores.scriptModules[0].pages[0].commands[0].note='短注释';return persistenceTest.tick();})()`);
  assert.equal(await sendText(), '短注释', 'short notes render in full at the same button width');
  console.log('PASS: notes stay in the original single-line send column via the context-menu editor');

  const originalRows = await evaluate('JSON.parse(JSON.stringify(persistenceTest.stores.scriptModules[0].pages[0].commands))');
  const toggleOrderMode = async () => evaluate(`(()=>{
    [...document.querySelectorAll('label.switch')].find(label=>label.textContent.trim()==='调整顺序').querySelector('input').click();
    return persistenceTest.tick();
  })()`);
  await toggleOrderMode();
  assert.equal(await evaluate(`document.querySelectorAll('tr[data-row] span[title="拖动调整顺序"]').length`), originalRows.length);
  const dragPoints = await evaluate(`(()=>{
    const handle=document.querySelector('tr[data-row] span[title="拖动调整顺序"]').getBoundingClientRect();
    const target=document.querySelectorAll('tr[data-row]')[1].getBoundingClientRect();
    return {x:handle.x+handle.width/2,startY:handle.y+handle.height/2,endY:target.bottom-2};
  })()`);
  await call('Input.dispatchMouseEvent', { type: 'mousePressed', x: dragPoints.x, y: dragPoints.startY, button: 'left', buttons: 1, clickCount: 1 });
  await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: dragPoints.x, y: dragPoints.endY, button: 'left', buttons: 1 });
  await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x: dragPoints.x, y: dragPoints.endY, button: 'left', buttons: 0, clickCount: 1 });
  await evaluate('persistenceTest.tick()');
  assert.deepEqual(await evaluate('JSON.parse(JSON.stringify(persistenceTest.stores.scriptModules[0].pages[0].commands))'), [originalRows[1], originalRows[0], ...originalRows.slice(2)]);
  await toggleOrderMode();
  assert.equal(await evaluate(`document.querySelectorAll('tr[data-row] [data-row-toggle]').length`), originalRows.length);
  await evaluate(`persistenceTest.stores.scriptModules[0].pages[0].commands=${JSON.stringify(originalRows)};persistenceTest.tick()`);
  await singleLineRows();
  console.log('PASS: reorder drag handles and row data survive order mode round-trips');

  await connectMock();
  await press('[data-m-hexsend] .switch-label');
  assert.equal(await evaluate('persistenceTest.stores.displayMode.value'), 'ascii', 'sending mode does not change display mode');
  await input('input[placeholder="输入要发送的内容..."]', '41 54');
  await evaluate(`(()=>{const box=document.querySelector('input[placeholder="输入要发送的内容..."]').closest('.flex.items-center');box.querySelector('.btn-primary').click();})()`);
  await until(`persistenceTest.calls.some(c=>c.cmd==='send')`);
  assert.deepEqual(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='send').at(-1).args`), { port: 'COM3', text: '41 54', ending: 'Crlf', isHex: true });
  await evaluate(`(()=>{[...document.querySelectorAll('button')].find(b=>b.textContent.includes('Ctrl-Z')).click();})()`);
  assert.deepEqual(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='send').at(-1).args`), { port: 'COM3', text: '1A', ending: 'None', isHex: true });
  await press('[data-m-hexsend] .switch-label');
  await evaluate(`persistenceTest.stores.scriptModules[0].pages[0].commands[0].command='ATI';persistenceTest.tick()`);
  await press('tr[data-row] td:last-child button');
  assert.deepEqual(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='send').at(-1).args`), { port: 'COM3', text: 'ATI', ending: 'Crlf', isHex: false });
  console.log('PASS: manual HEX send, Ctrl-Z, and row sends keep their original IPC payloads');

  await press('tr[data-row] [data-row-toggle]');
  await press('button[title="运行"]');
  await until(`persistenceTest.calls.some(call=>call.cmd==='sequence_run')`);
  const sequence = await evaluate(`persistenceTest.calls.filter(call=>call.cmd==='sequence_run').at(-1).args`);
  assert.equal(sequence.port, 'COM3');
  assert.equal(sequence.commands[0].enabled, false, 'Unchecked rows stay flagged for the backend');
  assert.equal(sequence.commands.slice(1).every(command => command.enabled), true);
  assert.equal(sequence.runCount, 1);
  assert.equal(sequence.loopInterval, 500);
  await press('button[title="停止"]');
  await until(`persistenceTest.calls.some(call=>call.cmd==='sequence_stop')`);
  console.log('PASS: batch run and stop preserve selection and sequence parameters');

  await press('button[title="展开文件发送 / 日志保存"]');
  await evaluate(`persistenceTest.reply('plugin:dialog|open','/isolated-test-only/payload.bin');persistenceTest.reply('plugin:dialog|save','/isolated-test-only/session.log');persistenceTest.reply('start_logging','/isolated-test-only/session.log');`);
  await evaluate(`(()=>{[...document.querySelectorAll('button')].find(b=>b.textContent.includes('点击选择发送文件路径')).click();})()`);
  await until(`persistenceTest.stores.fileSendPath.value==='/isolated-test-only/payload.bin'`);
  await click('发送文件');
  assert.deepEqual(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='send_file').at(-1).args`), { port: 'COM3', path: '/isolated-test-only/payload.bin' });
  await evaluate(`(()=>{[...document.querySelectorAll('button')].find(b=>b.textContent.includes('点击选择日志保存路径')).click();})()`);
  await until(`persistenceTest.stores.loggingPath.value==='/isolated-test-only/session.log'`);
  await click('开始记录');
  await until('persistenceTest.stores.loggingActive.value');
  assert.equal(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='start_logging').at(-1).args.path`), '/isolated-test-only/session.log');
  await click('停止');
  await until('!persistenceTest.stores.loggingActive.value');
  await click('继续记录');
  await until('persistenceTest.stores.loggingActive.value');
  assert.equal(await evaluate(`persistenceTest.calls.filter(c=>c.cmd==='start_logging').at(-1).args.path==null`), true, 'resume uses existing file rather than overwriting');
  await press('button[title="收起"]');
  await fits();
  console.log('PASS: collapsible file/log rows preserve file sending and log start/stop/resume');

  // A deterministic, simulated connected screen for visual QA (no device access).
  await fresh('main');
  await connectMock();
  await evaluate(`(()=>{
    const s=persistenceTest.stores,rows=s.scriptModules[0].pages[0].commands;
    ['AT','ATI','AT+CSQ','AT+CREG?'].forEach((text,i)=>rows[i].command=text);
    rows[1].note='查询模组信息';rows[2].note='读取信号质量';
    const log=[['tx','AT'],['rx','OK'],['tx','ATI'],['rx','NeoSerial UI Preview'],['rx','OK'],['tx','AT+CSQ'],['rx','+CSQ: 23,99'],['rx','OK']];
    s.appendLogLines(log.map(([dir,text],i)=>({dir,ascii:text,raw:Array.from(new TextEncoder().encode(text)),ts:'14:32:10.'+String(i*113).padStart(3,'0'),ts_ms:i*113,line_index:i+1,is_error:false})));
    s.sendText.value='AT+CSQ';return persistenceTest.tick();
  })()`);
  await fits();
  if (output) await screenshot(join(output, '01-main-light.png'));
  await evaluate(`persistenceTest.stores.applyTheme('preset-3');persistenceTest.tick()`);
  await fits();
  if (output) await screenshot(join(output, '02-main-dark.png'));
  await evaluate(`persistenceTest.stores.applyTheme('preset-1');persistenceTest.stores.scriptPanelWidth.value=600;persistenceTest.tick()`);
  await viewport(1320, 800);
  await fits();
  for (const [height, fontSize, gap] of [[22, 11, 0], [40, 18, 10]]) {
    await evaluate(`(()=>{
      const stores=persistenceTest.stores;
      stores.scriptModules[0].pages[0].commands[0].note='长注释不会挤压发送、行尾或延时控件。'.repeat(20);
      Object.assign(stores.cachedSettings.value.ui,{qc_input_height:${height},qc_font_size:${fontSize},qc_row_gap:${gap}});
      return persistenceTest.tick();
    })()`);
    await aligned();
    await singleLineRows();
    await fits();
  }
  await evaluate(`(()=>{
    const stores=persistenceTest.stores;
    stores.scriptModules[0].pages[0].commands[0].note='';
    Object.assign(stores.cachedSettings.value.ui,{qc_input_height:28,qc_font_size:13,qc_row_gap:4});
    return persistenceTest.tick();
  })()`);
  console.log('PASS: compact/large density and long notes keep rows single-line and aligned');
  await evaluate(`persistenceTest.stores.scriptPanelWidth.value=500;persistenceTest.tick()`);
  await viewport(1216, 600);
  await press('button[title="展开文件发送 / 日志保存"]');
  await fits();
  if (output) await screenshot(join(output, '03-minimum-height-expanded.png'));
  console.log('PASS: default/minimum-size layouts, expanded file/log rows, wider sidebar, and dark theme fit');
  if (output) console.log(`Screenshots: ${output}`);
});
