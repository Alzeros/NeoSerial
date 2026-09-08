import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  buildManualEntries,
  defaultSuggestLimits,
  displayName,
  exampleLines,
  matchSuggestions,
  searchCommands,
  shortTitle,
  splitSyntax,
  stripAtPrefix,
  type SuggestLimits,
  type Suggestion,
} from '../src/lib/suggest.ts';
import type { ManualCommand, ManualDocument } from '../src/lib/types.ts';

const doc = (id: number, title: string, cmd_status = 'done'): ManualDocument => ({
  id, title, filename: '', status: 'done', cmd_status, cmd_count: 0, category_id: 0, updated_at: '',
});
const cmd = (id: number, document_id: number, command: string, extra: Partial<ManualCommand> = {}): ManualCommand => ({
  id, document_id, command, name: '', syntax: '', parameters: [], example: '', page_no: null, summary: '', ...extra,
});

// 按接口实测形态造的夹具:文档按 id 降序,同名 AT+CSQ 出现在两本手册,SSL 手册还在提取中
const docs = [doc(4, 'LwM2M用户手册'), doc(3, 'MQTT用户手册'), doc(2, 'SSL用户手册', 'running')];
const cmds = [
  cmd(1, 3, 'AT+MQTTCFG', { name: '配置或查询MQTT参数' }),
  cmd(2, 3, 'AT+MQTTCONN', { name: '连接至MQTT或MQTTS服务器' }),
  cmd(17, 4, 'AT+MIPLCREATE', { name: '创建LwM2M设备实例' }),
  cmd(18, 4, 'AT+MIPLDELETE', { name: 'AT+MIPLDELETE', summary: '删除设备实例' }),
  cmd(50, 4, 'AT+CSQ', { name: '查询信号质量' }),
  cmd(51, 3, 'at+csq ', { name: '信号质量(MQTT手册)' }),
  cmd(60, 2, 'AT+MSSLCFG', { name: 'SSL配置' }),
];

test('buildManualEntries:过滤未启用/未提取的手册,同名合并,主记录取手册顺序靠前者', () => {
  const all = buildManualEntries(docs, cmds, []);
  assert.deepEqual(all.map((e) => e.key), ['AT+CSQ', 'AT+MIPLCREATE', 'AT+MIPLDELETE', 'AT+MQTTCFG', 'AT+MQTTCONN']);
  const csq = all[0];
  assert.equal(csq.primary.document_id, 4, '文档列表里 LwM2M(id 4) 排在 MQTT 前');
  assert.equal(csq.alsoIn.length, 1);
  assert.equal(csq.alsoIn[0].id, 51);

  const noLwm2m = buildManualEntries(docs, cmds, [4]);
  assert.deepEqual(noLwm2m.map((e) => e.key), ['AT+CSQ', 'AT+MQTTCFG', 'AT+MQTTCONN']);
  assert.equal(noLwm2m[0].primary.document_id, 3, '排除 LwM2M 后主记录落到 MQTT');
  assert.equal(noLwm2m[0].alsoIn.length, 0);
});

test('stripAtPrefix', () => {
  assert.equal(stripAtPrefix('AT+CSQ'), 'CSQ');
  assert.equal(stripAtPrefix('AT&W'), 'W');
  assert.equal(stripAtPrefix('ATE0'), 'E0');
  assert.equal(stripAtPrefix('CSQ'), 'CSQ');
});

/** 按字符数算门槛(旧行为)的 limits,用于不关心 AT 前缀那条规则的用例 */
const raw: SuggestLimits = { ...defaultSuggestLimits, ignoreAtPrefix: false };
const kinds = (r: { items: Suggestion[] }) =>
  r.items.map((s) => (s.kind === 'history' ? 'h:' + s.text : 'm:' + s.entry.key));
const manualKeys = (r: { items: Suggestion[] }) =>
  r.items.map((s) => s.kind === 'manual' && s.entry.key);

test('matchSuggestions:历史在前(按传入顺序即最近在前,排除与手册同名的),手册前缀次之按字母序', () => {
  const entries = buildManualEntries(docs, cmds, []);
  const history = ['AT+MQTTCONN=1,"new"', 'AT+MQTTCONN=0,"h"', 'AT+CSQ', 'AT+MIPLCREATE'];
  assert.deepEqual(
    kinds(matchSuggestions('AT+M', entries, history, raw)),
    ['h:AT+MQTTCONN=1,"new"', 'h:AT+MQTTCONN=0,"h"', 'm:AT+MIPLCREATE', 'm:AT+MIPLDELETE', 'm:AT+MQTTCFG', 'm:AT+MQTTCONN'],
    '两条历史按传入顺序(最近在前);历史里的 AT+MIPLCREATE 与手册同名,只出手册那条',
  );
});

