import test from 'node:test';
import assert from 'node:assert/strict';
import type {eventWithTime} from '@rrweb/types';
import {sanitizeReplayEvent} from '../src/lib/replayPrivacy.ts';

test('incremental attribute arrays preserve visibility removals, CSS deltas and node identity',()=>{
  const event={type:3,timestamp:123,data:{source:0,attributes:[{id:7,attributes:{hidden:null,style:{width:'340px',color:['red','important']},title:'PRIVATE_TITLE'}},{id:8,attributes:{'aria-expanded':'true',value:'PRIVATE_INPUT'}}],adds:[],removes:[],texts:[]}} as unknown as eventWithTime;
  const clean=sanitizeReplayEvent(event) as typeof event & {data:any};
  assert.ok(Array.isArray(clean.data.attributes));
  assert.equal(clean.data.attributes[0].id,7);
  assert.equal(clean.data.attributes[0].attributes.hidden,null);
  assert.deepEqual(clean.data.attributes[0].attributes.style,{width:'340px',color:['red','important']});
  assert.equal(clean.data.attributes[1].attributes['aria-expanded'],'true');
  assert.doesNotMatch(JSON.stringify(clean),/PRIVATE_/);
});

test('full snapshots retain styles and shape while masking text and removing resource/credential attributes',()=>{
  const event={type:2,timestamp:1,data:{node:{id:1,type:0,childNodes:[{id:2,type:2,tagName:'input',attributes:{id:'new-control',type:'password',value:'PRIVATE_PASSWORD',placeholder:'PRIVATE_PLACEHOLDER',style:'width:20px;background:url(https://example.com/PRIVATE_QUERY)'}},{id:3,type:3,textContent:'PRIVATE_TEXT'},{id:4,type:3,isStyle:true,textContent:'@import "https://example.com/PRIVATE_IMPORT";button{width:20px}'}]}}} as unknown as eventWithTime;
  const clean=JSON.stringify(sanitizeReplayEvent(event));
  assert.doesNotMatch(clean,/PRIVATE_/);
  assert.match(clean,/new-control/);assert.match(clean,/width:20px/);assert.match(clean,/•/);
});

test('new rendering attributes remain replayable without extending a whitelist per feature',()=>{
  const event={type:3,timestamp:1,data:{source:0,attributes:[{id:9,attributes:{'data-unseen-mode':'expanded','data-theme':'preset-3',dir:'rtl',colspan:'2','data-secret':'PRIVATE_SECRET',onclick:'PRIVATE_SCRIPT',srcdoc:'PRIVATE_INLINE_HTML'}}]}} as unknown as eventWithTime;
  const clean=sanitizeReplayEvent(event) as eventWithTime & {data:any};
  assert.equal(clean.data.attributes[0].attributes['data-unseen-mode'],'expanded');
  assert.equal(clean.data.attributes[0].attributes.dir,'rtl');assert.doesNotMatch(JSON.stringify(clean),/PRIVATE_/);
});
