import { addNotification, normalizeError, type Notification } from './notificationModel';

export const notifications = $state<{ items: Notification[] }>({ items: [] });
let nextId = 0;

export function notifyError(title: string, error: unknown): void {
  notifications.items = addNotification(notifications.items, {
    id: ++nextId, kind: 'error', title, detail: normalizeError(error),
  });
}

export function notifySuccess(title: string, detail = ''): void {
  notifications.items = addNotification(notifications.items, {
    id: ++nextId, kind: 'success', title, detail,
  });
}

export function dismissNotification(id: number): void {
  notifications.items = notifications.items.filter(item => item.id !== id);
}
