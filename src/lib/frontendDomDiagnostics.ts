/** Geometry and hit testing only. Never read text, titles, input values or option values. */
export function describeDiagnosticElement(element: Element | null) {
  if (!element) return null;
  const path: string[] = [];
  for (let node: Element | null = element; node && path.length < 5; node = node.parentElement) {
    const index = node.parentElement ? Array.prototype.indexOf.call(node.parentElement.children, node) + 1 : 1;
    path.unshift(`${node.tagName.toLowerCase()}:nth-child(${index})`);
  }
  return {
    tag: element.tagName.toLowerCase(), path: path.join(' > '), role: element.getAttribute('role'),
    control: element.getAttribute('data-diagnostic-control'), action: element.getAttribute('data-diagnostic-action'),
    region: element.closest('[data-diagnostic-region]')?.getAttribute('data-diagnostic-region'),
    disabled: element.matches(':disabled'), expanded: element.getAttribute('aria-expanded'),
  };
}

export function diagnosticDomSnapshot(element: Element | null | undefined) {
  try { return collectDomSnapshot(element); }
  catch (error) { return { present: Boolean(element), probe_error: error instanceof Error ? error.message : 'DOM probe failed' }; }
}

function collectDomSnapshot(element: Element | null | undefined) {
  if (!element) return { present: false };
  const rect = element.getBoundingClientRect();
  const css = getComputedStyle(element);
  const ancestors = [];
  let cssHidden = false;
  for (let parent: Element | null = element, depth = 0; parent && depth < 8; parent = parent.parentElement, depth++) {
    const style = getComputedStyle(parent);
    const hidden = style.display === 'none' || style.visibility !== 'visible' || Number(style.opacity) === 0;
    cssHidden ||= hidden;
    if (depth === 0 || hidden || style.overflow !== 'visible' || style.transform !== 'none') {
      ancestors.push({ tag: parent.tagName.toLowerCase(), depth, display: style.display, visibility: style.visibility,
        opacity: style.opacity, overflow: style.overflow, transform: style.transform, z_index: style.zIndex });
    }
  }
  const left = Math.max(0, rect.left), right = Math.min(innerWidth, rect.right);
  const top = Math.max(0, rect.top), bottom = Math.min(innerHeight, rect.bottom);
  const samples: { x: number; y: number; matches: boolean; hit_path?: string; hit_control?: string | null }[] = [];
  if (right > left && bottom > top) {
    for (const [rx, ry] of [[0.5, 0.5], [0.2, 0.2], [0.8, 0.8]]) {
      const x = Math.round(left + (right - left) * rx), y = Math.round(top + (bottom - top) * ry);
      const hit = document.elementFromPoint(x, y);
      const description = describeDiagnosticElement(hit);
      samples.push({ x, y, matches: Boolean(hit && element.contains(hit)), hit_path: description?.path, hit_control: description?.control });
    }
  }
  return {
    present: true, connected: element.isConnected, ...describeDiagnosticElement(element),
    rect: { x: Math.round(rect.x), y: Math.round(rect.y), width: Math.round(rect.width), height: Math.round(rect.height) },
    display: css.display, visibility: css.visibility, opacity: css.opacity, pointer_events: css.pointerEvents,
    z_index: css.zIndex, css_hidden: cssHidden, intersects_viewport: right > left && bottom > top,
    hit_matches: samples.filter(sample => sample.matches).length, samples, ancestors: ancestors.slice(0, 6),
  };
}

/** Observe after layout without delaying or changing the operation under investigation. */
export function afterDiagnosticLayout(callback: (via: string, elapsedMs: number) => void): void {
  const start = performance.now();
  let done = false, frame = 0;
  const finish = (via: string) => {
    if (done) return;
    done = true; clearTimeout(timer); cancelAnimationFrame(frame);
    try { callback(via, Math.round(performance.now() - start)); } catch { /* Observational only. */ }
  };
  const timer = setTimeout(() => finish('timer_fallback'), 500);
  frame = requestAnimationFrame(() => { frame = requestAnimationFrame(() => finish('animation_frame')); });
}
