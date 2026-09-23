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

test('custom select can display a fallback label without adding it to menu options', () => {
  const select = read('src/components/ui/CustomSelect.svelte');

  assert.match(select, /selectedFallbackLabel\?: string/);
  assert.match(select, /\?\.label \?\? selectedFallbackLabel \?\? value/);
});

test('data processing settings edit and persist hidden option lists', () => {
  const dialog = read('src/components/SettingsDialog.svelte');

  assert.match(dialog, /hidden_data_fill_patterns \?\? \[\]/);
  assert.match(dialog, /hidden_checksum_algorithms \?\? \[\]/);
  assert.match(dialog, /ui\.hidden_data_fill_patterns = cur\.hiddenDataFillPatterns/);
  assert.match(dialog, /ui\.hidden_checksum_algorithms = cur\.hiddenChecksumAlgorithms/);
  assert.match(dialog, /title="数据域生成方式"/);
  assert.match(dialog, /title="校验算法"/);
  assert.match(dialog, /option\.value === 'content'/);
  assert.match(dialog, /option\.value === 'none'/);
});

test('frame builder filters source and checksum menus without rewriting current values', () => {
  const frameBuilder = read('src/components/FrameBuilder.svelte');

  assert.match(frameBuilder, /visibleDataSourceOptions\(/);
  assert.match(frameBuilder, /visibleChecksumOptions\(/);
  assert.match(frameBuilder, /hidden_data_fill_patterns \?\? \[\]/);
  assert.match(frameBuilder, /hidden_checksum_algorithms \?\? \[\]/);
  assert.match(frameBuilder, /options=\{sourceChoices\.options\}/);
  assert.match(frameBuilder, /selectedFallbackLabel=\{sourceChoices\.currentLabel\}/);
  assert.match(frameBuilder, /options=\{checksumChoices\.options\}/);
  assert.match(frameBuilder, /selectedFallbackLabel=\{checksumChoices\.currentLabel\}/);
});
