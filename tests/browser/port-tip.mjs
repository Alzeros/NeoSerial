import assert from 'node:assert/strict';

// Start Vite and a dedicated Chrome with --remote-debugging-port=9335, then run:
// node tests/browser/port-tip.mjs (optional VITE_URL / CHROME_DEBUG_URL overrides).
const baseUrl=process.env.VITE_URL ?? 'http://localhost:5174';
const debugUrl=process.env.CHROME_DEBUG_URL ?? 'http://localhost:9335';
const pages=await(await fetch(`${debugUrl}/json/list`)).json();
const ws=new WebSocket(pages.find(p=>p.type==='page').webSocketDebuggerUrl);
await new Promise(r=>ws.addEventListener('open',r,{once:true}));
let id=0;
const pending=new Map();
ws.addEventListener('message',e=>{
  const m=JSON.parse(e.data);
  if(m.id){const p=pending.get(m.id);pending.delete(m.id);m.error?p.reject(m.error):p.resolve(m.result);}
});
function call(method,params={}){
  return new Promise((resolve,reject)=>{const k=++id;pending.set(k,{resolve,reject});ws.send(JSON.stringify({id:k,method,params}));});
}
async function ev(expression){
  const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});
  if(r.exceptionDetails)throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
const timeout=setTimeout(()=>{console.error('Browser test timed out');process.exit(1);},30000);
try{
  await call('Page.navigate',{url:`${baseUrl}/tests/browser/port-tip.html`});
  for(let i=0;i<100;i++){if(await ev('!!window.ready'))break;await new Promise(r=>setTimeout(r,100));}
  await ev(`window.trigger=document.querySelector('button');window.tipText=()=>document.querySelector('.custom-select-tip')?.textContent.trim()??null;window.menu=()=>document.querySelector('[role=option]')?.parentElement;window.key=k=>trigger.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true}));trigger.click();`);
  async function hover(index=0){
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:600,y:400});
    const pos=await ev(`(()=>{const r=document.querySelectorAll('[role=option]')[${index}].getBoundingClientRect();return {x:r.x+20,y:r.y+15};})()`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...pos});
    assert.equal(await ev('tipText()'),index===14?null:`Device ${index+1}`);
  }
  for(const close of [
    `key('Escape')`,
    `key('Tab')`,
    `key('Enter')`,
    `trigger.click()`,
    `document.querySelector('[role=option]').click()`,
    `document.body.dispatchEvent(new MouseEvent('mousedown',{bubbles:true}))`,
    `window.dispatchEvent(new Event('scroll'))`,
    `menu().lastElementChild.click()`,
  ]){
    await hover();
    await ev(close);
    assert.equal(await ev('Boolean(menu())'),false,`close: ${close}`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:600,y:400});
    await ev(`key('Enter')`);
    assert.equal(await ev('tipText()'),null,`no stale tooltip after: ${close}`);
  }
  assert.equal(await ev('window.added'),true,'add option still works');
  for(const key of ['ArrowDown','ArrowUp']){
    await hover();
    await ev(`key('${key}')`);
    assert.equal(await ev('tipText()'),null,`${key} clears tooltip`);
  }
  await hover(1);
  await ev('menu().scrollTop=180');
  await new Promise(r=>setTimeout(r,100));
  assert.equal(await ev('Boolean(menu())'),true,'internal scrolling keeps menu open');
  assert.equal(await ev('tipText()'),null,'internal scrolling clears tooltip');
  await hover(7);
  await ev('menu().scrollTop=menu().scrollHeight');
  await new Promise(r=>setTimeout(r,100));
  await hover(14);
  await ev('document.querySelectorAll("[role=option]")[14].click()');
  assert.equal(await ev('trigger.textContent.trim()'),'COM15','selection still binds the COM number');
  console.log('PASS: all close paths, reopen, keyboard navigation, internal scroll, fresh hover, no-name option, COM selection.');
}finally{clearTimeout(timeout);ws.close();}
