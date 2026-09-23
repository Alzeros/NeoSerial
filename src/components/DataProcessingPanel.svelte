<script lang="ts">
  import { scriptModules, cachedSettings, settingsRequest, activeDataTool } from '$lib/stores';
  import { DATA_TOOL_OPTIONS, isFrameBuilderTool, isCodecTool, isSmsTool, initializeSmsToolDefaults } from '$lib/dataProcessing';
  import { smsDefaultsFromSettings } from '$lib/sms';
  import FrameBuilder from './FrameBuilder.svelte';
  import CodecTool from './CodecTool.svelte';
  import SmsTool from './SmsTool.svelte';

  const module = $derived(scriptModules.find((module) => module.type === 'data_processing'));
  const frameTool = $derived(module?.type === 'data_processing' ? module.tools.find(isFrameBuilderTool) : undefined);
  const codecTool = $derived(module?.type === 'data_processing' ? module.tools.find(isCodecTool) : undefined);
  const smsTool = $derived(module?.type === 'data_processing' ? module.tools.find(isSmsTool) : undefined);
  const visibleTools = $derived(DATA_TOOL_OPTIONS.filter((option) =>
    !(cachedSettings.value?.ui?.hidden_data_tools ?? []).includes(option.value),
  ));
  const selected = $derived(activeDataTool.value);
  $effect(() => {
    if (!visibleTools.some((tool) => tool.value === selected)) activeDataTool.value = visibleTools[0]?.value ?? '';
  });
  $effect(() => {
    if (selected === 'sms' && smsTool?.defaults_applied === false && cachedSettings.value) {
      initializeSmsToolDefaults(smsTool, smsDefaultsFromSettings(cachedSettings.value.ui));
    }
  });
</script>

<div class="flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
  <div class="flex shrink-0 items-center border-b border-[var(--border)] px-4 py-2" style="background: var(--background-elevated);">
    {#if visibleTools.length}
      <div data-data-tool-tabs role="group" aria-label="数据处理工具"
        class="inline-flex items-center gap-0.5 rounded-lg bg-[var(--background-deep)] p-0.5">
        {#each visibleTools as tool (tool.value)}
          <button type="button" aria-pressed={selected === tool.value}
            class="h-7 min-w-[72px] rounded-md px-3 text-[12px] font-medium leading-none transition-colors {selected === tool.value ? 'bg-[var(--background-input)] text-[var(--primary)] shadow-sm' : 'text-[var(--muted-foreground)] hover:bg-[var(--overlay-hover)] hover:text-[var(--foreground)]'}"
            onclick={() => (activeDataTool.value = tool.value)}>{tool.label}</button>
        {/each}
      </div>
    {/if}
  </div>
  <!-- 保持挂载以保留切换前的结果和展开状态，隐藏面板不参与布局。 -->
  <div class="min-h-0 min-w-0 flex-1 flex-col overflow-hidden" style:display={selected === 'frame_builder' ? 'flex' : 'none'}>
    {#if frameTool}<FrameBuilder tool={frameTool} />{:else}<p class="p-4 text-[13px] text-[var(--muted-foreground)]">帧构造器未就绪</p>{/if}
  </div>
  <div class="min-h-0 min-w-0 flex-1 flex-col overflow-hidden" style:display={selected === 'codec' ? 'flex' : 'none'}>
    {#if codecTool}
      <CodecTool config={codecTool.config} onconfigchange={(config) => { if (codecTool) codecTool.config = config; }} />
    {:else}<p class="p-4 text-[13px] text-[var(--muted-foreground)]">编解码工具未就绪</p>{/if}
  </div>
  <div class="min-h-0 min-w-0 flex-1 flex-col overflow-hidden" style:display={selected === 'sms' ? 'flex' : 'none'}>
    {#if smsTool}
      <SmsTool config={smsTool.config} onconfigchange={(config) => {
        if (smsTool) { smsTool.config = config; smsTool.defaults_applied = true; }
      }} />
    {:else}<p class="p-4 text-[13px] text-[var(--muted-foreground)]">短信工具未就绪</p>{/if}
  </div>
  {#if visibleTools.length === 0}
    <div class="p-4 text-[13px] text-[var(--muted-foreground)]">
      <p>尚未启用数据处理工具。</p>
      <button type="button" class="mt-2 text-[var(--primary)] hover:underline"
        onclick={() => { settingsRequest.section = 'extensions'; settingsRequest.extModule = 'data'; }}>打开扩展设置</button>
    </div>
  {/if}
</div>
