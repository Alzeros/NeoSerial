<script lang="ts">
  import Button from './ui/Button.svelte';
  import Input from './ui/Input.svelte';
  import Progress from './ui/Progress.svelte';
  import { onMount } from 'svelte';
  import { onFileSendProgress, openFileDialog, sendFile } from '$lib/tauri';
  import { fileSendPath, fileSendProgress } from '$lib/stores';

  // 监听文件发送进度
  onMount(() => {
    const unlisten = onFileSendProgress((p) => {
      fileSendProgress.value = p.total > 0 ? Math.round((p.sent / p.total) * 100) : 0;
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  let pathStr = $state(fileSendPath.value || '');
  $effect(() => {
    fileSendPath.value = pathStr || null;
  });

  async function handleSelectFile() {
    const path = await openFileDialog('选择要发送的文件');
    if (path) {
      pathStr = path;
      fileSendPath.value = path;
    }
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
</script>

<div class="flex items-center gap-2 border-t border-border px-3 py-2 bg-muted/20">
  <Input
    bind:value={pathStr}
    placeholder="选择文件..."
    readonly
    class="flex-1"
  />
  {#if fileSendProgress.value > 0 && fileSendProgress.value < 100}
    <Progress value={fileSendProgress.value} class="w-20" />
  {/if}
  <Button variant="ghost" size="sm" onclick={handleSelectFile}>
    浏览
  </Button>
  <Button variant="primary" size="sm" onclick={handleSendFile} disabled={!fileSendPath.value}>
    发送文件
  </Button>
</div>
