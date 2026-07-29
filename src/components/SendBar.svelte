<script lang="ts">
  import Button from './ui/Button.svelte';
  import Input from './ui/Input.svelte';
  import { send } from '$lib/tauri';
  import { hexSend, lineEnding, sendHistory, sendHistoryIndex, sendText } from '$lib/stores';

  async function handleSend() {
    if (!sendText.value.trim()) return;
    try {
      await send(sendText.value, lineEnding.value, hexSend.value);
      sendHistory.value.unshift(sendText.value);
      if (sendHistory.value.length > 50) sendHistory.value.pop();
      sendText.value = '';
      sendHistoryIndex.value = -1;
    } catch (e) {
      console.error('发送失败:', e);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (sendHistoryIndex.value < sendHistory.value.length - 1) {
        sendHistoryIndex.value++;
        sendText.value = sendHistory.value[sendHistoryIndex.value];
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (sendHistoryIndex.value > 0) {
        sendHistoryIndex.value--;
        sendText.value = sendHistory.value[sendHistoryIndex.value];
      } else {
        sendHistoryIndex.value = -1;
        sendText.value = '';
      }
    }
  }
</script>

<div class="flex items-center gap-2 border-t border-border px-3 py-2 bg-muted/20">
  <Input
    bind:value={sendText.value}
    placeholder="输入要发送的内容..."
    onkeydown={handleKeydown}
  />
  <Button variant="primary" size="sm" onclick={handleSend}>
    发送
  </Button>
</div>
