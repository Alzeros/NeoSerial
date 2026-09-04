<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';
  import { Pin, PinOff, PanelRight, PanelRightClose, Settings as SettingsIcon, Plus } from 'lucide-svelte';
  import { scriptPanelOpen, toggleScriptPanel, currentPort, mcpOnlyConnections, settingsRequest, cachedSettings } from '$lib/stores';
  import { openPortWindow, getMcpOnlyConnections, onMcpConnectionsChanged } from '$lib/tauri';
  import SettingsDialog from '$components/SettingsDialog.svelte';

  const appWindow = getCurrentWindow();
  // 窗口一律平等(没有"主窗口"):标题只按本窗口的连接显示
  const titleText = $derived(
    currentPort.value ? `NeoSerial · ${currentPort.value}` : 'NeoSerial'
  );

  let alwaysOnTop = $state<{ value: boolean }>({ value: false });
  let settingsDialog: SettingsDialog;

  async function handleMinimize() {
    await appWindow.minimize();
  }

  async function handleToggleMaximize() {
    await appWindow.toggleMaximize();
  }

  // × 的实际行为由后端 CloseRequested 按"后台运行"设置决定(见 tray::close_plan):
  // 后台模式连接留在后台、应用常驻;轻量模式断开本窗口自己连的、关最后一个窗口退出。
  // 关非最后一个窗口不会伤到别的窗口或 agent,所以不需要前端二次确认。
  async function handleClose() {
    await appWindow.close();
  }
  const closeTitle = $derived(
    cachedSettings.value?.ui?.background_mode ? '关闭窗口（连接留在后台）' : '关闭'
  );

  async function handleToggleAlwaysOnTop() {
    alwaysOnTop.value = !alwaysOnTop.value;
    await appWindow.setAlwaysOnTop(alwaysOnTop.value);
  }

  // 点击设置按钮：直接打开设置面板（默认停在"关于"页）
  function handleOpenSettings() {
    settingsDialog?.show();
  }

  // 新窗口按钮:开一个完整串口界面的复制品(空白,用户进去自己选端口连接)。
  async function handleNewWindow() {
    try {
      await openPortWindow();
    } catch (e) {
      console.error('打开新窗口失败:', e);
    }
  }

  // 快捷打开一个后台连接(agent 建的 / 窗口关掉留下的):开新窗口并自动挂上。
  // 后端 open_port_window(port,baud) 先记 pending 再开窗,
  // 新窗口 onMount 取走 pending 后自动 connect 挂上(不重开串口)。
  async function handleTakeover(port: string, baud: number) {
    try {
      await openPortWindow(port, baud);
    } catch (e) {
      console.error('快捷打开端口失败:', e);
    }
  }

  // hover + 号时弹出"后台连接"菜单。用容器 hover(非按钮 hover)避免
  // 从按钮移到浮层时 leave 关闭。mouseleave 容器才收起。
  let showTakeoverMenu = $state(false);
  function openTakeoverMenu() {
    if (mcpOnlyConnections.value.length > 0) showTakeoverMenu = true;
  }
  function closeTakeoverMenu() {
    showTakeoverMenu = false;
  }

  // 拉取"没有窗口显示"的连接列表 + 监听变化刷新。
  // chip 是全局连接视图,任何窗口都看同一份;每个窗口 onMount 自行拉取。
  async function refreshMcpOnly() {
    try {
      mcpOnlyConnections.value = await getMcpOnlyConnections();
    } catch (e) {
      console.error('查询后台连接失败:', e);
    }
  }

  // 标题栏拖动区域：按住鼠标拖动移动窗口（data-tauri-drag-region 由 Tauri 拦截）
  // 双击标题栏切换最大化
  function handleTitleDblClick() {
    handleToggleMaximize();
  }

  onMount(() => {
    refreshMcpOnly();
    const unlisten = onMcpConnectionsChanged(() => refreshMcpOnly());
    return () => {
      unlisten.then((f) => f());
    };
  });

  // 跨组件打开设置页：settingsRequest.section 被写入时触发
  $effect(() => {
    const section = settingsRequest.section;
    if (section) {
      settingsDialog?.show(section as 'about' | 'general' | 'appearance' | 'mcp');
      settingsRequest.section = null;
    }
  });
</script>

<!-- 自定义标题栏：左侧应用名 + 右侧 脚本折叠按钮 | 窗口控制按钮 -->
<div
  class="flex items-center h-8 border-b border-[var(--border)] select-none"
  data-theme-target="background-elevated"
  style="background: var(--background-elevated);"
