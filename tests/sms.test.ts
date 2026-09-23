import test from 'node:test';
import assert from 'node:assert/strict';
import { parseSmsPdu, generateSms, parseSmsBatch, assembleSms } from '../src/lib/sms.ts';
import * as sms from '../src/lib/sms.ts';

const SUBMIT = '0001000B912143658709F100000AE8329BFD4697D9EC37';
const DELIVER = '00040B912143658709F10000629032214365230AE8329BFD4697D9EC37';
const generate = (text: string, options = {}) => generateSms({ recipient: '+12345678901', smsc: '', text, encoding: 'auto', reference: 127, ...options });

test('短信默认参数兼容旧设置，恢复只修改三项参数且不改变原草稿', () => {
  assert.deepEqual(sms.smsDefaultsFromSettings(), {smsc:'', encoding:'auto', validity_period:null});
  const defaults = sms.smsDefaultsFromSettings({sms_default_smsc:' +1234 ', sms_default_encoding:'ucs2', sms_default_validity_period:170});
  const config = {mode:'generate', recipient:'10086', text:'你好', pdu_input:'saved', smsc:'+999', encoding:'gsm7', validity_period:11, reference:45} as const;
  const restored = sms.restoreSmsDefaults(config, defaults);
  assert.deepEqual(restored, {...config, smsc:'+1234', encoding:'ucs2', validity_period:170});
  assert.equal(config.smsc, '+999');
  defaults.smsc = '+5678';
  assert.equal(restored.smsc, '+1234');
});

test('有效期未指定保持旧 PDU；4 天增加 AA 字段且长度增加 1', () => {
  const options = {recipient:'10086', smsc:'+8613800210500'};
  const original = generate('你好', options).parts[0];
  assert.equal(original.pdu, '0891683108200105F0010005810180F60008044F60597D');
  assert.equal(generate('你好', {...options, validity_period:null}).parts[0].pdu, original.pdu);
  const fourDays = generate('你好', {...options, validity_period:170}).parts[0];
  assert.equal(fourDays.pdu, '0891683108200105F0110005810180F60008AA044F60597D');
  assert.equal(fourDays.tpduLength, 15);
  const parsed = parseSmsPdu(fourDays.pdu);
  assert.equal(parsed.validity, '4 天');
  assert.equal(parsed.peer, '10086');
  assert.equal(parsed.text, '你好');
});

test('有效期保留长短信 UDHI，各段均增加有效期但不改变正文', () => {
  const original = generate('中'.repeat(71));
  const withValidity = generate('中'.repeat(71), {validity_period:170});
  withValidity.parts.forEach((part, index) => {
    const message = parseSmsPdu(part.pdu);
    assert.equal(message.firstOctet, 0x51);
    assert.equal(message.validity, '4 天');
    assert.equal(part.tpduLength, original.parts[index].tpduLength + 1);
    assert.equal(message.text, original.parts[index].text);
    assert.equal(message.concat?.sequence, index + 1);
  });
});

test('相对有效期支持 0–255，拒绝非法值；时长边界正确', () => {
  for (const [code, label] of [[0,'5 分钟'],[11,'60 分钟'],[71,'360 分钟'],[143,'720 分钟'],[144,'750 分钟'],[167,'1440 分钟'],[168,'2 天'],[170,'4 天'],[173,'7 天'],[196,'30 天'],[197,'5 周'],[255,'63 周']] as const) {
    assert.equal(parseSmsPdu(generate('A', {validity_period:code}).parts[0].pdu).validity, label);
  }
  for (const value of [-1,256,1.5,NaN,Infinity,'170']) {
    assert.throws(() => generate('A', {validity_period:value}), /有效期/);
  }
});

test('独立 GSM7 已知位串解析发送和接收短信，解析号码和时区', () => {
  const sent = parseSmsPdu(SUBMIT);
  assert.equal(sent.type, 'SMS-SUBMIT');
  assert.equal(sent.peer, '+12345678901');
  assert.equal(sent.text, 'hellohello');
  assert.equal(sent.tpduLength, 22);
  const received = parseSmsPdu(DELIVER);
  assert.equal(received.type, 'SMS-DELIVER');
  assert.equal(received.timestamp, '26/09/23 12:34:56 UTC+08:00');
  assert.equal(received.text, 'hellohello');
  assert.equal(parseSmsPdu(DELIVER.replace('436523', '436588')).timestamp, '26/09/23 12:34:56 UTC-02:00');
});

