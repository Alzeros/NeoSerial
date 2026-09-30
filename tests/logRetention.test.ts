import test from 'node:test';
import assert from 'node:assert/strict';
import {LogRetentionBuffer, resolveLogLimits, LOG_MIB} from '../src/lib/logRetention.ts';

const row = (n: number, size: number) => ({raw: Array(size).fill(65), ascii: 'A'.repeat(size),
  ts: String(n), ts_ms: 0, dir: 'rx' as const, line_index: n, is_error: false});

test('old, missing and invalid limits resolve to bounded values', () => {
  assert.deepEqual(resolveLogLimits(), {lines: 5000, bytes: 4 * LOG_MIB});
  assert.deepEqual(resolveLogLimits({ring_buffer_capacity: 100000}), {lines: 10000, bytes: 4 * LOG_MIB});
  assert.deepEqual(resolveLogLimits({ring_buffer_capacity: 0, log_buffer_max_bytes: 0}), {lines: 1000, bytes: LOG_MIB});
  assert.deepEqual(resolveLogLimits({ring_buffer_capacity: NaN, log_buffer_max_bytes: '4MB'}), {lines: 5000, bytes: 4 * LOG_MIB});
  assert.deepEqual(resolveLogLimits({log_buffer_max_bytes: Infinity}), {lines: 5000, bytes: 4 * LOG_MIB});
  assert.equal(resolveLogLimits({log_buffer_max_bytes: 100 * LOG_MIB}).bytes, 16 * LOG_MIB);
});

test('byte and row limits independently evict oldest records, including exact boundary', () => {
  const buffer = new LogRetentionBuffer();
  buffer.append([row(1, 3), row(2, 4), row(3, 3)], {lines: 10, bytes: 10});
  assert.equal(buffer.rawBytes, 10);
  buffer.append([row(4, 1)], {lines: 10, bytes: 10});
  assert.deepEqual(buffer.lines.map(l => l.line_index), [2, 3, 4]);
  assert.equal(buffer.rawBytes, 8);
  buffer.trim({lines: 2, bytes: 10});
  assert.deepEqual(buffer.lines.map(l => l.line_index), [3, 4]);
  assert.equal(buffer.rawBytes, 4);
});

test('history insertion preserves chronological order and byte accounting', () => {
  const buffer = new LogRetentionBuffer();
  buffer.append([row(3, 3), row(4, 3)], {lines: 10, bytes: 10});
  const survivor = buffer.lines[0];
  buffer.insert(0, [row(1, 3), row(2, 3)], {lines: 10, bytes: 10});
  assert.deepEqual(buffer.lines.map(l => l.line_index), [2, 3, 4]);
  assert.equal(buffer.lines[1], survivor);
  assert.equal(buffer.rawBytes, 9);
  buffer.insert(1, [row(5, 2)], {lines: 10, bytes: 10});
  assert.deepEqual(buffer.lines.map(l => l.line_index), [5, 3, 4]);
  assert.equal(buffer.rawBytes, 8);
});

test('oversized payload becomes a metadata-preserving notice and does not mutate input', () => {
  const buffer = new LogRetentionBuffer();
  const oversized = row(2, 11);
  buffer.append([row(1, 3), oversized], {lines: 10, bytes: 10});
  assert.equal(buffer.lines.length, 2);
  assert.equal(buffer.lines[1].omitted_bytes, 11);
  assert.equal(buffer.lines[1].ts, '2');
  assert.equal(buffer.lines[1].dir, 'rx');
  assert.deepEqual(buffer.lines[1].raw, []);
  assert.match(buffer.lines[1].ascii, /已省略/);
  assert.equal(buffer.rawBytes, 3);
  assert.equal(oversized.raw.length, 11);
});

test('lowering byte limit omits an existing oversized row and prunes immediately', () => {
  const buffer = new LogRetentionBuffer();
  buffer.append([row(1, 3), row(2, 11), row(3, 4)], {lines: 10, bytes: 20});
  assert.equal(buffer.trim({lines: 10, bytes: 10}), true);
  assert.deepEqual(buffer.lines.map(l => l.line_index), [1, 2, 3]);
  assert.equal(buffer.lines[1].omitted_bytes, 11);
  assert.equal(buffer.rawBytes, 7);
  assert.equal(buffer.trim({lines: 10, bytes: 10}), false);
});

test('clear resets accounting and releases references for subsequent append', () => {
  const buffer = new LogRetentionBuffer();
  buffer.append([row(1, 8)], {lines: 10, bytes: 10});
  const sameArray = buffer.lines;
  buffer.clear();
  assert.equal(buffer.lines, sameArray);
  assert.equal(buffer.rawBytes, 0);
  buffer.append([row(2, 9)], {lines: 10, bytes: 10});
  assert.deepEqual(buffer.lines.map(l => l.line_index), [2]);
  assert.equal(buffer.rawBytes, 9);
});

test('large bursts remain bounded and zero-byte notices still obey the row limit', () => {
  const buffer = new LogRetentionBuffer();
  buffer.append(Array.from({length: 20000}, (_, i) => row(i, 1)), {lines: 10000, bytes: 7000});
  assert.equal(buffer.lines.length, 7000);
  assert.equal(buffer.rawBytes, 7000);
  assert.equal(buffer.lines[0].line_index, 13000);
  buffer.clear();
  buffer.append(Array.from({length: 20}, (_, i) => row(i, 11)), {lines: 5, bytes: 10});
  assert.equal(buffer.lines.length, 5);
  assert.equal(buffer.rawBytes, 0);
});
