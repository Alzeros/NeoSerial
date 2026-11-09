<script lang="ts">
  import { tick } from 'svelte';
  import { ChevronDown, Repeat2 } from 'lucide-svelte';

  let { connected, autoEnabled, autoSelected, busy, title, onPrimary, onAutoToggle }: {
    connected: boolean;
    autoEnabled: boolean;
    autoSelected: boolean;
    busy: boolean;
    title: string;
    onPrimary: () => void;
    onAutoToggle: (checked: boolean) => void;
  } = $props();

  let open = $state(false);
  let container = $state<HTMLDivElement>();
  let trigger = $state<HTMLButtonElement>();
  let option = $state<HTMLInputElement>();
  let menu = $state<HTMLDivElement>();
  let position = $state({ right: 0, top: 0, width: 0 });
  const active = $derived(connected || autoEnabled);

  async function toggleMenu(focusOption = false) {
    if (open) { open = false; return; }
    const bounds = container?.getBoundingClientRect();
    if (!bounds) return;
    position = { right: Math.max(4, window.innerWidth - bounds.right), top: bounds.bottom + 4, width: bounds.width };
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

<div bind:this={container} class="connection-action flex shrink-0" style="width: 96px;" data-connection-action>
  <button
    class="btn connection-button inline-flex items-center justify-center gap-1 {active ? 'btn-danger' : 'btn-primary'}"
    style="width: 72px; padding-left: 8px; padding-right: 8px; border-top-right-radius: 0; border-bottom-right-radius: 0;"
    disabled={busy}
    {title}
    onclick={() => { closeMenu(); onPrimary(); }}
    data-diagnostic-action={active ? 'disconnect' : 'connect'}
  >
    {#if autoSelected}<Repeat2 size={13} aria-hidden="true" />{/if}
    {active ? '断开' : '连接'}
  </button>
  <button
    bind:this={trigger}
    class="btn connection-button relative inline-flex items-center justify-center {active ? 'btn-danger' : 'btn-primary'}"
    style="width: 24px; padding-left: 0; padding-right: 0; border-left: 0; border-top-left-radius: 0; border-bottom-left-radius: 0;"
    aria-label="连接选项"
    aria-haspopup="menu"
    aria-expanded={open}
    title={autoSelected ? '连接选项：已勾选自动重连' : '连接选项'}
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
    style="right: {position.right}px; top: {position.top}px; width: {position.width}px; max-width: calc(100vw - 8px); background: var(--background-elevated); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg);"
  >
    <label
      class="flex w-full items-center gap-1.5 px-2 text-left text-[13px] hover:bg-[var(--overlay-hover)] focus-within:bg-[var(--overlay-hover)] {busy ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'}"
      style="height: 32px; color: {autoSelected ? 'var(--primary)' : 'var(--foreground)'};"
      title="勾选后点击连接生效；手动断开会停止自动重连"
    >
      <input
        bind:this={option}
        type="checkbox"
        role="menuitemcheckbox"
        checked={autoSelected}
        aria-checked={autoSelected}
        disabled={busy}
        class="h-4 w-4 shrink-0 cursor-pointer disabled:cursor-not-allowed"
        style="accent-color: var(--primary);"
        data-auto-connect-toggle
        onchange={event => onAutoToggle(event.currentTarget.checked)}
        onkeydown={event => { if (event.key === 'Enter') { event.preventDefault(); event.currentTarget.click(); } }}
      />
      <span class="whitespace-nowrap">自动重连</span>
    </label>
  </div>
{/if}

<style>
  .connection-button:hover:not(:disabled),
  .connection-button:active:not(:disabled) {
    transform: none;
  }
</style>
