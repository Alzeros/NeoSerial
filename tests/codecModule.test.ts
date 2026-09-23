import test from 'node:test';
import assert from 'node:assert/strict';
import * as dp from '../src/lib/dataProcessing.ts';

const moduleOf = (tools: unknown[]) => ({ id: 'data_processing', type: 'data_processing', name: '数据处理', tools });
const presets = () => [moduleOf([dp.defaultFrameBuilderTool(), dp.defaultCodecTool()])];

test('新编解码工具默认使用 UTF-8 文本转 Hex', () => {
  assert.deepEqual(dp.defaultCodecTool(), {
    id: 'codec', kind: 'codec', config: { operation: 'text_to_hex', input: '', format: 'text' },
  });
});

test('旧数据处理配置补齐编解码，保留帧构造器和未知工具', () => {
  const frame = dp.defaultFrameBuilderTool();
  frame.config.header_hex = 'AA';
  const unknown = { id: 'future', kind: 'future', data: 'keep' };
  const merged = dp.mergePresetModules([moduleOf([frame, unknown])], [], presets());
  assert.equal(merged[0].tools.find(dp.isFrameBuilderTool).config.header_hex, 'AA');
  assert.deepEqual(merged[0].tools.find((tool: any) => tool.kind === 'future'), unknown);
  assert.ok(merged[0].tools.some(dp.isCodecTool));
  const twice = dp.mergePresetModules(JSON.parse(JSON.stringify(merged)), [], presets());
  assert.equal(twice[0].tools.filter(dp.isCodecTool).length, 1);
});

test('配置加载保留编解码输入；旧导入文件从当前配置补齐工具', () => {
  const codec = dp.defaultCodecTool();
  codec.config.input = '你好';
  codec.config.operation = 'base64_encode';
  const current = [moduleOf([dp.defaultFrameBuilderTool(), codec])];
  const merged = dp.mergePresetModules([moduleOf([dp.defaultFrameBuilderTool()])], current, presets());
  assert.deepEqual(merged[0].tools.find(dp.isCodecTool), codec);
  const loaded = dp.mergePresetModules(JSON.parse(JSON.stringify(merged)), [], presets());
  assert.deepEqual(loaded[0].tools.find(dp.isCodecTool), codec);
});

test('新增短信工具补齐旧配置，保留输入和所有已有工具', () => {
  const sms = dp.defaultSmsTool();
  assert.equal(sms.config.mode, 'parse');
  assert.equal(sms.config.encoding, 'auto');
  const allPresets = [moduleOf([...presets()[0].tools, sms])];
  const migrated = dp.mergePresetModules(presets(), [], allPresets);
  assert.equal(migrated[0].tools.length, 3);
  const stored = migrated[0].tools.find(dp.isSmsTool);
  stored.config.text = '你好';
  stored.config.recipient = '+12345678901';
  stored.config.validity_period = 170;
  const reloaded = dp.mergePresetModules(JSON.parse(JSON.stringify(migrated)), [], allPresets);
  assert.equal(reloaded[0].tools.filter(dp.isSmsTool).length, 1);
  assert.equal(reloaded[0].tools.find(dp.isSmsTool).config.text, '你好');
  assert.equal(reloaded[0].tools.find(dp.isSmsTool).config.validity_period, 170);
  assert.ok(reloaded[0].tools.some(dp.isCodecTool));
  assert.ok(reloaded[0].tools.some(dp.isFrameBuilderTool));
});

test('只有新短信工具首次使用默认值，重复初始化或旧草稿均不覆盖', () => {
  const defaults = {smsc:'+1234', encoding:'ucs2', validity_period:170} as const;
  const tool = dp.defaultSmsTool();
  assert.equal(dp.initializeSmsToolDefaults(tool, defaults), true);
  assert.equal(tool.config.smsc, '+1234');
  assert.equal(tool.config.encoding, 'ucs2');
  assert.equal(tool.config.validity_period, 170);
  tool.config.smsc = '+999';
  assert.equal(dp.initializeSmsToolDefaults(tool, defaults), false);
  assert.equal(tool.config.smsc, '+999');
  const legacy = dp.defaultSmsTool();
  delete legacy.defaults_applied;
  legacy.config.smsc = '+888';
  assert.equal(dp.initializeSmsToolDefaults(legacy, defaults), false);
  assert.equal(legacy.config.smsc, '+888');
});
