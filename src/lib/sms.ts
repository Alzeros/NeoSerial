/** SMS TPDU: 3GPP TS 23.040 / 23.038; AT 包装及 CMGS 长度: TS 27.005。
 * 输入/输出均包含 SMSC 长度字段；无 SMSC 时以 00 开头。
 */
export type SmsEncoding = 'gsm7' | 'ucs2' | '8bit';
export interface SmsGenerateConfig {
  recipient: string;
  smsc: string;
  text: string;
  encoding: 'auto' | 'gsm7' | 'ucs2';
  reference: number;
  /** 相对有效期的 TP-VP 字节；旧配置缺失或 null 表示不携带有效期。 */
  validity_period?: number | null;
}
export interface SmsConfig extends SmsGenerateConfig {
  mode: 'parse' | 'generate';
  pdu_input: string;
}
export interface SmsDefaults {
  smsc: string;
  encoding: SmsGenerateConfig['encoding'];
  validity_period: number | null;
}
export const SMS_ENCODING_OPTIONS = [
  {value:'auto', label:'自动'}, {value:'gsm7', label:'GSM 7-bit'}, {value:'ucs2', label:'UCS-2（中文）'},
];
export const SMS_VALIDITY_OPTIONS = [
  {value:'none', label:'不指定'}, {value:'11', label:'1 小时'}, {value:'71', label:'6 小时'},
  {value:'143', label:'12 小时'}, {value:'167', label:'1 天'}, {value:'168', label:'2 天'},
  {value:'170', label:'4 天'}, {value:'173', label:'7 天'}, {value:'196', label:'30 天'},
];

export function smsDefaultsFromSettings(ui?: {
  sms_default_smsc?: string;
  sms_default_encoding?: SmsDefaults['encoding'];
  sms_default_validity_period?: number | null;
}): SmsDefaults {
  return { smsc: ui?.sms_default_smsc?.trim() ?? '', encoding: ui?.sms_default_encoding ?? 'auto',
    validity_period: ui?.sms_default_validity_period ?? null };
}

/** 仅覆盖默认参数，不修改接收号码、正文、参考号、解析输入和工作模式。 */
export function restoreSmsDefaults(config: SmsConfig, defaults: SmsDefaults): SmsConfig {
  return { ...config, smsc: defaults.smsc, encoding: defaults.encoding, validity_period: defaults.validity_period };
}
export interface SmsConcat { reference: number; bits: 8 | 16; total: number; sequence: number }
export interface SmsMessage {
  pdu: string;
  type: 'SMS-SUBMIT' | 'SMS-DELIVER';
  peer: string;
  smsc: string;
  timestamp: string | null;
  encoding: SmsEncoding;
  text: string | null;
  dataHex: string;
  firstOctet: number;
  pid: number;
  dcs: number;
  udl: number;
  tpduLength: number;
  messageReference: number | null;
  validity: string | null;
  udh: { id: number; dataHex: string }[];
  concat: SmsConcat | null;
}
export interface SmsGenerated {
  encoding: 'gsm7' | 'ucs2';
  parts: { pdu: string; tpduLength: number; text: string; sequence: number; total: number }[];
}
export interface SmsAssembly {
  peer: string;
  reference: number;
  total: number;
  received: number;
  encoding: SmsEncoding;
  complete: boolean;
  text: string | null;
  dataHex: string | null;
  missing: number[];
  conflicts: number[];
}

const GSM = '@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞ\x1bÆæßÉ !"#¤%&\'()*+,-./0123456789:;<=>?¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà';
const EXT: Record<number, string> = { 10:'\f', 20:'^', 40:'{', 41:'}', 47:'\\', 60:'[', 61:'~', 62:']', 64:'|', 101:'€' };
const GSM_CODES = new Map<string, readonly number[]>(Array.from(GSM, (char, code) => [char, [code]] as const).filter(([char]) => char !== '\x1b'));
for (const [code, char] of Object.entries(EXT)) GSM_CODES.set(char, [27, Number(code)]);
const hex = (bytes: readonly number[]) => bytes.map(byte => byte.toString(16).padStart(2, '0').toUpperCase()).join('');