test('生成独立已知 PDU，CMGS 长度不包含 SMSC', () => {
  assert.equal(generate('hellohello').parts[0].pdu, SUBMIT);
  const part = generate('hellohello', {smsc:'+1234'}).parts[0];
  assert.equal(part.pdu, '03912143' + SUBMIT.slice(2));
  assert.equal(part.tpduLength, 22);
  assert.equal(parseSmsPdu(part.pdu).smsc, '+1234');
});

test('UCS-2 中文采用大端，GSM7 扩展字符和基本字符往返', () => {
  const chinese = generate('你好');
  assert.equal(chinese.encoding, 'ucs2');
  assert.ok(chinese.parts[0].pdu.endsWith('0008044F60597D'));
  assert.equal(parseSmsPdu(chinese.parts[0].pdu).text, '你好');
  const symbols = '@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ¤¡ÄÖÑÜ§¿äöñüà^{}\\[~]|€\f';
  assert.equal(parseSmsPdu(generate(symbols).parts[0].pdu).text, symbols);
  assert.throws(() => generate('你好', {encoding:'gsm7'}), /GSM/);
  assert.throws(() => generate('🙂'), /UCS-2/);
  assert.throws(() => generate('\uD800'), /UCS-2/);
});

test('GSM7 与 UCS2 单段边界及长短信自动拆分', () => {
  for (const [char, limit, multipartLimit] of [['A',160,153], ['中',70,67]] as const) {
    assert.equal(generate(char.repeat(limit)).parts.length, 1);
    const result = generate(char.repeat(limit+1));
    assert.equal(result.parts.length, 2);
    assert.equal(result.parts[0].text.length, multipartLimit);
    const messages = result.parts.map(part => parseSmsPdu(part.pdu));
    assert.equal(assembleSms(messages)[0].text, char.repeat(limit+1));
    assert.equal(messages[0].concat?.reference, 127);
  }
  const text = 'A'.repeat(152) + '^' + 'B'.repeat(20);
  const result = generate(text);
  assert.equal(result.parts[0].text, 'A'.repeat(152));
  assert.equal(result.parts[1].text[0], '^');
  assert.equal(assembleSms(result.parts.map(part => parseSmsPdu(part.pdu)))[0].text, text);
});

test('独立 UDH 向量：8 位参考号补齐到 septet，16 位参考号无需填充', () => {
  const first = parseSmsPdu('0041000291210000080500037F020182');
  assert.equal(first.text, 'A');
  assert.deepEqual(first.concat, {reference:127, bits:8, total:2, sequence:1});
  const second = parseSmsPdu('0041000291210000090608041234020141');
  assert.equal(second.text, 'A');
  assert.deepEqual(second.concat, {reference:0x1234, bits:16, total:2, sequence:1});
});

test('8-bit 保留二进制，字母发送方按 GSM7 解码', () => {
  const binary = parseSmsPdu('00010002912100040300FF80');
  assert.equal(binary.text, null);
  assert.equal(binary.encoding, '8bit');
  assert.equal(binary.dataHex, '00FF80');
  assert.equal(parseSmsPdu('000004D041210000629032214365230141').peer, 'AB');
});

test('SUBMIT 相对、绝对和增强有效期不会导致正文偏移', () => {
  const base = '0001000291210000';
  assert.equal(parseSmsPdu(base.replace('0001','0011') + 'AA0141').text, 'A');
  assert.equal(parseSmsPdu(base.replace('0001','0019') + '629032214365230141').text, 'A');
  assert.equal(parseSmsPdu(base.replace('0001','0009') + '000000000000000141').text, 'A');
});

