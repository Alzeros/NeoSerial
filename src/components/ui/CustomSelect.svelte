<script lang="ts">
  /** 选项:label = 触发按钮与菜单显示,value = 绑定值,tip = hover 提示(可空,如设备名)。 */
  type SelectOption = { label: string; value: string; tip?: string | null };
  let {
    value = $bindable(''),
    options,
    width = '90px',
    disabled = false,
    emptyLabel = '暂无选项',
    selectedFallbackLabel,
    onAddOption,
  }: {
    value?: string;
    options: SelectOption[];
    width?: string;
    disabled?: boolean;
    emptyLabel?: string;
    /** 当前 value 不在 options 中时，仅用于触发按钮显示，不会加入展开菜单。 */
    selectedFallbackLabel?: string;
    onAddOption?: () => void;
  } = $props();

  let open = $state(false);
  let highlightIndex = $state(-1);
  let container: HTMLDivElement;
  let triggerEl: HTMLButtonElement;
  let menuEl = $state<HTMLDivElement>();
  // fixed 定位坐标:突破父容器 overflow 裁剪
  let fixedPos = $state<{ left: number; top: number; width: number } | null>(null);

  const selectedLabel = $derived(
    options.find((o) => o.value === value)?.label ?? selectedFallbackLabel ?? value,
  );
  const selectedIndex = $derived(
    options.findIndex((o) => o.value === value),
  );

  // 选项 hover 提示:fixed 定在选项右侧,空间不足时翻到左侧。菜单容器
  // overflow-y-auto,提示必须渲染在菜单元素之外,否则会被裁掉;故这里独立渲染,
  // 生命周期跟着 open/fixedPos 走。
  const TIP_MAX_W = 240;
  let tip = $state<{ text: string; left: number; top: number; maxW: number } | null>(null);

  function showOptionTip(e: MouseEvent & { currentTarget: EventTarget & HTMLElement }, text: string) {
    const r = e.currentTarget.getBoundingClientRect();
    const gap = 8;
    const rightSpace = window.innerWidth - r.right - gap;
    const leftSpace = r.left - gap;
    // 默认放右侧;两侧都放不下整条提示时,选空间大的一侧并按该侧空间动态收窄
    // max-width(文字自动换行),保证既不裁切也不挡住菜单。
    const side = rightSpace >= leftSpace ? 'right' : 'left';
    const space = side === 'right' ? rightSpace : leftSpace;
    const maxW = Math.max(80, Math.min(TIP_MAX_W, space));
    const left = side === 'right'
      ? r.right + gap
      : Math.max(4, r.left - gap - maxW);
    // 顶出视口上沿时贴住,底部留出约一行提示的高度
    const top = Math.max(4, Math.min(r.top, window.innerHeight - 72));
    tip = { text, left, top, maxW };
  }

  function hideOptionTip() {
    tip = null;
  }

  // 所有关闭入口同步清理提示，避免下次展开时恢复上次悬停的设备名。
  function closeMenu() {
    open = false;
    fixedPos = null;
    hideOptionTip();
  }

  function toggleOpen() {
    if (disabled) return;
    if (open) {
      closeMenu();
      return;
    }
    hideOptionTip();
    open = true;
    highlightIndex = selectedIndex >= 0 ? selectedIndex : 0;
    // 计算 fixed 定位:突破父容器 overflow 裁剪
    const rect = triggerEl.getBoundingClientRect();
    fixedPos = { left: rect.left, top: rect.bottom + 4, width: rect.width };
  }

  function selectOption(opt: SelectOption) {
    value = opt.value;
    closeMenu();
  }

  // 选项是 div,不算交互元素:组件若放在 <label> 里(连接栏就是),点选项后 label 的默认行为
  // 会再给触发按钮补一次合成 click,把刚关掉的列表重新打开。preventDefault 取消这个默认行为。
  function handleOptionClick(e: MouseEvent, opt: SelectOption) {
    e.preventDefault();
    selectOption(opt);
  }

  function handleAddClick(e: MouseEvent) {
    e.preventDefault();
    onAddOption?.();
    closeMenu();
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
        hideOptionTip();
        highlightIndex = Math.min(highlightIndex + 1, onAddOption ? options.length : options.length - 1);
        break;
      case 'ArrowUp':
        e.preventDefault();
        hideOptionTip();
        highlightIndex = Math.max(highlightIndex - 1, 0);
        break;
      case 'Enter':
        e.preventDefault();
        if (highlightIndex === options.length && onAddOption) {
          onAddOption();
          closeMenu();
        } else if (highlightIndex >= 0 && highlightIndex < options.length) {
          selectOption(options[highlightIndex]);
        }
        break;
      case 'Escape':
        e.preventDefault();
        closeMenu();
        break;
      case 'Tab':
        closeMenu();
        break;
    }
  }

  function handleClickOutside(e: MouseEvent) {
    if (container && !container.contains(e.target as Node)) {
      closeMenu();
    }
  }

  $effect(() => {
    if (open) {
      document.addEventListener('mousedown', handleClickOutside);
      // fixed 定位不随外层滚动；菜单自身的滚动不能触发关闭。
      const closeOnScroll = (event: Event) => {
        // 菜单内滚动只隐藏提示，保留菜单；外层滚动则关闭整个菜单。
        hideOptionTip();
        if (event.target instanceof Node && menuEl?.contains(event.target)) return;
        closeMenu();
      };
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

  {#if open && fixedPos && tip}
    <div
      class="custom-select-tip fixed z-[310] px-2.5 py-1.5 rounded-md text-[12px] leading-normal shadow-lg pointer-events-none"
      style="left: {tip.left}px; top: {tip.top}px; max-width: {tip.maxW}px; white-space: normal; overflow-wrap: anywhere;
        background: var(--background-elevated); color: var(--foreground); border: 1px solid var(--border); border-radius: var(--radius-sm); box-shadow: var(--shadow-lg);"
    >
      {tip.text}
    </div>
  {/if}

  {#if open && fixedPos}
    <div
      bind:this={menuEl}
      class="fixed z-[300] py-1 overflow-y-auto overscroll-contain"
      style="left: {fixedPos.left}px; top: {fixedPos.top}px; width: {fixedPos.width}px; background: var(--background-elevated); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow-lg); max-height: 240px;"
    >
      {#if options.length === 0}
        <div class="flex items-center justify-center px-3 text-[12px] text-[var(--muted-foreground)]" style="height: 30px;">
          {emptyLabel}
        </div>
      {:else}
        {#each options as opt, i (opt.value)}
          <div
            role="option"
            tabindex="-1"
            class="custom-select-option flex items-center justify-center cursor-pointer select-none transition-colors"
            style="height: 30px; padding: 0 12px; font-size: 13px; line-height: 1;
              {i === highlightIndex ? 'background: var(--overlay-hover);' : ''}
              {opt.value === value ? 'color: var(--primary); font-weight: 600;' : 'color: var(--foreground);'}"
            onclick={(e) => handleOptionClick(e, opt)}
            onmouseenter={(e) => {
              highlightIndex = i;
              opt.tip ? showOptionTip(e, opt.tip) : hideOptionTip();
            }}
            onmouseleave={hideOptionTip}
          >
            {opt.label}
          </div>
        {/each}
      {/if}
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
