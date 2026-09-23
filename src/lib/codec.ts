/** 编解码只处理字节/UTF-8，不依赖串口连接或全局显示编码。 */
export type CodecOperation = 'text_to_hex' | 'hex_to_text' | 'base64_encode' | 'base64_decode';
export interface CodecConfig {
  operation: CodecOperation;
  input: string;
  /** Base64 编码的输入格式 / 解码的输出格式。 */
  format: 'text' | 'hex';
}
export interface CodecResult {
  value: string;
  format: 'text' | 'hex' | 'base64';
  byteLength: number;
}

export const CODEC_OPERATIONS: { value: CodecOperation; label: string }[] = [
  { value: 'text_to_hex', label: '文本 → Hex' },
  { value: 'hex_to_text', label: 'Hex → 文本' },
  { value: 'base64_encode', label: 'Base64 编码' },
  { value: 'base64_decode', label: 'Base64 解码' },
];

function utf8Bytes(text: string): Uint8Array {
  // TextEncoder 会默默替换孤立代理项；工具应报错，不能无声损坏输入。
  if (/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(text)) {
    throw new Error('文本含不完整的 Unicode 字符，请检查输入。');
  }
  return new TextEncoder().encode(text);
}

function hexBytes(input: string): Uint8Array {
  const groups = input.trim().split(/\s+/u).filter(Boolean);
  if (groups.some((group) => !/^(?:[\da-f]{2})+$/iu.test(group))) {
    throw new Error('Hex 需由完整的两位字节组成，可连续输入或用空白分隔，例如 41 42 FF。');
  }
  const hex = groups.join('');
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return bytes;
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0').toUpperCase()).join(' ');
}

function toText(bytes: Uint8Array): string {
  try {
    return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    throw new Error('数据不是有效的 UTF-8 文本；二进制数据请使用 Hex 格式。');
  }
}

function toBase64(bytes: Uint8Array): string {
  const chunks: string[] = [];
  for (let i = 0; i < bytes.length; i += 8192) {
    chunks.push(String.fromCharCode(...bytes.subarray(i, i + 8192)));
  }
  return btoa(chunks.join(''));
}

function base64Bytes(input: string): Uint8Array {
  const clean = input.replace(/\s/gu, '');
  const body = clean.replace(/=+$/u, '');
  if (!/^[A-Za-z0-9+/]*={0,2}$/u.test(clean) || body.length % 4 === 1 ||
      (clean.includes('=') && clean.length % 4 !== 0)) {
    throw new Error('Base64 格式无效，请检查字符和末尾的 = 补位。');
  }
  try {
    const binary = atob(clean);
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    // 校验被补位遮住的低位，拒绝非规范的同值编码。
    if (toBase64(bytes).replace(/=+$/u, '') !== body) throw new Error();
    return bytes;
  } catch {
    throw new Error('Base64 格式无效，请检查字符和末尾的 = 补位。');
  }
}

export function convertCodec(config: CodecConfig): CodecResult {
  switch (config.operation) {
    case 'text_to_hex': {
      const bytes = utf8Bytes(config.input);
      return { value: toHex(bytes), format: 'hex', byteLength: bytes.length };
    }
    case 'hex_to_text': {
      const bytes = hexBytes(config.input);
      return { value: toText(bytes), format: 'text', byteLength: bytes.length };
    }
    case 'base64_encode': {
      const bytes = config.format === 'hex' ? hexBytes(config.input) : utf8Bytes(config.input);
      const value = toBase64(bytes);
      return { value, format: 'base64', byteLength: value.length };
    }
    case 'base64_decode': {
      const bytes = base64Bytes(config.input);
      return { value: config.format === 'hex' ? toHex(bytes) : toText(bytes), format: config.format, byteLength: bytes.length };
    }
    default:
      throw new Error('请选择支持的编解码方式。');
  }
}

/** 文本经发送框还可能受全局编码、标点转换和单行输入限制影响。 */
export function codecSendPayload(result: CodecResult): { text: string; hex: boolean } {
  if (result.format === 'hex') return { text: result.value, hex: true };
  if (/^[\x20-\x7e]*$/u.test(result.value) && result.value.trim() === result.value) {
    return { text: result.value, hex: false };
  }
  return { text: toHex(utf8Bytes(result.value)), hex: true };
}
