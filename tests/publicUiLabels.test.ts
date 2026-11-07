import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {publicUiLabels} from '../scripts/public-ui-labels.mjs';
test('only literal UI template labels become readable in recordings, including future components',async()=>{
  const directory=await mkdtemp(path.join(tmpdir(),'neoserial-label-catalog-'));
  try {
    await writeFile(path.join(directory,'Future.svelte'),`<script>const credential='PRIVATE_SCRIPT';let value='PRIVATE_VALUE';</script><button title="PRIVATE_ATTRIBUTE">全新功能</button><p>{value}</p><input bind:value />`);
    const labels=publicUiLabels(directory);
    assert.ok(labels.includes('全新功能'));
    assert.doesNotMatch(JSON.stringify(labels),/PRIVATE_/);
  } finally { await rm(directory,{recursive:true,force:true}); }
});
