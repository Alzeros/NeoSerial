import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path: string) => readFileSync(path, 'utf8');

test('data processing extension setting is exposed and persisted', () => {
  const types = read('src/lib/types.ts');
  const dialog = read('src/components/SettingsDialog.svelte');
  const stores = read('src/lib/stores.svelte.ts');

  assert.match(types, /show_data_tab: boolean/);
  assert.match(stores, /extModule: 'suggest' \| 'mcp' \| 'quick' \| 'data' \| null/);
  assert.match(dialog, /extModule === 'data'/);
  assert.match(dialog, /ui\.show_data_tab = cur\.showDataTab/);
  assert.match(dialog, />数据处理</);
});

test('data processing tab follows its setting and links to extension settings', () => {
  const sequencer = read('src/components/ScriptSequencer.svelte');

  assert.match(sequencer, /const showDataTab = \$derived/);
  assert.match(sequencer, /!showDataTab && scriptView === 'data'/);
  assert.match(sequencer, /\{#if showDataTab\}[\s\S]*?>数据处理<\/button>[\s\S]*?\{\/if\}/);
  assert.match(sequencer, /module: 'data'/);
});
