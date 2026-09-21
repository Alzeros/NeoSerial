<script lang="ts">
  // 数据处理视图:从 scriptModules 取 data_processing 模块的 frame_builder 子工具,
  // 渲染子工具页签(v1 一项)+ 表单;模块未就绪时给空态。
  import { scriptModules } from '$lib/stores';
  import { isDataProcessingModule, isFrameBuilderTool, type FrameBuilderTool } from '$lib/dataProcessing';
  import FrameBuilder from './FrameBuilder.svelte';

  const tool = $derived.by((): FrameBuilderTool | null => {
    // find 的谓词不收窄联合类型,这里只关心 tools 数组,取 any 再用结构守卫筛子工具
    const m = scriptModules.find(isDataProcessingModule) as any;
    const t = m?.tools?.find(isFrameBuilderTool);
    return t ?? null;
  });
</script>

<div class="flex h-full flex-col">
  <div class="flex items-center gap-4 border-b border-[var(--border)] px-4 py-1" style="background: var(--background);">
    <button class="text-[13px] font-medium pb-0.5 border-b-2 text-[var(--foreground)] border-[var(--primary)]">帧构造器</button>
  </div>
  {#if tool}
    <FrameBuilder {tool} />
  {:else}
    <div class="p-4 text-[13px] text-[var(--muted-foreground)]">数据处理模块未就绪</div>
  {/if}
</div>
