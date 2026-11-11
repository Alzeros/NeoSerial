import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ evaluate, until, click, fresh, call, delay }) => {
  await fresh('settings');
  await click('通用');
  assert.ok(await evaluate('Boolean(document.querySelector("[data-settings-collapse]"))'));
  const collapse = 'document.querySelector("[data-settings-collapse] button")';
  await evaluate(`${collapse}.click(); persistenceTest.tick()`);
  await until('Boolean(document.querySelector("[data-collapse-content]"))');
  await delay(200);
  await evaluate(`${collapse}.click(); persistenceTest.tick()`);
  assert.equal(await evaluate('document.querySelector("[data-collapse-content]")?.inert'), true);
  await evaluate(`${collapse}.click(); persistenceTest.tick()`);
  await delay(200);
  assert.equal(await evaluate('document.querySelector("[data-collapse-content]").inert'), false);
  await call('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
  await evaluate(`${collapse}.click(); persistenceTest.tick()`);
  await until('!document.querySelector("[data-collapse-content]")');
  await fresh('main');
  await evaluate('persistenceTest.stores.cachedSettings.value.ui.show_mcp_tab = true; persistenceTest.tick()');
  await until('Boolean(document.querySelector("[data-module-indicator]"))');
  await click('数据处理');
  await until('document.querySelector("[data-module-tab][aria-current=page]")?.textContent.trim() === "数据处理"');
  const checkIndicator = async () => {
    const geometry = await evaluate(`(() => {
      const selected = document.querySelector('[data-module-tab][aria-current=page]').getBoundingClientRect();
      const line = document.querySelector('[data-module-indicator]').getBoundingClientRect();
      return { gap: Math.abs(selected.left-line.left), width: Math.abs(selected.width-line.width) };
    })()`);
    assert.ok(geometry.gap < 1 && geometry.width < 1, JSON.stringify(geometry));
  };
  await checkIndicator();
  await call('Emulation.setDeviceMetricsOverride', { width: 1000, height: 700, deviceScaleFactor: 1, mobile: false });
  await delay(60);
  await checkIndicator();
  await click('编解码');
  await evaluate(`Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async text => { window.copiedPayload = text; } } });
    const input = document.querySelector('#codec-input'); input.value = 'hello'; input.dispatchEvent(new Event('input', { bubbles: true }));`);
  await click('转换');
  await until('!persistenceTest.button("复制结果").disabled');
  const beforeWidth = await evaluate('persistenceTest.button("复制结果").getBoundingClientRect().width');
  await click('复制结果');
  await until('Boolean(document.querySelector(".codec-panel [data-copy-feedback][data-copied=true]"))');
  const afterWidth = await evaluate('document.querySelector(".codec-panel [data-copy-feedback]").closest("button").getBoundingClientRect().width');
  assert.equal(beforeWidth, afterWidth);
  assert.ok(await evaluate('Boolean(window.copiedPayload)'));
  assert.equal(await evaluate('getComputedStyle(document.querySelector(".codec-panel [data-copy-feedback] .copy-success")).animationName'), 'none');
  await evaluate(`navigator.clipboard.writeText = async () => { throw Error('denied'); }; document.querySelector('.codec-panel [data-copy-feedback]').closest('button').click();`);
  await until('document.body.innerText.includes("复制失败")');
  assert.equal(await evaluate('Boolean(document.querySelector(".codec-panel [data-copy-feedback][data-copied=true]"))'), false);
  await evaluate(`navigator.clipboard.writeText = async text => { window.copiedPayload = text; };
    persistenceTest.reply('build_frame', { hex: '48 49', total_len: 2, breakdown: [{ kind: 'data', offset: 0, len: 2 }] });`);
  await click('帧构造器');
  assert.deepEqual(await evaluate(`(() => {
    const footer = persistenceTest.button('存为模板').parentElement.parentElement;
    return [...footer.children].slice(0, 2).map(button => button.textContent.trim());
  })()`), ['生成', '填入发送框'], 'frame actions start with generate and fill');
  const generateWidth = await evaluate('persistenceTest.button("生成").getBoundingClientRect().width');
  await evaluate(`persistenceTest.holdNext('build_frame')`);
  await click('生成');
  assert.equal(await evaluate('persistenceTest.button("生成中…").disabled'), true);
  assert.equal(await evaluate('persistenceTest.button("生成中…").getBoundingClientRect().width'), generateWidth, 'generation status never shifts the footer');
  assert.equal(await evaluate('persistenceTest.button("填入发送框").disabled'), true);
  await evaluate('persistenceTest.release()');
  await until(`Boolean(document.querySelector('button[title="复制整帧 hex 到剪贴板"]'))`);
  await evaluate(`document.querySelector('button[title="复制整帧 hex 到剪贴板"]').click()`);
  await until(`Boolean(document.querySelector('button[title="复制整帧 hex 到剪贴板"] [data-copied=true]'))`);
  assert.equal(await evaluate('window.copiedPayload'), '48 49');
  await click('数据文本');
  assert.equal(await evaluate('document.querySelector("[data-copy-feedback]").dataset.copied'), 'false', 'mode switches clear previous copy feedback');
  await evaluate(`document.querySelector('[data-copy-feedback]').closest('button').click()`);
  assert.equal(await evaluate('window.copiedPayload'), 'HI', 'text mode copies data characters');
  assert.equal(await evaluate(`document.querySelector('[data-copy-feedback]').closest('button').title`), '复制完整数据域文本到剪贴板');

  // The collapsed text preview shows only 128 bytes; copying must retain the whole data field.
  const randomText = 'aB3dE5fG7h'.repeat(16);
  const randomHex = [...Buffer.from(randomText)].map(byte => byte.toString(16).padStart(2, '0').toUpperCase()).join(' ');
  await click('随机字符串');
  await evaluate(`persistenceTest.stores.scriptModules.find(module => module.type === 'data_processing').tools.find(tool => tool.kind === 'frame_builder').config.data.length = 160;
    persistenceTest.reply('build_frame', ${JSON.stringify({ hex: randomHex, total_len: 160, breakdown: [{ kind: 'data', offset: 0, len: 160 }] })}); persistenceTest.tick()`);
  await click('生成');
  await until(`document.querySelector('span[title="数据域 160 字节"]')?.textContent === ${JSON.stringify(randomText.slice(0, 128))}`);
  const previewLayout = await evaluate(`(() => {
    const content = document.querySelector('span[title="数据域 160 字节"]').parentElement;
    const copy = document.querySelector('[data-copy-feedback]').closest('button');
    return { gap: Math.abs(copy.getBoundingClientRect().left - content.getBoundingClientRect().left),
      rows: content.clientHeight / parseFloat(getComputedStyle(content).lineHeight),
      selected: document.querySelector('[aria-label="预览显示模式"] [aria-pressed="true"]').textContent.trim() };
  })()`);
  assert.ok(previewLayout.gap < 1, 'copy button aligns with the left edge of the preview');
  assert.ok(previewLayout.rows >= 2.98, 'preview provides at least three lines');
  assert.equal(previewLayout.selected, '数据文本');
  const previewGeometry = () => evaluate(`(() => {
    const content = document.querySelector('.frame-preview-content');
    const rect = content.parentElement.getBoundingClientRect();
    return { contentHeight: content.getBoundingClientRect().height, previewHeight: rect.height, top: rect.top };
  })()`);
  const textPreviewGeometry = await previewGeometry();
  await evaluate(`document.querySelector('[data-copy-feedback]').closest('button').click()`);
  assert.equal(await evaluate('window.copiedPayload'), randomText, 'random text copy is not truncated');
  const checkExpandedCopy = async (expected, otherMode, otherExpected) => {
    await click('… 展开全部');
    await until('Boolean(persistenceTest.button("返回设置"))');
    assert.ok(await evaluate(`document.querySelector('[aria-label="预览显示模式"]').getClientRects().length > 0`), 'expanded results expose the preview mode switch');
    const originalMode = await evaluate(`document.querySelector('[aria-label="预览显示模式"] [aria-pressed="true"]').textContent.trim()`);
    const visibleCopy = `[...document.querySelectorAll('[data-copy-feedback]')].map(label => label.closest('button')).find(button => button.getClientRects().length > 0)`;
    assert.ok(await evaluate(`Boolean(${visibleCopy})`), 'expanded results expose a visible copy button');
    await evaluate(`window.copiedPayload = null; ${visibleCopy}.click()`);
    await until('window.copiedPayload !== null');
    assert.equal(await evaluate('window.copiedPayload'), expected, 'expanded copy preserves the complete current preview mode');
    await click(otherMode);
    assert.ok(await evaluate('persistenceTest.button("返回设置").getClientRects().length > 0'), 'mode switches keep the results expanded');
    assert.equal(await evaluate(`document.querySelector('[aria-label="预览显示模式"] [aria-pressed="true"]').textContent.trim()`), otherMode);
    await evaluate(`window.copiedPayload = null; ${visibleCopy}.click()`);
    await until('window.copiedPayload !== null');
    assert.equal(await evaluate('window.copiedPayload'), otherExpected, 'expanded copy follows the selected mode');
    await click(originalMode);
    await click('返回设置');
  };
  await checkExpandedCopy(randomText, 'Hex', randomHex);
  await click('填入发送框');
  assert.deepEqual(await evaluate(`({ text: persistenceTest.stores.suggestFillRequest.text, hex: persistenceTest.stores.hexSend.value })`), { text: randomText, hex: false });
  await click('Hex');
  assert.deepEqual(await previewGeometry(), textPreviewGeometry, 'Hex and text preview keep identical heights and position');
  await evaluate(`document.querySelector('[data-copy-feedback]').closest('button').click()`);
  assert.equal(await evaluate('window.copiedPayload'), randomHex, 'HEX mode copies the complete frame');

  const framedHex = `00 FF ${randomHex} 99`;
  await evaluate(`(() => {
    const config = persistenceTest.stores.scriptModules.find(module => module.type === 'data_processing').tools.find(tool => tool.kind === 'frame_builder').config;
    config.header_hex = '00 FF'; config.checksum.algo = 'sum8';
    persistenceTest.reply('build_frame', ${JSON.stringify({ hex: framedHex, total_len: 163, breakdown: [{ kind: 'header', offset: 0, len: 2 }, { kind: 'data', offset: 2, len: 160 }, { kind: 'checksum', offset: 162, len: 1 }] })});
    return persistenceTest.tick();
  })()`);
  assert.equal(await evaluate('document.querySelector("[data-copy-feedback]").closest("button").disabled'), true, 'stale frames cannot be copied');
  await click('生成');
  await until('!document.querySelector("[data-copy-feedback]").closest("button").disabled');
  await click('数据文本');
  await evaluate(`document.querySelector('[data-copy-feedback]').closest('button').click()`);
  assert.equal(await evaluate('window.copiedPayload'), randomText, 'text copy excludes binary headers and checksums');
  await click('Hex');
  await evaluate(`document.querySelector('[data-copy-feedback]').closest('button').click()`);
  assert.equal(await evaluate('window.copiedPayload'), framedHex, 'HEX copy includes headers and checksums');
  await checkExpandedCopy(framedHex, '数据文本', randomText);
  assert.equal(await evaluate(`persistenceTest.calls.some(call => call.cmd === 'send')`), false, 'generating, copying and filling never send serial data');
  console.log('PASS: frame copy follows the preview mode and copies complete data from collapsed previews');

  // Match the app's minimum window width so hit tests include the entire sidebar.
  await call('Emulation.setDeviceMetricsOverride', { width: 1216, height: 700, deviceScaleFactor: 1, mobile: false });
  await delay(60);
  const actionWidths = await evaluate(`(() => {
    window.frameActions = persistenceTest.button('存为模板').parentElement.parentElement;
    window.frameTemplateStrip = frameActions.querySelector('[title^="载入模板"]').closest('.overflow-x-auto');
    return [...frameActions.querySelectorAll(':scope > button, :scope > .relative > button')].map(button => button.getBoundingClientRect().width);
  })()`);
  await evaluate(`(() => {
    const tool = persistenceTest.stores.scriptModules.find(module => module.type === 'data_processing').tools.find(tool => tool.kind === 'frame_builder');
    for (let index = 1; index <= 16; index++) {
      const config = JSON.parse(JSON.stringify(tool.config)); config.data.length = index;
      tool.templates.push({ name: '模板' + index, config });
    }
    return persistenceTest.tick();
  })()`);
  const templateLayout = await evaluate(`(() => {
    frameTemplateStrip.scrollLeft = 0;
    const strip = frameTemplateStrip.getBoundingClientRect();
    const first = frameTemplateStrip.querySelector('[title^="载入模板"]').getBoundingClientRect();
    return { left: strip.left, firstLeft: first.left, width: frameTemplateStrip.clientWidth, scrollWidth: frameTemplateStrip.scrollWidth,
      actionWidths: [...frameActions.querySelectorAll(':scope > button, :scope > .relative > button')].map(button => button.getBoundingClientRect().width) };
  })()`);
  assert.ok(templateLayout.firstLeft >= templateLayout.left - 1, 'oldest template remains reachable at the start of the scroll area');
  assert.ok(templateLayout.scrollWidth > templateLayout.width, 'overflowing templates provide a horizontal scroll range');
  assert.deepEqual(templateLayout.actionWidths, actionWidths, 'templates never shrink the fixed action buttons');
  const wheelTemplates = async (deltaX, deltaY, modifiers = 0) => {
    const point = await evaluate(`(() => {
      const rect = frameTemplateStrip.getBoundingClientRect();
      return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
    })()`);
    await call('Input.dispatchMouseEvent', { type: 'mouseMoved', ...point });
    await call('Input.dispatchMouseEvent', { type: 'mouseWheel', ...point, deltaX, deltaY, modifiers });
  };
  await wheelTemplates(0, 120);
  await until('frameTemplateStrip.scrollLeft > 0', 2000);
  await wheelTemplates(0, -120);
  await until('frameTemplateStrip.scrollLeft === 0', 2000);
  await wheelTemplates(0, 120, 8); // Shift + wheel
  await until('frameTemplateStrip.scrollLeft > 0', 2000);
  await wheelTemplates(-120, 0); // Native horizontal gesture
  await until('frameTemplateStrip.scrollLeft === 0', 2000);
  await evaluate(`frameTemplateStrip.querySelector('[title^="载入模板"]').click(); persistenceTest.tick()`);
  assert.equal(await evaluate(`persistenceTest.stores.scriptModules.find(module => module.type === 'data_processing').tools.find(tool => tool.kind === 'frame_builder').config.data.length`), 256);
  await evaluate(`frameTemplateStrip.scrollLeft = frameTemplateStrip.scrollWidth`);
  assert.equal(await evaluate(`(() => {
    const button = [...frameTemplateStrip.querySelectorAll('[title^="载入模板"]')].at(-1);
    const rect = button.getBoundingClientRect();
    return document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2) === button;
  })()`), true, 'newest template can be reached and clicked after scrolling');
  await evaluate(`[...frameTemplateStrip.querySelectorAll('[title^="载入模板"]')].at(-1).click(); persistenceTest.tick()`);
  assert.equal(await evaluate(`persistenceTest.stores.scriptModules.find(module => module.type === 'data_processing').tools.find(tool => tool.kind === 'frame_builder').config.data.length`), 16);
  console.log('PASS: overflowing templates scroll between oldest and newest without shrinking action buttons');
  await click('短信');
  await evaluate(`[...document.querySelectorAll('.sms-panel button')].find(button=>button.textContent.trim()==='生成 PDU').click(); persistenceTest.tick()`);
  await evaluate(`for (const [selector, value] of [['#sms-recipient','13800138000'],['#sms-text-input','hello']]) {
    const input=document.querySelector(selector); input.value=value; input.dispatchEvent(new Event('input',{bubbles:true}));
  } persistenceTest.tick()`);
  await evaluate(`[...document.querySelectorAll('.sms-panel button')].find(button=>button.textContent.trim()==='生成').click(); persistenceTest.tick()`);
  await until('Boolean(document.querySelector("[data-sms-generated]"))');
  await evaluate(`document.querySelector('[data-sms-generated] button').click()`);
  await until('Boolean(document.querySelector("[data-sms-generated] [data-copied=true]"))');
  assert.equal(await evaluate('document.querySelectorAll(".sms-panel [data-copied=true]").length'), 1);
  assert.equal(await evaluate('window.copiedPayload'), await evaluate('document.querySelector("[data-sms-generated] textarea").value'));
  await evaluate(`[...document.querySelectorAll('.sms-panel button')].find(button=>button.textContent.startsWith('复制全部 PDU')).click()`);
  await until('Boolean(document.querySelector(".sms-action [data-copied=true]"))');
  assert.equal(await evaluate('document.querySelectorAll(".sms-panel [data-copied=true]").length'), 1);
  await call('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'no-preference' }] });
  await click('快捷指令');
  await delay(180);
  await checkIndicator();
  await click('MCP 日志');
  await evaluate('persistenceTest.stores.cachedSettings.value.ui.show_data_tab = false; persistenceTest.tick()');
  await delay(180);
  await checkIndicator();
  console.log('PASS: reversible collapse, immediate tabs, measured underline, fixed copy width, reduced motion and clipboard failure');
}, { timeoutMs: 180000 });
