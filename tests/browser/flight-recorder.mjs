import assert from 'node:assert/strict';
import {withHeadlessBrowser} from './headless.mjs';

await withHeadlessBrowser(async ({base,call,evaluate,until,delay,screenshot})=>{
  await call('Page.navigate',{url:`${base}/tests/browser/flight-recorder.html`});
  await until('window.flightTest && flightTest.records.some(r=>r.kind==="replay.started")');
  await evaluate('flightTest.addUnknownFeature()');
  await delay(100);
  const point=await evaluate(`(()=>{const r=document.querySelector('#future-trigger').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  await call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:1});
  await call('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:1});
  await evaluate(`flightTest.emit('future-event',{active:true,count:73,text:'PRIVATE_EVENT'})`);
  await evaluate(`flightTest.invoke('plugin:future|new_operation',{token:'PRIVATE_TOKEN'})`);
  await evaluate(`(async()=>{
    const response=await fetch('/package.json?token=PRIVATE_FETCH');if(response.status!==200)throw new Error('fetch changed');await response.json();
    await new Promise((resolve,reject)=>{const request=new XMLHttpRequest();request.open('GET','/package.json?key=PRIVATE_XHR');request.onload=()=>request.status===200?resolve():reject(new Error('XHR changed'));request.onerror=reject;request.send();});
    const socket=new WebSocket('ws://localhost/future?token=PRIVATE_SOCKET_TOKEN');await new Promise(resolve=>socket.addEventListener('open',resolve,{once:true}));socket.send('PRIVATE_SOCKET_BODY');socket.close();
    const source=new EventSource('http://localhost/future?token=PRIVATE_SSE');await new Promise(resolve=>source.addEventListener('open',resolve,{once:true}));source.dispatchEvent(new MessageEvent('message',{data:'PRIVATE_EVENT_BODY'}));source.close();
    const worker=new Worker('data:text/javascript,PRIVATE_WORKER_SOURCE');worker.dispatchEvent(new ErrorEvent('error',{message:'isolated worker error'}));worker.terminate();
  })()`);
  await delay(200);
  await evaluate('flightTest.flushFrontendDiagnostics()');
  const records=await evaluate('flightTest.records');
  assert.ok(records.some(r=>r.kind==='ipc.ok'&&r.data.command==='future_command'&&r.data.result_schema.ready), JSON.stringify(await evaluate(`({state:flightTest.state,calls:flightTest.calls.map(call=>call.command),inputs:flightTest.records.filter(r=>r.kind==='ui.pointerdown'),ipc:flightTest.records.filter(r=>r.kind==='ipc.ok'&&r.data.command==='future_command')})`)));
  assert.ok(records.some(r=>r.kind==='ipc.start'&&r.data.command==='plugin:future|new_operation'));
  assert.ok(records.some(r=>r.kind==='event.received'&&r.data.event==='future-event'&&r.data.schema.active));
  assert.ok(records.some(r=>r.kind==='state.changed'&&r.data.module==='future-module'&&r.data.current.enabled));
  assert.ok(records.some(r=>r.kind==='ui.pointerdown'&&r.data.replay_node>0));
  for(const kind of ['network.end','network.xhr_end','network.websocket_send','network.websocket_close','network.eventsource_close','worker.started','worker.error','worker.terminated']) assert.ok(records.some(record=>record.kind===kind),kind);
  let report=await evaluate('flightTest.report()');
  const serialized=JSON.stringify(report);
  assert.doesNotMatch(serialized,/PRIVATE_/,'New content and attributes are masked automatically');
  assert.match(serialized,/future-panel/,'A new uninstrumented DOM component enters the recording');
  assert.ok(report.replay.segments.some(segment=>segment.events.some(event=>event.type===2)));
  assert.ok(report.replay.segments.some(segment=>segment.events.some(event=>event.type===3&&event.data.source===0)));

  const before=serialized.length;
  await evaluate(`(()=>{const logs=document.createElement('div');logs.dataset.diagnosticPrivate='';document.body.append(logs);for(let i=0;i<2000;i++){const row=document.createElement('div');row.textContent='PRIVATE_SERIAL_'+i;logs.append(row);}})()`);
  await delay(100);await evaluate('flightTest.flushFrontendDiagnostics()');
  report=await evaluate('flightTest.report()');
  assert.doesNotMatch(JSON.stringify(report),/PRIVATE_SERIAL/);
  assert.ok(JSON.stringify(report).length-before<100000,'High volume serial regions do not flood the DOM recording');

  await evaluate(`document.querySelector('[data-diagnostic-action="view-frontend-logs"]').click()`);
  await until('Boolean(document.querySelector("[data-replay-file]"))');
  await evaluate(`(()=>{const file=new File([JSON.stringify(flightTest.report())],'future.json',{type:'application/json'});const transfer=new DataTransfer();transfer.items.add(file);const input=document.querySelector('[data-replay-file]');input.files=transfer.files;input.dispatchEvent(new Event('change',{bubbles:true}));})()`);
  await until('Boolean(document.querySelector("[data-replay-viewer] iframe"))');
  assert.equal(await evaluate('document.querySelector("[data-replay-viewer] [role=alert]")?.textContent ?? ""'),'');
  await evaluate(`(()=>{const slider=document.querySelector('[aria-label="回放进度"]');slider.value=slider.max;slider.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  await delay(300);
  await until(`(()=>{const iframe=document.querySelector('[data-replay-viewer] iframe');return iframe?.contentDocument?.getElementById('future-panel')?.hidden===false;})()`);
  assert.equal(await evaluate(`document.querySelector('[data-replay-viewer] iframe').contentDocument.getElementById('future-panel').style.width`),'340px');
  assert.equal(await evaluate(`document.querySelector('[data-replay-viewer] iframe').contentDocument.documentElement.dataset.theme`),'preset-3');
  assert.equal(await evaluate(`(()=>{const frame=document.querySelector('[data-replay-viewer] iframe');return frame.contentWindow.getComputedStyle(frame.contentDocument.getElementById('future-panel')).borderTopWidth;})()`),'4px');
  assert.ok(!(await evaluate(`document.querySelector('[data-replay-viewer] iframe').getAttribute('sandbox')`)).includes('allow-scripts'));
  await screenshot('/tmp/neoserial-flight-recorder-viewer.png');
  console.log('PASS generic flight recorder: unknown DOM/plugin/event/state, HTTP/XHR/WebSocket/SSE/Worker, privacy, serial pressure, report and sandboxed replay');
});
