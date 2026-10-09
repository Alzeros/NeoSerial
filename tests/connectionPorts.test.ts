import test from 'node:test';
import assert from 'node:assert/strict';
import { normalizePorts, shouldPollPorts } from '../src/lib/connectionPorts.ts';

test('merges duplicate ports without losing names or changing enumeration order', () => {
  const ports = [
    { port: 'COM3', device: null },
    { port: 'COM3', device: 'USB serial device' },
    { port: 'COM8', device: 'Second device' },
    { port: 'COM3', device: 'Other name' },
  ];
  assert.deepEqual(normalizePorts(ports), [
    { port: 'COM3', device: 'USB serial device' },
    { port: 'COM8', device: 'Second device' },
  ]);
  assert.equal(ports[0].device, null);
});

test('normalizes COM spelling and ignores empty port names', () => {
  assert.deepEqual(normalizePorts([
    { port: ' com3 ', device: ' ' },
    { port: 'COM3', device: ' Device ' },
    { port: ' ', device: null },
    { port: '/dev/ttyUSB0', device: null },
  ]), [{ port: 'COM3', device: 'Device' }, { port: '/dev/ttyUSB0', device: null }]);
  assert.deepEqual(normalizePorts([]), []);
});

test('keeps polling when an already-connected window has no GUI port list yet', () => {
  assert.equal(shouldPollPorts(true, []), true);
});

test('stops the timer once a connected window has a port list', () => {
  assert.equal(shouldPollPorts(true, [{ port: 'COM3' }, { port: 'COM4' }]), false);
});

test('keeps polling while disconnected for hot-plug detection', () => {
  assert.equal(shouldPollPorts(false, [{ port: 'COM3' }]), true);
});
