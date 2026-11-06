import test from 'node:test';
import assert from 'node:assert/strict';
import { withSettingsTimeout } from '../src/lib/settingsTimeout.ts';

test('settings deadlines retain successful values and original errors', async () => {
  assert.equal(await withSettingsTimeout(Promise.resolve(42), '读取设置', 100), 42);
  const failure = new Error('IPC failed');
  await assert.rejects(withSettingsTimeout(Promise.reject(failure), '读取设置', 100), error => error === failure);
});

test('unresolved settings IPC rejects with its stage and does not prevent a retry', async () => {
  await assert.rejects(withSettingsTimeout(new Promise(() => {}), '读取设置', 10), /读取设置.*超时/);
  assert.equal(await withSettingsTimeout(Promise.resolve('retry'), '读取设置', 100), 'retry');
});

test('a late response cannot settle a timed-out settings operation again', async () => {
  let release: (value: string) => void = () => {};
  const delayed = new Promise<string>(resolve => { release = resolve; });
  const operation = withSettingsTimeout(delayed, '打开设置窗口', 10);
  await assert.rejects(operation, /打开设置窗口.*超时/);
  release('late');
  await assert.rejects(operation, /打开设置窗口.*超时/);
});
