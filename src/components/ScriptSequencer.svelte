<script lang="ts">
  // 滑块图标而非齿轮:标题栏右上角已经有一枚齿轮(应用设置),两枚一模一样的齿轮上下挨着
  // 分不清谁是谁。滑块 = "调这个模块的参数",跟"应用设置"一眼区分开。
  import { SlidersHorizontal } from 'lucide-svelte';
  import CommandDetail from '$components/CommandDetail.svelte';
  import McpCallLog from '$components/McpCallLog.svelte';
  import QuickCommands from '$components/QuickCommands.svelte';
  import DataProcessingPanel from '$components/DataProcessingPanel.svelte';
  import { cachedSettings, settingsRequest } from '$lib/stores';

  // 面板顶部视图切换:快捷指令 / 数据处理 / 指令查询 / MCP 日志
  let scriptView = $state<'scripts' | 'data' | 'reference' | 'mcp'>('scripts');

  // tab 显隐 = 功能启用 AND 该模块 tab 开关。功能关时 tab 不显示但开关值保留(不改值,
  // 只控制是否生效);功能重开按原值生效。避免"关功能把 tab 值清掉、重开要重设"。
  const showSuggestTab = $derived(
    (cachedSettings.value?.command_index?.suggest_enabled ?? true) &&
    (cachedSettings.value?.ui?.show_suggest_tab ?? true),
  );
  const showMcpTab = $derived(
    (cachedSettings.value?.mcp?.auto_start ?? true) &&
    (cachedSettings.value?.ui?.show_mcp_tab ?? true),
  );
  // 功能/tab 关掉、当前正停在该 tab 时,弹回"快捷指令"
  $effect(() => {
    if (!showSuggestTab && scriptView === 'reference') scriptView = 'scripts';
    if (!showMcpTab && scriptView === 'mcp') scriptView = 'scripts';
  });

  // 用类型标注而不是 $derived<...>() 泛型:标注形式对三元里的对象字面量有上下文类型,
  // module 不会被推宽成 string,也不用给每个分支加 as const。
  type SettingsTarget = { module: 'quick' | 'suggest' | 'mcp'; title: string };
  const settingsTarget: SettingsTarget | null = $derived(
    scriptView === 'scripts'
      ? { module: 'quick', title: '快捷指令设置(字号/输入框高度/行距/字体)' }
      : scriptView === 'reference'
        ? { module: 'suggest', title: '指令联想设置(知识库地址/刷新/手册勾选)' }
        : scriptView === 'mcp'
          ? { module: 'mcp', title: 'MCP 服务设置(端口/自启/接入命令)' }
          : null, // 'data':无设置页
  );
</script>

<div class="flex h-full flex-col border-l border-[var(--border)]" data-theme-target="background-elevated" style="background: var(--background-elevated);">
  <!-- 顶部视图切换:快捷指令 / 数据处理 / 指令查询 / MCP 日志。用文字下划线风格(轻),与下方页签的实心块(重)拉开层级。 -->
  <div class="flex items-center gap-4 border-b border-[var(--border)] px-4 py-1" style="background: var(--background);">
    <button
      class="text-[13px] font-medium transition-colors cursor-pointer pb-0.5 border-b-2 {scriptView === 'scripts'
        ? 'text-[var(--foreground)] border-[var(--primary)]'
        : 'text-[var(--muted-foreground)] border-transparent hover:text-[var(--foreground)]'}"
      onclick={() => (scriptView = 'scripts')}
    >快捷指令</button>
    <!-- 紧接在「快捷指令」之后:数据处理页签(spec:第二项,不是加在最后) -->
    <button
      class="text-[13px] font-medium transition-colors cursor-pointer pb-0.5 border-b-2 {scriptView === 'data'
        ? 'text-[var(--foreground)] border-[var(--primary)]'
        : 'text-[var(--muted-foreground)] border-transparent hover:text-[var(--foreground)]'}"
      onclick={() => (scriptView = 'data')}
    >数据处理</button>
    {#if showSuggestTab}
      <button
        class="text-[13px] font-medium transition-colors cursor-pointer pb-0.5 border-b-2 {scriptView === 'reference'
          ? 'text-[var(--foreground)] border-[var(--primary)]'
          : 'text-[var(--muted-foreground)] border-transparent hover:text-[var(--foreground)]'}"
        onclick={() => (scriptView = 'reference')}
      >指令查询</button>
    {/if}
    {#if showMcpTab}
      <button
        class="text-[13px] font-medium transition-colors cursor-pointer pb-0.5 border-b-2 {scriptView === 'mcp'
          ? 'text-[var(--foreground)] border-[var(--primary)]'
          : 'text-[var(--muted-foreground)] border-transparent hover:text-[var(--foreground)]'}"
        onclick={() => (scriptView = 'mcp')}
      >MCP 日志</button>
    {/if}
    <!-- 跳当前 tab 对应的扩展设置子页。三个 tab 共用这一枚、位置固定在右上角,
         比各自在内容区里再摆一个更好找(MCP 日志那页也没有能挂按钮的表头行)。
         22px 见方,不超过 tab 文字那行的高度,不会把这条撑高。
         'data' 视图无设置页,滑块整枚隐藏。 -->
    {#if settingsTarget}
      <button
        type="button"
        class="ml-auto shrink-0 flex items-center justify-center rounded transition-colors text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer"
        style="width: 22px; height: 22px;"
        title={settingsTarget.title}
        onclick={() => { settingsRequest.section = 'extensions'; settingsRequest.extModule = settingsTarget!.module; }}
      ><SlidersHorizontal size={14} /></button>
    {/if}
  </div>

  {#if scriptView === 'scripts'}
    <QuickCommands />
  {:else if scriptView === 'data'}
    <DataProcessingPanel />
  {:else if scriptView === 'reference'}
    <CommandDetail />
  {:else}
    <McpCallLog />
  {/if}
</div>
