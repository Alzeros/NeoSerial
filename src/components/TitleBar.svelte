<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { ask } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import { Pin, PinOff, PanelRight, PanelRightClose, Settings as SettingsIcon, Plus } from 'lucide-svelte';
  import { scriptPanelOpen, toggleScriptPanel, currentPort, mcpOnlyConnections } from '$lib/stores';
  import { openPortWindow, hasActiveSessions, getMcpOnlyConnections, onMcpConnectionsChanged } from '$lib/tauri';
  import SettingsDialog from '$components/SettingsDialog.svelte';

  const appWindow = getCurrentWindow();
  // 副窗口(label=win-*)顶部加提示区分 main;连了端口后显示 port
  const isMain = getCurrentWebview().label === 'main';
  const titleText = $derived(
    isMain ? 'NeoSerial'
    : currentPort.value ? `NeoSerial · ${currentPort.value}`
    : 'NeoSerial · 新窗口'
  );

  let alwaysOnTop = $state<{ value: boolean }>({ value: false });
  let settingsDialog: SettingsDialog;

  async function handleMinimize() {
    await appWindow.minimize();
  }

  async function handleToggleMaximize() {
    await appWindow.toggleMaximize();
  }

  async function handleClose() {
    // main 窗口:有其他窗口 或 活跃连接时,二次确认(关 main 会断所有连接+退 app+停 MCP)
    if (isMain) {
      const { other_windows, connections } = await hasActiveSessions();
      if (other_windows > 0 || connections > 0) {
        const detail = [
          other_windows > 0 ? `${other_windows} 个其他窗口` : '',
          connections > 0 ? `${connections} 个串口连接` : '',
        ].filter(Boolean).join('、');
        const confirmed = await ask(
          `关闭主窗口会同时关闭${detail}并停止 MCP 服务,确定吗?`,
          { title: '关闭 NeoSerial', kind: 'warning', okLabel: '关闭', cancelLabel: '取消' }
        );
        if (!confirmed) return;
      }
    }
    await appWindow.close();
  }

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

  // 快捷打开 MCP 已连端口:开一个窗口并自动接管该 MCP 连接。
  // 后端 open_port_window(port,baud) 开窗后发 auto-connect-port 事件,
  // 新窗口 onMount 匹配自己的 label 后自动 connect 接管。
  async function handleTakeover(port: string, baud: number) {
    try {
      await openPortWindow(port, baud);
    } catch (e) {
      console.error('快捷打开端口失败:', e);
    }
  }

  // hover + 号时弹出"可接管端口"菜单。用容器 hover(非按钮 hover)避免
  // 从按钮移到浮层时 leave 关闭。mouseleave 容器才收起。
  let showTakeoverMenu = $state(false);
  function openTakeoverMenu() {
    if (mcpOnlyConnections.value.length > 0) showTakeoverMenu = true;
  }
  function closeTakeoverMenu() {
    showTakeoverMenu = false;
  }

  // 拉取"agent 连了但没 GUI 接管"的端口列表 + 监听变化刷新。
  // chip 是全局连接视图,任何窗口都看同一份;主窗口 onMount 启动刷新。
  async function refreshMcpOnly() {
    try {
      mcpOnlyConnections.value = await getMcpOnlyConnections();
    } catch (e) {
      console.error('查询 MCP 连接失败:', e);
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
</script>

<!-- 自定义标题栏：左侧应用名 + 右侧 脚本折叠按钮 | 窗口控制按钮 -->
<div
  class="flex items-center h-8 border-b border-[var(--border)] select-none"
  style="background: var(--background-elevated);"
>
  <!-- 左侧：应用名 + 拖动区域(main 显 NeoSerial;副窗口加提示/端口区分) -->
  <div
    data-tauri-drag-region
    class="flex-1 h-full flex items-center px-3 text-[13px] font-medium text-[var(--muted-foreground)]"
    ondblclick={handleTitleDblClick}
  >
    {titleText}
  </div>

  <!-- 新窗口按钮 + MCP 可接管提示:
       - 无 MCP 连接:普通 + 号,点击开空白窗口。
       - 有 MCP 连接(agent 连了没 GUI 接管的):+ 号右上角显红点;
         hover 弹出浮层列出可接管端口(点击开窗接管)+ "新开空白窗口"项。 -->
  <div
    class="relative h-full flex items-center"
    onmouseenter={openTakeoverMenu}
    onmouseleave={closeTakeoverMenu}
  >
    <button
      class="relative flex items-center h-full px-3 text-[13px] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
      onclick={handleNewWindow}
      title={mcpOnlyConnections.value.length > 0 ? '打开新窗口 / 接管 MCP 连接' : '打开新窗口'}
    >
      <Plus size={15} />
      {#if mcpOnlyConnections.value.length > 0}
        <!-- 红点:右上角,提示有可接管的 MCP 连接。pointer-events-none 避免挡点击 -->
        <span
          class="absolute top-1 right-1.5 w-1.5 h-1.5 rounded-full pointer-events-none"
          style="background: #dc2626; box-shadow: 0 0 0 1px var(--background-elevated);"
        ></span>
      {/if}
    </button>

    {#if showTakeoverMenu}
      <!-- hover 浮层:列出 agent 连了但没接管的端口 + 新开空白窗口。
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
            <span class="font-medium">接管 {c.port}</span>
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
    title="关闭"
  >
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
      <path d="M1 1L11 11M11 1L1 11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
    </svg>
  </button>
</div>

<SettingsDialog bind:this={settingsDialog} />