function readHex(input: string): number[] {
  const compact = input.replace(/\s/gu, '');
  if (!compact || !/^(?:[\da-f]{2})+$/iu.test(compact)) throw new Error('PDU 必须是完整的 Hex 字节，不能含奇数位或非 Hex 字符。');
  return compact.match(/../gu)!.map(byte => Number.parseInt(byte, 16));
}

function packSeptets(codes: readonly number[], header: number[] = []): number[] {
  const offset = Math.ceil(header.length * 8 / 7) * 7;
  const bytes = Array<number>(Math.ceil((offset + codes.length * 7) / 8)).fill(0);
  header.forEach((byte, i) => (bytes[i] = byte));
  codes.forEach((code, i) => {
    const bit = offset + i * 7;
    bytes[bit >> 3] |= (code << (bit % 8)) & 255;
    if (bit % 8 > 1) bytes[(bit >> 3) + 1] |= code >> (8 - bit % 8);
  });
  return bytes;
}

function unpackSeptets(bytes: number[], count: number, offset = 0): string {
  if (offset + count * 7 > bytes.length * 8 || count < 0) throw new Error('GSM 7-bit 数据长度不足。');
  const chars: string[] = [];
  let escaped = false;
  for (let i = 0; i < count; i++) {
    const bit = offset + i * 7;
    const code = ((bytes[bit >> 3] >> (bit % 8)) | ((bytes[(bit >> 3) + 1] ?? 0) << (8 - bit % 8))) & 127;
    if (escaped) {
      if (!(code in EXT)) throw new Error('包含暂不支持的 GSM 7-bit 扩展字符。');
      chars.push(EXT[code]);
      escaped = false;
    } else if (code === 27) escaped = true;
    else chars.push(GSM[code]);
  }
  if (escaped) throw new Error('GSM 7-bit 扩展字符被截断。');
  return chars.join('');
}

function ucs2Bytes(text: string): number[] {
  const bytes: number[] = [];
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdfff) throw new Error('UCS-2 不支持 emoji 等增补字符或不完整的 Unicode 字符，请修改正文。');
    bytes.push(code >> 8, code & 255);
  }
  return bytes;
}

function ucs2Text(bytes: number[]): string {
  if (bytes.length % 2) throw new Error('UCS-2 正文需要偶数字节。');
  const chars: string[] = [];
  for (let i = 0; i < bytes.length; i += 2) {
    const code = bytes[i] * 256 + bytes[i + 1];
    if (code >= 0xd800 && code <= 0xdfff) throw new Error('正文包含 UCS-2 不支持的代理项，无法按 UCS-2 解码。');
    chars.push(String.fromCharCode(code));
  }
  return chars.join('');
}

function encodeAddress(value: string, label: string): { digits: number; bytes: number[] } {
  const clean = value.trim();
  if (!/^\+?\d{1,20}$/u.test(clean)) throw new Error(`${label}需为 1–20 位数字，可用 + 表示国际号码。`);
  const digits = clean.replace(/^\+/u, '');
  const padded = digits.length % 2 ? digits + 'F' : digits;
  const bytes = [clean.startsWith('+') ? 0x91 : 0x81];
  for (let i = 0; i < padded.length; i += 2) bytes.push(Number.parseInt(padded[i + 1] + padded[i], 16));
  return { digits: digits.length, bytes };
}

function decodeAddress(bytes: number[], toa: number, digits?: number): string {
  if (!(toa & 128)) throw new Error('地址类型字段无效。');
  if ((toa & 0x70) === 0x50) {
    if (digits === undefined) throw new Error('短信中心地址不支持字母格式。');
    return unpackSeptets(bytes, Math.floor(digits * 4 / 7));
  }
  const alphabet = '0123456789*#abc';
  const nibbles = bytes.flatMap(byte => [byte & 15, byte >> 4]);
  const count = digits ?? (nibbles.at(-1) === 15 ? nibbles.length - 1 : nibbles.length);
  if (!count || count > 20 || nibbles.slice(0, count).some(n => n === 15) ||
      (count % 2 && nibbles[count] !== 15)) throw new Error('号码的半字节编码或长度无效。');
  return ((toa & 0x70) === 0x10 ? '+' : '') + nibbles.slice(0, count).map(n => alphabet[n]).join('');
}

