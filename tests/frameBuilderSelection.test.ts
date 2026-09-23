import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

test('expanded frame result body opts into text selection', () => {
  const source = readFileSync('src/components/FrameBuilder.svelte', 'utf8');
  const start = source.indexOf('{#if expanded && preview}');
  const expanded = source.slice(start, source.indexOf('{/if}', start));

  assert.match(expanded, /overflow-y-auto[^\"]*select-text/);
});