test('解析拒绝非法字节、截断、余字节、不支持类型和编码', () => {
  for (const input of ['', 'GG', '0', SUBMIT.slice(0,-2), SUBMIT+'00', '0002', 'FF00', '00010002912100200141', '000100029121000801FF']) {
    assert.throws(() => parseSmsPdu(input));
  }
  assert.throws(() => parseSmsPdu('0041000291210000080500037F020082'), /分段/);
  assert.throws(() => parseSmsPdu('0041000291210000080500037F020382'), /分段/);
  assert.throws(() => generate('A', {recipient:'not-a-number'}), /号码/);
  assert.throws(() => generate('A', {smsc:'+'}), /短信中心/);
  assert.throws(() => generate('A'.repeat(153*255+1)), /255/);
  assert.throws(() => generate('A', {reference:256}), /参考号/);
});

test('长短信乱序合并、缺段提示、重复去重、冲突拒绝', () => {
  const parts = generate('A'.repeat(161)).parts.map(part => parseSmsPdu(part.pdu));
  assert.equal(assembleSms([parts[1],parts[0],parts[0]])[0].text, 'A'.repeat(161));
  const incomplete = assembleSms([parts[1]])[0];
  assert.equal(incomplete.complete, false);
  assert.equal(incomplete.text, null);
  assert.deepEqual(incomplete.missing, [1]);
  const conflicting = generate('B'.repeat(161)).parts.map(part => parseSmsPdu(part.pdu));
  const group = assembleSms([...parts, conflicting[0]])[0];
  assert.equal(group.complete, false);
  assert.deepEqual(group.conflicts, [1]);
  assert.equal(group.text, null);
  const otherPeer = generate('B'.repeat(161), {recipient:'+987654321'}).parts.map(part => parseSmsPdu(part.pdu));
  assert.equal(assembleSms([...parts,...otherPeer]).length, 2);
});

test('批量输入接受 CMGR/CMT 包装，错误保留行号，正确记录继续解析', () => {
  const batch = parseSmsBatch(`+CMGR: 0,,22\n${SUBMIT}\nOK\n+CMT: ,28\n${DELIVER}\nGG`);
  assert.equal(batch.messages.length, 2);
  assert.equal(batch.errors.length, 1);
  assert.match(batch.errors[0], /6/);
  assert.equal(parseSmsBatch('+CMGR: 0,,22').errors.length, 1);
  assert.equal(parseSmsBatch(`+CMGR: 0,,99\n${SUBMIT}`).errors.length, 1);
  assert.equal(parseSmsBatch(`+CMGR: "REC READ","+12345"\nhello`).messages.length, 0);
});

test('不同位偏移与扩展字符的分段往返，用户数据不超 140 字节', () => {
  for (let length = 0; length <= 330; length++) {
    const source = 'A€@^\n'.repeat(66).slice(0, length);
    const result = generate(source);
    const messages = result.parts.map(part => parseSmsPdu(part.pdu));
    assert.equal(messages.map(message => message.text).join(''), source);
    for (const message of messages) assert.ok(Math.ceil(message.udl * 7 / 8) <= 140);
  }
});

test('DCS 分类、非法时间/UDH、国际号码填充及空正文', () => {
  for (const dcs of ['00','10','40','C0','D8','F0']) {
    assert.equal(parseSmsPdu(`00010002912100${dcs}0141`).text, 'A');
  }
  for (const dcs of ['04','44','F4']) {
    assert.equal(parseSmsPdu(`00010002912100${dcs}01FF`).dataHex, 'FF');
  }
  assert.equal(parseSmsPdu('00010002912100E8024F60').text, '你');
  for (const dcs of ['0C','80','F8','C4']) assert.throws(() => parseSmsPdu(`00010002912100${dcs}0141`), /DCS/);
  assert.throws(() => parseSmsPdu(DELIVER.replace('629032', '629013')), /时间/);
  assert.throws(() => parseSmsPdu('0041000291210000050500037F02'), /UDH/);
  assert.throws(() => parseSmsPdu('004100029121000006032401004104'), /国家语言/);
  assert.throws(() => parseSmsPdu('0001000391210300000141'), /号码/);
  assert.equal(parseSmsPdu(generate('').parts[0].pdu).text, '');
  assert.equal(parseSmsPdu(generate('A', {recipient:'1234'}).parts[0].pdu).peer, '1234');
});
