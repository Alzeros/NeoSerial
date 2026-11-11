import assert from 'node:assert/strict';
import { withHeadlessBrowser } from './headless.mjs';

await withHeadlessBrowser(async ({ base, call, evaluate, until, delay, screenshot }) => {
  await call('Emulation.setDeviceMetricsOverride', { width: 1216, height: 800, deviceScaleFactor: 1, mobile: false });
  await call('Page.navigate', { url: `${base}/tests/browser/log-copy.html` });
  await until('Boolean(window.logTest)', 30000);
  await evaluate(`(() => {
    const s=logTest.stores;s.clearLogLines();s.showLineIndex.value=true;s.showTimestamp.value=true;
    s.logSendContent.value=true;s.logDirLabelStyle.value='full';s.displayMode.value='hex';
    s.applyLogFont(14,1.6,'Consolas');
    window.hexRow=(i)=>({ascii:'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef',raw:Array.from({length:32},(_,j)=>65+j),
      ts:'11:10:29.306',ts_ms:i,dir:'rx',line_index:688+i,is_error:false});
    s.appendLogLines([hexRow(0),hexRow(1)]);return logTest.tick();
  })()`);
  await evaluate('document.fonts.ready');
  await delay(100);

  const geometry = () => evaluate(`(() => {
    const root=document.querySelector('[role=log]');
    return [...root.querySelectorAll('[data-log-row]')].map(row=>{
      const body=row.lastElementChild,rect=body.getBoundingClientRect();
      return {lines:body.textContent.trimEnd().split('\\n').length,height:rect.height,
        lineHeight:parseFloat(getComputedStyle(body).lineHeight),overflow:body.scrollWidth-body.clientWidth,
        right:rect.right,viewportRight:root.getBoundingClientRect().right};
    });
  })()`);
  const checkAligned = async () => {
    for(const row of await geometry()) {
      assert.ok(Math.abs(row.height-row.lines*row.lineHeight)<2, 'HEX rows must never soft-wrap between bytes or inside the ASCII column');
      assert.ok(row.overflow<=1 && row.right<=row.viewportRight, 'HEX and ASCII columns fit the available width');
    }
  };
  await checkAligned();
  assert.equal((await geometry())[0].lines,4,'default width with all metadata uses eight bytes per row');
  const narrowText = await evaluate(`document.querySelector('[data-log-row]').lastElementChild.textContent`);

  await evaluate(`logTest.stores.showLineIndex.value=false;logTest.stores.showTimestamp.value=false;logTest.stores.logSendContent.value=false;logTest.tick()`);
  await until(`document.querySelector('[data-log-row]').lastElementChild.textContent.trimEnd().split('\\n').length===2`);
  await checkAligned();
  await evaluate(`logTest.stores.showLineIndex.value=true;logTest.stores.showTimestamp.value=true;logTest.stores.logSendContent.value=true;document.querySelector('#app').style.width='950px';logTest.tick()`);
  await delay(100);
  await checkAligned();
  assert.equal((await geometry())[0].lines,2,'wide layout keeps sixteen bytes per row');

  await evaluate(`logTest.stores.appendLogLines(Array.from({length:40},(_,i)=>hexRow(i+2)));logTest.tick()`);
  await delay(100);
  await evaluate(`document.querySelector('#app').style.width='700px';logTest.tick()`);
  await until(`document.querySelector('[data-log-row]').lastElementChild.textContent.trimEnd().split('\\n').length===4`);
  await delay(60);
  await checkAligned();
  assert.ok(await evaluate(`(()=>{const root=document.querySelector('[role=log]');return Math.abs(root.scrollHeight-root.clientHeight-root.scrollTop)<3&&logTest.stores.autoScroll.value;})()`), 'HEX reflow keeps following at the bottom');

  await evaluate(`logTest.stores.applyLogFont(22,1.6,'Consolas');logTest.tick()`);
  await until(`document.querySelector('[data-log-row]').lastElementChild.textContent.trimEnd().split('\\n').length===8`);
  await checkAligned();
  await evaluate(`logTest.stores.applyLogFont(14,1.6,'Consolas');logTest.tick()`);
  await until(`document.querySelector('[data-log-row]').lastElementChild.textContent.trimEnd().split('\\n').length===4`);
  await evaluate(`window.dispatchEvent(new KeyboardEvent('keydown',{key:'f',ctrlKey:true}));logTest.tick()`);
  await evaluate(`const input=document.querySelector('input[placeholder="搜索日志..."]');input.value='41 42';input.dispatchEvent(new Event('input',{bubbles:true}));logTest.tick()`);
  await until(`document.querySelectorAll('[data-log-row] mark').length===42`);
  await checkAligned();
  await evaluate(`(()=>{const input=document.querySelector('input[placeholder="搜索日志..."]');input.value='FGHI';input.dispatchEvent(new Event('input',{bubbles:true}));return logTest.tick();})()`);
  await until(`document.querySelectorAll('[data-log-row] mark').length===84`);
  assert.equal(await evaluate(`[...document.querySelector('[data-log-row]').querySelectorAll('mark')].map(mark=>mark.textContent).join('')`),'FGHI','search highlights both parts of an ASCII match across display rows');
  await checkAligned();
  await evaluate(`document.querySelector('#app').style.width='950px';logTest.tick()`);
  await until(`document.querySelectorAll('[data-log-row] mark').length===42`);
  await checkAligned();
  await evaluate(`document.querySelector('#app').style.width='700px';logTest.tick()`);
  await until(`document.querySelectorAll('[data-log-row] mark').length===84`);
  await evaluate(`(()=>{const input=document.querySelector('input[placeholder="搜索日志..."]');input.value='48  49';input.dispatchEvent(new Event('input',{bubbles:true}));return logTest.tick();})()`);
  await until(`document.querySelectorAll('[data-log-row] mark').length===84`);
  assert.equal(await evaluate(`[...document.querySelector('[data-log-row]').querySelectorAll('mark')].map(mark=>mark.textContent).join('').replace(/\\s/g,'')`),'4849','HEX-byte searches also highlight across display rows');
  await evaluate(`(()=>{const input=document.querySelector('input[placeholder="搜索日志..."]');input.value=' ABC';input.dispatchEvent(new Event('input',{bubbles:true}));return logTest.tick();})()`);
  await until(`document.querySelectorAll('[data-log-row] mark').length===42`);
  assert.equal(await evaluate(`document.querySelector('[data-log-row] mark').textContent`),' ABC','additional display separators do not create duplicate highlights');
  assert.equal(await evaluate(`document.querySelector('[data-log-row]').lastElementChild.textContent`),narrowText,'highlight projection preserves every byte, space and newline');
  const selectedText = await evaluate(`(() => {
    const root=document.querySelector('[role=log]'),range=document.createRange();
    range.selectNodeContents(root.querySelector('[data-log-row]').lastElementChild);
    getSelection().removeAllRanges();getSelection().addRange(range);
    const clipboardData=new DataTransfer();root.dispatchEvent(new ClipboardEvent('copy',{bubbles:true,cancelable:true,clipboardData}));
    getSelection().removeAllRanges();return clipboardData.getData('text/plain');
  })()`);
  assert.equal(selectedText,narrowText,'copy preserves reflowed HEX text while matches are highlighted');
  await screenshot('docs/hex-layout-fixed.png');

  await evaluate(`window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));logTest.stores.cachedSettings.value={ui:{show_log_tools:true}};logTest.tick()`);
  await evaluate(`document.querySelector('[data-log-tools-toggle]').click();logTest.tick()`);
  await evaluate(`(()=>{const include=document.querySelector('[data-log-include]');include.value='FGHI';include.dispatchEvent(new Event('input',{bubbles:true}));return logTest.tick();})()`);
  await until(`document.querySelectorAll('[data-log-row]').length===42`);
  await evaluate(`window.hexSnippet='';Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async(text)=>{window.hexSnippet=text;}}});document.querySelector('[data-log-snippet-copy]').click()`);
  await until(`hexSnippet.includes('ABCDEFGHIJKLMNOP')`);
  await evaluate(`document.querySelector('[data-log-select-toggle]').click();logTest.tick()`);
  await evaluate(`document.querySelector('[data-log-select-row]').click();logTest.tick()`);
  await evaluate(`const marker=document.querySelector('[data-log-marker-input]');marker.value='a-long-local-marker';marker.dispatchEvent(new Event('input',{bubbles:true}));logTest.tick()`);
  await evaluate(`document.querySelector('[data-log-marker-add]').click();logTest.tick()`);
  await delay(80);
  await checkAligned();

  await evaluate(`document.querySelector('[role=log]').scrollTop=300;new Promise(requestAnimationFrame)`);
  const anchor = await evaluate(`(() => {
    const root=document.querySelector('[role=log]'),top=root.getBoundingClientRect().top;
    const row=[...root.querySelectorAll('[data-log-row]')].find(el=>el.getBoundingClientRect().bottom>top);
    return {id:row.dataset.logId,offset:row.getBoundingClientRect().top-top};
  })()`);
  await evaluate(`document.querySelector('#app').style.width='950px';logTest.tick()`);
  await until(`document.querySelectorAll('[data-log-row]')[1].lastElementChild.textContent.trimEnd().split('\\n').length===2`);
  await delay(80);
  assert.equal(await evaluate(`document.querySelectorAll('[data-log-row]').length`),42,'resizing never changes filter results');
  const anchorAfter = await evaluate(`document.querySelector('[data-log-id="${anchor.id}"]').getBoundingClientRect().top-document.querySelector('[role=log]').getBoundingClientRect().top`);
  assert.ok(Math.abs(anchorAfter-anchor.offset)<2, 'HEX reflow preserves the reading anchor when following is stopped');
  await checkAligned();
  await evaluate(`(()=>{const include=document.querySelector('[data-log-include]');include.value='';include.dispatchEvent(new Event('input',{bubbles:true}));return logTest.tick();})()`);

  await evaluate(`window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));logTest.stores.displayMode.value='ascii';logTest.stores.logLines[0].ascii='LONG_TEXT_'.repeat(30);logTest.tick()`);
  await delay(60);
  const ascii = (await geometry())[0];
  assert.ok(ascii.height > ascii.lineHeight * 2, 'ordinary text continues to wrap');
  assert.ok(ascii.overflow<=1, 'ordinary text stays within the viewport');
  console.log('PASS HEX layout: widths, metadata, fonts, following/reading anchors, highlights, filter/export stability, markers and ordinary text wrapping');
}, { timeoutMs: 180000 });
