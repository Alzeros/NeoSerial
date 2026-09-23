import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as dataProcessing from '../src/lib/dataProcessing.ts';
import {
  defaultFrameBuilderTool,
  defaultFrameConfig,
  isDataProcessingModule,
  isQuickCommandsModule,
  mergePresetModules,
  addTemplate,
  removeTemplate,
  loadTemplate,
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

test('加载旧整帧配置: 扣除帧头、长度字段、校验后保留数据长度,再次加载不重复扣减', () => {
  const tool = defaultFrameBuilderTool();
  tool.config.header_hex = 'AA\t55';
  tool.config.length.size = 'two';
  tool.config.length.coverage = 'whole_frame';
  tool.config.checksum.algo = 'crc16_modbus';
  tool.config.data.mode = 'fill';
  tool.config.data.length_basis = 'whole_frame';
  tool.config.data.length = 256;
  addTemplate(tool, '旧模板');
  const modules = mergePresetModules([qc(), dp([tool])], [], PRESETS());
  const migrated = modules[1].tools[0];
  assert.equal(migrated.config.data.length, 250);
  assert.equal(migrated.config.data.length_basis, 'data_field');
  assert.equal(migrated.config.length.coverage, 'whole_frame');
  assert.equal(migrated.templates[0].config.data.length, 250);
  const reloaded = mergePresetModules(JSON.parse(JSON.stringify(modules)), [], PRESETS());
  assert.equal(reloaded[1].tools[0].config.data.length, 250);
});

test('加载旧模板: SUM8、XOR8、CRC32 的宽度正确,编辑不修改模板原件', () => {
  for (const [algo, width] of [['none', 0], ['sum8', 1], ['xor8', 1], ['crc32', 4]] as const) {
    const tool = defaultFrameBuilderTool();
    const config = defaultFrameConfig();
    config.data.mode = 'fill';
    config.data.length_basis = 'whole_frame';
    config.data.length = 16;
    config.checksum.algo = algo;
    tool.templates.push({ name: 'legacy', config });
    loadTemplate(tool, 'legacy');
    assert.equal(tool.config.data.length, 16 - width);
    assert.equal(tool.config.data.length_basis, 'data_field');
    tool.config.data.length = 100;
    assert.equal(tool.templates[0].config.data.length, 16);
  }
});

test('旧模板无 random_text 配置时补齐,自定义字符池不混入默认字母数字', () => {
  const tool = defaultFrameBuilderTool();
  const legacy = defaultFrameConfig();
  legacy.data.mode = 'fill';
  legacy.data.pattern = 'random_text';
  legacy.data.charset = 'AB';
  delete (legacy.data as any).random_text;
  tool.templates.push({ name: 'AB', config: legacy });
  loadTemplate(tool, 'AB');
  assert.deepEqual(tool.config.data.random_text, {
    upper: false, lower: false, digits: false, special: true,
    special_chars: 'AB', min_digits: 0, min_special: 0,
  });
});

test('过渡版本同时存在 random_text 默认值和旧 charset 时迁移 charset', () => {
  const tool = defaultFrameBuilderTool();
  tool.config.data.pattern = 'random_text';
  tool.config.data.charset = 'AB';
  const modules = mergePresetModules([dp([tool])], [], PRESETS());
  const randomText = modules[1].tools[0].config.data.random_text;
  assert.equal(randomText.special, true);
  assert.equal(randomText.special_chars, 'AB');
  assert.equal(modules[1].tools[0].config.data.charset, '');
});

test('迁移不替换非法旧整帧配置,保留原值供构帧校验报错', () => {
  for (const header of ['AA 55', 'GG']) {
    const tool = defaultFrameBuilderTool();
    tool.config.header_hex = header;
    tool.config.data.mode = 'fill';
    tool.config.data.length_basis = 'whole_frame';
    tool.config.data.length = 1;
    const modules = mergePresetModules([dp([tool])], [], PRESETS());
    assert.equal(modules[1].tools[0].config.data.length, 1);
    assert.equal(modules[1].tools[0].config.data.length_basis, 'whole_frame');
  }
});

test('数据来源候选始终保留手动输入并过滤隐藏的填充模式', () => {
  const visible = dataProcessing.visibleDataSourceOptions(
    ['random_bytes'],
    'random_bytes',
  );

  assert.deepEqual(
    visible.options.map((option: { value: string }) => option.value),
    ['random_text', 'content', 'zeros', 'ff', 'increment', 'custom_loop'],
  );
  assert.equal(visible.currentLabel, '随机字节（已隐藏）');
});

test('校验候选始终保留无并过滤隐藏算法', () => {
  const visible = dataProcessing.visibleChecksumOptions(['crc32'], 'crc32');

  assert.equal(visible.options[0].value, 'none');
  assert.ok(!visible.options.some((option: { value: string }) => option.value === 'crc32'));
  assert.equal(visible.currentLabel, 'CRC32（已隐藏）');
});
