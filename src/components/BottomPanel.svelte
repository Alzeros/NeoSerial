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
    loggingActive,
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
      // 发送后保留输入内容，便于重复发送/修改后再发
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

  // 点路径框：选/换日志文件路径（仅设路径，不自动开始记录）
  async function handleSelectLogPath() {
    try {
      const selectedPath = await saveFileDialog(
        '保存日志文件',
        `neoserial_${Date.now()}.log`,
        [{ name: '日志文件', extensions: ['log'] }, { name: '文本文件', extensions: ['txt'] }]
      );
      if (!selectedPath) return;
      // 仅记录路径，等用户点"开始记录"再启动；标记该文件尚未记录过
      loggingPath.value = selectedPath;
      loggingActive.value = false;
      logFileStarted = false;
    } catch (e) {
      console.error('选择日志路径失败:', e);
    }
  }

  // 该路径是否已启动过记录（区分首次新建 vs 停止后续写）
  let logFileStarted = $state(false);

  // 点"开始记录"按钮：
  // - 无路径 → 弹对话框选路径（选完不自动开始）
  // - 有路径且首次 → 传 path 新建覆盖
  // - 有路径且曾停止 → 不传 path 续写同一文件
  async function handleStartLogging() {
    if (!loggingPath.value) {
      await handleSelectLogPath();
      return;
    }
    try {
      const path = logFileStarted
        ? await startLogging()                 // 续写：不传 path，后端用 last_log_path append
        : await startLogging(loggingPath.value); // 首次：传 path 新建覆盖
      loggingPath.value = path;
      loggingActive.value = true;
      logFileStarted = true;
    } catch (e) {
      console.error('启动日志失败:', e);
    }
  }

  async function handleStopLogging() {
    try {
      await stopLogging();
      // 停止记录但保留路径，再次开始时续写同一文件
      loggingActive.value = false;
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

  // 文件发送 / 日志保存 折叠区：默认收起，点击标题展开
  let extraOpen = $state(false);
</script>

<div class="border-t border-[var(--border)]" style="background: var(--background-elevated);">
  <!-- 第一排：操作按钮 + 复选框（不换行，由左栏 min-width 兜底） -->
  <!-- 第一排：操作按钮 + 显示/发送开关 -->
  <div class="flex items-center gap-3 px-5 pt-3 pb-1.5">
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

    <div class="ml-auto flex-shrink-0">
      {#if paused.value}
        <button class="btn btn-primary" onclick={() => (paused.value = false)}>继续</button>
      {:else}
        <button class="btn btn-secondary" onclick={() => (paused.value = true)} disabled={!connected.value}>暂停</button>
      {/if}
    </div>
  </div>

  <!-- 第二排：时间戳 / 回车换行 / 记录发送 -->
  <div class="flex items-center gap-3 px-5 pb-3">
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

    <label class="switch flex-shrink-0">
      <input type="checkbox" bind:checked={logSendContent.value} />
      <span class="switch-track"></span>
      <span class="switch-label">记录发送</span>
    </label>
  </div>

  <!-- 第二排：发送输入（输入框始终可输入，仅发送按钮在未连接时禁用） -->
  <div class="flex items-center gap-0 px-5 pb-3">
    <input
      type="text"
      class="!rounded-r-0 !border-r-0"
      style="flex:1 1 0%;min-width:0;height:40px;"
      placeholder="输入要发送的内容..."
      bind:value={sendText.value}
      onkeydown={handleKeydown}
    />
    <button
      style="height:40px;padding:0 20px;border-radius:0 6px 6px 0;"
      class="btn btn-primary"
      onclick={handleSend}
      disabled={!connected.value}
    >发送</button>
  </div>

  <!-- 第三、四排：文件发送 + 日志保存（可折叠，默认收起，向上展开） -->
  <div class="flex flex-col-reverse">
    {#if extraOpen}
      <!-- 第四排：日志保存 -->
      <div class="flex items-center gap-3 px-5 pb-3">
        <button
          class="flex-1 min-w-0 h-10 rounded px-3 text-left text-sm cursor-pointer transition-colors flex items-center"
          style="background: var(--background-input); border: 1px solid var(--border); color: var(--muted-foreground);"
          onclick={handleSelectLogPath}
          title="点击选择/更换日志文件（新建覆盖）"
        >
          <span class="truncate">{loggingPath.value || '点击选择日志保存路径'}</span>
        </button>
        {#if loggingActive.value}
          <button class="btn btn-secondary min-w-[96px] h-10" onclick={handleStopLogging}>停止</button>
        {:else}
          <button class="btn btn-secondary min-w-[96px] h-10" onclick={handleStartLogging}>
            {logFileStarted ? '继续记录' : '开始记录'}
          </button>
        {/if}
      </div>

      <!-- 第三排：文件发送（进度内置进按钮） -->
      <div class="flex items-center gap-3 px-5 pb-3">
        <button
          class="flex-1 min-w-0 h-10 rounded px-3 text-left text-sm cursor-pointer transition-colors flex items-center"
          style="background: var(--background-input); border: 1px solid var(--border); color: var(--muted-foreground);"
          onclick={handleSelectFile}
        >
          <span class="truncate">{fileSendPath.value || '点击选择发送文件路径'}</span>
        </button>
        <button
          class="relative overflow-hidden min-w-[96px] h-10 btn btn-secondary"
          onclick={handleSendFile}
          disabled={!connected.value || !fileSendPath.value}
          title="发送文件"
        >
          <!-- 进度填充层 -->
          <span
            class="absolute inset-y-0 left-0 transition-[width] duration-150"
            style="width: {fileSendProgress.value}%; background: var(--primary); opacity: 0.18;"
          ></span>
          <span class="relative z-10 font-medium">发送文件 {fileSendProgress.value > 0 && fileSendProgress.value < 100 ? fileSendProgress.value + '%' : ''}</span>
        </button>
      </div>
    {/if}

    <!-- 折叠标题栏 -->
    <button
      class="flex items-center gap-1 px-5 py-1.5 text-[12px] text-[var(--muted-foreground)] hover:text-[var(--foreground)] hover:bg-[var(--border-subtle)] cursor-pointer transition-colors border-t border-[var(--border-subtle)]"
      onclick={() => (extraOpen = !extraOpen)}
      title={extraOpen ? '收起' : '展开文件发送 / 日志保存'}
    >
      <span class="inline-block transition-transform {extraOpen ? 'rotate-180' : ''}">▾</span>
      文件发送 / 日志保存
    </button>
  </div>
</div>
