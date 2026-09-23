import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

test('page tab strip has a fixed compact height and only scrolls horizontally', () => {
  const source = readFileSync('src/components/QuickCommands.svelte', 'utf8');
  const start = source.indexOf('data-page-tabs');
  const openingTag = source.slice(start, source.indexOf('>', start));

  assert.ok(start >= 0, 'page tab strip should have a stable selector');
  assert.match(openingTag, /h-11/);
  assert.match(openingTag, /shrink-0/);
  assert.match(openingTag, /items-start/);
  assert.match(openingTag, /pt-1/);
  assert.match(openingTag, /overflow-x-auto/);
  assert.match(openingTag, /overflow-y-hidden/);
  assert.doesNotMatch(openingTag, /py-2/);
});
