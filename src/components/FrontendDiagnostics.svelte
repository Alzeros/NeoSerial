<script lang="ts">
  import { Download, Loader2 } from 'lucide-svelte';
  import { saveFileDialog, exportFrontendDiagnostics } from '$lib/tauri';
  import SettingsHelp from '$components/ui/SettingsHelp.svelte';

  let busy = $state(false);
  let message = $state('');
  let failed = $state(false);
  async function exportLogs() {
    if (busy) return;
    busy = true; message = ''; failed = false;
    try {
      const date = new Date().toISOString().replace(/[:.]/g, '-');
      const path = await saveFileDialog('导出应用诊断日志', `NeoSerial-frontend-${date}.json`, [{ name: '诊断日志', extensions: ['json'] }]);
      if (!path) return;
      const result = await exportFrontendDiagnostics(path);
      message = result.unresponsive_windows.length || result.failed_windows.length
        ? '已导出。部分窗口未能提交最新记录，文件中已注明，并保留它们此前保存的日志。'
        : '已导出。反馈问题时，请附上此文件、发生时间和操作步骤。';
    } catch (error) {
      failed = true; message = `导出失败：${error}`;
      console.error('导出前端诊断日志失败:', error);
    } finally { busy = false; }
  }
</script>

<!-- 与上面的更新卡片同宽同底色(都是 --border-subtle + 2px 透明边),
     内容边缘也跟着对齐;标题和导出按钮排一行,不再各占一行 -->
<div class="mt-4 w-full max-w-[360px] mx-auto rounded-lg p-4 text-left" style="background: var(--border-subtle); border: 2px solid transparent;" data-frontend-diagnostics>
  <div class="flex items-center justify-between gap-3">
    <div class="flex items-center gap-1">
      <span class="text-[13px] font-semibold">应用诊断日志</span>
      <SettingsHelp icon="alert" label="应用诊断日志" text={'持续保留界面变化、操作、通信和 WebView 状态，重启后仍可导出用于排查，仅存本机。\n输入和动态文本做脱敏，但属性、地址、控制台或异常仍可能含业务信息，分享前请确认。'} />
    </div>
    <button class="btn btn-secondary shrink-0 inline-flex items-center gap-1.5" style="font-size: 12px; padding: 2px 10px;" disabled={busy} onclick={exportLogs} data-diagnostic-action="export-frontend-logs">
      {#if busy}<Loader2 size={13} class="animate-spin" />{:else}<Download size={13} />{/if}
      {busy ? '正在导出…' : '导出诊断日志'}
    </button>
  </div>
  {#if message}<p class="mt-2 break-words text-[12px] select-text" role="status" style:color={failed ? 'var(--error)' : 'var(--muted-foreground)'}>{message}</p>{/if}
</div>
