<script module lang="ts">
  let nextId = 0;
</script>

<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Info } from 'lucide-svelte';

  let { label, text }: { label: string; text: string } = $props();
  const id = `settings-help-${++nextId}`;
  let trigger: HTMLButtonElement;
  let tooltip = $state<HTMLDivElement>();
  let visible = $state(false);
  let pinned = false;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  function cancelHide() {
    clearTimeout(hideTimer);
  }

  function show() {
    cancelHide();
    visible = true;
  }

  function close() {
    cancelHide();
    visible = false;
    pinned = false;
  }

  function scheduleHide() {
    cancelHide();
    hideTimer = setTimeout(() => {
      if (!pinned && document.activeElement !== trigger) close();
    }, 150);
  }

  function position(node: HTMLDivElement) {
    const anchor = trigger.getBoundingClientRect();
    const bounds = node.getBoundingClientRect();
    const left = Math.max(8, Math.min(anchor.left, window.innerWidth - bounds.width - 8));
    const below = anchor.bottom + 8;
    const top = below + bounds.height <= window.innerHeight - 8
      ? below : Math.max(8, anchor.top - bounds.height - 8);
    node.style.left = `${left}px`;
    node.style.top = `${top}px`;
  }

  $effect(() => {
    if (!visible) return;
    const outside = (event: PointerEvent) => {
      if (!trigger.contains(event.target as Node) && !tooltip?.contains(event.target as Node)) close();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopImmediatePropagation();
      close();
    };
    const scroll = (event: Event) => {
      if (!tooltip || tooltip.contains(event.target as Node)) return;
      let container = trigger.parentElement;
      while (container && getComputedStyle(container).overflowY !== 'auto') {
        container = container.parentElement;
      }
      const anchor = trigger.getBoundingClientRect();
      const bounds = container?.getBoundingClientRect();
      if (bounds && (anchor.top < bounds.top || anchor.bottom > bounds.bottom)) close();
      else position(tooltip);
    };
    document.addEventListener('pointerdown', outside, true);
    window.addEventListener('keydown', escape, true);
    document.addEventListener('scroll', scroll, true);
    window.addEventListener('resize', close);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      window.removeEventListener('keydown', escape, true);
      document.removeEventListener('scroll', scroll, true);
      window.removeEventListener('resize', close);
    };
  });

  onDestroy(cancelHide);
</script>

<button bind:this={trigger} type="button" class="settings-help"
  aria-label={`${label}说明`} aria-describedby={visible ? id : undefined}
  onpointerenter={show} onpointerleave={scheduleHide} onfocus={show} onblur={scheduleHide}
  onclick={() => { if (pinned) close(); else { pinned = true; show(); } }}>
  <Info size={14} aria-hidden="true" />
</button>
{#if visible}
  <div bind:this={tooltip} use:position {id} role="tooltip" class="settings-help-tooltip"
    onpointerenter={cancelHide} onpointerleave={scheduleHide}>{text}</div>
{/if}

<style>
  .settings-help {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    flex-shrink: 0;
    vertical-align: middle;
    color: var(--muted-foreground);
    border-radius: var(--radius-sm);
    cursor: help;
  }
  .settings-help:hover, .settings-help:focus-visible {
    color: var(--primary);
    background: var(--overlay-hover);
  }
  .settings-help:focus-visible {
    outline: 2px solid var(--primary);
    outline-offset: 1px;
  }
  .settings-help-tooltip {
    position: fixed;
    z-index: 300;
    width: max-content;
    max-width: min(320px, calc(100vw - 16px));
    max-height: calc(100vh - 16px);
    overflow: auto;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--background-elevated);
    color: var(--foreground);
    box-shadow: var(--shadow-md);
    font-size: 12px;
    font-weight: 400;
    line-height: 1.6;
    white-space: pre-line;
    overflow-wrap: anywhere;
    user-select: text;
  }
</style>
