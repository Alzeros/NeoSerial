import test from 'node:test';
import assert from 'node:assert/strict';
import * as dataProcessing from '../src/lib/dataProcessing.ts';

const breakdown = [
  { kind: 'header', offset: 0, len: 1 },
  { kind: 'data', offset: 1, len: 3 },
  { kind: 'checksum', offset: 4, len: 1 },
];

test('数据文本只提取数据域，不混入帧头和校验', () => {
  const result = dataProcessing.dataTextPreview(
    '55 41 42 43 99',
    breakdown,
  );

  assert.deepEqual(result, { text: 'ABC', totalBytes: 3, truncated: false });
});

test('数据文本按数据域自身的字节数截断', () => {
  const result = dataProcessing.dataTextPreview(
    '55 41 42 43 99',
    breakdown,
    2,
  );

  assert.deepEqual(result, { text: 'AB', totalBytes: 3, truncated: true });
});
