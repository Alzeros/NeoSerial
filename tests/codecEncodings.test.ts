import test from 'node:test';
import assert from 'node:assert/strict';
import { convertCodec, codecSendPayload, type CodecConfig } from '../src/lib/codec.ts';

const convert = (operation: string, input: string, encoding = 'utf8', format = 'text') =>
  convertCodec({ operation, input, encoding, format } as CodecConfig);

test('UTF-16 大小端已知字节、代理对、BOM 和空白无损往返', async () => {
  for (const [encoding, hex] of [
    ['utf16le', 'FF FE 60 4F 7D 59 3D D8 42 DE 0D 00 0A 00 20 00'],
    ['utf16be', 'FE FF 4F 60 59 7D D8 3D DE 42 00 0D 00 0A 00 20'],
  ]) {
    const text = '\uFEFF你好🙂\r\n ';
    assert.equal((await convert('text_to_hex', text, encoding)).value, hex);
    const decoded = await convert('hex_to_text', hex, encoding);
    assert.equal(decoded.value, text);
    assert.equal(decoded.byteLength, 16);
    assert.deepEqual(codecSendPayload(decoded), { text: hex, hex: true });
  }
  assert.equal((await convert('text_to_hex', 'A', 'utf16be')).value, '00 41', 'no implicit BOM');
});

test('UTF-16 拒绝截断字节和非法代理项，UTF-8 旧配置保持兼容', async () => {
  for (const hex of ['00', '00 D8', '00 DC', '00 D8 41 00']) {
    await assert.rejects(async () => convert('hex_to_text', hex, 'utf16le'), /UTF-16/);
  }
  for (const encoding of ['utf8', 'utf16le', 'utf16be', 'gbk']) {
    await assert.rejects(async () => convert('text_to_hex', '\uD800', encoding), /Unicode/);
  }
  assert.equal((await convertCodec({ operation: 'hex_to_text', input: 'E4 BD A0', format: 'text' })).value, '你');
});

test('GBK 解码中文和欧元，填入保留输入字节，拒绝非法序列', async () => {
  const text = await convert('hex_to_text', 'C4 E3 BA C3 80', 'gbk');
  assert.equal(text.value, '你好€');
  assert.deepEqual(codecSendPayload(text), { text: 'C4 E3 BA C3 80', hex: true });
  for (const hex of ['81', 'FF', '81 30 81 30']) {
    await assert.rejects(async () => convert('hex_to_text', hex, 'gbk'), /GBK/);
  }
});

test('Base64 文本路径使用所选编码，纯 Hex 路径保持字节语义', async () => {
  assert.equal((await convert('base64_encode', '你好', 'utf16be')).value, 'T2BZfQ==');
  assert.equal((await convert('base64_decode', 'T2BZfQ==', 'utf16be')).value, '你好');
  assert.equal((await convert('base64_decode', 'xOO6ww==', 'gbk')).value, '你好');
  assert.deepEqual(codecSendPayload(await convert('base64_decode', 'QQA=', 'utf16le')), { text: '41 00', hex: true });
  assert.equal((await convert('base64_encode', '00 FF', 'gbk', 'hex')).value, 'AP8=');
});

test('转义混合普通文字与原始字节，UTF-16 普通字符和转义字节独立处理', async () => {
  const encoded = await convert('escaped_to_hex', String.raw`你好\r\n\xFF\\\0`);
  assert.equal(encoded.value, 'E4 BD A0 E5 A5 BD 0D 0A FF 5C 00');
  assert.equal((await convert('escaped_to_hex', String.raw`A\r\n\xFF`, 'utf16le')).value, '41 00 0D 0A FF');
  const bytes = await convert('hex_to_escaped', '41 0D 0A 00 09 5C FF');
  assert.equal(bytes.value, String.raw`A\r\n\0\t\\\xFF`);
  assert.equal(bytes.byteLength, 7);
  assert.deepEqual(codecSendPayload(bytes), { text: '41 0D 0A 00 09 5C FF', hex: true });
});

test('全部 256 种字节可经转义显示往返，字面反斜杠不被误解码', async () => {
  const all = Array.from({ length: 256 }, (_, i) => i.toString(16).padStart(2, '0').toUpperCase()).join(' ');
  const escaped = await convert('hex_to_escaped', all);
  assert.equal((await convert('escaped_to_hex', escaped.value)).value, all);
  assert.equal((await convert('escaped_to_hex', String.raw`\\x41`)).value, '5C 78 34 31');
});

test('非法转义不静默修改或吞字节，空输入有效', async () => {
  for (const input of ['\\', '\\x', '\\xA', '\\xGG', '\\q', '\\u0041']) {
    await assert.rejects(async () => convert('escaped_to_hex', input), /转义/);
  }
  for (const operation of ['escaped_to_hex', 'hex_to_escaped']) {
    assert.equal((await convert(operation, '')).value, '');
  }
});
