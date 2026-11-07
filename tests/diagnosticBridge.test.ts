import test from 'node:test';
import assert from 'node:assert/strict';
import { observeDiagnosticBridge } from '../src/lib/diagnosticBridge.ts';
import { diagnosticSummary } from '../src/lib/diagnosticSummary.ts';
import { Channel } from '@tauri-apps/api/core';

test('unknown future commands and plugin calls are captured without changing results or rejection identity', async () => {
  const records: {kind:string; data:any}[] = [];
  const answer = { ok: true, count: 42, secret: 'PRIVATE_RESULT' };
  const failure = new Error('future failure');
  const bridge = { async invoke<T>(command: string, _args?: unknown): Promise<T> { if (command === 'future_failure') throw failure; return answer as T; } };
  observeDiagnosticBridge(bridge, (kind, data) => records.push({kind, data}));
  assert.equal(await bridge.invoke('brand_new_command', { text: 'PRIVATE_INPUT', credentials: 'PRIVATE_KEY' }), answer);
  await bridge.invoke('plugin:future|unknown_operation');
  await assert.rejects(bridge.invoke('future_failure'), error => error === failure);
  assert.ok(records.some(record => record.kind === 'ipc.ok' && record.data.command === 'brand_new_command' && record.data.result_schema.ok === true));
  assert.ok(records.some(record => record.kind === 'ipc.start' && record.data.command === 'plugin:future|unknown_operation'));
  assert.doesNotMatch(JSON.stringify(records), /PRIVATE_/);
  const count = records.length;
  await bridge.invoke('append_frontend_replay');
  assert.equal(records.length, count, 'Internal recorder writes do not record themselves');
});

test('new event names, handler errors, high rate summaries and unregistration work through the common bridge', async () => {
  const callbacks = new Map<number, (value: unknown) => unknown>();
  const records: {kind:string; data:any}[] = [];
  let next = 0;
  const bridge = {
    async invoke<T>(_command: string, _args?: unknown): Promise<T> { return 99 as T; },
    transformCallback(callback?: (value: unknown) => unknown) { const id=++next; callbacks.set(id,callback!); return id; },
    unregisterCallback(id:number) { callbacks.delete(id); },
  };
  const observer = observeDiagnosticBridge(bridge, (kind, data) => records.push({kind, data}));
  let delivered = 0;
  const id = bridge.transformCallback(() => delivered++);
  await bridge.invoke('plugin:event|listen', {event:'never_seen_before',handler:id});
  for (let i=0;i<1000;i++) callbacks.get(id)!({payload:{active:true,count:i,text:'PRIVATE_EVENT'}});
  observer.flushTraffic();
  assert.equal(delivered,1000);
  assert.equal(records.filter(record=>record.kind==='event.received').length,5);
  assert.equal(records.find(record=>record.kind==='event.traffic')!.data.count,1000);
  assert.doesNotMatch(JSON.stringify(records), /PRIVATE_/);
  const failure = new Error('handler failed');
  const badId = bridge.transformCallback(() => { observer.flushTraffic(); throw failure; });
  await bridge.invoke('plugin:event|listen', {event:'future_bad_handler',handler:badId});
  assert.throws(()=>callbacks.get(badId)!({payload:{}}), error=>error===failure);
  assert.ok(records.some(record=>record.kind==='event.handler_error' && record.data.event==='future_bad_handler'));
  bridge.unregisterCallback(id);
  assert.equal(callbacks.has(id),false);
});

test('schema snapshots remain bounded for large payloads and do not read serial content', () => {
  const raw = new Array(1000000).fill(65);
  const summary = diagnosticSummary({connected:true,bytes:1000000,raw,token:'PRIVATE_TOKEN',message:'PRIVATE_MESSAGE',futureArray:[{enabled:true}, {enabled:false}]});
  assert.ok(JSON.stringify(summary).length < 1000);
  assert.doesNotMatch(JSON.stringify(summary), /PRIVATE_/);
  assert.match(JSON.stringify(summary), /1000000/);
  let reads=0;const getter={get futureValue(){reads++;return 'PRIVATE_GETTER';}};
  diagnosticSummary(getter);assert.equal(reads,0,'Recording must not execute application getters');
});

test('new string states of equal length remain distinguishable and boolean controls are not hidden as content',()=>{
  assert.notDeepEqual(diagnosticSummary('newA'),diagnosticSummary('newB'));
  assert.equal(diagnosticSummary(true,0,'hexSend'),true);
  const summary = diagnosticSummary({value:'utf8'},0,'textEncoding') as {value:{value:string}};
  assert.equal(summary.value.value,'utf8');
});

test('future plugin Channel streams enter the same callback timeline and keep their handler',async()=>{
  let next=0,delivered=0;
  const callbacks=new Map<number,(value:unknown)=>unknown>();
  const records:{kind:string;data:any}[]=[];
  const bridge={async invoke<T>(_command:string,_args?:unknown):Promise<T>{return null as T;},transformCallback(fn?:(value:unknown)=>unknown){const id=++next;callbacks.set(id,fn!);return id;}};
  observeDiagnosticBridge(bridge,(kind,data)=>records.push({kind,data}));
  const id=bridge.transformCallback(()=>delivered++);
  const channel=Object.assign(Object.create(Channel.prototype),{id});
  await bridge.invoke('plugin:future|stream',{onEvent:channel});
  callbacks.get(id)!({index:0,message:{progress:75,text:'PRIVATE_STREAM'}});
  assert.equal(delivered,1);
  assert.ok(records.some(record=>record.kind==='event.received'&&record.data.event==='channel:plugin:future|stream:onEvent'));
  assert.doesNotMatch(JSON.stringify(records),/PRIVATE_/);
});
