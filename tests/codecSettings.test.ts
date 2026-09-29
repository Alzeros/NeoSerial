import test from 'node:test';
import assert from 'node:assert/strict';
import * as prefs from '../src/lib/codec.ts';
import { defaultCodecTool, initializeCodecToolDefaults } from '../src/lib/dataProcessing.ts';

test('默认编码兼容旧设置，显式恢复仅改变编码', () => {
  assert.equal(prefs.codecDefaultEncoding(), 'utf8');
  assert.equal(prefs.codecDefaultEncoding({ codec_default_encoding: 'gbk' }), 'gbk');
  assert.equal(prefs.codecDefaultEncoding({ codec_default_encoding: 'invalid' }), 'utf8');
  const config = { operation: 'escaped_to_hex', input: 'AT\\r\\n', format: 'text', encoding: 'utf8' } as const;
  assert.deepEqual(prefs.restoreCodecEncoding(config, 'utf16be'), { ...config, encoding: 'utf16be' });
});

test('下拉过滤隐藏方式，基础双向文本转换固定保留，当前草稿不变', () => {
  const menu = prefs.visibleCodecOperations(['text_to_hex', 'hex_to_text', 'base64_encode', 'hex_to_escaped'], 'base64_encode');
  assert.ok(menu.options.some(option => option.value === 'text_to_hex'));
  assert.ok(menu.options.some(option => option.value === 'hex_to_text'));
  assert.ok(!menu.options.some(option => option.value === 'base64_encode'));
  assert.ok(!menu.options.some(option => option.value === 'hex_to_escaped'));
  assert.equal(menu.currentLabel, 'Base64 编码（已隐藏）');
  assert.equal(prefs.visibleCodecOperations(['base64_encode'], 'text_to_hex').currentLabel, undefined);
});

test('新编解码仅首次应用默认值，旧工具与已经编辑的草稿保持不变', () => {
  const tool = defaultCodecTool();
  assert.equal(initializeCodecToolDefaults(tool, 'gbk'), true);
  assert.equal(tool.config.encoding, 'gbk');
  tool.config.input = '你好';
  assert.equal(initializeCodecToolDefaults(tool, 'utf16be'), false);
  assert.equal(tool.config.encoding, 'gbk');
  assert.equal(tool.config.input, '你好');
  const legacy = defaultCodecTool();
  delete legacy.defaults_applied;
  assert.equal(initializeCodecToolDefaults(legacy, 'gbk'), false);
  assert.equal(legacy.config.encoding, 'utf8');
  const edited = defaultCodecTool();
  edited.defaults_applied = true;
  edited.config.encoding = 'utf16le';
  assert.equal(initializeCodecToolDefaults(edited, 'gbk'), false);
  assert.equal(edited.config.encoding, 'utf16le');
});
