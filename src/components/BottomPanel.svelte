<script lang="ts">
  import { send, onFileSendProgress } from '$lib/tauri';
  import {
    connected,
    clearLogLines,
    fileSendPath,
    fileSendProgress,
    hexDisplay,
    hexSend,
    lineEnding,
    logSendContent,
    loggingPath,
    paused,
    sendText,
    showTimestamp,
  } from '$lib/stores';
  import { openFileDialog, saveFileDialog, sendFile, startLogging, stopLogging } from '$lib/tauri';
  import { onMount } from 'svelte';

  onMount(() => {
    const unlisten = onFileSendProgress((p) => {
      fileSendProgress.value = p.total > 0 ? Math.round((p.sent / p.total) * 100) : 0;
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  async function handleSend() {
    if (!sendText.value.trim()) return;
    try {
      await send(sendText.value, lineEnding.value, hexSend.value);
      sendText.value = '';
    } catch (e) {
      console.error('发送失败:', e);
    }
  }

  async function handleSendCtrlZ() {
    try {
      await send('1A', 'None', true);
    } catch (e) {
      console.error('发送 Ctrl-Z 失败:', e);
    }
  }

  async function handleSelectFile() {
    const path = await openFileDialog('选择要发送的文件');
    if (path) fileSendPath.value = path;
  }

  async function handleSendFile() {
    if (!fileSendPath.value) return;
    try {
      fileSendProgress.value = 0;
      await sendFile(fileSendPath.value);
      fileSendProgress.value = 100;
    } catch (e) {
      console.error('文件发送失败:', e);
    }
  }

  async function handleStartLogging() {
    try {
      const selectedPath = await saveFileDialog(
        '保存日志文件',
        `neoserial_${Date.now()}.log`,
        [{ name: '日志文件', extensions: ['log'] }, { name: '文本文件', extensions: ['txt'] }]
      );
      if (!selectedPath) return;
      const path = await startLogging(selectedPath);
      loggingPath.value = path;
    } catch (e) {
      console.error('启动日志失败:', e);
    }
  }

  async function handleStopLogging() {
    try {
      await stopLogging();
      loggingPath.value = null;
    } catch (e) {
      console.error('停止日志失败:', e);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }
</script>

<div class="border-t border-[var(--border)]" style="background: var(--background-elevated);">
  <!-- 第一排：操作按钮 + 复选框（不换行，由左栏 min-width 兜底） -->
  <div class="flex items-center gap-3 px-5 py-3">
    <button class="btn btn-secondary flex-shrink-0" onclick={clearLogLines}>清空</button>
    <button class="btn btn-secondary flex-shrink-0" onclick={handleSendCtrlZ} disabled={!connected.value}>发送 Ctrl-Z</button>

    <div class="w-px h-5 bg-[var(--border)] mx-1 flex-shrink-0"></div>

    <label class="switch flex-shrink-0">
      <input type="checkbox" bind:checked={hexDisplay.value} />
      <span class="switch-track"></span>
      <span class="switch-label">HEX显示</span>
    </label>

    <label class="switch flex-shrink-0">
      <input type="checkbox" bind:checked={hexSend.value} />
      <span class="switch-track"></span>
      <span class="switch-label">HEX发送</span>
    </label>

    <label class="switch flex-shrink-0">
      <input type="checkbox" bind:checked={showTimestamp.value} />
      <span class="switch-track"></span>
      <span class="switch-label">时间戳</span>
    </label>

    <label class="switch flex-shrink-0">
      <input type="checkbox" checked={lineEnding.value === 'Crlf'} onchange={(e) => lineEnding.value = (e.target as HTMLInputElement).checked ? 'Crlf' : 'None'} />
      <span class="switch-track"></span>
      <span class="switch-label">回车换行</span>
    </label>

    <div class="ml-auto flex-shrink-0">
      {#if paused.value}
        <button class="btn btn-primary" onclick={() => (paused.value = false)}>继续</button>
      {:else}
        <button class="btn btn-secondary" onclick={() => (paused.value = true)} disabled={!connected.value}>暂停</button>
      {/if}
    </div>
  </div>

  <!-- 第二排：发送输入 -->
  <div class="flex items-center gap-0 px-5 pb-3">
    <input
      type="text"
      class="!rounded-r-0 !border-r-0"
      style="flex:1 1 0%;min-width:0;height:40px;"
      placeholder="输入要发送的内容..."
      bind:value={sendText.value}
      onkeydown={handleKeydown}
      disabled={!connected.value}
    />
    <button
      style="height:40px;padding:0 20px;border-radius:0 6px 6px 0;"
      class="btn btn-primary"
      onclick={handleSend}
      disabled={!connected.value}
    >发送</button>
  </div>

  <!-- 第三排：文件发送 + 进度 -->
  <div class="flex items-center gap-3 px-5 pb-3">
    <button
      class="flex-1 min-w-0 h-10 rounded px-3 text-left text-sm cursor-pointer transition-colors flex items-center"
      style="background: var(--background-input); border: 1px solid var(--border); color: var(--muted-foreground);"
      onclick={handleSelectFile}
    >
      <span class="truncate">{fileSendPath.value || '点击选择发送文件路径'}</span>
    </button>
    <button class="btn btn-secondary" onclick={handleSendFile} disabled={!connected.value || !fileSendPath.value}>发送文件</button>
    <span class="w-16 text-center text-[var(--muted-foreground)] text-sm">{fileSendProgress.value}%</span>
  </div>

  <!-- 第四排：日志保存 + 记录选项 -->
  <div class="flex items-center gap-3 px-5 pb-3">
    <button
      class="flex-1 min-w-0 h-10 rounded px-3 text-left text-sm cursor-pointer transition-colors flex items-center"
      style="background: var(--background-input); border: 1px solid var(--border); color: var(--muted-foreground);"
      onclick={handleStartLogging}
    >
      <span class="truncate">{loggingPath.value || '点击选择日志保存路径'}</span>
    </button>
    <label class="switch">
      <input type="checkbox" bind:checked={logSendContent.value} />
      <span class="switch-track"></span>
      <span class="switch-label">记录发送</span>
    </label>
    {#if loggingPath.value}
      <button class="btn btn-secondary" onclick={handleStopLogging}>停止</button>
    {:else}
      <button class="btn btn-secondary" onclick={handleStartLogging}>开始记录</button>
    {/if}
  </div>
</div>
