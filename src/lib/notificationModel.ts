export const MAX_NOTIFICATIONS = 3;

export interface Notification {
  id: number;
  kind: 'error' | 'success';
  title: string;
  detail: string;
  count: number;
}

export function normalizeError(error: unknown): string {
  try {
    if (error == null || (typeof error === 'string' && !error.trim())) return '未知错误';
    if (typeof error === 'string') return error;
    if (error instanceof Error) return error.stack || error.message || error.name;
    if (typeof error !== 'object') return String(error);
    const seen = new WeakSet<object>();
    return JSON.stringify(error, (_key, value: unknown) => {
      if (typeof value === 'bigint') return String(value);
      if (value && typeof value === 'object') {
        if (seen.has(value)) return '[Circular]';
        seen.add(value);
        if (value instanceof Error) return { name: value.name, message: value.message, stack: value.stack };
      }
      return value;
    }, 2) || '未知错误';
  } catch {
    try {
      return String(error) || '未知错误';
    } catch {
      return '未知错误';
    }
  }
}

export function addNotification(items: readonly Notification[], incoming: Omit<Notification, 'count'>): Notification[] {
  const duplicate = items.find(item => item.kind === incoming.kind && item.title === incoming.title && item.detail === incoming.detail);
  const next = duplicate
    ? { ...duplicate, count: Math.min(duplicate.count + 1, Number.MAX_SAFE_INTEGER) }
    : { ...incoming, count: 1 };
  return [next, ...items.filter(item => item.id !== next.id)].slice(0, MAX_NOTIFICATIONS);
}
