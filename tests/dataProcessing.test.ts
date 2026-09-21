import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  defaultFrameBuilderTool,
  defaultFrameConfig,
  isDataProcessingModule,
  isQuickCommandsModule,
  mergePresetModules,
  addTemplate,
  removeTemplate,
} from '../src/lib/dataProcessing.ts';

const qc = (id = 'quick_commands') => ({ id, name: '快捷指令', type: 'quick_commands', pages: [{ name: 'Page0', commands: [] }] });
const dp = (tools: any[] = [defaultFrameBuilderTool()]) =>
  ({ id: 'data_processing', name: '数据处理', type: 'data_processing', tools });
/** 预置由调用方注入(去环):测试里显式造一份,顺序即结果顺序 */
const PRESETS = () => [
  { id: 'quick_commands', name: '快捷指令', type: 'quick_commands', pages: [{ name: 'Page0', commands: [] }] },
  dp(),
];

test('defaultFrameConfig 形状', () => {
  const c = defaultFrameConfig();
  assert.equal(c.length.size, 'none');
  assert.equal(c.checksum.algo, 'none');
  assert.equal(c.data.mode, 'content');
});

test('mergePresetModules: 老文件只有 quick_commands 追加 data_processing', () => {
  const merged = mergePresetModules([qc()] as any, [] as any, PRESETS() as any);
  assert.equal(merged.length, 2);
  assert.ok(isDataProcessingModule(merged[1]));
});

test('mergePresetModules: 已有两者保留用户内容', () => {
  const d = dp();
  (d.tools[0] as any).templates = [{ name: 'X', config: defaultFrameConfig() }];
  const merged = mergePresetModules([qc(), d] as any, [] as any, PRESETS() as any);
  assert.equal((merged[1] as any).tools[0].templates.length, 1);
});

test('mergePresetModules: 加载文件缺 data_processing 时从 fallback 补', () => {
  // fallback = 当前 store(含用户模板);加载的旧导出文件没有 data_processing
  const dpStore = dp();
  (dpStore.tools[0] as any).templates = [{ name: '我的帧', config: defaultFrameConfig() }];
  const merged = mergePresetModules([qc()] as any, [qc(), dpStore] as any, PRESETS() as any);
  assert.equal((merged[1] as any).tools[0].templates[0].name, '我的帧');
});

test('mergePresetModules: data_processing 缺 frame_builder 补默认', () => {
  const merged = mergePresetModules([qc(), dp([])] as any, [] as any, PRESETS() as any);
  assert.equal((merged[1] as any).tools.length, 1);
});

test('mergePresetModules: 加载文件顺序颠倒 → 按预置顺序归位', () => {
  const merged = mergePresetModules([dp(), qc()] as any, [] as any, PRESETS() as any);
  assert.equal((merged[0] as any).type, 'quick_commands');
  assert.equal((merged[1] as any).type, 'data_processing');
});

test('mergePresetModules: 未知 type 原样附在末尾(降级再升级不丢数据)', () => {
  const alien = { id: 'x', name: '未来模块', type: 'codec', tools: [{ kind: 'codec' }] };
  const merged = mergePresetModules([qc(), alien] as any, [] as any, PRESETS() as any);
  assert.equal(merged.length, 3);
  assert.equal((merged[2] as any).type, 'codec');
});

test('模板增删', () => {
  const tool = defaultFrameBuilderTool();
  addTemplate(tool, 'A');
  addTemplate(tool, 'B');
  assert.equal(tool.templates.length, 2);
  addTemplate(tool, 'A'); // 同名覆盖
  assert.equal(tool.templates.length, 2);
  removeTemplate(tool, 'A');
  assert.equal(tool.templates[0].name, 'B');
});

test('isQuickCommandsModule / isDataProcessingModule', () => {
  assert.ok(isQuickCommandsModule(qc() as any));
  assert.ok(!isDataProcessingModule(qc() as any));
});
