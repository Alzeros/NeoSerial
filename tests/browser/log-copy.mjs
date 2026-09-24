import assert from 'node:assert/strict';
// Run with Vite and a dedicated headless Chrome (--remote-debugging-port=9335).
// node tests/browser/log-copy.mjs; optional VITE_URL / CHROME_DEBUG_URL overrides.
const baseUrl=process.env.VITE_URL ?? 'http://localhost:5173';
const debugUrl=process.env.CHROME_DEBUG_URL ?? 'http://localhost:9335';
const pages=await(await fetch(`${debugUrl}/json/list`)).json();
const ws=new WebSocket(pages.find(p=>p.type==='page').webSocketDebuggerUrl);
await new Promise(resolve=>ws.addEventListener('open',resolve,{once:true}));
let id=0;const pending=new Map();
ws.addEventListener('message',e=>{const m=JSON.parse(e.data);if(m.id){const p=pending.get(m.id);pending.delete(m.id);m.error?p.reject(m.error):p.resolve(m.result);}});
function call(method,params={}){return new Promise((resolve,reject)=>{const requestId=++id;pending.set(requestId,{resolve,reject});ws.send(JSON.stringify({id:requestId,method,params}));});}
async function evaluate(expression){if(expression.includes('await '))expression=`(async()=>{${expression}})()`;const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(r.exceptionDetails)throw new Error(JSON.stringify(r.exceptionDetails));return r.result.value;}
const timeout=setTimeout(()=>process.exit(1),30000);
try {
  await call('Page.enable');
  await call('Page.navigate',{url:`${baseUrl}/tests/browser/log-copy.html`});
  for(let i=0;i<100;i++){if(await evaluate('Boolean(window.logTest)'))break;await new Promise(r=>setTimeout(r,100));}
  await evaluate(`window.root=document.querySelector('[role=log]');window.selectAll=()=>{const r=document.createRange();r.selectNodeContents(root);const s=getSelection();s.removeAllRanges();s.addRange(r);};window.copy=()=>{const data=new DataTransfer();const e=new ClipboardEvent('copy',{clipboardData:data,bubbles:true,cancelable:true});root.dispatchEvent(e);return e.defaultPrevented?data.getData('text/plain'):getSelection().toString();};selectAll();`);
  const copied=await evaluate('copy()');
  console.log('Full selection:',JSON.stringify(copied));
  assert.equal(copied,'3 Rx 09:19:44.763 OK\n4 Tx 09:19:44.763 at\n5 Rx 09:19:44.763\n6 Rx 09:19:44.763 +CFUN: 1\n7 Rx 09:19:44.763   spaced  ');
  await evaluate(`window.content=i=>root.querySelectorAll('[data-log-row]')[i].lastElementChild;window.pick=(a,start,b,end,backward=false)=>{const s=getSelection();s.removeAllRanges();s.setBaseAndExtent(backward?b:a,backward?end:start,backward?a:b,backward?start:end);};`);
  await evaluate(`pick(content(3).firstChild,1,content(3).firstChild,5)`);
  assert.equal(await evaluate('copy()'),'CFUN','partial content');
  await evaluate(`pick(content(0).firstChild,1,content(3).firstChild,5,true)`);
  assert.equal(await evaluate('copy()'),'K\n4 Tx 09:19:44.763 at\n5 Rx 09:19:44.763\n6 Rx 09:19:44.763 +CFUN','backward selection across rows');
  await evaluate(`pick(content(0).firstChild,0,content(1).firstChild,0)`);
  assert.equal(await evaluate('copy()'),'OK\n4 Tx 09:19:44.763','exclusive end at next content start');
  await evaluate(`logTest.stores.showLineIndex.value=false;logTest.stores.showTimestamp.value=false;logTest.stores.logSendContent.value=false;await logTest.tick();selectAll();`);
  assert.equal(await evaluate('copy()'),'OK\nat\n\n+CFUN: 1\n  spaced  ','hidden metadata and blank row');
  await evaluate(`root.style.width='95px';logTest.stores.appendLogLine({ascii:'abcdefghijklmnopqrstuvwxyz',raw:[],ts:'x',ts_ms:0,dir:'rx',line_index:8,is_error:false});await logTest.tick();selectAll();`);
  assert.equal(await evaluate('copy()'),'OK\nat\n\n+CFUN: 1\n  spaced  \nabcdefghijklmnopqrstuvwxyz','visual wrapping does not add newlines');
  await evaluate(`root.style.width='';window.dispatchEvent(new KeyboardEvent('keydown',{key:'f',ctrlKey:true}));await logTest.tick();await new Promise(requestAnimationFrame);const input=document.querySelector('input');input.value='CFUN';input.dispatchEvent(new Event('input',{bubbles:true}));await logTest.tick();const r=document.createRange();r.selectNodeContents(content(3));getSelection().removeAllRanges();getSelection().addRange(r);`);
  assert.equal(await evaluate('copy()'),'+CFUN: 1','search highlights preserve content');
  assert.equal(await evaluate(`(()=>{const d=new DataTransfer();const e=new ClipboardEvent('copy',{clipboardData:d,bubbles:true,cancelable:true});document.querySelector('input').dispatchEvent(e);return e.defaultPrevented;})()`),false,'input copy is not intercepted');
  await evaluate(`window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));await logTest.tick();logTest.stores.displayMode.value='hex';logTest.stores.logLines[0].raw=Array.from({length:20},(_,i)=>65+i);await logTest.tick();const r=document.createRange();r.selectNodeContents(content(0));getSelection().removeAllRanges();getSelection().addRange(r);`);
  const hex=await evaluate('content(0).textContent');
  assert.equal(hex.split('\n').length,2);
  assert.equal(await evaluate('copy()'),hex,'HEX keeps actual newlines and padding');
  await call('Browser.grantPermissions',{origin:new URL(baseUrl).origin,permissions:['clipboardReadWrite','clipboardSanitizedWrite']});
  await evaluate(`root.focus()`);
  await call('Input.dispatchKeyEvent',{type:'keyDown',key:'c',code:'KeyC',windowsVirtualKeyCode:67,modifiers:2});
  await call('Input.dispatchKeyEvent',{type:'keyUp',key:'c',code:'KeyC',windowsVirtualKeyCode:67,modifiers:2});
  assert.equal(await evaluate('navigator.clipboard.readText()'),hex,'real Ctrl+C clipboard');
  await evaluate(`navigator.clipboard.writeText('context-copy-sentinel')`);
  await evaluate(`root.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:200,clientY:150}));await logTest.tick();document.querySelectorAll('button').forEach(b=>{if(b.textContent.trim()==='复制')b.click();});`);
  assert.equal((await evaluate('navigator.clipboard.readText()')).replace(/\r\n/g,'\n'),hex,'context menu clipboard');
  console.log('PASS: whole/partial/backward selection, blank rows, metadata toggles, soft wrapping, highlights, HEX, input isolation, Ctrl+C and context menu.');
} finally {clearTimeout(timeout);ws.close();}
