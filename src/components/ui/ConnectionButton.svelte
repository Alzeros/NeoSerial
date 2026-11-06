<script lang="ts">
  import { tick } from 'svelte';
  import { Check, ChevronDown, Repeat2 } from 'lucide-svelte';

  let { connected, autoEnabled, busy, canAutoConnect, title, onPrimary, onAutoToggle }: {
    connected: boolean;
    autoEnabled: boolean;
    busy: boolean;
    canAutoConnect: boolean;
    title: string;
    onPrimary: () => void;
    onAutoToggle: () => void;
  } = $props();

  let open = $state(false);
  let container = $state<HTMLDivElement>();
  let trigger = $state<HTMLButtonElement>();
  let option = $state<HTMLButtonElement>();
  let menu = $state<HTMLDivElement>();
  let position = $state({ right: 0, top: 0 });
  const active = $derived(connected || autoEnabled);

  async function toggleMenu(focusOption = false) {
    if (open) { open = false; return; }
    const bounds = container?.getBoundingClientRect();
    if (!bounds) return;
    position = { right: Math.max(4, window.innerWidth - bounds.right), top: bounds.bottom + 4 };
    open = true;
    if (focusOption) { await tick(); option?.focus(); }
  }

  function closeMenu(restoreFocus = false) {
    open = false;
    if (restoreFocus) trigger?.focus();
  }

  function handleOutside(event: PointerEvent) {
    if (open && event.target instanceof Node && !container?.contains(event.target) && !menu?.contains(event.target)) closeMenu();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === 'Escape') { event.preventDefault(); closeMenu(true); }
    else if (event.key === 'Tab') closeMenu();
    else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) { event.preventDefault(); option?.focus(); }
  }
</script>

<svelte:window onpointerdown={handleOutside} onkeydown={handleKeydown} onresize={() => closeMenu()} onscroll={() => closeMenu()} />

<div bind:this={container} class="connection-action flex shrink-0" style="width: 104px;" data-connection-action>
  <button
    class="btn connection-button inline-flex items-center justify-center gap-1 {active ? 'btn-danger' : 'btn-primary'}"
    style="width: 76px; padding-left: 8px; padding-right: 8px; border-top-right-radius: 0; border-bottom-right-radius: 0;"
    disabled={busy}
    {title}
    onclick={() => { closeMenu(); onPrimary(); }}
  >
    {#if autoEnabled}<Repeat2 size={13} aria-hidden="true" />{/if}
    {active ? '断开' : '连接'}
  </button>
  <button
    bind:this={trigger}
    class="btn connection-button relative inline-flex items-center justify-center {active ? 'btn-danger' : 'btn-primary'}"
    style="width: 28px; padding-left: 0; padding-right: 0; border-left: 0; border-top-left-radius: 0; border-bottom-left-radius: 0;"
    aria-label="连接选项"
    aria-haspopup="menu"
    aria-expanded={open}
    title={autoEnabled ? '连接选项：自动连接已开启' : '连接选项'}
    onclick={() => toggleMenu()}
    onkeydown={event => { if (event.key === 'ArrowDown') { event.preventDefault(); void toggleMenu(true); } }}
  >
    <span class="absolute left-0 h-4 border-l border-current opacity-30" aria-hidden="true"></span>
    <ChevronDown size={13} aria-hidden="true" />
  </button>
</div>

{#if open}
  <div
    bind:this={menu}
    role="menu"
    tabindex="-1"
    aria-label="连接选项"
    class="fixed z-[300] py-1"
    style="right: {position.right}px; top: {position.top}px; width: 160px; max-width: calc(100vw - 8px); background: var(--background-elevated); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg);"
  >
    <button
      bind:this={option}
      type="button"
      role="menuitemcheckbox"
      aria-checked={autoEnabled}
      disabled={busy || (!autoEnabled && !canAutoConnect)}
      class="flex w-full items-center gap-2 px-3 text-left text-[13px] hover:bg-[var(--overlay-hover)] focus-visible:bg-[var(--overlay-hover)] focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
      style="height: 32px; color: {autoEnabled ? 'var(--primary)' : 'var(--foreground)'};"
      title="仅本窗口本次运行生效；手动断开会关闭自动连接"
      data-auto-connect-toggle
      onclick={() => { closeMenu(true); onAutoToggle(); }}
    >
      <span class="inline-flex w-4 justify-center">{#if autoEnabled}<Check size={14} aria-hidden="true" />{/if}</span>
      自动连接
    </button>
  </div>
{/if}

<style>
  .connection-button:hover:not(:disabled),
  .connection-button:active:not(:disabled) {
    transform: none;
  }
</style>
