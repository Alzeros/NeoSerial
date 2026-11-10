<script lang="ts">
  import { ChevronRight } from 'lucide-svelte';
  import { slide } from 'svelte/transition';
  import { prefersReducedMotion } from 'svelte/motion';
  import SettingsHelp from './SettingsHelp.svelte';

  let {
    title,
    summary = '',
    help = '',
    open = $bindable(false),
    variant = 'row',
    helpIcon = 'info',
    children,
  }: {
    title: string;
    /** 收起时头部右侧的当前值摘要:收起来也看得见状态;展开后控件本身就显示当前值,不再重复 */
    summary?: string;
    help?: string;
    /** 说明气泡的图标:info 是常规帮助,alert 用于需要提醒的后果 */
    helpIcon?: 'info' | 'alert';
    open?: boolean;
    /** row:SettingsGroup 框里的一行,行间分隔线由框负责;inline:嵌在普通内容里的折叠(McpSetup) */
    variant?: 'row' | 'inline';
    children: any;
  } = $props();
  const row = $derived(variant === 'row');
</script>

<!-- 设置页里的一节:头部一行(箭头 + 标题 + 摘要),展开才渲染内容。
     按内容高度排布,不参与父容器的高度分配——超出由设置页内容区自己滚。
     row:头部上下 6px(行高约 31.5px,通用页 8 行全收起一屏放得下),悬停铺满整行;
     展开内容左缩 31px,与标题文字对齐。inline:保持原样式(底部淡分隔线)。 -->
<div data-settings-collapse class={row ? undefined : 'mb-0.5'} style={row ? undefined : 'border-bottom: 1px solid var(--border-subtle);'}>
  <div class="flex items-center gap-1 {row ? 'pr-2.5 transition-colors hover:bg-[var(--overlay-hover)]' : ''}">
  <button
    type="button"
    class="flex flex-1 min-w-0 items-center gap-1.5 w-full text-left {row ? 'py-1.5 pl-3' : 'px-1 py-2 rounded transition-colors hover:bg-[var(--overlay-hover)]'}"
    style="color: var(--muted-foreground);"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <span class="collapse-chevron" class:expanded={open} aria-hidden="true"><ChevronRight size={13} /></span>
    <span class="text-[13px] font-medium shrink-0" style="color: var(--foreground);">{title}</span>
    {#if summary && !open}
      <span class="ml-auto text-[12px] truncate pl-2" style="max-width: 60%;">{summary}</span>
    {/if}
  </button>
  {#if help}<SettingsHelp label={title} icon={helpIcon} text={help} />{/if}
  </div>
  {#if open}
    <!-- inline 的 pt-2:按钮自带 py-2,hover 灰底会贴着第一行内容,这 8px 把两者分开。
         row 的悬停底色在头部容器上,内容只需 2px 起步。 -->
    <div data-collapse-content class={row ? 'pt-0.5 pb-3.5 pl-[31px] pr-3' : 'px-1 pt-2 pb-3'} inert={!open}
      onintrostart={(event) => { event.currentTarget.inert = false; }}
      onoutrostart={(event) => { event.currentTarget.inert = true; }}
      transition:slide={{ duration: prefersReducedMotion.current ? 0 : 150 }}>
      {@render children?.()}
    </div>
  {/if}
</div>

<style>
  .collapse-chevron { display: inline-flex; flex-shrink: 0; transition: transform 150ms ease; }
  .collapse-chevron.expanded { transform: rotate(90deg); }
  @media (prefers-reduced-motion: reduce) {
    .collapse-chevron { transition: none; }
  }
</style>