function timestamp(bytes: number[]): string {
  const bcd = (byte: number) => {
    if ((byte & 15) > 9 || (byte >> 4) > 9) throw new Error('时间字段不是有效的 BCD。');
    return (byte & 15) * 10 + (byte >> 4);
  };
  const [year, month, day, hour, minute, second] = bytes.slice(0,6).map(bcd);
  const zone = bcd(bytes[6] & 0xf7) * 15;
  const maxDay = new Date(Date.UTC(2000 + year, month, 0)).getUTCDate();
  if (month < 1 || month > 12 || day < 1 || day > maxDay || hour > 23 || minute > 59 || second > 59 || zone > 14 * 60) {
    throw new Error('短信时间或时区超出有效范围。');
  }
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${pad(year)}/${pad(month)}/${pad(day)} ${pad(hour)}:${pad(minute)}:${pad(second)} UTC${bytes[6] & 8 ? '-' : '+'}${pad(Math.floor(zone / 60))}:${pad(zone % 60)}`;
}

function dcsEncoding(dcs: number): SmsEncoding {
  if (dcs < 128) {
    if (dcs & 32) throw new Error('暂不支持压缩短信（DCS）。');
    const code = (dcs >> 2) & 3;
    if (code === 3) throw new Error('DCS 使用保留的编码值。');
    return (['gsm7', '8bit', 'ucs2'] as const)[code];
  }
  const group = dcs >> 4;
  if (group >= 12 && group <= 14 && !(dcs & 4)) return group === 14 ? 'ucs2' : 'gsm7';
  if (group === 15 && !(dcs & 8)) return dcs & 4 ? '8bit' : 'gsm7';
  throw new Error(`暂不支持 DCS 0x${hex([dcs])} 的编码。`);
}

export function parseSmsPdu(input: string): SmsMessage {
  const bytes = readHex(input);
  let cursor = 0;
  function take(count: number, field: string): number[] {
    if (cursor + count > bytes.length) throw new Error(`${field} 数据被截断（字节 ${cursor + 1}）。`);
    const data = bytes.slice(cursor, cursor + count);
    cursor += count;
    return data;
  }
  const byte = (field: string) => take(1, field)[0];
  const smscLength = byte('SMSC 长度');
  if (smscLength > 12) throw new Error('SMSC 长度无效；请输入包含 SMSC 长度字段的完整 PDU。');
  const smscData = take(smscLength, 'SMSC');
  const smsc = smscLength ? decodeAddress(smscData.slice(1), smscData[0]) : '';
  const tpduLength = bytes.length - cursor;
  const firstOctet = byte('TPDU 首字节');
  const mti = firstOctet & 3;
  if (mti !== 0 && mti !== 1) throw new Error('首版仅支持 SMS-DELIVER 和 SMS-SUBMIT，暂不支持状态报告等类型。');
  const type = mti === 0 ? 'SMS-DELIVER' : 'SMS-SUBMIT';
  const messageReference = mti === 1 ? byte('TP-MR') : null;
  const addressLength = byte('号码长度');
  if (!addressLength || addressLength > 20) throw new Error('号码长度应为 1–20 个半字节。');
  const toa = byte('号码类型');
  const peer = decodeAddress(take(Math.ceil(addressLength / 2), '号码'), toa, addressLength);
  const pid = byte('PID');
  const dcs = byte('DCS');
  const encoding = dcsEncoding(dcs);
  let time: string | null = null;
  let validity: string | null = null;
  if (mti === 0) time = timestamp(take(7, '短信时间'));
  else {
    const format = (firstOctet >> 3) & 3;
    if (format === 2) {
      const vp = byte('相对有效期');
      validity = vp <= 143 ? `${(vp+1)*5} 分钟` : vp <= 167 ? `${12*60+(vp-143)*30} 分钟` : vp <= 196 ? `${vp-166} 天` : `${vp-192} 周`;
    } else if (format === 3) validity = timestamp(take(7, '绝对有效期'));
    else if (format === 1) validity = `增强格式 ${hex(take(7, '增强有效期'))}`;
  }
  const udl = byte('UDL');
  const dataLength = encoding === 'gsm7' ? Math.ceil(udl * 7 / 8) : udl;
  if (dataLength > 140) throw new Error('短信用户数据超过 140 字节。');
  const userData = take(dataLength, '短信正文');
  if (cursor !== bytes.length) throw new Error('PDU 尾部存在多余字节，与 UDL 长度不符。');
  const udh: SmsMessage['udh'] = [];
  let concat: SmsConcat | null = null;
  let headerLength = 0;
  if (firstOctet & 64) {
    if (!userData.length) throw new Error('UDHI 已设置，但没有 UDH。');
    headerLength = userData[0] + 1;
    if (headerLength > userData.length) throw new Error('UDH 长度超过用户数据长度。');
    let at = 1;
    while (at < headerLength) {
      if (at + 2 > headerLength) throw new Error('UDH 信息元素被截断。');
      const id = userData[at++];
      const length = userData[at++];
      if (at + length > headerLength) throw new Error('UDH 信息元素长度无效。');
      const data = userData.slice(at, at + length);
      at += length;
      udh.push({ id, dataHex: hex(data) });
      if (id === 0 || id === 8) {
        if (concat || length !== (id === 0 ? 3 : 4)) throw new Error('长短信分段信息重复或长度无效。');
        concat = { bits: id === 0 ? 8 : 16, reference: id === 0 ? data[0] : data[0] * 256 + data[1], total: data.at(-2)!, sequence: data.at(-1)! };
        if (!concat.total || !concat.sequence || concat.sequence > concat.total) throw new Error('长短信分段序号或总数无效。');
      }
      if (encoding === 'gsm7' && (id === 0x24 || id === 0x25)) throw new Error('暂不支持国家语言切换表，未将其按默认 GSM7 误解码。');
    }
  }
  const data = userData.slice(headerLength);
  const headerSeptets = Math.ceil(headerLength * 8 / 7);
  const text = encoding === 'gsm7' ? unpackSeptets(userData, udl - headerSeptets, headerSeptets * 7)
    : encoding === 'ucs2' ? ucs2Text(data) : null;
  return { pdu: hex(bytes), type, peer, smsc, timestamp: time, encoding, text, dataHex: hex(data), firstOctet,
    pid, dcs, udl, tpduLength, messageReference, validity, udh, concat };
}

export function generateSms(config: SmsGenerateConfig): SmsGenerated {
  const address = encodeAddress(config.recipient, '接收号码');
  const sc = config.smsc.trim() ? encodeAddress(config.smsc, '短信中心号码').bytes : [];
  if (!Number.isInteger(config.reference) || config.reference < 0 || config.reference > 255) throw new Error('长短信参考号需为 0–255 的整数。');
  const validity = config.validity_period ?? null;
  if (validity !== null && (!Number.isInteger(validity) || validity < 0 || validity > 255)) {
    throw new Error('相对有效期需为 0–255 的整数，或选择不指定。');
  }
  const chars = Array.from(config.text);
  const gsmCompatible = chars.every(char => GSM_CODES.has(char));
  const encoding = config.encoding === 'auto' ? (gsmCompatible ? 'gsm7' : 'ucs2') : config.encoding;
  if (encoding !== 'gsm7' && encoding !== 'ucs2') throw new Error('请选择支持的短信编码。');
  if (encoding === 'gsm7' && !gsmCompatible) throw new Error('正文包含 GSM 7-bit 无法表示的字符，请使用自动或 UCS-2。');
  if (encoding === 'ucs2') ucs2Bytes(config.text);
  const units = chars.map(char => encoding === 'gsm7' ? GSM_CODES.get(char)!.length : 1);
  const totalUnits = units.reduce((sum, size) => sum + size, 0);
  const singleLimit = encoding === 'gsm7' ? 160 : 70;
  const partLimit = totalUnits <= singleLimit ? singleLimit : encoding === 'gsm7' ? 153 : 67;
  const parts: string[] = [];
  let current: string[] = [], size = 0;
  chars.forEach((char, index) => {
    if (size + units[index] > partLimit) { parts.push(current.join('')); current = []; size = 0; }
    current.push(char); size += units[index];
  });
  parts.push(current.join(''));
  if (parts.length > 255) throw new Error('长短信最多支持 255 段，请缩短正文。');
  return { encoding, parts: parts.map((text, index) => {
    const header = parts.length > 1 ? [5, 0, 3, config.reference, parts.length, index + 1] : [];
    const codes = encoding === 'gsm7' ? Array.from(text).flatMap(char => GSM_CODES.get(char)!) : [];
    const data = encoding === 'gsm7' ? packSeptets(codes, header) : [...header, ...ucs2Bytes(text)];
    const udl = encoding === 'gsm7' ? codes.length + Math.ceil(header.length * 8 / 7) : data.length;
    const firstOctet = (header.length ? 0x41 : 0x01) | (validity === null ? 0 : 0x10);
    const tpdu = [firstOctet, 0, address.digits, ...address.bytes, 0, encoding === 'gsm7' ? 0 : 8,
      ...(validity === null ? [] : [validity]), udl, ...data];
    return { pdu: hex([sc.length, ...sc, ...tpdu]), tpduLength: tpdu.length, text, sequence: index + 1, total: parts.length };
  }) };
}

export function assembleSms(messages: SmsMessage[]): SmsAssembly[] {
  const groups = new Map<string, SmsMessage[]>();
  for (const message of messages) {
    const c = message.concat;
    if (!c) continue;
    const key = JSON.stringify([message.type, message.peer, message.smsc, message.pid, message.dcs,
      c.bits, c.reference, c.total, message.udh.filter(ie => ie.id !== 0 && ie.id !== 8)]);
    const group = groups.get(key) ?? [];
    group.push(message);
    groups.set(key, group);
  }
  return [...groups.values()].map(group => {
    const first = group[0];
    const { total, reference } = first.concat!;
    const parts = new Map<number, SmsMessage>();
    const conflicts = new Set<number>();
    for (const part of group) {
      const sequence = part.concat!.sequence;
      const old = parts.get(sequence);
      if (old && (old.text !== part.text || old.dataHex !== part.dataHex)) conflicts.add(sequence);
      parts.set(sequence, part);
    }
    const missing = Array.from({length:total}, (_, i) => i + 1).filter(i => !parts.has(i));
    const complete = !missing.length && !conflicts.size;
    const ordered = [...parts.entries()].sort(([a], [b]) => a - b).map(([, part]) => part);
    return { peer: first.peer, reference, total, received: parts.size, encoding: first.encoding, complete, missing,
      conflicts: [...conflicts].sort((a,b) => a-b),
      text: complete && first.encoding !== '8bit' ? ordered.map(part => part.text).join('') : null,
      dataHex: complete && first.encoding === '8bit' ? ordered.map(part => part.dataHex).join('') : null };
  });
}

export function parseSmsBatch(input: string): { messages: SmsMessage[]; errors: string[]; groups: SmsAssembly[] } {
  const messages: SmsMessage[] = [];
  const errors: string[] = [];
  let pending: { line: number; length: number } | null = null;
  for (const [index, source] of input.split(/\r?\n/u).entries()) {
    const line = source.trim();
    if (!line) continue;
    if (/^\+(?:CMGR|CMT|CMGL):/u.test(line)) {
      if (pending) errors.push(`第 ${pending.line} 行：返回头后缺少 PDU。`);
      const match = line.match(/^\+(?:CMGR|CMGL):\s*\d+.*,(\d+)\s*$/u) ?? line.match(/^\+CMT:\s*(?:"[^"]*"|)\s*,\s*(\d+)\s*$/u);
      pending = match ? { line: index + 1, length: Number(match[1]) } : null;
      if (!match) errors.push(`第 ${index+1} 行：不是支持的 PDU 模式返回头，Text 模式解析暂未提供。`);
      continue;
    }
    if (line === 'OK' || /^AT\+CM(?:GR|GL)(?:=|\?)/u.test(line)) continue;
    try {
      const message = parseSmsPdu(line);
      if (pending && pending.length !== message.tpduLength) throw new Error(`返回头长度 ${pending.length} 与实际 TPDU 长度 ${message.tpduLength} 不一致。`);
      messages.push(message);
    } catch (e) {
      errors.push(`第 ${index+1} 行：${e instanceof Error ? e.message : String(e)}`);
    }
    pending = null;
  }
  if (pending) errors.push(`第 ${pending.line} 行：返回头后缺少 PDU。`);
  if (!messages.length && !errors.length) errors.push('请输入完整 PDU，每条一行。');
  return { messages, errors, groups: assembleSms(messages) };
}
