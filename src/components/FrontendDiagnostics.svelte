<script lang="ts">
  import { Download, Loader2 } from 'lucide-svelte';
  import { saveFileDialog, exportFrontendDiagnostics } from '$lib/tauri';
  import type { Component } from 'svelte';
  let Viewer = $state<Component<{ onclose: () => void }> | null>(null);
  async function viewLogs() { Viewer = (await import('./DiagnosticReplayViewer.svelte')).default; }

  let busy = $state(false);
  let message = $state('');
  let failed = $state(false);
  async function exportLogs() {
    if (busy) return;
    busy = true; message = ''; failed = false;
    try {
      const date = new Date().toISOString().replace(/[:.]/g, '-');
      const path = await saveFileDialog('导出前端诊断日志', `NeoSerial-frontend-${date}.json`, [{ name: '诊断日志', extensions: ['json'] }]);
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

<div class="mt-4 rounded-lg border p-4 text-left" style="border-color: var(--border); background: var(--background-elevated);" data-frontend-diagnostics>
  <div class="mb-1 text-[13px] font-medium">前端诊断日志</div>
  <p class="text-[12px] leading-relaxed text-[var(--muted-foreground)]">
    持续保留界面变化、操作、通信和 WebView 状态，支持回放故障前后的界面；重启后仍可导出。仅存本机。
  </p>
  <p class="mt-1 text-[12px] leading-relaxed text-[var(--muted-foreground)]">
    输入和动态文本节点做脱敏，保留固定标签及渲染属性。属性、地址、控制台或异常仍可能含业务信息，分享前请确认。
  </p>
  <button class="btn btn-secondary mt-3 inline-flex items-center gap-1.5" style="font-size: 12px; padding: 6px 10px;" disabled={busy} onclick={exportLogs} data-diagnostic-action="export-frontend-logs">
    {#if busy}<Loader2 size={13} class="animate-spin" />{:else}<Download size={13} />{/if}
    {busy ? '正在导出…' : '导出前端日志'}
  </button>
  <button class="btn btn-secondary mt-3 ml-2" style="font-size:12px;padding:6px 10px;" onclick={viewLogs} data-diagnostic-action="view-frontend-logs">查看诊断文件</button>
  {#if message}<p class="mt-2 break-words text-[12px] select-text" role="status" style:color={failed ? 'var(--error)' : 'var(--muted-foreground)'}>{message}</p>{/if}
</div>
{#if Viewer}<Viewer onclose={() => Viewer = null} />{/if}
