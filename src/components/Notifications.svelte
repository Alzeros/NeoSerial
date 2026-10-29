<script lang="ts">
  import { dismissNotification, notifications, notifyError } from '$lib/notifications.svelte';
  import type { Notification } from '$lib/notificationModel';
  import CopyFeedback from '$components/ui/CopyFeedback.svelte';

  let copiedId = $state<number | null>(null);

  async function copyDetails(item: Notification) {
    copiedId = null;
    try {
      await navigator.clipboard.writeText(`${item.title}\n${item.detail}`);
      copiedId = item.id;
    } catch (error) {
      notifyError('复制错误详情失败', error);
    }
  }
</script>

<section data-notifications class="notification-host" class:has-items={notifications.items.length > 0} aria-label="操作通知">
  {#each notifications.items as item (item.id)}
    <div data-notification data-kind={item.kind} class="notification" class:is-error={item.kind === 'error'} role={item.kind === 'error' ? 'alert' : 'status'} aria-atomic="true">
      <div class="notification-heading">
        <strong>{item.title}</strong>
        {#if item.count > 1}<span class="repeat-count" aria-label={`重复 ${item.count} 次`}>×{item.count}</span>{/if}
      </div>
      <button class="dismiss" aria-label="关闭通知" title="关闭通知" onclick={() => dismissNotification(item.id)}>×</button>
      {#if item.detail}
        <details>
          <summary title="展开完整详情">{item.detail}</summary>
          <pre>{item.detail}</pre>
        </details>
      {/if}
      {#if item.kind === 'error'}
        <button class="copy" data-copy-details onclick={() => copyDetails(item)}>
          <CopyFeedback label="复制详情" copied={copiedId === item.id} />
        </button>
      {/if}
    </div>
  {/each}
</section>

<style>
  .notification-host {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: min(30vh, 260px);
    overflow-y: auto;
  }
  .notification-host.has-items { padding: 0 20px 8px; }
  .notification {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 2px 12px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-left: 3px solid var(--primary);
    border-radius: var(--radius-sm);
    background: var(--background-elevated);
    color: var(--foreground);
    font-size: 12px;
    line-height: 1.5;
  }
  .notification.is-error { border-left-color: var(--error); }
  .notification-heading { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  strong { font-weight: 600; overflow-wrap: anywhere; }
  .repeat-count { color: var(--muted-foreground); white-space: nowrap; }
  details { min-width: 0; color: var(--foreground-secondary); }
  summary { cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  pre { margin-top: 4px; max-height: 120px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font: inherit; }
  button { cursor: pointer; border-radius: var(--radius-sm); }
  button:hover { background: var(--overlay-hover); }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--primary); outline-offset: 2px; }
  .dismiss { width: 24px; height: 20px; font-size: 16px; color: var(--muted-foreground); }
  .copy { grid-column: 2; grid-row: 2; padding: 0 4px; color: var(--foreground-secondary); font-size: 11px; }
</style>
