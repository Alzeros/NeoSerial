import test from 'node:test';
import assert from 'node:assert/strict';
import { shouldPollPorts } from '../src/lib/connectionPorts.ts';

test('keeps polling when an already-connected window has no GUI port list yet', () => {
  assert.equal(shouldPollPorts(true, []), true);
});

test('stops the timer once a connected window has a port list', () => {
  assert.equal(shouldPollPorts(true, [{ port: 'COM3' }, { port: 'COM4' }]), false);
});

test('keeps polling while disconnected for hot-plug detection', () => {
  assert.equal(shouldPollPorts(false, [{ port: 'COM3' }]), true);
});
