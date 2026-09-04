import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  buildManualEntries,
  displayName,
  exampleLines,
  matchSuggestions,
  shortTitle,
  splitSyntax,
  stripAtPrefix,
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

test('matchSuggestions:历史在前(按传入顺序即最近在前,排除与手册同名的),手册前缀次之按字母序', () => {
  const entries = buildManualEntries(docs, cmds, []);
  const history = ['AT+MQTTCONN=1,"new"', 'AT+MQTTCONN=0,"h"', 'AT+CSQ', 'AT+MIPLCREATE'];
  const out = matchSuggestions('AT+M', entries, history);
  assert.deepEqual(
    out.map((s) => (s.kind === 'history' ? 'h:' + s.text : 'm:' + s.entry.key)),
    ['h:AT+MQTTCONN=1,"new"', 'h:AT+MQTTCONN=0,"h"', 'm:AT+MIPLCREATE', 'm:AT+MIPLDELETE', 'm:AT+MQTTCFG', 'm:AT+MQTTCONN'],
    '两条历史按传入顺序(最近在前);历史里的 AT+MIPLCREATE 与手册同名,只出手册那条',
  );
});

test('matchSuggestions:历史最多占前 10 席,超出部分补在手册候选之后', () => {
  const entries = buildManualEntries(docs, cmds, []);
  const variants = Array.from({ length: 15 }, (_, i) => `AT+CSQ=${i}`);
  const out = matchSuggestions('AT+CS', entries, variants);
  assert.deepEqual(out.slice(0, 10).map((s) => s.kind === 'history' && s.text), variants.slice(0, 10), '前 10 席是最靠前(最近)的 10 条历史');
  assert.deepEqual(out[10], { kind: 'manual', entry: entries.find((e) => e.key === 'AT+CSQ') }, '手册 AT+CSQ 紧接着 10 席历史之后');
  assert.deepEqual(out.slice(11).map((s) => s.kind === 'history' && s.text), variants.slice(10), '剩余 5 条历史补在手册候选之后');
});

test('matchSuggestions:输入不以 AT 开头时按去前缀的指令体匹配;中文名包含排最后', () => {
  const entries = buildManualEntries(docs, cmds, []);
  assert.deepEqual(matchSuggestions('csq', entries, ['AT+CSQ']).map((s) => s.kind === 'manual' && s.entry.key), ['AT+CSQ']);
  const mqtt = matchSuggestions('MQTT', entries, []);
  assert.deepEqual(
    mqtt.map((s) => s.kind === 'manual' && s.entry.key),
    ['AT+MQTTCFG', 'AT+MQTTCONN', 'AT+CSQ'],
    'AT+CSQ 靠 alsoIn 的名称"信号质量(MQTT手册)"包含匹配,排在前缀匹配之后',
  );
  assert.deepEqual(matchSuggestions('信号', entries, []).map((s) => s.kind === 'manual' && s.entry.key), ['AT+CSQ']);
  assert.deepEqual(
    matchSuggestions('lwm2m', entries, []).map((s) => s.kind === 'manual' && s.entry.key),
    ['AT+MIPLCREATE'],
    '中文名里夹的英文缩写"LwM2M"包含匹配不分大小写',
  );
});

test('matchSuggestions:不足 2 字符为空;排除与当前输入相同的历史;截到 limit', () => {
  const entries = buildManualEntries(docs, cmds, []);
  assert.deepEqual(matchSuggestions('A', entries, ['AT']), []);
  assert.deepEqual(matchSuggestions('AT+CGDCONT=1', entries, ['AT+CGDCONT=1']), [], '和输入一模一样的历史不重复给');
  assert.equal(matchSuggestions('AT+CGDCONT=1', entries, ['AT+CGDCONT=1,"IP"']).length, 1);
  const many = Array.from({ length: 60 }, (_, i) => `AT+H${String(i).padStart(2, '0')}`);
  assert.equal(matchSuggestions('AT+H', entries, many).length, 50);
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
