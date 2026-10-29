<script lang="ts">
  import { onMount } from 'svelte';
  import { connected, currentPort, windowPort, connectionParams, scriptPanelOpen, mcpLogRequest } from '$lib/stores';
  import { getMcpActivity, onMcpActivity } from '$lib/tauri';
  import { acceptActivitySnapshot, activityPort, activitySummary, type McpActivitySnapshot } from '$lib/mcpActivity';

  let snapshot = $state.raw<McpActivitySnapshot | null>(null);
  let running = $state<boolean | null>(null);
  let unavailable = $state(false);
  let now = $state(Date.now());
  const port = $derived(activityPort(connected.value, currentPort.value, windowPort.value, connectionParams.port));
  const view = $derived(unavailable
    ? { text: '状态暂不可用', detail: '未能读取 MCP 活动状态；恢复后会自动更新。点击查看 MCP 日志。', tone: 'unknown' }
    : running === false
      ? { text: '未启用', detail: 'MCP 服务未运行。不表示串口断开；点击仍可查看 MCP 日志。', tone: 'disabled' }
      : snapshot
        ? activitySummary(snapshot, port, now)
        : { text: '正在读取状态…', detail: '正在读取 MCP 服务和工具调用状态。', tone: 'loading' });

  function validSnapshot(value: McpActivitySnapshot | null | undefined): value is McpActivitySnapshot {
    return !!value && Number.isSafeInteger(value.revision) && Array.isArray(value.active) && Array.isArray(value.recent);
  }

  onMount(() => {
    let live = true;
    let pending = false;
    let unsubscribe: (() => void) | undefined;

    async function refresh() {
      if (pending || !live) return;
      pending = true;
      const revision = snapshot?.revision ?? -1;
      try {
        const state = await getMcpActivity();
        if (!live) return;
        if (!validSnapshot(state) || typeof state.running !== 'boolean') throw new Error('Invalid MCP activity snapshot');
        if (snapshot && state.revision < snapshot.revision) return;
        snapshot = acceptActivitySnapshot(snapshot, state);
        running = state.running;
        unavailable = false;
        now = Date.now();
      } catch {
        if (live && revision === (snapshot?.revision ?? -1)) unavailable = true;
      } finally {
        pending = false;
      }
    }

    void onMcpActivity(incoming => {
      if (!live || !validSnapshot(incoming) || typeof incoming.running !== 'boolean' || (snapshot && incoming.revision < snapshot.revision)) return;
      snapshot = acceptActivitySnapshot(snapshot, incoming);
      running = incoming.running;
      unavailable = false;
      now = Date.now();
    }).then(remove => {
      if (!live) { remove(); return; }
      unsubscribe = remove;
      void refresh();
    }).catch(() => { if (live) void refresh(); });

    const clock = setInterval(() => { now = Date.now(); }, 500);
    const reconcile = setInterval(() => { void refresh(); }, 5000);
    const focus = () => { void refresh(); };
    window.addEventListener('focus', focus);
    return () => {
      live = false;
      unsubscribe?.();
      clearInterval(clock);
      clearInterval(reconcile);
      window.removeEventListener('focus', focus);
    };
  });

  function openLog() {
    mcpLogRequest.pending = true;
    scriptPanelOpen.value = true;
  }
</script>

<button
  data-mcp-activity
  data-tone={view.tone}
  class="ml-auto flex min-w-0 max-w-sm flex-1 items-center justify-end gap-1.5 text-[12px] leading-none cursor-pointer"
  style:color={view.tone === 'error' ? 'var(--error)' : view.tone === 'active' ? 'var(--rx)' : 'var(--muted-foreground)'}
  title={view.detail}
  aria-label={`查看 MCP 日志，AI/MCP：${view.text}`}
  onclick={openLog}
>
  <span class="h-1.5 w-1.5 shrink-0 rounded-full" style:background="currentColor"></span>
  <span class="truncate">AI/MCP：{view.text}</span>
</button>
