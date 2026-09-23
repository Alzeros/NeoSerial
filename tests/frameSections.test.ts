import test from 'node:test';
import assert from 'node:assert/strict';
import * as dataProcessing from '../src/lib/dataProcessing.ts';

test('clearing a field in the same config does not close its enabled section', () => {
  const config = dataProcessing.defaultFrameConfig();
  config.header_hex = '55';
  const open = { header: true, length: false, checksum: false };

  config.header_hex = '';
  const next = (dataProcessing as any).resolveFrameSectionState(open, config, config);

  assert.deepEqual(next, open);
});

test('replacing config resyncs sections from the newly loaded template', () => {
  const previous = dataProcessing.defaultFrameConfig();
  const replacement = dataProcessing.defaultFrameConfig();
  replacement.length.size = 'two';

  const next = (dataProcessing as any).resolveFrameSectionState(
    { header: true, length: false, checksum: true },
    previous,
    replacement,
  );

  assert.deepEqual(next, { header: false, length: true, checksum: false });
});
