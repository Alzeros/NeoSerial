<script lang="ts">
  import Button from './ui/Button.svelte';
  import Checkbox from './ui/Checkbox.svelte';
  import Input from './ui/Input.svelte';
  import { saveFileDialog, startLogging, stopLogging } from '$lib/tauri';
  import { logSendContent, loggingPath } from '$lib/stores';

  let pathStr = $state(loggingPath.value || '');
  $effect(() => {
    loggingPath.value = pathStr || null;
  });

  async function handleStartLogging() {
    try {
      // 让用户选择保存位置
      const selectedPath = await saveFileDialog(
        '保存日志文件',
        `neoserial_${Date.now()}.log`,
        [{ name: '日志文件', extensions: ['log'] }, { name: '文本文件', extensions: ['txt'] }]
      );
      if (!selectedPath) return; // 用户取消

      const path = await startLogging(selectedPath);
      loggingPath.value = path;
      pathStr = path;
    } catch (e) {
      console.error('启动日志失败:', e);
    }
  }

  async function handleStopLogging() {
    try {
      await stopLogging();
      loggingPath.value = null;
      pathStr = '';
    } catch (e) {
      console.error('停止日志失败:', e);
    }
  }
</script>

<div class="flex items-center gap-2 border-t border-border px-3 py-2 bg-muted/20">
  <Input
    bind:value={pathStr}
    placeholder="日志文件路径..."
    readonly
    class="flex-1"
  />
  <Checkbox bind:checked={logSendContent.value} label="记录发送" />
  {#if loggingPath.value}
    <Button variant="destructive" size="sm" onclick={handleStopLogging}>
      停止
    </Button>
  {:else}
    <Button variant="primary" size="sm" onclick={handleStartLogging}>
      开始记录
    </Button>
  {/if}
</div>
