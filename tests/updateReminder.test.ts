import test from 'node:test';
import assert from 'node:assert/strict';
import { UpdateReminder, defaultUpdaterSettings, noticeVersion, checkDue, DAY } from '../src/lib/updateReminder.ts';

const now = 10 * DAY;
const cached = () => ({ ...defaultUpdaterSettings(), checked_version: '0.3.7', available_version: '0.3.8', last_check_at: now });

test('defaults on; daily checks are independent of reminder suppression', () => {
  assert.equal(defaultUpdaterSettings().auto_check, true);
  assert.equal(checkDue(cached(), '0.3.7', now + DAY - 1), false);
  assert.equal(checkDue(cached(), '0.3.7', now + DAY), true);
  assert.equal(checkDue({ ...cached(), auto_check: false }, '0.3.7', now + DAY), false);
  assert.equal(checkDue(cached(), '0.3.8', now), true);
  assert.equal(checkDue(cached(), '0.3.7', now - 1), true);
});

test('disabled, ignored and snoozed reminders stay hidden; newer versions can notify', () => {
  assert.equal(noticeVersion(cached(), '0.3.7', now), '0.3.8');
  assert.equal(noticeVersion({ ...cached(), auto_check: false }, '0.3.7', now), '');
  assert.equal(noticeVersion({ ...cached(), ignored_version: '0.3.8' }, '0.3.7', now), '');
  const snoozed = { ...cached(), snoozed_version: '0.3.8', snooze_until: now + 7 * DAY };
  assert.equal(noticeVersion(snoozed, '0.3.7', now), '');
  assert.equal(noticeVersion(snoozed, '0.3.7', now + 7 * DAY), '0.3.8');
  assert.equal(noticeVersion({ ...snoozed, available_version: '0.3.9' }, '0.3.7', now), '0.3.9');
  assert.equal(noticeVersion(cached(), '0.3.8', now), '');
});

function harness(automatic = true) {
  let prefs = defaultUpdaterSettings();
  let calls = 0;
  let resolveCheck: (value: { version: string; body: string } | null) => void = () => {};
  const manager = new UpdateReminder('0.3.7', automatic, {
    now: () => now,
    save: async patch => (prefs = { ...prefs, ...patch }),
    check: () => { calls++; return new Promise(resolve => { resolveCheck = resolve; }); },
    changed: () => {},
  });
  manager.sync(prefs);
  return { manager, calls: () => calls, resolve: () => resolveCheck({ version: '0.3.8', body: 'Changes' }) };
}

test('automatic requests are single-flight and disabled results cannot revive notices', async () => {
  const run = harness();
  const request = run.manager.runAutomatic();
  await Promise.resolve();
  await run.manager.runAutomatic();
  assert.equal(run.calls(), 1);
  await run.manager.update({ auto_check: false });
  run.resolve();
  await request;
  assert.equal(run.manager.settings.available_version, '');
  assert.equal(run.manager.settings.auto_check, false);
  run.manager.dispose();
});

test('secondary windows never automatically check', async () => {
  const run = harness(false);
  await run.manager.runAutomatic();
  assert.equal(run.calls(), 0);
  run.manager.dispose();
});

test('success persists cache; manual results bypass ignore and do not re-enable checking', async () => {
  const run = harness();
  const request = run.manager.runAutomatic();
  await Promise.resolve();
  run.resolve();
  await request;
  assert.equal(run.manager.settings.available_version, '0.3.8');
  await run.manager.update({ auto_check: false, ignored_version: '0.3.8' });
  await run.manager.record({ version: '0.3.9', body: 'New' });
  assert.equal(run.manager.settings.auto_check, false);
  assert.equal(run.manager.settings.available_version, '0.3.9');
  run.manager.dispose();
});

test('disposed automatic requests never publish results', async () => {
  const run = harness();
  const request = run.manager.runAutomatic();
  await Promise.resolve();
  run.manager.dispose();
  run.resolve();
  await request;
  assert.equal(run.manager.settings.available_version, '');
});

test('network errors are silent and attempts are throttled across launches', async () => {
  let saved = defaultUpdaterSettings();
  let calls = 0;
  const deps = {
    now: () => now,
    save: async (patch: Partial<typeof saved>) => (saved = { ...saved, ...patch }),
    check: async () => { calls++; throw Error('offline'); },
    changed: () => {},
  };
  const first = new UpdateReminder('0.3.7', true, deps);
  first.sync(saved);
  await first.runAutomatic();
  first.dispose();
  const second = new UpdateReminder('0.3.7', true, deps);
  second.sync(saved);
  await second.runAutomatic();
  assert.equal(calls, 1);
  second.dispose();
});

test('failed preference persistence restores the previous state', async () => {
  const manager = new UpdateReminder('0.3.7', false, {
    now: () => now, changed: () => {}, check: async () => null,
    save: async () => { throw Error('disk failure'); },
  });
  manager.sync(cached());
  await assert.rejects(manager.update({ auto_check: false }), /disk failure/);
  assert.equal(manager.settings.auto_check, true);
  manager.dispose();
});
