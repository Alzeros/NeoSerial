import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, fresh, click, evaluate, until }) => {
  const targets = [
    ['快捷指令', 'quick', '常驻开启'],
    ['数据处理', 'data', '控制侧栏“数据处理”tab'],
    ['指令查询', 'suggest', '输入时弹出候选'],
    ['MCP 日志', 'mcp', '支持 MCP Streamable HTTP'],
  ];
  const requests = [];
  await fresh('main');
  for (const [tab, module] of targets) {
    await click(tab);
    const count = await evaluate('persistenceTest.calls.filter(call => call.cmd === "open_settings_window").length');
    await evaluate(`document.querySelector('[data-module-navigation] button[title]').click()`);
    await until(`persistenceTest.calls.filter(call=>call.cmd==='open_settings_window').length > ${count}`);
    const args = await evaluate('persistenceTest.calls.filter(call => call.cmd === "open_settings_window").at(-1).args');
    assert.deepEqual(args, { section: 'extensions', extModule: module, anchor: null }, `${tab}: native command uses camelCase argument names`);
    requests.push({ section: args.section, ext_module: args.extModule, anchor: args.anchor });
  }
  await call('Page.navigate', { url: `${base}/tests/browser/persistence-regressions.html?window=settings&run=deep-link&extModule=data` });
  await until('window.persistenceTest?.run === "deep-link"', 30000);
  await until('document.body.innerText.includes("控制侧栏“数据处理”tab")');
  for (let index = 0; index < targets.length; index++) {
    await evaluate(`persistenceTest.reopenSettings(${JSON.stringify(requests[index])})`);
    await until(`document.body.innerText.includes(${JSON.stringify(targets[index][2])})`);
    assert.ok(await evaluate('document.body.innerText.includes("返回扩展")'));
    await click('取消');
    await until('!document.querySelector("[data-tauri-drag-region]")');
  }
  await evaluate(`persistenceTest.reopenSettings({section:'extensions',ext_module:null,anchor:null})`);
  await until('document.body.innerText.includes("扩展功能按模块独立管理")');
  assert.equal(await evaluate('document.body.innerText.includes("返回扩展")'), false);
  await evaluate(`persistenceTest.reopenSettings({section:'general',ext_module:null,anchor:'baud'})`);
  await until('document.querySelector("[data-settings-collapse] button")?.getAttribute("aria-expanded") === "true"');
  console.log('PASS: all four shortcut IPC payloads, first-open deep link, visible/hidden settings routing, extensions overview and baud anchor');
}, { timeoutMs: 180000 });
