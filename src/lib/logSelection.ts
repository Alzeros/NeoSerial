/** Intersect a DOM selection with an element without adding layout-generated newlines. */
function selectedRange(element: Element, selection: Range): Range | null {
  if (!selection.intersectsNode(element)) return null;
  const range = element.ownerDocument.createRange();
  range.selectNodeContents(element);
  if (range.compareBoundaryPoints(Range.START_TO_START, selection) < 0) {
    range.setStart(selection.startContainer, selection.startOffset);
  }
  if (range.compareBoundaryPoints(Range.END_TO_END, selection) > 0) {
    range.setEnd(selection.endContainer, selection.endOffset);
  }
  return range.collapsed ? null : range;
}

/**
 * Copy the visible, selected log fields in row order. Range.toString() preserves
 * actual text (including HEX line breaks and search highlights), whereas
 * Selection.toString() inserts newlines between flex items.
 * 元数据(行号/方向/时间)与正文之间用 ` | ` 分隔,对应界面上的竖线;
 * 正文为空的行不留下孤立的 ` | `。
 * null leaves selections outside this log to the browser's normal copy behavior.
 */
export function selectedLogText(container: HTMLElement, selection: Selection | null): string | null {
  if (!selection || selection.isCollapsed || selection.rangeCount !== 1) return null;
  const range = selection.getRangeAt(0);
  if (!container.contains(range.startContainer) || !container.contains(range.endContainer)) return null;

  const lines: string[] = [];
  for (const row of container.querySelectorAll('[data-log-row]')) {
    if (!selectedRange(row, range)) continue;
    const meta: string[] = [];
    const content: string[] = [];
    for (const field of row.querySelectorAll('[data-log-field]')) {
      const text = selectedRange(field, range)?.toString();
      if (!text) continue;
      // 标记和正文在界面竖线右侧；行号/方向/时间在左侧。
      if (field.hasAttribute('data-log-marker') || field.hasAttribute('data-log-content')) content.push(text);
      else meta.push(text);
    }
    const head = meta.join(' ');
    const body = content.join(' ');
    lines.push(head && body ? `${head} | ${body}` : head || body);
  }
  return lines.join('\n');
}
