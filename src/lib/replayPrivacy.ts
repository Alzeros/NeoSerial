import type { eventWithTime } from '@rrweb/types';
import { diagnosticSummary } from './diagnosticSummary.ts';

const publicLabels = new Set(typeof __NEOSERIAL_PUBLIC_LABELS__ === 'undefined' ? [] : __NEOSERIAL_PUBLIC_LABELS__);
export const maskReplayInput = (text: string) => text.slice(0, 4096).replace(/\S/g, '•');
export const maskReplayText = (text: string) => publicLabels.has(text.replace(/\s+/g, ' ').trim()) ? text : maskReplayInput(text);
const cleanCSS = (text: string) => text.replace(/url\(\s*(['"]?)(.*?)\1\s*\)/gi, (original, _quote, url) => url.startsWith('#') ? original : 'url()').replace(/@import\s+[^;]+;/gi, '');
const sanitizeStyle = (value: unknown): unknown => typeof value === 'string' ? cleanCSS(value)
  : Array.isArray(value) ? value.map(sanitizeStyle)
  : value && typeof value === 'object' ? Object.fromEntries(Object.entries(value).map(([key, item]) => [key, sanitizeStyle(item)])) : value;
const PRIVATE_ATTRIBUTE = /^(?:value|title|alt|placeholder|aria-label|aria-valuetext|content|http-equiv)$/i;
const RESOURCE_ATTRIBUTE = /^(?:src|srcset|srcdoc|action|formaction|poster|background|data|codebase|archive|manifest|ping)$/i;
function keepAttribute(name: string, value: unknown): boolean {
  if (/^on/i.test(name) || PRIVATE_ATTRIBUTE.test(name) || RESOURCE_ATTRIBUTE.test(name)) return false;
  if (/href$/i.test(name)) return typeof value === 'string' && value.startsWith('#');
  if (/^data-.*(?:password|secret|token|credential|api[-_]?key)/i.test(name)) return false;
  return true; // Preserve unknown rendering state such as data-theme/data-state/dir/colspan.
}

/** Preserve rrweb's mutation schema while removing user-facing attributes and resource URLs. */
export function sanitizeReplayEvent(event: eventWithTime): eventWithTime {
  const visit = (value: unknown): unknown => {
    if (Array.isArray(value)) return value.map(visit);
    if (!value || typeof value !== 'object') return value;
    const result: Record<string, unknown> = {};
    const object = value as Record<string, unknown>;
    for (const [key, item] of Object.entries(object)) {
      // Full nodes have a dictionary; incremental mutations have an ARRAY named attributes.
      if (key === 'attributes' && item && typeof item === 'object' && !Array.isArray(item)) {
        result[key] = Object.fromEntries(Object.entries(item).filter(([name, content]) => keepAttribute(name, content)).map(([name, content]) => [name,
          name === 'style' || name === '_cssText' ? sanitizeStyle(content) : content]));
      } else if (key === 'textContent' || key === 'text') result[key] = typeof item === 'string' ? (object.isStyle ? cleanCSS(item) : maskReplayText(item)) : item;
      else if ((key === 'rule' || key === 'cssText') && typeof item === 'string') result[key] = cleanCSS(item);
      else if (key === 'href') result[key] = 'about:blank';
      else result[key] = visit(item);
    }
    return result;
  };
  if (event.type === 5) return { ...event, data: { tag: 'custom', payload: diagnosticSummary(event.data) } } as eventWithTime;
  return visit(event) as eventWithTime;
}
