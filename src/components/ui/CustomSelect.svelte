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
  let triggerEl: HTMLButtonElement;
  // fixed 定位坐标:突破父容器 overflow 裁剪
  let fixedPos = $state<{ left: number; top: number; width: number } | null>(null);

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
      // 计算 fixed 定位:突破父容器 overflow 裁剪
      const rect = triggerEl.getBoundingClientRect();
      fixedPos = { left: rect.left, top: rect.bottom + 4, width: rect.width };
    } else {
      fixedPos = null;
    }
  }

  function selectOption(opt: { label: string; value: string }) {
    value = opt.value;
    open = false;
  }

  // 选项是 div,不算交互元素:组件若放在 <label> 里(连接栏就是),点选项后 label 的默认行为
  // 会再给触发按钮补一次合成 click,把刚关掉的列表重新打开。preventDefault 取消这个默认行为。
  function handleOptionClick(e: MouseEvent, opt: { label: string; value: string }) {
    e.preventDefault();
    selectOption(opt);
  }

  function handleAddClick(e: MouseEvent) {
    e.preventDefault();
    onAddOption?.();
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
      // fixed 定位不随父容器滚动:滚动时关闭,避免选项列表悬浮在错误位置
      const closeOnScroll = () => { open = false; fixedPos = null; };
      window.addEventListener('scroll', closeOnScroll, true);
      return () => {
        document.removeEventListener('mousedown', handleClickOutside);
        window.removeEventListener('scroll', closeOnScroll, true);
      };
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
    bind:this={triggerEl}
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

  {#if open && options.length > 0 && fixedPos}
    <div
      class="fixed z-[300] py-1 overflow-y-auto"
      style="left: {fixedPos.left}px; top: {fixedPos.top}px; width: {fixedPos.width}px; background: var(--background-elevated); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg); max-height: 240px;"
    >
      {#each options as opt, i (opt.value)}
        <div
          role="option"
          tabindex="-1"
          class="custom-select-option flex items-center justify-center cursor-pointer select-none transition-colors"
          style="height: 30px; padding: 0 12px; font-size: 13px; line-height: 1;
            {i === highlightIndex ? 'background: var(--overlay-hover);' : ''}
            {opt.value === value ? 'color: var(--primary); font-weight: 600;' : 'color: var(--foreground);'}"
          onclick={(e) => handleOptionClick(e, opt)}
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
          onclick={handleAddClick}
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
