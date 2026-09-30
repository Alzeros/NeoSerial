import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
const base=process.env.VITE_URL??'http://localhost:5173';
const pages=await(await fetch((process.env.WEBVIEW_DEBUG_URL??'http://localhost:9336')+'/json/list')).json();
const page=pages.find(p=>p.type==='page'&&p.url.startsWith(base));
assert.ok(page,'isolated native window running');
const ws=new WebSocket(page.webSocketDebuggerUrl);
await new Promise(r=>ws.addEventListener('open',r,{once:true}));
let id=0;const pending=new Map();
ws.addEventListener('message',e=>{const m=JSON.parse(e.data);if(!m.id)return;const p=pending.get(m.id);pending.delete(m.id);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);});
const call=(method,params={})=>new Promise((resolve,reject)=>{const next=++id;pending.set(next,{resolve,reject});ws.send(JSON.stringify({id:next,method,params}));});
async function ev(expression){const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true,userGesture:true});if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value;}
async function until(expression){for(let i=0;i<100;i++){if(await ev(expression))return;await new Promise(r=>setTimeout(r,50));}assert.fail(expression);}
const btn=text=>"[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==="+JSON.stringify(text)+")";
const timeout=setTimeout(()=>{console.error('native MCP test timed out');process.exit(1);},60000);
try{
  const dirs=await ev("window.__TAURI_INTERNALS__.invoke('data_dirs')");
  assert.ok(dirs.config.replaceAll('\\','/').endsWith('/docs/codec-test-data'),'requires disposable profile');
  const check=await ev("window.__TAURI_INTERNALS__.invoke('check_mcp_connection')");
  assert.equal(check.server_name,'NeoSerial');assert.ok(check.tool_count>=19);
  await writeFile('docs/mcp-test-endpoint.json',JSON.stringify(check));
  console.log('PASS: real native HTTP discovery + tools/list',JSON.stringify(check));
  await call('Emulation.setDeviceMetricsOverride',{width:680,height:500,deviceScaleFactor:1,mobile:false});
  await call('Page.navigate',{url:base+'/tests/browser/mcp-settings.html'});
  await until('Boolean(window.mcpSettingsTest)');
  // onMount reads default section asynchronously; explicitly focus the MCP section after ready.
  await ev('mcpSettingsTest.show()');
  await until("Boolean(document.querySelector('#mcp-client'))");
  assert.equal(await ev("document.querySelector('input[aria-label=\"MCP 服务地址\"]').value"),`http://127.0.0.1:${check.port}/mcp`);
  await call('Emulation.setFocusEmulationEnabled',{enabled:true});
  await ev(`(()=>{window.copyWrites=[];const write=navigator.clipboard.writeText.bind(navigator.clipboard);navigator.clipboard.writeText=async text=>{await write(text);copyWrites.push({text,actual:await navigator.clipboard.readText(),focused:document.hasFocus()});};})()`);
  await ev(btn('复制地址')+'.click()');
  await until("document.body.textContent.includes('已复制')");
  const clipboardWrites=await ev('copyWrites');
  assert.equal(clipboardWrites.at(-1).text,`http://127.0.0.1:${check.port}/mcp`,'exact address sent to real Clipboard API');
  if(clipboardWrites.at(-1).actual!==clipboardWrites.at(-1).text){
    console.log('LIMIT: hidden native Clipboard API resolves but readback is empty; OS clipboard round-trip not verified.');
  }
  await ev(btn('检测服务')+'.click()');
  await until("document.body.textContent.includes('个工具可读取')");
  await ev("(()=>{const s=document.querySelector('#mcp-client');s.value='vscode';s.dispatchEvent(new Event('change',{bubbles:true}));})()");
  await until("Boolean(document.querySelector('textarea'))");
  const config=JSON.parse(await ev("document.querySelector('textarea').value"));
  assert.equal(config.servers.neoserial.url,`http://127.0.0.1:${check.port}/mcp`);
  await ev("document.querySelector('#mcp-client').scrollIntoView({block:'start'})");
  assert.ok(await ev('document.documentElement.scrollWidth<=window.innerWidth'),'no horizontal overflow');
  const save=await ev("(()=>{const r="+btn('保存')+".getBoundingClientRect();return {top:r.top,bottom:r.bottom};})()");
  assert.ok(save.top>=0&&save.bottom<=500,'footer remains visible');
  const shot=await call('Page.captureScreenshot',{format:'png'});
  await writeFile('docs/mcp-clients-settings.png',Buffer.from(shot.data,'base64'));
  console.log('PASS: actual SettingsDialog at 680x500, clipboard write arguments, diagnostics, templates and visible footer.');
}finally{clearTimeout(timeout);ws.close();}
