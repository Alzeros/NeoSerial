import test from 'node:test';
import assert from 'node:assert/strict';
import { convertCodec, codecSendPayload } from '../src/lib/codec.ts';

const convert = (operation: string, input: string, format = 'text') =>
  convertCodec({ operation, input, format } as Parameters<typeof convertCodec>[0]);

test('UTF-8 文本和 Hex 无损互转，保留中文、emoji、空白和 BOM', () => {
  const text = '\uFEFF你好🙂\r\n ';
  const encoded = convert('text_to_hex', text);
  assert.equal(encoded.value, 'EF BB BF E4 BD A0 E5 A5 BD F0 9F 99 82 0D 0A 20');
  assert.equal(encoded.byteLength, 16);
  assert.equal(encoded.format, 'hex');
  assert.equal(convert('hex_to_text', encoded.value).value, text);
});

test('Hex 接受连续字节和空白分隔，不接受半个字节或非法字符', () => {
  assert.equal(convert('hex_to_text', '41 4243\n44\t45').value, 'ABCDE');
  for (const input of ['4', '4 1', 'GG', '0x41', '41,42']) {
    assert.throws(() => convert('hex_to_text', input), /Hex/);
  }
});

test('标准 Base64 编解码已知向量', () => {
  for (const [raw, encoded] of [['', ''], ['f', 'Zg=='], ['fo', 'Zm8='], ['foo', 'Zm9v'], ['foobar', 'Zm9vYmFy'], ['你好', '5L2g5aW9']]) {
    assert.equal(convert('base64_encode', raw).value, encoded);
    assert.equal(convert('base64_decode', encoded).value, raw);
  }
});

test('Base64 支持二进制 Hex 输入和输出', () => {
  const encoded = convert('base64_encode', '00 FF 80 41', 'hex');
  assert.equal(encoded.value, 'AP+AQQ==');
  assert.equal(encoded.format, 'base64');
  const decoded = convert('base64_decode', encoded.value, 'hex');
  assert.deepEqual(decoded, { value: '00 FF 80 41', format: 'hex', byteLength: 4 });
});

test('Base64 可忽略空白、省略末尾补位，但拒绝非法字符和补位', () => {
  assert.equal(convert('base64_decode', ' Z m\n8\t').value, 'fo');
  for (const input of ['A', 'Zg=', 'Zg===', '=Zg=', 'Zg==AAAA', 'Z@==', 'Zh==', 'Zm9=', '_w==']) {
    assert.throws(() => convert('base64_decode', input), /Base64/);
  }
});

test('非法 UTF-8 不替换成乱码，Base64 解码仍可选择 Hex', () => {
  assert.throws(() => convert('hex_to_text', 'FF'), /UTF-8/);
  assert.throws(() => convert('base64_decode', '/w=='), /UTF-8/);
  assert.equal(convert('base64_decode', '/w==', 'hex').value, 'FF');
  assert.throws(() => convert('text_to_hex', '\uD800'), /Unicode/);
});

test('空输入有效，超长 Base64 不因参数展开导致栈溢出', () => {
  for (const operation of ['text_to_hex', 'hex_to_text', 'base64_encode', 'base64_decode']) {
    assert.equal(convert(operation, '').value, '');
  }
  const raw = '你好🙂'.repeat(20000);
  assert.equal(convert('base64_decode', convert('base64_encode', raw).value).value, raw);
});

test('填入格式区分 Hex 字节和 Base64 文本，非 ASCII 保持 UTF-8 字节', () => {
  assert.deepEqual(codecSendPayload(convert('text_to_hex', 'AB')), { text: '41 42', hex: true });
  assert.deepEqual(codecSendPayload(convert('base64_encode', 'AB')), { text: 'QUI=', hex: false });
  assert.deepEqual(codecSendPayload(convert('hex_to_text', '41 42')), { text: 'AB', hex: false });
  assert.deepEqual(codecSendPayload(convert('base64_decode', '5L2g5aW9')), { text: 'E4 BD A0 E5 A5 BD', hex: true });
  assert.deepEqual(codecSendPayload(convert('hex_to_text', '41 0D 0A')), { text: '41 0D 0A', hex: true });
  assert.deepEqual(codecSendPayload(convert('hex_to_text', '20 20')), { text: '20 20', hex: true });
});
