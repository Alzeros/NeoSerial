import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
// Disposable native instance only, with docs/codec-test-data and debugging 9336.
const base=process.env.VITE_URL??'http://localhost:5173';
const pages=await(await fetch((process.env.WEBVIEW_DEBUG_URL??'http://localhost:9336')+'/json/list')).json();
const page=pages.find(p=>p.type==='page'&&p.url.startsWith(base));
const ws=new WebSocket(page.webSocketDebuggerUrl);
await new Promise(r=>ws.addEventListener('open',r,{once:true}));
let id=0;const pending=new Map();
ws.addEventListener('message',e=>{const m=JSON.parse(e.data);if(!m.id)return;const p=pending.get(m.id);pending.delete(m.id);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);});
function call(method,params={}){return new Promise((resolve,reject)=>{const next=++id;pending.set(next,{resolve,reject});ws.send(JSON.stringify({id:next,method,params}));});}
async function ev(expression){const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true,userGesture:true});if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value;}
const wait=ms=>new Promise(r=>setTimeout(r,ms));
async function until(expression){for(let i=0;i<100;i++){if(await ev(expression))return;await wait(50);}assert.fail(expression);}
const buttons=(text,scope='document')=>"[..."+scope+".querySelectorAll('button')].find(b=>b.textContent.trim()==="+JSON.stringify(text)+")";
const timeout=setTimeout(()=>process.exit(1),45000);
try{
  const dirs=await ev("window.__TAURI_INTERNALS__.invoke('data_dirs')");
  assert.ok(dirs.config.replaceAll('\\','/').endsWith('/docs/codec-test-data'));
  await ev("window.__TAURI_INTERNALS__.invoke('patch_settings',{patch:{ui:{codec_default_encoding:'gbk',hidden_codec_operations:[]}}})");
  await call('Page.navigate',{url:base+'/tests/browser/codec-settings.html'});
  await until('Boolean(window.codecSettingsTest)');
  await ev('codecSettingsTest.fresh()');
  assert.equal(await ev('codecSettingsTest.config().encoding'),'gbk','new tool uses default');
  await ev("codecSettingsTest.configPatch({encoding:'utf8',input:'keep',operation:'base64_encode'})");
  await ev('codecSettingsTest.show()');
  await ev("[...document.querySelectorAll('#settings button[aria-expanded]')].find(b=>b.textContent.includes('默认参数')).click()");
  assert.ok(await ev("(()=>{const t=document.querySelector('#settings').innerText;return t.indexOf('帧构造器选项')<t.indexOf('编解码选项')&&t.indexOf('编解码选项')<t.indexOf('短信选项');})()"),'group order follows tool order');
  await ev("(()=>{const e=document.querySelector('#settings-codec-encoding');e.value='utf16be';e.dispatchEvent(new Event('change',{bubbles:true}));})()");
  await ev(buttons('取消',"document.querySelector('#settings')")+'.click()');
  assert.equal(await ev('codecSettingsTest.ui().codec_default_encoding'),'gbk','cancel does not apply');
  await ev('codecSettingsTest.show()');
  await ev("[...document.querySelectorAll('#settings button[aria-expanded]')].find(b=>b.textContent.includes('默认参数')).click()");
  assert.equal(await ev("document.querySelector('#settings-codec-encoding').value"),'gbk','reopen reads saved value');
  await ev("(()=>{const e=document.querySelector('#settings-codec-encoding');e.value='utf16be';e.dispatchEvent(new Event('change',{bubbles:true}));[...document.querySelectorAll('#settings button[aria-expanded]')].find(e=>e.textContent.includes('处理方式')).click();})()");
  await ev("(()=>{window.codecGroup=document.querySelector('#settings-codec-encoding').closest('.mt-3');for(const l of codecGroup.querySelectorAll('label')){if(l.textContent.includes('Base64 编码'))l.querySelector('input').click();}})()");
  assert.equal(await ev("codecGroup.querySelectorAll('input:disabled').length"),2,'basic operations fixed');
  await ev("codecGroup.scrollIntoView({block:'center'})");
  const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile('docs/codec-settings.png',Buffer.from(shot.data,'base64'));
  await ev(buttons('保存',"document.querySelector('#settings')")+'.click()');
  await until("document.querySelector('#settings').textContent.trim()===''");
  const ui=await ev('codecSettingsTest.ui()');
  assert.equal(ui.codec_default_encoding,'utf16be');
  assert.deepEqual(ui.hidden_codec_operations,['base64_encode']);
  const saved=JSON.parse(await readFile(join(dirs.config,'settings.json'),'utf8'));
  assert.equal(saved.ui.codec_default_encoding,'utf16be','default persisted');
  assert.deepEqual(saved.ui.hidden_codec_operations,['base64_encode'],'visibility persisted');
  assert.equal(await ev('codecSettingsTest.config().input'),'keep');
  assert.equal(await ev('codecSettingsTest.config().encoding'),'utf8','existing draft is untouched');
  assert.ok(await ev("document.querySelector('.codec-panel .custom-select-trigger').textContent.includes('已隐藏')"));
  await ev("document.querySelector('.codec-panel .custom-select-trigger').click()");
  assert.ok(!(await ev("[...document.querySelectorAll('[role=option]')].some(e=>e.textContent.trim()==='Base64 编码')")));
  await ev("document.querySelector('.codec-panel .custom-select-trigger').click()");
  await ev("document.querySelector('.codec-panel .btn-primary').click()");
  await until("document.querySelector('#codec-result').value==='a2VlcA=='");
  await ev(buttons('使用默认编码')+'.click()');
  assert.equal(await ev('codecSettingsTest.config().encoding'),'utf16be');
  assert.equal(await ev('codecSettingsTest.config().input'),'keep');
  assert.ok(await ev("document.querySelector('.codec-panel').textContent.includes('已过期')"));
  await ev('codecSettingsTest.fresh(true)');
  assert.equal(await ev('codecSettingsTest.config().encoding'),'utf16le','legacy keeps chosen encoding');
  await ev('codecSettingsTest.fresh()');
  assert.equal(await ev('codecSettingsTest.config().encoding'),'utf16be');
  await ev("codecSettingsTest.defaults('gbk')");
  assert.equal(await ev('codecSettingsTest.config().encoding'),'utf16be','late setting does not overwrite initialized draft');
  console.log('PASS: native save/cancel/persistence, section order, fixed options, hidden-current label, default action, stale results, first-use and legacy draft protection');
}finally{clearTimeout(timeout);ws.close();}
