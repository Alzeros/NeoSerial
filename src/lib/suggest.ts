// 输入框指令联想的纯函数:候选构建、匹配排序、显示兜底。不依赖 Svelte,tests/suggest.test.ts 直接跑。
import type { ManualCommand, ManualDocument } from './types';

/** 同名指令(大小写无关)合并后的一条候选。primary 是手册列表里排前面那本的记录,其余进 alsoIn(各自完整记录,详情卡可切换)。
 *  UI 展示与填入输入框一律用 key(规范大写);primary.command 是原始 DB 字符串,可能脏(如 "at+csq " —— 小写+尾随空格),不要直接拿去填。 */
export interface ManualEntry {
  /** 合并键:指令去首尾空格后大写 */
  key: string;
  primary: ManualCommand;
  alsoIn: ManualCommand[];
}

export type Suggestion =
  | { kind: 'history'; text: string }
  | { kind: 'manual'; entry: ManualEntry };

/** 参与候选的手册:cmd_status=done 且不在排除名单。 */
export function enabledDocIds(documents: ManualDocument[], disabledDocIds: number[]): Set<number> {
  const disabled = new Set(disabledDocIds);
  return new Set(documents.filter((d) => d.cmd_status === 'done' && !disabled.has(d.id)).map((d) => d.id));
}

/** 按勾选手册过滤 + 同名合并。结果按 key 字母序。 */
export function buildManualEntries(
  documents: ManualDocument[],
  commands: ManualCommand[],
  disabledDocIds: number[],
): ManualEntry[] {
  const enabled = enabledDocIds(documents, disabledDocIds);
  const order = new Map(documents.map((d, i) => [d.id, i] as const));
  const groups = new Map<string, ManualCommand[]>();
  for (const c of commands) {
    if (!enabled.has(c.document_id)) continue;
    const key = c.command.trim().toUpperCase();
    if (!key) continue;
    const g = groups.get(key);
    if (g) g.push(c);
    else groups.set(key, [c]);
  }
  const entries: ManualEntry[] = [];
  for (const [key, recs] of groups) {
    recs.sort((a, b) => (order.get(a.document_id) ?? 0) - (order.get(b.document_id) ?? 0) || a.id - b.id);
    entries.push({ key, primary: recs[0], alsoIn: recs.slice(1) });
  }
  entries.sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
  return entries;
}

/** 去掉 AT 前缀与紧随的 + / &(大小写无关):"AT+CSQ"→"CSQ","AT&W"→"W","ATE0"→"E0"。输入不以 AT 开头时用它匹配。 */
export function stripAtPrefix(cmd: string): string {
  return cmd.replace(/^AT[+&]?/i, '');
}

function prefixHit(upperText: string, upperQuery: string): boolean {
  if (upperText.startsWith(upperQuery)) return true;
  return !upperQuery.startsWith('AT') && stripAtPrefix(upperText).startsWith(upperQuery);
}

/** 历史只占前 10 席,多出的排在手册候选之后——常用指令发得越多,不该越难看到它的手册卡。 */
const HISTORY_FIRST_MAX = 10;

/** 匹配 + 排序。query 去首尾空格后不足 2 字符返回空。
 *  ① 历史前缀,最多占前 HISTORY_FIRST_MAX 席(按传入顺序即最近在前;排除与当前输入相同的、与手册指令同名的——后者手册那条有详情)
 *  ② 手册指令前缀;输入不以 AT 开头时也用去前缀的指令体匹配(CSQ→AT+CSQ)
 *  ③ 手册名称包含(含 alsoIn 的名称),大小写无关
 *  ④ ①里超出 10 席的剩余历史,补在手册候选之后
 *  同一条只出现一次,总量截到 limit。 */
export function matchSuggestions(query: string, entries: ManualEntry[], history: string[], limit = 50): Suggestion[] {
  const q = query.trim();
  const Q = q.toUpperCase();
  if (Q.length < 2) return [];
  const manualKeys = new Set(entries.map((e) => e.key));
  const historyHits: Suggestion[] = [];
  for (const h of history) {
    const t = h.trim();
    const T = t.toUpperCase();
    if (t !== q && !manualKeys.has(T) && prefixHit(T, Q)) historyHits.push({ kind: 'history', text: h });
  }
  const manualHits: Suggestion[] = [];
  const seen = new Set<string>();
  for (const e of entries) {
    if (prefixHit(e.key, Q)) {
      seen.add(e.key);
      manualHits.push({ kind: 'manual', entry: e });
    }
  }
  for (const e of entries) {
    if (seen.has(e.key)) continue;
    if (e.primary.name.toUpperCase().includes(Q) || e.alsoIn.some((r) => r.name.toUpperCase().includes(Q))) {
      seen.add(e.key);
      manualHits.push({ kind: 'manual', entry: e });
    }
  }
  const out = [...historyHits.slice(0, HISTORY_FIRST_MAX), ...manualHits, ...historyHits.slice(HISTORY_FIRST_MAX)];
  return out.slice(0, limit);
}

/** 列表行/详情标题用的名称:name 为空或就是指令本身(如 "AT+MIPLDELETE"、"+MIPLREADRSP")时改用 summary;超 max 截断加省略号。 */
export function displayName(rec: ManualCommand, max = 30): string {
  const norm = (s: string) => s.trim().toUpperCase().replace(/^AT/, '').replace(/^[+&]/, '');
  const name = rec.name.trim();
  const text = !name || norm(name) === norm(rec.command) ? rec.summary.trim() : name;
  return text.length > max ? text.slice(0, max) + '…' : text;
}

/** 语法可能多种形式挤在一个字串里:按换行、" | "、"; " 拆行显示,不解析结构;拆不开就原样一行。 */
export function splitSyntax(syntax: string): string[] {
  return syntax
    .split(/\r?\n| \| |;\s+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/** 示例按行拆;只有以 AT 开头(大小写无关,去首尾空格)的行可点填入,响应行(如 "+MIPLCREATE: 0")、OK 只展示。 */
export function exampleLines(example: string): { text: string; fillable: boolean }[] {
  return example
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((text) => ({ text, fillable: /^AT/i.test(text) }));
}

/** 来源徽标用的短手册名:去掉"用户手册/手册"后缀,超 max 截断。 */
export function shortTitle(title: string, max = 8): string {
  const t = title.trim().replace(/(用户)?手册$/, '').trim() || title.trim();
  return t.length > max ? t.slice(0, max - 1) + '…' : t;
}

export function docTitle(documents: ManualDocument[], docId: number): string {
  return documents.find((d) => d.id === docId)?.title ?? `手册 ${docId}`;
}
