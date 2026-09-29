/** 编解码使用本模块的编码选择，不依赖串口连接或全局显示编码。 */
export type CodecOperation = 'text_to_hex' | 'hex_to_text' | 'base64_encode' | 'base64_decode' | 'escaped_to_hex' | 'hex_to_escaped';
export type CodecEncoding = 'utf8' | 'gbk' | 'utf16le' | 'utf16be';
/** GBK 编码由原生层提供；多段转义输入也只需一次 IPC。 */
export type GbkEncoder = (texts: string[]) => Promise<number[][]>;
export interface CodecConfig {
  operation: CodecOperation;
  input: string;
  /** Base64 编码的输入格式 / 解码的输出格式。 */
  format: 'text' | 'hex';
  /** 老配置缺失时沿用 UTF-8。 */
  encoding?: CodecEncoding;
}
export interface CodecResult {
  value: string;
  format: 'text' | 'hex' | 'base64' | 'escaped';
  byteLength: number;
  /** 文本与转义展示的原始字节，填入时不重新编码。 */
  bytesHex?: string;
  encoding?: CodecEncoding;
}

export const CODEC_OPERATIONS: { value: CodecOperation; label: string }[] = [
  { value: 'text_to_hex', label: '文本 → Hex' },
  { value: 'hex_to_text', label: 'Hex → 文本' },
  { value: 'base64_encode', label: 'Base64 编码' },
  { value: 'base64_decode', label: 'Base64 解码' },
  { value: 'escaped_to_hex', label: '转义文本 → Hex' },
  { value: 'hex_to_escaped', label: 'Hex → 转义文本' },
];

export const CODEC_ENCODINGS: { value: CodecEncoding; label: string }[] = [
  { value: 'utf8', label: 'UTF-8' },
  { value: 'gbk', label: 'GBK' },
  { value: 'utf16le', label: 'UTF-16 LE' },
  { value: 'utf16be', label: 'UTF-16 BE' },
];

export function codecEncodingLabel(encoding: CodecEncoding = 'utf8'): string {
  return CODEC_ENCODINGS.find(option => option.value === encoding)?.label ?? encoding;
}

export function codecUsesEncoding(config: CodecConfig): boolean {
  return config.operation === 'text_to_hex' || config.operation === 'hex_to_text' ||
    config.operation === 'escaped_to_hex' ||
    (config.operation.startsWith('base64_') && config.format === 'text');
}

export function codecDefaultEncoding(ui?: { codec_default_encoding?: string }): CodecEncoding {
  return CODEC_ENCODINGS.find(option => option.value === ui?.codec_default_encoding)?.value ?? 'utf8';
}

export function restoreCodecEncoding(config: CodecConfig, encoding: CodecEncoding): CodecConfig {
  return { ...config, encoding };
}

export function isFixedCodecOperation(operation: string): boolean {
  return operation === 'text_to_hex' || operation === 'hex_to_text';
}

export function visibleCodecOperations(hidden: readonly string[], current: CodecOperation): {
  options: typeof CODEC_OPERATIONS;
  currentLabel?: string;
} {
  const options = CODEC_OPERATIONS.filter(option => isFixedCodecOperation(option.value) || !hidden.includes(option.value));
  const selected = CODEC_OPERATIONS.find(option => option.value === current);
  const currentLabel = !isFixedCodecOperation(current) && hidden.includes(current) && selected
    ? selected.label + '（已隐藏）' : undefined;
  return { options, currentLabel };
}

function validateText(text: string): void {
  // TextEncoder 会默默替换孤立代理项；工具应报错，不能无声损坏输入。
  if (/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(text)) {
    throw new Error('文本含不完整的 Unicode 字符，请检查输入。');
  }
}

