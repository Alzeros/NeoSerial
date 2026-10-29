import assert from 'node:assert/strict';
import test from 'node:test';
import { activityPort, activitySummary, acceptActivitySnapshot, type McpActivityCall, type McpActivitySnapshot } from '../src/lib/mcpActivity.ts';

const call = (id: number, port: string | null, tool = 'send_and_read', ok: boolean | null = null): McpActivityCall => ({
  id, port, tool, started_at_ms: 1000, finished_at_ms: ok === null ? null : 2000, ok,
});
const snapshot = (active: McpActivityCall[] = [], recent: McpActivityCall[] = [], revision = 1): McpActivitySnapshot => ({ revision, active, recent });

test('disconnected windows ignore stale currentPort and follow their binding or selection', () => {
  assert.equal(activityPort(true, 'COM1', null, 'COM2'), 'COM1');
  assert.equal(activityPort(false, 'COM1', null, 'COM2'), 'COM2');
  assert.equal(activityPort(false, 'COM1', null, ''), null);
  assert.equal(activityPort(false, 'COM1', 'COM3', 'COM2'), 'COM3');
});

test('activity is scoped to the exact port and never infers ownership or client presence', () => {
  const view = activitySummary(snapshot([call(1, 'COM2')], [call(2, 'COM2', 'send', true)]), 'COM1', 5000);
  assert.equal(view.tone, 'idle');
  assert.match(view.text, /暂无执行中的调用/);
  assert.doesNotMatch(view.text, /COM2|控制|已连接|思考/);
  assert.match(view.detail, /COM1/);
});

test('concurrent calls count only the current port and use the oldest active call elapsed time', () => {
  const view = activitySummary(snapshot([call(1, 'COM1'), { ...call(2, 'COM1', 'send_file'), started_at_ms: 2000 }, call(3, 'COM2')]), 'COM1', 5500);
  assert.equal(view.tone, 'active');
  assert.match(view.text, /2 项/);
  assert.match(view.text, / · 4s$/);
  assert.match(view.detail, /等待串口响应/);
  assert.match(view.detail, /发送文件/);
  assert.doesNotMatch(view.detail, /COM2/);
});

test('active elapsed time uses whole seconds without rounding up', () => {
  const state = snapshot([call(1, 'COM1')]);
  for (const [elapsed, seconds] of [[-1, 0], [0, 0], [499, 0], [500, 0], [999, 0], [1000, 1], [1500, 1], [1999, 1], [19200, 19], [19999, 19], [20000, 20]]) {
    const view = activitySummary(state, 'COM1', 1000 + elapsed);
    assert.match(view.text, new RegExp(` · ${seconds}s$`));
    assert.doesNotMatch(view.text, /\d+\.\d+s/);
  }
});

test('tooltip details contain only scope and concise call status', () => {
  assert.equal(activitySummary(snapshot(), 'COM1', 5000).detail, '当前端口：COM1');
  assert.equal(activitySummary(snapshot(), null, 5000).detail, '应用级');
  assert.equal(activitySummary(snapshot([call(1, 'COM1')]), 'COM1', 5000).detail, '当前端口：COM1\n发送并等待串口响应');
  assert.equal(activitySummary(snapshot([call(1, 'COM1'), call(2, 'COM1', 'send_file')]), 'COM1', 5000).detail, '当前端口：COM1\n发送并等待串口响应、发送文件');
  assert.equal(activitySummary(snapshot([], [call(1, 'COM1', 'send', true)]), 'COM1', 5000).detail, '当前端口：COM1\n发送已提交');
  assert.equal(activitySummary(snapshot([], [call(1, 'COM1', 'sequence_run', true)]), 'COM1', 5000).detail, '当前端口：COM1\n启动序列调用已完成');
});

test('no selected port shows only explicitly application-scoped operations', () => {
  const view = activitySummary(snapshot([call(1, 'COM1'), call(2, null, 'get_settings')]), null, 5000);
  assert.match(view.text, /应用级/);
  assert.match(view.text, /读取配置/);
  assert.doesNotMatch(view.text, /等待串口响应/);
});

test('completed sends and sequences do not claim device success or sequence completion', () => {
  for (const [tool, expected] of [['send', /已提交/], ['sequence_run', /启动序列调用已完成/], ['send_file', /写入驱动/]]) {
    const view = activitySummary(snapshot([], [call(1, 'COM1', tool, true)]), 'COM1', 12000);
    assert.match(view.text, expected);
    assert.match(view.text, /10 秒前/);
    assert.doesNotMatch(view.text, /设备.*成功|序列执行完成/);
    assert.match(view.detail, expected);
    assert.doesNotMatch(view.detail, /设备.*成功|序列执行完成/);
  }
});

test('failure, unknown tools, clock skew and idle states have bounded honest labels', () => {
  assert.equal(activitySummary(snapshot([], [call(1, 'COM1', 'send', false)]), 'COM1', 1000).tone, 'error');
  assert.match(activitySummary(snapshot([], [call(1, 'COM1', 'send', false)]), 'COM1', 1000).text, /失败.*0 秒前/);
  assert.match(activitySummary(snapshot([call(1, 'COM1', 'unexpected')]), 'COM1', 0).text, /工具调用 · 0s$/);
  assert.equal(activitySummary(snapshot(), null, 0).tone, 'idle');
});

test('stale event or initial snapshots cannot overwrite a newer revision', () => {
  const current = snapshot([], [call(1, 'COM1', 'send', true)], 4);
  assert.equal(acceptActivitySnapshot(current, snapshot([call(1, 'COM1')], [], 2)), current);
  assert.equal(acceptActivitySnapshot(current, current), current);
  assert.equal(acceptActivitySnapshot(null, current), current);
  assert.equal(acceptActivitySnapshot(current, snapshot([], [], 5)).revision, 5);
});
