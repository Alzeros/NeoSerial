import assert from 'node:assert/strict';
import {withHeadlessBrowser} from './headless.mjs';

await withHeadlessBrowser(async({base,call,evaluate,until,delay})=>{
  await call('Page.navigate',{url:`${base}/tests/browser/replay-backpressure.html`});
  await until('window.pressureTest && pressureTest.attemptedBytes.length===1');
  assert.ok(await evaluate('pressureTest.attemptedBytes[0]>3*1024*1024'));
  await evaluate('pressureTest.fill()');
  assert.equal(await evaluate('pressureTest.logs.some(row=>row.kind==="replay.gap")'),false,'Pending queue alone remains below its byte limit');
  await evaluate('pressureTest.release()');
  await until('pressureTest.logs.some(row=>row.kind==="replay.gap"&&row.data.reason==="retry_queue_limit")');
  assert.ok(await evaluate('pressureTest.logs.find(row=>row.data?.reason==="retry_queue_limit").data.events_lost>=95'));
  await until('pressureTest.batches.some(batch=>batch.events.some(event=>event.type===2))');
  assert.ok(await evaluate('pressureTest.logs.some(row=>row.kind==="replay.write_resumed")'));
  assert.equal(await evaluate('pressureTest.logs.some(row=>row.kind==="replay.disabled")'),false);
  await evaluate('document.querySelector("#pressure").dataset.recovery="after-recovery"');
  await delay(50);await evaluate('pressureTest.replay.flush()');
  assert.ok(await evaluate('JSON.stringify(pressureTest.batches).includes("after-recovery")'));
  assert.ok(await evaluate('pressureTest.batches.every(batch=>batch.segment>1)'),'Recovery begins with a new checkpoint rather than orphaned deltas');
  await evaluate('pressureTest.replay.stop()');
  console.log('PASS replay backpressure: a delayed write failure cannot exceed the retry queue budget, and recording resumes from a new complete checkpoint');
});