async function encodeTexts(texts: string[], encoding: CodecEncoding, encodeGbk?: GbkEncoder): Promise<Uint8Array[]> {
  texts.forEach(validateText);
  if (encoding === 'gbk') {
    if (texts.length === 0) return [];
    if (!encodeGbk) throw new Error('GBK 编码需要原生编解码服务。');
    return (await encodeGbk(texts)).map(bytes => new Uint8Array(bytes));
  }
  return texts.map(text => {
    if (encoding === 'utf8') return new TextEncoder().encode(text);
    if (encoding !== 'utf16le' && encoding !== 'utf16be') throw new Error('请选择支持的文本编码。');
    // 按 UTF-16 码元写入；前面已验证代理对。BOM 仅在输入中存在时保留。
    const bytes = new Uint8Array(text.length * 2);
    const view = new DataView(bytes.buffer);
    for (let i = 0; i < text.length; i++) view.setUint16(i * 2, text.charCodeAt(i), encoding === 'utf16le');
    return bytes;
  });
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

function toText(bytes: Uint8Array, encoding: CodecEncoding): string {
  const label = codecEncodingLabel(encoding);
  try {
    // Web 标准的 GBK decoder 也接受 GB18030 四字节序列；本工具明确选择 GBK，
    // 因此先限制为 ASCII、0x80 欧元及合法的双字节结构。
    if (encoding === 'gbk') {
      for (let i = 0; i < bytes.length; i++) {
        const lead = bytes[i];
        if (lead <= 0x80) continue;
        const trail = bytes[++i];
        if (lead < 0x81 || lead > 0xfe || trail === undefined || trail < 0x40 || trail > 0xfe || trail === 0x7f) throw new Error();
      }
    }
    const decoderLabel = { utf8: 'utf-8', gbk: 'gbk', utf16le: 'utf-16le', utf16be: 'utf-16be' }[encoding];
    if (!decoderLabel) throw new Error();
    return new TextDecoder(decoderLabel, { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    throw new Error('数据不是有效的 ' + label + ' 文本；请检查编码，二进制数据可转为转义文本查看。');
  }
}

const ESCAPE_BYTES: Record<string, number> = {
  '\\': 0x5c, r: 0x0d, n: 0x0a, t: 0x09, '0': 0x00,
  b: 0x08, f: 0x0c, v: 0x0b, '"': 0x22, "'": 0x27,
};

async function escapedBytes(input: string, encoding: CodecEncoding, encodeGbk?: GbkEncoder): Promise<Uint8Array> {
  const parts: (string | number)[] = [];
  let start = 0;
  for (let i = 0; i < input.length; i++) {
    if (input[i] !== '\\') continue;
    if (i > start) parts.push(input.slice(start, i));
    const offset = i;
    const escape = input[++i];
    if (escape === 'x') {
      const hex = input.slice(i + 1, i + 3);
      if (!/^[\da-f]{2}$/iu.test(hex)) throw new Error('第 ' + (offset + 1) + ' 个字符处的转义无效：\\x 后需两位 Hex。');
      parts.push(Number.parseInt(hex, 16));
      i += 2;
    } else if (Object.hasOwn(ESCAPE_BYTES, escape)) {
      parts.push(ESCAPE_BYTES[escape]);
    } else {
      throw new Error('第 ' + (offset + 1) + ' 个字符处的转义无效：支持 \\\\、\\r、\\n、\\t、\\0、\\b、\\f、\\v、引号及 \\xHH。');
    }
    start = i + 1;
  }
  if (start < input.length) parts.push(input.slice(start));
  const texts = await encodeTexts(parts.filter((part): part is string => typeof part === 'string'), encoding, encodeGbk);
  const bytes = new Uint8Array(texts.reduce((sum, text) => sum + text.length, 0) + parts.filter(part => typeof part === 'number').length);
  let cursor = 0, textIndex = 0;
  for (const part of parts) {
    if (typeof part === 'number') bytes[cursor++] = part;
    else {
      const text = texts[textIndex++];
      bytes.set(text, cursor);
      cursor += text.length;
    }
  }
  return bytes;
}

function toEscaped(bytes: Uint8Array): string {
  const controls: Record<number, string> = { 0: '\\0', 8: '\\b', 9: '\\t', 10: '\\n', 11: '\\v', 12: '\\f', 13: '\\r', 92: '\\\\' };
  return Array.from(bytes, byte => controls[byte] ??
    (byte >= 0x20 && byte <= 0x7e ? String.fromCharCode(byte) : '\\x' + byte.toString(16).toUpperCase().padStart(2, '0'))).join('');
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

export async function convertCodec(config: CodecConfig, encodeGbk?: GbkEncoder): Promise<CodecResult> {
  const encoding = config.encoding ?? 'utf8';
  switch (config.operation) {
    case 'text_to_hex': {
      const [bytes] = await encodeTexts([config.input], encoding, encodeGbk);
      return { value: toHex(bytes), format: 'hex', byteLength: bytes.length };
    }
    case 'hex_to_text': {
      const bytes = hexBytes(config.input);
      return { value: toText(bytes, encoding), format: 'text', byteLength: bytes.length, bytesHex: toHex(bytes), encoding };
    }
    case 'base64_encode': {
      const bytes = config.format === 'hex' ? hexBytes(config.input) : (await encodeTexts([config.input], encoding, encodeGbk))[0];
      const value = toBase64(bytes);
      return { value, format: 'base64', byteLength: value.length };
    }
    case 'base64_decode': {
      const bytes = base64Bytes(config.input);
      if (config.format === 'hex') return { value: toHex(bytes), format: 'hex', byteLength: bytes.length };
      return { value: toText(bytes, encoding), format: 'text', byteLength: bytes.length, bytesHex: toHex(bytes), encoding };
    }
    case 'escaped_to_hex': {
      const bytes = await escapedBytes(config.input, encoding, encodeGbk);
      return { value: toHex(bytes), format: 'hex', byteLength: bytes.length };
    }
    case 'hex_to_escaped': {
      const bytes = hexBytes(config.input);
      return { value: toEscaped(bytes), format: 'escaped', byteLength: bytes.length, bytesHex: toHex(bytes) };
    }
    default:
      throw new Error('请选择支持的编解码方式。');
  }
}

/** 文本经发送框还可能受全局编码、标点转换和单行输入限制影响。 */
export function codecSendPayload(result: CodecResult): { text: string; hex: boolean } {
  if (result.format === 'hex') return { text: result.value, hex: true };
  if (result.format === 'escaped' || (result.encoding && result.encoding !== 'utf8')) {
    return { text: result.bytesHex ?? '', hex: true };
  }
  if (/^[\x20-\x7e]*$/u.test(result.value) && result.value.trim() === result.value) {
    return { text: result.value, hex: false };
  }
  return { text: result.bytesHex ?? toHex(new TextEncoder().encode(result.value)), hex: true };
}
