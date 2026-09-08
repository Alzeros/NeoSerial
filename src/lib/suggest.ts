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

/** 联想行为(设置 → 指令联想,持久化在 settings.command_index)。 */
export interface SuggestLimits {
  /** 输入(去首尾空格)至少这么多字符才给候选 */
  minChars: number;
  /** 算门槛时忽略 AT / AT+ / AT& 前缀 */
  ignoreAtPrefix: boolean;
  /** 手册候选上限 */
  maxManual: number;
  /** 历史候选上限 */
  maxHistory: number;
}

/** 与后端 CommandIndexSettings 的 default_suggest_* 一致。 */
export const defaultSuggestLimits: SuggestLimits = {
  minChars: 2,
  ignoreAtPrefix: true,
  maxManual: 20,
  maxHistory: 10,
};

export interface MatchResult {
  items: Suggestion[];
  /** 命中了但被上限挡住没显示的条数,>0 时弹层顶部提示"还有 N 条" */
  hidden: number;
}

/** 匹配 + 排序。
 *  门槛:query 去首尾空格(ignoreAtPrefix 时再去掉 AT/AT+/AT& 前缀)后不足 minChars 字符返回空。
 *  忽略 AT 前缀是默认行为:模组指令几乎全以 AT+ 开头,"AT""AT+"按原样算就是 2、3 个字符,
 *  却命中整本手册,而它们是打任何一条指令的必经之路。
 *  ① 历史前缀(按传入顺序即最近在前;排除与当前输入相同的、与手册指令同名的——后者手册那条有详情),取前 maxHistory 条
 *  ② 手册指令前缀;输入不以 AT 开头时也用去前缀的指令体匹配(CSQ→AT+CSQ)
 *  ③ 手册名称包含(含 alsoIn 的名称),大小写无关
 *  ②③ 合起来取前 maxManual 条。两类各自限量,历史再多也挤不掉手册卡片。
 *  同一条只出现一次。 */
export function matchSuggestions(
  query: string,
  entries: ManualEntry[],
  history: string[],
  limits: SuggestLimits = defaultSuggestLimits,
): MatchResult {
  const empty: MatchResult = { items: [], hidden: 0 };
  const q = query.trim();
  const Q = q.toUpperCase();
  const minChars = Math.max(1, Math.floor(limits.minChars));
  const gauged = limits.ignoreAtPrefix ? stripAtPrefix(Q) : Q;
  if (gauged.length < minChars) return empty;
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
  const maxHistory = Math.max(0, Math.floor(limits.maxHistory));
  const maxManual = Math.max(0, Math.floor(limits.maxManual));
  const shownHistory = historyHits.slice(0, maxHistory);
  const shownManual = manualHits.slice(0, maxManual);
  return {
    items: [...shownHistory, ...shownManual],
    hidden: historyHits.length - shownHistory.length + (manualHits.length - shownManual.length),
  };
}

/** 模糊搜索(指令查询 tab 用,与输入框联想的精确前缀匹配分开):query 拆空格成多 token,
 *  每个 token 都要在 command / name(alsoIn 也算) / summary 任一字段里 contains 命中(大小写无关)。
 *  不带 AT/+ 前缀也能命中(mqtt → AT+MQTTCFG)。按命中字段与位置排序:command 命中 > name > summary,
 *  前缀命中 > 包含命中。空 query 返回空(不预填全量,让用户主动输)。 */
export function searchCommands(query: string, entries: ManualEntry[]): ManualEntry[] {
  const tokens = query.trim().toUpperCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return [];
  const scored: { e: ManualEntry; score: number }[] = [];
  for (const e of entries) {
    const cmd = e.key;
    const summary = e.primary.summary.toUpperCase();
    const names = [e.primary.name.toUpperCase(), ...e.alsoIn.map((r) => r.name.toUpperCase())];
    let score = 0;
    let allMatch = true;
    for (const t of tokens) {
      const cmdPos = cmd.indexOf(t);
      const nameHit = names.some((n) => n.includes(t));
      const sumPos = summary.indexOf(t);
      if (cmdPos < 0 && !nameHit && sumPos < 0) {
        allMatch = false;
        break;
      }
      score += cmdPos >= 0 ? 1000 - cmdPos : nameHit ? 500 : 100 - sumPos;
    }
    if (allMatch) scored.push({ e, score });
  }
  scored.sort((a, b) => b.score - a.score || (a.e.key < b.e.key ? -1 : a.e.key > b.e.key ? 1 : 0));
  return scored.map((s) => s.e);
}


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

/** 把示例行拆成"前缀标签 + AT 指令";拆不出 AT 指令返回 null(那行只是响应或说明)。
 *  手册里的示例行常带标签或序号——"Test Command: AT+GSN=?"、"1. AT+CSQ"、"> AT+CSQ"——
 *  标签不能跟着填进输入框,所以单独拆出来:UI 只把 at 那段做成可点按钮,点什么进去什么。
 *  响应行 "+MIPLCREATE: 0" 也带冒号,但冒号后面不是 AT,照旧不可填。 */
function splitFillable(line: string): { prefix: string; at: string } | null {
  const t = line.trim();
  if (/^AT/i.test(t)) return { prefix: '', at: t };
  // 前缀 = 以冒号(中/英文)结尾的标签、列表符号或序号;其后必须紧跟 AT 才算
  const m = t.match(/^((?:[^:：]{0,40}[:：]|[-*>•]|\d+[.)])\s*)(AT\S.*)$/i);
  return m ? { prefix: m[1].trim(), at: m[2].trim() } : null;
}

/** 示例块按行拆:
 *  - `fillable`:这一行有没有可填入的指令(响应行、OK、纯说明没有)
 *  - `prefix`:行首标签("Test Command:"、"1."),显示用,不参与填入;没有则为空串
 *  - `fill`:可点按钮的文本,也正是点下去填进输入框的内容(所见即所得)
 *  - `text`:整行原文,给 fillable=false 的行显示 */
export function exampleLines(
  example: string,
): { text: string; fillable: boolean; prefix: string; fill: string }[] {
  return example
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((text) => {
      const parts = splitFillable(text);
      return {
        text,
        fillable: parts !== null,
        prefix: parts?.prefix ?? '',
        fill: parts?.at ?? text,
      };
    });
}

/** 来源徽标用的短手册名:去掉"用户手册/手册"后缀,超 max 截断。 */
export function shortTitle(title: string, max = 8): string {
  const t = title.trim().replace(/(用户)?手册$/, '').trim() || title.trim();
  return t.length > max ? t.slice(0, max - 1) + '…' : t;
}

export function docTitle(documents: ManualDocument[], docId: number): string {
  return documents.find((d) => d.id === docId)?.title ?? `手册 ${docId}`;
}
