import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

test('page tab strip has a fixed compact height and only scrolls horizontally', () => {
  const source = readFileSync('src/components/QuickCommands.svelte', 'utf8');
  const start = source.indexOf('data-page-tabs');
  const openingTag = source.slice(start, source.indexOf('>', start));
  const rowStart = source.indexOf('data-page-tabs-row');
  const rowTag = source.slice(rowStart, source.indexOf('>', rowStart));

  assert.ok(start >= 0, 'page tab strip should have a stable selector');
  assert.match(openingTag, /h-\[41px\]/);
  assert.match(openingTag, /shrink-0/);
  assert.match(openingTag, /overflow-x-auto/);
  assert.match(openingTag, /overflow-y-hidden/);
  assert.doesNotMatch(openingTag, /py-2/);

  assert.ok(rowStart >= 0, 'page buttons should have a dedicated fixed-height row');
  assert.match(rowTag, /h-9/);
  assert.match(rowTag, /w-max/);
  assert.match(rowTag, /min-w-full/);
  assert.match(rowTag, /items-center/);
});

test('command list always reserves the vertical scrollbar gutter', () => {
  const css = readFileSync('src/app.css', 'utf8');
  const start = css.indexOf('.script-list {');
  const rule = css.slice(start, css.indexOf('}', start));

  assert.match(rule, /scrollbar-gutter:\s*stable;/);
});