>
  <!-- 左侧：应用名 + 拖动区域(连了端口显示端口名) -->
  <div
    data-tauri-drag-region
    class="flex-1 h-full flex items-center px-3 text-[13px] font-medium text-[var(--muted-foreground)]"
    ondblclick={handleTitleDblClick}
  >
    {titleText}
  </div>

  <!-- 新窗口按钮 + 后台连接提示:
       - 无后台连接:普通 + 号,点击开空白窗口。
       - 有后台连接(agent 建的 / 窗口关掉留下的,没窗口显示):+ 号右上角显红点;
         hover 弹出浮层列出这些端口(点击开窗挂上)+ "新开空白窗口"项。托盘菜单是同一份列表。 -->
  <div
    class="relative h-full flex items-center"
    onmouseenter={openTakeoverMenu}
    onmouseleave={closeTakeoverMenu}
  >
    <button
      class="relative flex items-center h-full px-3 text-[13px] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
      onclick={handleNewWindow}
      title={mcpOnlyConnections.value.length > 0 ? '打开新窗口 / 打开后台连接' : '打开新窗口'}
    >
      <Plus size={15} />
      {#if mcpOnlyConnections.value.length > 0}
        <!-- 红点:右上角,提示有后台连接没窗口显示。pointer-events-none 避免挡点击 -->
        <span
          class="absolute top-1 right-1.5 w-1.5 h-1.5 rounded-full pointer-events-none"
          style="background: #dc2626; box-shadow: 0 0 0 1px var(--background-elevated);"
        ></span>
      {/if}
    </button>

    {#if showTakeoverMenu}
      <!-- hover 浮层:列出后台连接 + 新开空白窗口。
           absolute 定位在 + 号下方右对齐,top-0 紧贴按钮下沿(无间隙),
           避免鼠标从按钮移到浮层时穿过间隙触发 mouseleave 关闭。
           z-index 高于状态栏。 -->
      <div
        class="absolute right-0 top-full z-[200] min-w-[180px] rounded-md border shadow-lg py-1 text-[13px]"
        style="background: var(--background-elevated); border-color: var(--border);"
      >
        {#each mcpOnlyConnections.value as c (c.port)}
          <button
            class="w-full text-left px-3 py-1.5 hover:bg-[var(--border-subtle)] cursor-pointer transition-colors flex items-center gap-2"
            onclick={() => { handleTakeover(c.port, c.baud); closeTakeoverMenu(); }}
          >
            <span class="w-1.5 h-1.5 rounded-full flex-shrink-0" style="background: #dc2626;"></span>
            <span class="font-medium">打开 {c.port}</span>
            <span class="text-[var(--muted-foreground)] text-[11px] ml-auto">@{c.baud}</span>
          </button>
        {/each}
        <!-- 分隔线 -->
        <div class="my-1 border-t" style="border-color: var(--border);"></div>
        <button
          class="w-full text-left px-3 py-1.5 hover:bg-[var(--border-subtle)] cursor-pointer transition-colors flex items-center gap-2"
          onclick={() => { handleNewWindow(); closeTakeoverMenu(); }}
        >
          <Plus size={13} class="flex-shrink-0" />
          <span>新开空白窗口</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- 置顶按钮：切换窗口是否始终置于其他窗口之上。
       lucide pin / pin-off：未置顶=实心图钉（待钉），已置顶=斜杠图钉（已钉住）。 -->
  <button
    class="flex items-center h-full px-3 text-[13px] cursor-pointer transition-colors {alwaysOnTop.value
      ? 'text-[var(--primary)]'
      : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)]'}"
    onclick={handleToggleAlwaysOnTop}
    title={alwaysOnTop.value ? '取消置顶' : '窗口置顶'}
  >
    {#if alwaysOnTop.value}
      <PinOff size={15} />
    {:else}
      <Pin size={15} />
    {/if}
  </button>

  <!-- 脚本面板折叠按钮：lucide PanelRight / PanelRightClose -->
  <button
    class="flex items-center h-full px-3 text-[13px] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
    onclick={toggleScriptPanel}
    title={scriptPanelOpen.value ? '收起脚本面板' : '展开脚本面板'}
  >
    {#if scriptPanelOpen.value}
      <PanelRightClose size={16} />
    {:else}
      <PanelRight size={16} />
    {/if}
  </button>

  <!-- 设置按钮：直接打开设置面板（默认停在"关于"页） -->
  <button
    class="flex items-center h-full px-3 text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
    onclick={handleOpenSettings}
    title="设置"
  >
    <SettingsIcon size={15} />
  </button>

  <div class="w-px h-4 bg-[var(--border)]"></div>

  <!-- 窗口控制按钮 -->
  <button
    class="flex items-center justify-center w-12 h-full text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
    onclick={handleMinimize}
    title="最小化"
  >
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
      <rect x="1" y="5.5" width="10" height="1" fill="currentColor" />
    </svg>
  </button>
  <button
    class="flex items-center justify-center w-12 h-full text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
    onclick={handleToggleMaximize}
    title="最大化/还原"
  >
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
      <rect x="1.5" y="1.5" width="9" height="9" stroke="currentColor" stroke-width="1" fill="none" rx="1" />
    </svg>
  </button>
  <button
    class="flex items-center justify-center w-12 h-full text-[var(--muted-foreground)] hover:bg-[var(--error)] hover:text-white cursor-pointer transition-colors"
    onclick={handleClose}
    title={closeTitle}
  >
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
      <path d="M1 1L11 11M11 1L1 11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
    </svg>
  </button>
</div>

<SettingsDialog bind:this={settingsDialog} />
