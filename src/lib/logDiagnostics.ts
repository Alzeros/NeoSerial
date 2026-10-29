import type { LogLine } from './types';

export interface LogFilter {
  direction: 'all' | 'rx' | 'tx';
  errorsOnly: boolean;
  include: string;
  exclude: string;
}

export function matchesLog(line: LogLine, filter: LogFilter, text: string | (() => string)): boolean {
  if (filter.direction !== 'all' && line.dir !== filter.direction) return false;
  if (filter.errorsOnly && !line.is_error) return false;
  const include = filter.include.trim().toLocaleLowerCase();
  const exclude = filter.exclude.trim().toLocaleLowerCase();
  if (!include && !exclude) return true;
  const candidate = (typeof text === 'function' ? text() : text).toLocaleLowerCase();
  return (!include || candidate.includes(include)) && (!exclude || !candidate.includes(exclude));
}

export function snippetLines(lines: readonly LogLine[], selected: readonly LogLine[], context = 10): LogLine[] {
  if (selected.length === 0) return [...lines];
  const indices = selected.map(line => lines.indexOf(line));
  if (indices.some(index => index < 0)) throw new Error('选中的记录已不在当前缓存中，可能已被淘汰或清空，请重新选择。');
  const padding = Number.isFinite(context) ? Math.min(1000, Math.max(0, Math.floor(context))) : 10;
  return lines.slice(Math.max(0, Math.min(...indices) - padding), Math.max(...indices) + padding + 1);
}

export function elapsedMilliseconds(first: LogLine, second: LogLine): number | null {
  if (![first.ts_ms, second.ts_ms].every(value => Number.isFinite(value) && value > 0)) return null;
  return Math.abs(second.ts_ms - first.ts_ms);
}

interface SnippetOptions {
  rows: readonly LogLine[];
  capturedAt: string;
  port: string | null;
  settings: string;
  mode: string;
  scope: string;
  render: (line: LogLine) => string;
  markers?: ReadonlyMap<LogLine, string>;
}

function singleLine(text: string): string {
  return text.replace(/[\r\n]+/g, ' ');
}

export function formatDiagnosticSnippet(options: SnippetOptions): string {
  const output = [
    '# NeoSerial 日志片段',
    `导出快照：${options.capturedAt}`,
    `端口：${options.port || '未连接 / 未知'}`,
    `窗口当前设置：${singleLine(options.settings)}`,
    '说明：以上为窗口当前设置，历史可能跨连接，不代表每条记录产生时的设备参数。',
    `显示：${options.mode}；日志时间为 UTC+8；时间戳缺失时不推算。`,
    `范围：${options.scope}；共 ${options.rows.length} 条记录`,
    '',
  ];
  const encoder = new TextEncoder();
  let byteLength = encoder.encode(`${output.join('\n')}\n`).byteLength;
  function append(text: string) {
    byteLength += encoder.encode(`${text}\n`).byteLength;
    if (byteLength > 32 * 1024 * 1024) throw new Error('日志片段超过 32 MiB UTF-8 文本大小限制，请缩小选择范围或减少上下文。');
    output.push(text);
  }
  for (const line of options.rows) {
    const marker = options.markers?.get(line);
    if (marker) append(`--- 标记：${singleLine(marker)} ---`);
    const direction = line.dir === 'tx' ? 'Tx' : 'Rx';
    const timestamp = line.ts || '时间未知';
    append(`${timestamp} [${direction}] #${line.line_index || '?'} ${options.render(line)}`);
  }
  return `${output.join('\n')}\n`;
}