test('matchSuggestions:忽略 AT 前缀算门槛(默认)——AT / AT+ / AT+M 都不弹,AT+MI 才弹', () => {
  const entries = buildManualEntries(docs, cmds, []);
  const history = ['AT+MQTTCONN=1,"x"'];
  for (const q of ['AT', 'AT+', 'AT+M', 'at+m']) {
    assert.deepEqual(matchSuggestions(q, entries, history), { items: [], hidden: 0 }, `${q} 去前缀后不足 2 字符,不该弹`);
  }
  assert.deepEqual(manualKeys(matchSuggestions('AT+MI', entries, [])), ['AT+MIPLCREATE', 'AT+MIPLDELETE']);
  // 关掉该开关就回到按字符数算:AT+M 照旧弹一堆
  assert.equal(matchSuggestions('AT+M', entries, history, raw).items.length, 5);
  // 不带 AT 前缀的输入不受影响(stripAtPrefix 对它是恒等)
  assert.deepEqual(manualKeys(matchSuggestions('MI', entries, [])), ['AT+MIPLCREATE', 'AT+MIPLDELETE']);
  // ATE0 这类不带 + 的指令:AT 也算前缀,去掉后 E0 是两字符,照弹
  assert.deepEqual(matchSuggestions('ATE', entries, ['ATE0=1']).items, [], '去 AT 后只剩 E,1 字符不够');
  assert.deepEqual(kinds(matchSuggestions('ATE0', entries, ['ATE0=1'])), ['h:ATE0=1'], '去 AT 后 E0 够 2 字符');
});

test('matchSuggestions:minChars 可调', () => {
  const entries = buildManualEntries(docs, cmds, []);
  // 门槛 1:去前缀后 1 个字符就弹
  assert.equal(matchSuggestions('AT+M', entries, [], { ...defaultSuggestLimits, minChars: 1 }).items.length, 4);
  // 门槛 4:AT+MI(2)不弹,AT+MIPL(4)才弹
  const strict = { ...defaultSuggestLimits, minChars: 4 };
  assert.deepEqual(matchSuggestions('AT+MI', entries, [], strict).items, []);
  assert.equal(matchSuggestions('AT+MIPL', entries, [], strict).items.length, 2);
});

test('matchSuggestions:历史与手册各自限量,超出的计入 hidden', () => {
  const entries = buildManualEntries(docs, cmds, []);
  const variants = Array.from({ length: 15 }, (_, i) => `AT+CSQ=${i}`);
  const out = matchSuggestions('AT+CS', entries, variants, { ...defaultSuggestLimits, maxHistory: 10 });
  assert.deepEqual(out.items.slice(0, 10).map((s) => s.kind === 'history' && s.text), variants.slice(0, 10), '取最靠前(最近)的 10 条历史');
  assert.deepEqual(out.items[10], { kind: 'manual', entry: entries.find((e) => e.key === 'AT+CSQ') }, '手册候选接在历史之后');
  assert.equal(out.items.length, 11);
  assert.equal(out.hidden, 5, '多出的 5 条历史被上限挡掉,计入 hidden');

  // 手册侧同理:上限 2 时 AT+MI 的两条全给,上限 1 时挡掉一条
  assert.equal(matchSuggestions('AT+MI', entries, [], { ...defaultSuggestLimits, maxManual: 1 }).hidden, 1);
  // 历史上限 0 = 联想里不出历史
  const noHist = matchSuggestions('AT+CS', entries, variants, { ...defaultSuggestLimits, maxHistory: 0 });
  assert.ok(noHist.items.every((s) => s.kind === 'manual'));
  assert.equal(noHist.hidden, 15);
});

test('matchSuggestions:输入不以 AT 开头时按去前缀的指令体匹配;中文名包含排最后', () => {
  const entries = buildManualEntries(docs, cmds, []);
  assert.deepEqual(manualKeys(matchSuggestions('csq', entries, ['AT+CSQ'])), ['AT+CSQ']);
  assert.deepEqual(
    manualKeys(matchSuggestions('MQTT', entries, [])),
    ['AT+MQTTCFG', 'AT+MQTTCONN', 'AT+CSQ'],
    'AT+CSQ 靠 alsoIn 的名称"信号质量(MQTT手册)"包含匹配,排在前缀匹配之后',
  );
  assert.deepEqual(manualKeys(matchSuggestions('信号', entries, [])), ['AT+CSQ']);
  assert.deepEqual(
    manualKeys(matchSuggestions('lwm2m', entries, [])),
    ['AT+MIPLCREATE'],
    '中文名里夹的英文缩写"LwM2M"包含匹配不分大小写',
  );
});

