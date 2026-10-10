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
    children,
  }: {
    title: string;
    /** 折叠时头部右侧的当前值摘要:收起来也看得见状态,不必逐个点开找 */
    summary?: string;
    help?: string;
    open?: boolean;
    children: any;
  } = $props();
</script>

<!-- 设置页里的一节:头部一行(箭头 + 标题 + 摘要),展开才渲染内容。
     按内容高度排布,不参与父容器的高度分配——超出由设置页内容区自己滚。
     分隔线下留 2px:展开时内容与下一节的标题行贴在一起看着发闷。 -->
<div data-settings-collapse class="mb-0.5" style="border-bottom: 1px solid var(--border-subtle);">
  <div class="flex items-center gap-1">
  <button
    type="button"
    class="flex flex-1 min-w-0 items-center gap-1.5 w-full text-left px-1 py-2 rounded transition-colors hover:bg-[var(--overlay-hover)]"
    style="color: var(--muted-foreground);"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <span class="collapse-chevron" class:expanded={open} aria-hidden="true"><ChevronRight size={13} /></span>
    <span class="text-[13px] font-medium shrink-0" style="color: var(--foreground);">{title}</span>
    {#if summary}
      <span class="ml-auto text-[12px] truncate pl-2" style="max-width: 60%;">{summary}</span>
    {/if}
  </button>
  {#if help}<SettingsHelp label={title} text={help} />{/if}
  </div>
  {#if open}
    <!-- pt-2:标题行的 py-2 是按钮内边距,hover/展开时的灰底跟着一起延伸,
         底边会贴着第一行内容。这 8px 让灰条与内容分开。 -->
    <div data-collapse-content class="px-1 pt-2 pb-3" inert={!open}
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
