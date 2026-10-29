import test from 'node:test';
import assert from 'node:assert/strict';
import { matchesLog, snippetLines, elapsedMilliseconds, formatDiagnosticSnippet } from '../src/lib/logDiagnostics.ts';

const row = (index: number, ascii = `record ${index}`, extra = {}) => ({
  ts: `12:00:00.${String(index).padStart(3, '0')}`, ts_ms: 1000 + index,
  dir: 'rx' as const, ascii, raw: [...new TextEncoder().encode(ascii)],
  line_index: index, is_error: false, ...extra,
});
const filter = { direction: 'all' as const, errorsOnly: false, include: '', exclude: '' };

test('filters use literal case-insensitive matching and compose direction, errors and exclusion', () => {
  const record = row(1, '[OK] +CME ERROR', { is_error: true });
  assert.equal(matchesLog(record, { ...filter, include: '[ok]' }, record.ascii), true);
  assert.equal(matchesLog(record, { ...filter, include: '.*' }, record.ascii), false);
  assert.equal(matchesLog(record, { ...filter, direction: 'tx' }, record.ascii), false);
  assert.equal(matchesLog(record, { ...filter, direction: 'rx', errorsOnly: true, include: 'cme', exclude: '[ok]' }, record.ascii), false);
  assert.equal(matchesLog(record, { ...filter, errorsOnly: true }, record.ascii), true);
  assert.equal(matchesLog(row(2), { ...filter, errorsOnly: true }, 'record 2'), false);
  assert.equal(matchesLog(record, { ...filter, include: '解码' }, '已解码'), true);
});

test('metadata-only filters never request text conversion and keywords render only eligible rows', () => {
  let conversions = 0;
  const render = () => { conversions++; return 'ERROR'; };
  assert.equal(matchesLog(row(1), { ...filter, direction: 'rx' }, render), true);
  assert.equal(matchesLog(row(1), { ...filter, direction: 'tx', include: 'ERROR' }, render), false);
  assert.equal(matchesLog(row(1), { ...filter, errorsOnly: true, include: 'ERROR' }, render), false);
  assert.equal(conversions, 0);
  assert.equal(matchesLog(row(1), { ...filter, include: 'ERROR' }, render), true);
  assert.equal(conversions, 1);
});

test('context comes from retained chronological records rather than filtered results or line numbers', () => {
  const rows = Array.from({ length: 30 }, (_, index) => row(index % 5));
  assert.deepEqual(snippetLines(rows, [rows[12]], 2), rows.slice(10, 15));
  assert.deepEqual(snippetLines(rows, [rows[18], rows[12]], 1), rows.slice(11, 20));
  assert.deepEqual(snippetLines(rows, [], 10), rows);
  assert.deepEqual(snippetLines(rows, [rows[0]], 2), rows.slice(0, 3));
  assert.deepEqual(snippetLines(rows, [rows[29]], 2), rows.slice(27));
});

test('stale selections fail explicitly and context limits reject invalid amplification', () => {
  const rows = Array.from({ length: 30 }, (_, index) => row(index));
  assert.throws(() => snippetLines(rows, [row(12)], 2), /不在|淘汰/);
  assert.deepEqual(snippetLines(rows, [rows[12]], -5), [rows[12]]);
  assert.deepEqual(snippetLines(rows, [rows[12]], NaN), rows.slice(2, 23));
  assert.deepEqual(snippetLines(rows, [rows[12]], Infinity), rows.slice(2, 23));
});

test('elapsed time uses real timestamps across midnight and never fabricates missing timestamps', () => {
  assert.equal(elapsedMilliseconds(row(1, '', { ts_ms: 1727999999900 }), row(2, '', { ts_ms: 1728000000150 })), 250);
  assert.equal(elapsedMilliseconds(row(1), row(10)), 9);
  assert.equal(elapsedMilliseconds(row(10), row(1)), 9);
  assert.equal(elapsedMilliseconds(row(0, '', { ts_ms: 0 }), row(1)), null);
  assert.equal(elapsedMilliseconds(row(0, '', { ts_ms: NaN }), row(1)), null);
});

test('snippet carries explicit snapshot metadata, direction, timestamp, omissions and local markers', () => {
  const rows = [row(1, 'AT', { dir: 'tx' }), row(2, 'OK'), row(3, '日志内容过大，已省略 100 字节。', { omitted_bytes: 100, raw: [] })];
  const text = formatDiagnosticSnippet({
    rows, capturedAt: '2026-10-28T08:00:00.000Z', port: 'COM8',
    settings: '115200 / Eight / None / 1 / None', mode: 'ascii / utf8',
    scope: '选中记录及前后上下文', render: line => line.ascii,
    markers: new Map([[rows[1], '重启后\n首次响应']]),
  });
  assert.match(text, /COM8/);
  assert.match(text, /115200/);
  assert.match(text, /窗口当前设置/);
  assert.match(text, /历史可能跨连接/);
  assert.match(text, /2026-10-28T08:00:00.000Z/);
  assert.match(text, /12:00:00\.001.*Tx.*AT/);
  assert.match(text, /12:00:00\.002.*Rx.*OK/);
  assert.match(text, /标记.*重启后 首次响应/);
  assert.match(text, /已省略 100 字节/);
  assert.match(text, /选中记录及前后上下文/);
});

test('oversized text snippets fail before they can reach the clipboard or native bridge', () => {
  assert.throws(() => formatDiagnosticSnippet({
    rows: [row(1)], capturedAt: 'test', port: null, settings: 'test', mode: 'hex', scope: 'test',
    render: () => 'A'.repeat(32 * 1024 * 1024),
  }), /32 MiB/);
});
