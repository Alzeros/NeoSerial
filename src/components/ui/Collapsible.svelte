<script lang="ts">
  import { ChevronDown, ChevronRight } from 'lucide-svelte';

  let {
    title,
    summary = '',
    open = $bindable(false),
    children,
  }: {
    title: string;
    /** 折叠时头部右侧的当前值摘要:收起来也看得见状态,不必逐个点开找 */
    summary?: string;
    open?: boolean;
    children: any;
  } = $props();
</script>

<!-- 设置页里的一节:头部一行(箭头 + 标题 + 摘要),展开才渲染内容。
     按内容高度排布,不参与父容器的高度分配——超出由设置页内容区自己滚。
     分隔线下留 2px:展开时内容与下一节的标题行贴在一起看着发闷。 -->
<div class="mb-0.5" style="border-bottom: 1px solid var(--border-subtle);">
  <button
    type="button"
    class="flex items-center gap-1.5 w-full text-left px-1 py-2 rounded transition-colors hover:bg-[var(--border-subtle)]"
    style="color: var(--muted-foreground);"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    {#if open}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
    <span class="text-[13px] font-medium shrink-0" style="color: var(--foreground);">{title}</span>
    {#if summary}
      <span class="ml-auto text-[12px] truncate pl-2" style="max-width: 60%;">{summary}</span>
    {/if}
  </button>
  {#if open}
    <!-- pt-2:标题行的 py-2 是按钮内边距,hover/展开时的灰底跟着一起延伸,
         底边会贴着第一行内容。这 8px 让灰条与内容分开。 -->
    <div class="px-1 pt-2 pb-3">
      {@render children?.()}
    </div>
  {/if}
</div>
