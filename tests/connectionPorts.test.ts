import test from 'node:test';
import assert from 'node:assert/strict';
import { shouldPollPorts } from '../src/lib/connectionPorts.ts';

test('keeps polling when an already-connected window has no GUI port list yet', () => {
  assert.equal(shouldPollPorts(true, []), true);
});

test('stops the timer once a connected window has a port list', () => {
  assert.equal(shouldPollPorts(true, ['COM3', 'COM4']), false);
});

test('keeps polling while disconnected for hot-plug detection', () => {
  assert.equal(shouldPollPorts(false, ['COM3']), true);
});
