<script lang="ts">
  let {
    value = $bindable(''),
    options,
    width = '90px',
    disabled = false,
    onAddOption,
  }: {
    value?: string;
    options: { label: string; value: string }[];
    width?: string;
    disabled?: boolean;
    onAddOption?: () => void;
  } = $props();

  let open = $state(false);
  let highlightIndex = $state(-1);
  let container: HTMLDivElement;

  const selectedLabel = $derived(
    options.find((o) => o.value === value)?.label ?? value,
  );
  const selectedIndex = $derived(
    options.findIndex((o) => o.value === value),
  );

  function toggleOpen() {
    if (disabled) return;
    open = !open;
    if (open) {
      highlightIndex = selectedIndex >= 0 ? selectedIndex : 0;
    }
  }

  function selectOption(opt: { label: string; value: string }) {
    value = opt.value;
    open = false;
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (disabled) return;
    if (!open) {
      if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown') {
        e.preventDefault();
        toggleOpen();
      }
      return;
    }
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        highlightIndex = Math.min(highlightIndex + 1, onAddOption ? options.length : options.length - 1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        highlightIndex = Math.max(highlightIndex - 1, 0);
        break;
      case 'Enter':
        e.preventDefault();
        if (highlightIndex === options.length && onAddOption) {
          onAddOption();
          open = false;
        } else if (highlightIndex >= 0 && highlightIndex < options.length) {
          selectOption(options[highlightIndex]);
        }
        break;
      case 'Escape':
        e.preventDefault();
        open = false;
        break;
      case 'Tab':
        open = false;
        break;
    }
  }

  function handleClickOutside(e: MouseEvent) {
    if (container && !container.contains(e.target as Node)) {
      open = false;
    }
  }

  $effect(() => {
    if (open) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  });
</script>

<div
  bind:this={container}
  class="relative flex-shrink-0"
  style="width: {width};"
>
  <button
    type="button"
    class="custom-select-trigger w-full flex items-center justify-center cursor-pointer select-none transition-[border-color,box-shadow]"
    style="height: 32px; padding: 0 28px 0 12px; border-radius: var(--radius-sm); border: 1px solid var(--border-strong); background-color: var(--background-input); color: var(--foreground); font-size: 13px; font-family: var(--font-sans); box-shadow: var(--shadow-sm);"
    {disabled}
    onclick={toggleOpen}
    onkeydown={handleKeyDown}
  >
    <span class="truncate text-center" style="flex: 1;">{selectedLabel}</span>
    <svg
      class="absolute right-2.5 transition-transform {open ? 'rotate-180' : ''}"
      width="10" height="6" viewBox="0 0 10 6" fill="none"
    >
      <path d="M1 1l4 4 4-4" stroke="var(--muted-foreground)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
    </svg>
  </button>

  {#if open && options.length > 0}
    <div
      class="absolute left-0 top-full mt-0.5 z-[300] py-1 overflow-y-auto"
      style="background: var(--background-elevated); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg); max-height: 240px; min-width: 100%;"
    >
      {#each options as opt, i (opt.value)}
        <div
          role="option"
          tabindex="-1"
          class="custom-select-option flex items-center justify-center cursor-pointer select-none transition-colors"
          style="height: 30px; padding: 0 12px; font-size: 13px; line-height: 1;
            {i === highlightIndex ? 'background: var(--overlay-hover);' : ''}
            {opt.value === value ? 'color: var(--primary); font-weight: 600;' : 'color: var(--foreground);'}"
          onclick={() => selectOption(opt)}
          onmouseenter={() => (highlightIndex = i)}
        >
          {opt.label}
        </div>
      {/each}
      {#if onAddOption}
        <div class="my-1 border-t" style="border-color: var(--border);"></div>
        <div
          class="custom-select-option flex items-center justify-center gap-1 cursor-pointer select-none transition-colors"
          style="height: 30px; padding: 0 12px; font-size: 13px; line-height: 1;
            {highlightIndex === options.length ? 'background: var(--overlay-hover);' : ''}
            color: var(--primary);"
          onclick={() => { onAddOption(); open = false; }}
          onmouseenter={() => (highlightIndex = options.length)}
        >
          <svg width="10" height="10" viewBox="0 0 12 12" fill="none">
            <path d="M6 1v10M1 6h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          添加…
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .custom-select-trigger:hover:not(:disabled) {
    border-color: var(--primary);
  }
  .custom-select-trigger:focus-visible {
    border-color: var(--primary);
    box-shadow: 0 0 0 3px var(--focus-ring);
    outline: none;
  }
  .custom-select-trigger:disabled {
    background-color: var(--background);
    opacity: 0.6;
    cursor: not-allowed;
    box-shadow: none;
  }
  .custom-select-option:hover {
    background: var(--overlay-hover);
  }
</style>
