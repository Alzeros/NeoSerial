<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getVersion } from '@tauri-apps/api/app';
  import { Pin, PinOff, PanelRight, PanelRightClose, Settings as SettingsIcon, SlidersHorizontal, Info } from 'lucide-svelte';
  import { scriptPanelOpen, toggleScriptPanel } from '$lib/stores';
  import SettingsDialog from '$components/SettingsDialog.svelte';
  // 应用图标：从 src/assets 引入，Vite 自动处理打包（src-tauri/icons 在 watch ignored 中，无法直接 import）
  import appIcon from '$assets/icon.png';

  const appWindow = getCurrentWindow();

  let alwaysOnTop = $state<{ value: boolean }>({ value: false });
  // 设置下拉菜单开合 + 关于弹窗开合 + 应用版本号（懒加载）
  let menuOpen = $state<{ value: boolean }>({ value: false });
  let aboutOpen = $state<{ value: boolean }>({ value: false });
  let version = $state<{ value: string }>({ value: '' });
  let settingsDialog: SettingsDialog;

  async function handleMinimize() {
    await appWindow.minimize();
  }

  async function handleToggleMaximize() {
    await appWindow.toggleMaximize();
  }

  async function handleClose() {
    await appWindow.close();
  }

  async function handleToggleAlwaysOnTop() {
    alwaysOnTop.value = !alwaysOnTop.value;
    await appWindow.setAlwaysOnTop(alwaysOnTop.value);
  }

  // 打开设置下拉，同时（首次）拉取版本号，供"关于"项展示
  async function handleToggleMenu() {
    menuOpen.value = !menuOpen.value;
    if (menuOpen.value && !version.value) {
      try {
        version.value = await getVersion();
      } catch {
        version.value = '0.1.0';
      }
    }
  }

  function handleOpenAbout() {
    menuOpen.value = false;
    aboutOpen.value = true;
  }

  function handleOpenSettings() {
    menuOpen.value = false;
    settingsDialog?.show();
  }

  // 标题栏拖动区域：按住鼠标拖动移动窗口（data-tauri-drag-region 由 Tauri 拦截）
  // 双击标题栏切换最大化
  function handleTitleDblClick() {
    handleToggleMaximize();
  }
</script>

<svelte:window
  on:click={(e) => {
    // 点击下拉菜单外部时关闭（菜单内部点击 stopPropagation 阻止冒泡）
    if (menuOpen.value && !(e.target as HTMLElement).closest('[data-menu-root]')) {
      menuOpen.value = false;
    }
  }}
  on:keydown={(e) => {
    // Esc 关闭关于弹窗（弹窗不再支持点遮罩关闭）
    if (e.key === 'Escape' && aboutOpen.value) aboutOpen.value = false;
  }}
/>

<!-- 自定义标题栏：左侧应用名 + 右侧 脚本折叠按钮 | 窗口控制按钮 -->
<div
  class="flex items-center h-8 border-b border-[var(--border)] select-none"
  style="background: var(--background-elevated);"
>
  <!-- 左侧：应用名 + 拖动区域 -->
  <div
    data-tauri-drag-region
    class="flex-1 h-full flex items-center px-3 text-[13px] font-medium text-[var(--muted-foreground)]"
    ondblclick={handleTitleDblClick}
  >
    NeoSerial
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

  <!-- 设置按钮 + 下拉菜单 -->
  <div class="relative h-full flex items-center" data-menu-root>
    <button
      class="flex items-center h-full px-3 text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
      onclick={handleToggleMenu}
      title="设置"
    >
      <SettingsIcon size={15} />
    </button>
    {#if menuOpen.value}
      <div
        class="absolute right-0 top-8 min-w-[140px] border rounded-md shadow-lg py-1 z-50"
        style="background: var(--background-elevated); border-color: var(--border);"
        onclick={(e) => e.stopPropagation()}
      >
        <button
          class="flex items-center gap-2 w-full px-3 py-2 text-[13px] text-left text-[var(--foreground)] hover:bg-[var(--border-subtle)] cursor-pointer"
          onclick={handleOpenSettings}
        >
          <SlidersHorizontal size={14} />
          设置
        </button>
        <button
          class="flex items-center gap-2 w-full px-3 py-2 text-[13px] text-left text-[var(--foreground)] hover:bg-[var(--border-subtle)] cursor-pointer"
          onclick={handleOpenAbout}
        >
          <Info size={14} />
          关于
        </button>
      </div>
    {/if}
  </div>

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

<!-- 关于弹窗 -->
{#if aboutOpen.value}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center"
    style="background: rgba(0,0,0,0.35);"
  >
    <div
      class="rounded-lg shadow-xl w-[320px] border"
      style="background: var(--background-elevated); border-color: var(--border);"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="px-6 py-5 text-center">
        <img src={appIcon} alt="NeoSerial" class="w-16 h-16 mx-auto mb-3 rounded-lg shadow-sm" />
        <div class="text-[15px] font-semibold text-[var(--foreground)] mb-1">NeoSerial</div>
        <div class="text-[13px] text-[var(--muted-foreground)] mb-4">串口通信调试工具</div>
        <div class="text-[13px] text-[var(--muted-foreground)]">
          版本 <span class="text-[var(--foreground)] font-medium">{version.value || '0.1.0'}</span>
        </div>
      </div>
      <div class="flex justify-end px-4 pb-4">
        <button
          class="btn btn-primary"
          style="padding: 6px 16px;"
          onclick={() => (aboutOpen.value = false)}
        >确定</button>
      </div>
    </div>
  </div>
{/if}

<SettingsDialog bind:this={settingsDialog} />
