import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

test('expanded frame result body opts into text selection', () => {
  const source = readFileSync('src/components/FrameBuilder.svelte', 'utf8');
  const start = source.indexOf('{#if expanded && preview}');
  const expanded = source.slice(start, source.indexOf('  <div class="border-t', start));

  assert.match(expanded, /overflow-y-auto[^\"]*select-text/);
});

test('data text mode renders only the extracted data field', () => {
  const source = readFileSync('src/components/FrameBuilder.svelte', 'utf8');

  assert.match(source, /dataTextPreview\(/);
  assert.match(source, />数据文本<\/button>/);
  assert.match(source, /previewMode === 'hex'/);
  assert.doesNotMatch(source, /previewMode === 'text' && s\.kind === 'data'/);
});
