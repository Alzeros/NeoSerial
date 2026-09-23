<script lang="ts">
  // 滑块图标而非齿轮:标题栏右上角已经有一枚齿轮(应用设置),两枚一模一样的齿轮上下挨着
  // 分不清谁是谁。滑块 = "调这个模块的参数",跟"应用设置"一眼区分开。
  import { SlidersHorizontal } from 'lucide-svelte';
  import CommandDetail from '$components/CommandDetail.svelte';
  import McpCallLog from '$components/McpCallLog.svelte';
  import QuickCommands from '$components/QuickCommands.svelte';
  import DataProcessingPanel from '$components/DataProcessingPanel.svelte';
  import { cachedSettings, settingsRequest, scriptModules, activeScriptModule, activeScriptPage, currentModulePages } from '$lib/stores';
  import { loadSequenceAuto, saveSequenceAuto, onSequenceChanged } from '$lib/tauri';
  import { takeSequencePreload } from '$lib/startup';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { onMount } from 'svelte';

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
  const showDataTab = $derived(cachedSettings.value?.ui?.show_data_tab ?? true);
  // 功能/tab 关掉、当前正停在该 tab 时,弹回"快捷指令"
  $effect(() => {
    if (!showDataTab && scriptView === 'data') scriptView = 'scripts';
    if (!showSuggestTab && scriptView === 'reference') scriptView = 'scripts';
    if (!showMcpTab && scriptView === 'mcp') scriptView = 'scripts';
  });

  // ---- 自动加载 & 自动保存(常驻挂载,覆盖所有视图的自动保存与多窗口同步) ----
  // 本组件是右栏根组件、所有视图下都挂载;帧构造器编辑发生在 data 视图(QuickCommands
  // 已卸载),保存逻辑不能跟着 QuickCommands 走,否则 data 视图里改的配置不落盘、
  // 切回快捷指令时磁盘旧内容还会经 autoLoad 覆盖掉内存里的编辑。因此自动保存、
  // sequence-changed 监听、预取/autoLoad 三件事必须整体放在这个常驻组件里。
  // 启动时从默认路径加载(挂载前预取,见下方 onMount)；数据变化时防抖自动保存
  let autoSaveLoaded = $state(false);
  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null;
  // 最近一次落盘/载入内容的快照。自动保存只在当前内容与它不同时才写:
  // 同步 reload 把磁盘内容灌进 scriptModules 后自动保存 effect 会再跑一遍,不比对就会
  // "A reload → A 保存 → 广播 → B reload → B 保存 → 广播 → A reload …"每 800ms 一轮。
  let lastPersistedJson: string | null = null;

  // 从磁盘重新载入,尽量停在当前模块/页签(越界才回 0)。用于多窗口同步 reload;
  // 首次挂载不走这里——main.ts 挂载前已预取,见下方 onMount。
  async function autoLoad() {
    try {
      const modules = await loadSequenceAuto();
      if (modules.length > 0) {
        scriptModules.length = 0;
        scriptModules.push(...modules);
        lastPersistedJson = JSON.stringify(scriptModules);
        activeScriptModule.value = Math.min(activeScriptModule.value, scriptModules.length - 1);
        const pages = currentModulePages();
        activeScriptPage.value = Math.min(activeScriptPage.value, Math.max(0, pages.length - 1));
      }
    } catch (e) {
      console.error('自动加载序列配置失败:', e);
    }
  }

  // 防抖自动保存：数据变化 800ms 后写盘
  $effect(() => {
    // 深度追踪 scriptModules 的变化
    const json = JSON.stringify(scriptModules);
    if (!autoSaveLoaded) return;
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer);
      autoSaveTimer = null;
    }
    // 与磁盘一致(刚 reload / 刚保存 / 改回去了):不写,也不广播
    if (json === lastPersistedJson) return;
    autoSaveTimer = setTimeout(async () => {
      // 先清 timer:保存期间(await 中)的新改动会由 effect 重新起一个,不会丢;
      // 不清的话它永远非 null,下面 sequence-changed 的"有未保存改动"判断永远为真,
      // 多窗口同步就成了死代码。
      autoSaveTimer = null;
      const snapshot = JSON.stringify(scriptModules);
      try {
        await saveSequenceAuto(scriptModules);
        lastPersistedJson = snapshot;
      } catch (e) {
        console.error('自动保存序列配置失败:', e);
      }
    }, 800);
  });

  // 多窗口快捷指令同步:其他窗口改了 sequence.json 并保存时,广播 sequence-changed。
  // 本窗口收到(非自己触发的)→ reload 同步。若自己有未保存改动(autoSaveTimer pending)
  // → 跳过(等自己保存,最后保存的赢;改动频率低,冲突概率极小)。
  // 监听挂在常驻组件上,任何视图(包括 B 窗口停在 data 视图)都能收到广播,不会漏同步。
  const myLabel = getCurrentWebview().label;
  onMount(() => {
    // 首次挂载:main.ts 挂载前已经发起预取并(通常)完成,首帧画的就是 sequence.json 的内容;
    // 这里只接过"与磁盘一致"的基准快照(没有文件时为 null,预置内容随后被自动保存写盘)并放开自动保存。
    const preloaded = takeSequencePreload();
    if (preloaded) {
      preloaded.then((persisted) => {
        lastPersistedJson = persisted;
        autoSaveLoaded = true;
      });
    } else {
      autoLoad().then(() => { autoSaveLoaded = true; });
    }
    const unlisten = onSequenceChanged((e) => {
      if (e.source === myLabel) return; // 自己触发的跳过
      if (autoSaveTimer) return;        // 自己有未保存改动,跳过避免丢失
      autoLoad(); // reload 同步,停在当前页签
    });
    return () => { unlisten.then((f) => f()); };
  });

  // 用类型标注而不是 $derived<...>() 泛型:标注形式对三元里的对象字面量有上下文类型,
  // module 不会被推宽成 string,也不用给每个分支加 as const。
  type SettingsTarget = { module: 'quick' | 'data' | 'suggest' | 'mcp'; title: string };
  const settingsTarget: SettingsTarget | null = $derived(
    scriptView === 'scripts'
      ? { module: 'quick', title: '快捷指令设置(字号/输入框高度/行距/字体)' }
      : scriptView === 'data'
        ? { module: 'data', title: '数据处理设置(页签显示)' }
        : scriptView === 'reference'
          ? { module: 'suggest', title: '指令联想设置(知识库地址/刷新/手册勾选)' }
          : { module: 'mcp', title: 'MCP 服务设置(端口/自启/接入命令)' },
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
    {#if showDataTab}
      <!-- 紧接在「快捷指令」之后:数据处理页签(spec:第二项,不是加在最后) -->
      <button
        class="text-[13px] font-medium transition-colors cursor-pointer pb-0.5 border-b-2 {scriptView === 'data'
          ? 'text-[var(--foreground)] border-[var(--primary)]'
          : 'text-[var(--muted-foreground)] border-transparent hover:text-[var(--foreground)]'}"
        onclick={() => (scriptView = 'data')}
      >数据处理</button>
    {/if}
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
    <!-- 跳当前 tab 对应的扩展设置子页。四个 tab 共用这一枚、位置固定在右上角,
         比各自在内容区里再摆一个更好找(MCP 日志那页也没有能挂按钮的表头行)。
         22px 见方,不超过 tab 文字那行的高度,不会把这条撑高。 -->
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
