<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { Pin, PinOff, PanelRight, PanelRightClose } from 'lucide-svelte';
  import { scriptPanelOpen, toggleScriptPanel } from '$lib/stores';

  const appWindow = getCurrentWindow();

  let alwaysOnTop = $state<{ value: boolean }>({ value: false });

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

  // 标题栏拖动区域：按住鼠标拖动移动窗口（data-tauri-drag-region 由 Tauri 拦截）
  // 双击标题栏切换最大化
  function handleTitleDblClick() {
    handleToggleMaximize();
  }
</script>

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
