<script lang="ts">
  import { send, onFileSendProgress } from '$lib/tauri';
  import {
    connected,
    clearLogLines,
    fileSendPath,
    fileSendProgress,
    displayMode,
    hexSend,
    lineEnding,
    logSendContent,
    loggingPath,
    loggingActive,
    paused,
    sendText,
    showTimestamp,
    appendLogLine,
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

  // 发送节流：30ms 最小间隔（约 33 次/秒），防止极端连按刷爆 IPC/串口
  let lastSendTime = 0;
  // 必须用 $state：发送中按钮要禁用+显示"..."。若用普通 let，finally 里设回 false
  // 不会触发 Svelte 重渲染，按钮会永远卡在"..."禁用态。
  let pendingSend = $state(false);
  let sendErrorFlash = $state(false);

  async function handleSend() {
    if (!sendText.value.trim()) return;
    if (!connected.value) return;

    const now = Date.now();
    // 节流：30ms 内的重复调用直接忽略
    if (now - lastSendTime < 30) return;
    lastSendTime = now;

    // 防重入：上一次 invoke 还没返回时忽略
    if (pendingSend) return;
    pendingSend = true;

    try {
      await send(sendText.value, lineEnding.value, hexSend.value);
      // 发送后保留输入内容，便于重复发送/修改后再发
    } catch (e) {
      console.error('发送失败:', e);
      sendErrorFlash = true;
      setTimeout(() => (sendErrorFlash = false), 300);
    } finally {
      pendingSend = false;
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

  // IME 状态：用于诊断和调试。组合中按 Enter 也立即发送（不补发），
  // 因为中文用户回车 = 发送意图，候选词确认走空格即可。
  let isComposing = $state(false);

  function handleKeydown(e: KeyboardEvent) {
    // Enter 键：无论是否在 IME 组合中都触发发送。
    // 中文输入法下 e.key 是 'Process' 不是 'Enter'，所以用 e.code 判断物理键位。
    if (e.code === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function handleCompositionStart() {
    isComposing = true;
  }
  function handleCompositionEnd() {
    isComposing = false;
  }

  // 文件发送 / 日志保存 折叠区：默认收起，点击标题展开
  let extraOpen = $state(false);
</script>

<div class="border-t border-[var(--border)]" style="background: var(--background-elevated);">
  <!-- 工具条 + 输入行：共用同一水平 padding（px-5），保证六点边缘对齐 -->
  <div class="px-5">
    <!-- 第一行：接收组（左按钮 + 右开关） -->
    <div class="flex items-center gap-2 pt-3 pb-2">
      <button data-m-clear class="btn btn-secondary flex-shrink-0 flex items-center justify-center btn-clear-hover" style="height: 32px; width: 96px; padding: 0;" onclick={clearLogLines}>清空</button>
      {#if paused.value}
        <button data-m-pause class="btn btn-primary flex-shrink-0 flex items-center justify-center" style="height: 32px; width: 96px; padding: 0;" onclick={() => (paused.value = false)}>继续</button>
      {:else}
        <button data-m-pause class="btn btn-secondary flex-shrink-0 flex items-center justify-center" style="height: 32px; width: 96px; padding: 0;" onclick={() => (paused.value = true)} disabled={!connected.value}>暂停</button>
      {/if}
      <div class="ml-auto flex items-center gap-4 flex-shrink-0">
        <label data-m-hexdisp class="switch flex-shrink-0 min-w-[96px]">
          <input type="checkbox" checked={displayMode.value === 'hex'} onchange={(e) => (displayMode.value = (e.target as HTMLInputElement).checked ? 'hex' : 'ascii')} />
          <span class="switch-track"></span>
          <span class="switch-label">HEX显示</span>
        </label>
        <label data-m-hexsend class="switch flex-shrink-0 min-w-[96px]">
          <input type="checkbox" bind:checked={hexSend.value} />
          <span class="switch-track"></span>
          <span class="switch-label">HEX发送</span>
        </label>
      </div>
    </div>

    <!-- 第二行：发送组（左按钮 + 右开关），行距 8px -->
    <div class="flex items-center gap-2 pb-3">
      <button class="btn btn-secondary flex-shrink-0 flex items-center justify-center" style="height: 32px; width: 96px; padding: 0;" onclick={handleSendCtrlZ} disabled={!connected.value}>发送 Ctrl-Z</button>
      <div class="ml-auto flex items-center gap-4 flex-shrink-0">
        <label data-m-ts class="switch flex-shrink-0 min-w-[96px]">
          <input type="checkbox" bind:checked={showTimestamp.value} />
          <span class="switch-track"></span>
          <span class="switch-label">时间戳</span>
        </label>
        <label data-m-crlf class="switch flex-shrink-0 min-w-[96px]">
          <input type="checkbox" checked={lineEnding.value === 'Crlf'} onchange={(e) => lineEnding.value = (e.target as HTMLInputElement).checked ? 'Crlf' : 'None'} />
          <span class="switch-track"></span>
          <span class="switch-label">回车换行</span>
        </label>
        <label data-m-logsend class="switch flex-shrink-0 min-w-[96px]">
          <input type="checkbox" bind:checked={logSendContent.value} />
          <span class="switch-track"></span>
          <span class="switch-label">记录发送</span>
        </label>
      </div>
    </div>

    <!-- 发送输入（输入框始终可输入，仅发送按钮在未连接时禁用）；与上方工具条同容器同 padding -->
    <div class="flex items-center gap-3 pb-3">
      <input
        type="text"
        style="flex:1 1 0%;min-width:0;height:40px;"
        placeholder="输入要发送的内容..."
        bind:value={sendText.value}
        onkeydown={handleKeydown}
        oncompositionstart={handleCompositionStart}
        oncompositionend={handleCompositionEnd}
      />
      <button
        class="btn btn-primary min-w-[96px] h-10 transition-all {sendErrorFlash ? 'bg-[var(--error)] border-[var(--error)]' : ''}"
        onclick={handleSend}
        disabled={!connected.value || pendingSend}
      >{pendingSend ? '...' : '发送'}</button>
    </div>
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
