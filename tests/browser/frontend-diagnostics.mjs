import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, evaluate, until, delay, screenshot }) => {
  for (const label of ['main', 'settings', 'theme-editor']) {
    await call('Page.navigate', { url: `${base}/tests/browser/frontend-diagnostics.html?window=${label}` });
    await until('Boolean(window.diagTest)');
    await evaluate('diagTest.flushFrontendDiagnostics()');
    const start = await evaluate('diagTest.records.find(record => record.kind === "frontend.start")');
    assert.equal(start.data.window, label);
    assert.ok(start.data.user_agent.includes('Chrome'));
    assert.ok(start.data.build.version && start.data.build.id);
    assert.equal(await evaluate('diagTest.records.filter(record => record.kind === "frontend.start").length'), 1);
  }

  await evaluate(`(() => {
    console.error('render failure', new Error('caught frontend stack'));
    window.dispatchEvent(new ErrorEvent('error', {message:'uncaught frontend error',error:new Error('uncaught stack'),filename:'test.js',lineno:42,colno:9}));
    window.dispatchEvent(new PromiseRejectionEvent('unhandledrejection', {promise:Promise.resolve(),reason:new Error('rejected promise')}));
    const script = document.createElement('script'); document.body.append(script); script.dispatchEvent(new Event('error'));
    document.querySelector('#private-button').click();
    document.querySelector('#private-input').dispatchEvent(new Event('change', {bubbles:true}));
    return diagTest.flushFrontendDiagnostics();
  })()`);
  assert.equal(await evaluate('diagTest.consoleCalls()'), 1, 'Original console remains active');
  const kinds = await evaluate('diagTest.records.map(record => record.kind)');
  for (const kind of ['console.error', 'window.error', 'promise.unhandled', 'resource.error', 'ui.click', 'ui.change']) assert.ok(kinds.includes(kind), kind);
  assert.match(await evaluate('JSON.stringify(diagTest.records)'), /caught frontend stack/);
  assert.doesNotMatch(await evaluate('JSON.stringify(diagTest.records)'), /PRIVATE_/);

  await evaluate(`(async () => {
    await diagTest.diagnosticInvoke('kb_set_api_key', {key:'PRIVATE_KEY'});
    await diagTest.diagnosticInvoke('send', {text:'PRIVATE_SERIAL'});
    try { await diagTest.diagnosticInvoke('failing_command'); }
    catch (error) { if (error !== diagTest.failure) throw new Error('IPC rejection changed'); }
    await diagTest.flushFrontendDiagnostics();
  })()`);
  const records = await evaluate('diagTest.records');
  assert.ok(records.some(record => record.kind === 'ipc.error' && record.data.command === 'failing_command'));
  assert.ok(records.some(record => record.kind === 'ipc.ok' && record.data.command === 'send'));
  assert.doesNotMatch(JSON.stringify(records), /PRIVATE_/);

  await evaluate(`void (diagTest.pending = diagTest.diagnosticInvoke('pending_command'))`);
  await until(`diagTest.records.some(record => record.kind === 'ipc.pending' && record.data.command === 'pending_command')`, 8000);
  await evaluate(`diagTest.releasePending(); diagTest.pending.then(() => diagTest.flushFrontendDiagnostics())`);
  assert.ok(await evaluate(`diagTest.records.some(record => record.kind === 'ipc.ok' && record.data.command === 'pending_command' && record.data.elapsed_ms >= 5000)`));

  await evaluate(`diagTest.recordFrontend('pending-before-export'); diagTest.requestFlush()`);
  await until('diagTest.flushReplies.length === 1');
  assert.deepEqual(await evaluate('diagTest.flushReplies[0]'), { window: 'theme-editor', ok: true });
  assert.ok(await evaluate(`diagTest.records.some(record => record.kind === 'pending-before-export')`));

  await evaluate(`(async () => {
    diagTest.failWrites(true);
    console.warn('queued while disk unavailable');
    await diagTest.requestFlush();
  })()`);
  await until('diagTest.flushReplies.length === 2');
  assert.equal(await evaluate('diagTest.flushReplies[1].ok'), false, 'A failed write is not acknowledged as saved');
  await evaluate(`(async () => {
    diagTest.failWrites(false);
    await diagTest.flushFrontendDiagnostics();
  })()`);
  assert.ok(await evaluate(`diagTest.records.some(record => record.kind === 'console.warn' && JSON.stringify(record.data).includes('queued while disk unavailable'))`));

  await evaluate(`document.querySelector('[data-diagnostic-action="export-frontend-logs"]').click()`);
  await until('document.body.innerText.includes("已导出")');
  assert.equal(await evaluate('diagTest.calls.find(call => call.cmd === "export_frontend_diagnostics").args.path'), '/isolated/report.json');
  assert.ok(await evaluate(`diagTest.calls.filter(call => call.cmd === 'append_frontend_diagnostics').some(call => call.args.records.some(record => record.data?.action === 'export-frontend-logs'))`));

  await evaluate(`diagTest.failExport(true); document.querySelector('[data-diagnostic-action="export-frontend-logs"]').click()`);
  await until('document.querySelector("[role=status]").textContent.includes("isolated export error")');
  assert.equal(await evaluate('document.querySelector("[data-diagnostic-action]").disabled'), false);
  await evaluate(`diagTest.savePath(null); document.querySelector('[data-diagnostic-action="export-frontend-logs"]').click()`);
  await until('!document.querySelector("[data-diagnostic-action]").disabled');
  assert.equal(await evaluate('diagTest.calls.filter(call => call.cmd === "export_frontend_diagnostics").length'), 2, 'Cancel does not export');

  await evaluate(`diagTest.failExport(false); diagTest.partialExport(true); diagTest.savePath('/isolated/partial.json'); document.querySelector('[data-diagnostic-action="export-frontend-logs"]').click()`);
  await until('document.querySelector("[role=status]").textContent.includes("部分窗口")');

  // A real event-loop task, rather than DevTools evaluation (which Chromium excludes).
  await evaluate(`setTimeout(() => { const end = performance.now()+260; while(performance.now()<end) {} }, 0); undefined`);
  await until(`diagTest.records.some(record => record.kind === 'performance.longtask' && record.data.duration_ms >= 200)`);
  await screenshot('/tmp/neoserial-frontend-diagnostics.png');
  for (const label of ['main', 'settings']) {
    await call('Page.navigate', { url: `${base}/tests/browser/persistence-regressions.html?window=${label}&entry=1` });
    await until('Boolean(window.persistenceTest)');
    await until(`persistenceTest.calls.filter(call => call.cmd === 'append_frontend_diagnostics').flatMap(call => call.args.records).some(record => record.kind === 'frontend.mounted')`);
    const entries = await evaluate(`persistenceTest.calls.filter(call => call.cmd === 'append_frontend_diagnostics').flatMap(call => call.args.records)`);
    assert.equal(entries[0].kind, 'frontend.start', 'Diagnostics start before the UI imports and IPC');
    assert.ok(!entries.some(entry => entry.kind === 'frontend.start_failed'));
    if(label === 'main') {
      await evaluate('persistenceTest.stores.paused.value = true');
      await until(`persistenceTest.calls.filter(call=>call.cmd==='append_frontend_diagnostics').flatMap(call=>call.args.records).some(record=>record.kind==='state.changed'&&record.data.field==='paused'&&record.data.current.value===true)`);
    }
    if (label === 'settings') {
      await evaluate(`persistenceTest.button('关于').click()`);
      await until('Boolean(document.querySelector("[data-frontend-diagnostics]"))');
      await call('Emulation.setDeviceMetricsOverride', { width: 680, height: 500, deviceScaleFactor: 1, mobile: false });
      await evaluate(`document.querySelector('[data-frontend-diagnostics]').scrollIntoView({block:'end'})`);
      await screenshot('/tmp/neoserial-settings-frontend-diagnostics.png');
    }
  }
  console.log('PASS frontend diagnostics: all windows, errors, stack traces, resources, interactions, private values, IPC timing, retry, export, cancel, long tasks, and real main/settings entry');
});