test('matchSuggestions:不足门槛为空;排除与当前输入相同的历史', () => {
  const entries = buildManualEntries(docs, cmds, []);
  assert.deepEqual(matchSuggestions('A', entries, ['AT'], raw).items, []);
  assert.deepEqual(matchSuggestions('AT+CGDCONT=1', entries, ['AT+CGDCONT=1']).items, [], '和输入一模一样的历史不重复给');
  assert.equal(matchSuggestions('AT+CGDCONT=1', entries, ['AT+CGDCONT=1,"IP"']).items.length, 1);
});

test('searchCommands:多 token contains 匹配 command/name/summary;不带 AT 前缀也命中;空 query 空', () => {
  const entries = buildManualEntries(docs, cmds, []);
  assert.deepEqual(searchCommands('', entries), []);
  // mqtt → 命令体含 MQTT 的都命中
  const mqtt = searchCommands('mqtt', entries).map((e) => e.key);
  assert.ok(mqtt.includes('AT+MQTTCFG'));
  assert.ok(mqtt.includes('AT+MQTTCONN'));
  // 中文名/summary 命中:信号质量 → AT+CSQ
  const csq = searchCommands('信号', entries).map((e) => e.key);
  assert.deepEqual(csq, ['AT+CSQ']);
  // 多 token:两词都得命中(配置 在 name,mqtt 在 command)
  const two = searchCommands('mqtt 配置', entries).map((e) => e.key);
  assert.deepEqual(two, ['AT+MQTTCFG']);
  // 命中不了的字段组合不算
  assert.deepEqual(searchCommands('mqtt 不存在词xyz', entries), []);
});

test('displayName:name 就是指令本身时改用 summary;超长截断', () => {
  assert.equal(displayName(cmd(1, 1, 'AT+MIPLDELETE', { name: 'AT+MIPLDELETE', summary: '删除设备实例' })), '删除设备实例');
  assert.equal(displayName(cmd(1, 1, 'AT+MIPLREADRSP', { name: '+MIPLREADRSP', summary: '读操作回复' })), '读操作回复');
  assert.equal(displayName(cmd(1, 1, 'AT+X', { name: '', summary: '' })), '');
  const long = '该命令用于设置指定object的所需资源列表,一共三十多个字符长度的说明';
  const shown = displayName(cmd(1, 1, 'AT+X', { name: long }));
  assert.equal(shown.length, 31);
  assert.ok(shown.endsWith('…'));
});

test('splitSyntax / exampleLines / shortTitle', () => {
  assert.deepEqual(splitSyntax('AT+MQTTREAD=<connect_id>\nAT+MQTTREAD=<connect_id>,<count>'), ['AT+MQTTREAD=<connect_id>', 'AT+MQTTREAD=<connect_id>,<count>']);
  assert.deepEqual(splitSyntax('AT+A=<x>; AT+A=<y>'), ['AT+A=<x>', 'AT+A=<y>']);
  assert.deepEqual(splitSyntax('AT+A=<x> | AT+A'), ['AT+A=<x>', 'AT+A']);
  assert.deepEqual(splitSyntax("{'test_command': 'AT+MSSLCHECK=?', 'set_command': 'AT+MSSLCHECK=<cert_name>'}"), ["{'test_command': 'AT+MSSLCHECK=?', 'set_command': 'AT+MSSLCHECK=<cert_name>'}"]);
  assert.deepEqual(splitSyntax('  '), []);

  assert.deepEqual(exampleLines('AT+MIPLCREATE=1\n+MIPLCREATE: 0\n\nOK\nat+miplopen=0,86400'), [
    { text: 'AT+MIPLCREATE=1', fillable: true },
    { text: '+MIPLCREATE: 0', fillable: false },
    { text: 'OK', fillable: false },
    { text: 'at+miplopen=0,86400', fillable: true },
  ]);

  assert.equal(shortTitle('LwM2M用户手册'), 'LwM2M');
  assert.equal(shortTitle('MQTT手册'), 'MQTT');
  assert.equal(shortTitle('HTTP-HTTPS用户手册'), 'HTTP-HT…');
  assert.equal(shortTitle('手册'), '手册', '去后缀后为空则退回原标题');
});
