<script lang="ts">
  import { onMount } from 'svelte';
  import ConnectionBar from '$components/ConnectionBar.svelte';
  import LogView from '$components/LogView.svelte';
  import BottomPanel from '$components/BottomPanel.svelte';
  import StatusBar from '$components/StatusBar.svelte';
  import ScriptSequencer from '$components/ScriptSequencer.svelte';
  import {
    appendLogLine,
    connected,
    currentPort,
    logLines,
    rxBytes,
    scriptPanelOpen,
    scriptPanelWidth,
    scriptRunning,
    txBytes,
  } from '$lib/stores';
  import type { LogLine } from '$lib/types';
  import {
    onConnectionMode,
    onConnectionState,
    onError,
    onRxLine,
    onRxUpdate,
    onSequenceDone,
    onSequenceProgress,
    onTxLine,
    onTxUpdate,
  } from '$lib/tauri';

  function handleRxLine(line: LogLine) {
    appendLogLine(line);
  }

  let connectionMode = $state<{ mode: string | null }>({ mode: null });
  let showModeNotification = $state<{ value: boolean }>({ value: false });

  onMount(() => {
    const unlistenRxLine = onRxLine((line) => handleRxLine(line));
    const unlistenTxLine = onTxLine((line) => handleRxLine(line));
    const unlistenTx = onTxUpdate((u) => (txBytes.value = u.total));
    const unlistenRx = onRxUpdate((u) => (rxBytes.value = u.total));
    const unlistenState = onConnectionState((s) => {
      connected.value = s.connected;
      currentPort.value = s.port;
    });
    const unlistenSeqDone = onSequenceDone(() => {
      scriptRunning.value = false;
    });
    const unlistenError = onError((e) => {
      console.error('[Serial Error]', e.message);
    });
    const unlistenMode = onConnectionMode((mode) => {
      connectionMode.mode = mode.mode;
      if (mode.mode === 'shared') {
        showModeNotification.value = true;
        setTimeout(() => {
          showModeNotification.value = false;
        }, 5000);
      }
    });

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      unlistenRxLine.then((f) => f());
      unlistenTxLine.then((f) => f());
      unlistenTx.then((f) => f());
      unlistenRx.then((f) => f());
      unlistenState.then((f) => f());
      unlistenSeqDone.then((f) => f());
      unlistenError.then((f) => f());
      unlistenMode.then((f) => f());
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  });

  let isDragging = false;
  let leftPanel: HTMLDivElement;

  function handleMouseDown() {
    isDragging = true;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isDragging || !leftPanel) return;
    const containerWidth = leftPanel.parentElement?.clientWidth ?? 1200;
    const newWidth = containerWidth - e.clientX;
    scriptPanelWidth.value = Math.max(200, Math.min(600, newWidth));
  }

  function handleMouseUp() {
    isDragging = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  }
</script>

{#if showModeNotification.value && connectionMode.mode === 'shared'}
  <div class="fixed top-3 left-1/2 -translate-x-1/2 z-50 px-4 py-2 rounded-md text-[13px] font-medium shadow-lg flex items-center gap-2"
       style="background: var(--warning); color: var(--primary-foreground);">
    <span>⚠️</span>
    <span>串口克隆失败，已降级为共享端口模式（性能受限）</span>
    <button
      class="ml-2 opacity-70 hover:opacity-100 cursor-pointer"
      onclick={() => (showModeNotification.value = false)}
    >×</button>
  </div>
{/if}

<div class="flex h-screen w-screen overflow-hidden">
  <!-- 左侧主区域 -->
  <div bind:this={leftPanel} class="flex flex-col min-w-0" style="flex: 1;">
    <!-- 1. 会话配置区（顶部） -->
    <ConnectionBar />
    <!-- 2. 数据显示区（中部，占满剩余高度） -->
    <LogView />
    <!-- 3. 底部功能区 -->
    <BottomPanel />
    <!-- 状态栏 -->
    <StatusBar />
  </div>

  <!-- 分隔条 + 右侧脚本面板 -->
  {#if scriptPanelOpen.value}
    <div
      role="separator"
      aria-orientation="vertical"
      tabindex="0"
      class="w-1 cursor-col-resize shrink-0 transition-colors"
      style="background: var(--border);"
      onmousedown={handleMouseDown}
    ></div>
    <div class="shrink-0" style="width: {scriptPanelWidth.value}px;">
      <ScriptSequencer />
    </div>
  {/if}
</div>
