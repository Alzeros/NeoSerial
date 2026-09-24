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
 * null leaves selections outside this log to the browser's normal copy behavior.
 */
export function selectedLogText(container: HTMLElement, selection: Selection | null): string | null {
  if (!selection || selection.isCollapsed || selection.rangeCount !== 1) return null;
  const range = selection.getRangeAt(0);
  if (!container.contains(range.startContainer) || !container.contains(range.endContainer)) return null;

  const lines: string[] = [];
  for (const row of container.querySelectorAll('[data-log-row]')) {
    if (!selectedRange(row, range)) continue;
    const fields: string[] = [];
    for (const field of row.querySelectorAll('[data-log-field]')) {
      const text = selectedRange(field, range)?.toString();
      if (text) fields.push(text);
    }
    lines.push(fields.join(' '));
  }
  return lines.join('\n');
}
