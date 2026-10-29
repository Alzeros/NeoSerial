import test from 'node:test';
import assert from 'node:assert/strict';
import {
  MAX_NOTIFICATIONS,
  addNotification,
  normalizeError,
  type Notification,
} from '../src/lib/notificationModel.ts';

test('normalizes rejected strings and Error instances without losing diagnostic details', () => {
  assert.equal(normalizeError('COM3: access denied'), 'COM3: access denied');
  const error = new Error('write failed');
  assert.equal(normalizeError(error), error.stack);
  const noStack = new Error('permission denied');
  noStack.stack = undefined;
  assert.match(normalizeError(noStack), /permission denied/);
});

test('normalizes structured rejections as readable JSON, retaining context', () => {
  const error = { message: 'disk full', path: '/logs/session.log', code: 28 };
  assert.deepEqual(JSON.parse(normalizeError(error)), error);
  assert.equal(normalizeError(42), '42');
  assert.equal(normalizeError(false), 'false');
  assert.equal(normalizeError(123n), '123');
});

test('unknown, cyclic and non-serializable errors cannot break failure feedback', () => {
  for (const value of [undefined, null, '', '   ']) {
    assert.ok(normalizeError(value).trim().length > 0);
  }
  const cyclic: { message: string; self?: unknown; count: bigint } = { message: 'cycle', count: 10n };
  cyclic.self = cyclic;
  const text = normalizeError(cyclic);
  assert.match(text, /cycle/);
  assert.match(text, /10/);
  assert.match(text, /Circular/);
  assert.doesNotThrow(() => normalizeError({ toJSON() { throw new Error('broken serializer'); } }));
  assert.doesNotThrow(() => normalizeError(Object.create(null)));
});

test('repeated identical errors update one card and move it to the front', () => {
  let items: Notification[] = [];
  items = addNotification(items, { id: 1, kind: 'error', title: '连接失败', detail: 'busy' });
  items = addNotification(items, { id: 2, kind: 'error', title: '发送失败', detail: 'offline' });
  const before = items;
  items = addNotification(items, { id: 3, kind: 'error', title: '连接失败', detail: 'busy' });
  assert.equal(items.length, 2);
  assert.equal(items[0].id, 1);
  assert.equal(items[0].count, 2);
  assert.equal(items[1].id, 2);
  assert.equal(before[1].count, 1, 'Does not mutate the previous render snapshot');
});

test('title, details and severity are all part of the deduplication key', () => {
  let items: Notification[] = [];
  items = addNotification(items, { id: 1, kind: 'error', title: '操作', detail: 'one' });
  items = addNotification(items, { id: 2, kind: 'error', title: '操作', detail: 'two' });
  items = addNotification(items, { id: 3, kind: 'success', title: '操作', detail: 'two' });
  assert.equal(items.length, 3);
  assert.deepEqual(items.map(item => item.count), [1, 1, 1]);
  const differentTitle = addNotification(items, { id: 4, kind: 'success', title: '另一个操作', detail: 'two' });
  assert.equal(differentTitle[0].count, 1);
  assert.equal(differentTitle[1].title, '操作');
});

test('notifications stay bounded, replace the oldest, and use no expiration timers', () => {
  let items: Notification[] = [];
  for (let index = 1; index <= 1000; index++) {
    items = addNotification(items, { id: index, kind: 'error', title: `failure ${index}`, detail: 'disk full' });
    assert.ok(items.length <= MAX_NOTIFICATIONS);
  }
  assert.equal(MAX_NOTIFICATIONS, 3);
  assert.deepEqual(items.map(item => item.id), [1000, 999, 998]);
  assert.equal(items[0].count, 1);
  assert.equal('expiresAt' in items[0], false);
});
