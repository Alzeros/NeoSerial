import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

// Real settings IPC against the isolated native instance, not the user's profile.
const base=process.env.VITE_URL??'http://localhost:5173';
const targets=await(await fetch((process.env.WEBVIEW_DEBUG_URL??'http://localhost:9336')+'/json/list')).json();
const page=targets.find(p=>p.type==='page'&&p.url.startsWith(base));
const ws=new WebSocket(page.webSocketDebuggerUrl);
await new Promise(resolve=>ws.addEventListener('open',resolve,{once:true}));
let id=0;const pending=new Map();
ws.addEventListener('message',e=>{const m=JSON.parse(e.data);if(!m.id)return;const p=pending.get(m.id);pending.delete(m.id);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);});
function call(method,params={}){return new Promise((resolve,reject)=>{const next=++id;pending.set(next,{resolve,reject});ws.send(JSON.stringify({id:next,method,params}));});}
async function ev(expression){const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true,userGesture:true});if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value;}
async function until(expression){for(let i=0;i<100;i++){if(await ev(expression))return;await new Promise(r=>setTimeout(r,50));}assert.fail(expression);}
const button=text=>"[...document.querySelectorAll('#settings button')].find(b=>b.textContent.trim()==="+JSON.stringify(text)+")";
async function expand(){await ev("[...document.querySelectorAll('#settings button[aria-expanded]')].find(b=>b.textContent.includes('日志内容')).click()");}
async function setRange(label,value){await ev("(()=>{const el=document.querySelector('input[aria-label="+JSON.stringify(label)+"]');el.value="+JSON.stringify(String(value))+";el.dispatchEvent(new Event('input',{bubbles:true}));})()");}
const timeout=setTimeout(()=>{console.error('native log settings regression timed out');process.exit(1);},60000);
try{
  const dirs=await ev("window.__TAURI_INTERNALS__.invoke('data_dirs')");
  assert.ok(dirs.config.replaceAll('\\','/').endsWith('/docs/codec-test-data'),'requires disposable profile');
  await ev("window.__TAURI_INTERNALS__.invoke('patch_settings',{patch:{ui:{ring_buffer_capacity:100000,log_buffer_max_bytes:4194304}}})");
  await call('Page.navigate',{url:base+'/tests/browser/log-settings.html'});
  await until('Boolean(window.logSettingsTest)');
  assert.equal(await ev('logSettingsTest.ui().ring_buffer_capacity'),10000,'backend clamps legacy high limit');
  await ev('logSettingsTest.show()');await expand();
  assert.equal(await ev("document.querySelector('input[aria-label=\"日志保留行数\"]').max"),'10000');
  assert.equal(await ev("document.querySelector('input[aria-label=\"日志缓存数据大小\"]').value"),'4');
  await setRange('日志保留行数',3000);await setRange('日志缓存数据大小',1);
  await ev(button('取消')+'.click()');
  assert.equal(await ev('logSettingsTest.ui().log_buffer_max_bytes'),4194304,'cancel preserves saved byte budget');
  await ev('logSettingsTest.show()');await expand();
  assert.equal(await ev("document.querySelector('input[aria-label=\"日志缓存数据大小\"]').value"),'4');
  await ev('logSettingsTest.seed(1025,2048)');
  assert.equal(await ev('logSettingsTest.rows()'),1025);
  await setRange('日志保留行数',3000);await setRange('日志缓存数据大小',1);
  await ev(button('应用')+'.click()');
  await until('logSettingsTest.ui().log_buffer_max_bytes===1048576');
  assert.equal(await ev('logSettingsTest.rows()'),512,'applying smaller byte budget trims existing cache');
  assert.equal(await ev("document.querySelectorAll('[data-log-row]').length"),512);
  const disk=JSON.parse(await readFile(join(dirs.config,'settings.json'),'utf8'));
  assert.equal(disk.ui.ring_buffer_capacity,3000);
  assert.equal(disk.ui.log_buffer_max_bytes,1048576);
  await ev("document.querySelector('input[aria-label=\"日志缓存数据大小\"]').scrollIntoView({block:'center'})");
  const screenshot=await call('Page.captureScreenshot',{format:'png'});
  await writeFile('docs/log-retention-settings.png',Buffer.from(screenshot.data,'base64'));
  await ev(button('保存')+'.click()');
  await until("document.querySelector('#settings').textContent.trim()===''");
  await ev('logSettingsTest.show()');await expand();
  assert.equal(await ev("document.querySelector('input[aria-label=\"日志保留行数\"]').value"),'3000');
  assert.equal(await ev("document.querySelector('input[aria-label=\"日志缓存数据大小\"]').value"),'1');
  console.log('PASS: native limits, range UI, cancel/reopen, apply trimming, save and disk persistence.');
}finally{clearTimeout(timeout);ws.close();}
