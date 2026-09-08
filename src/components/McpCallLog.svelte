<script lang="ts">
  import { onMount } from 'svelte';
  import { getMcpCallLog, onMcpCall, type McpCallRecord } from '$lib/tauri';

  // 最新在前(倒序展示,刚发生的在最上)。开 tab 拉历史,之后监听事件 unshift。
  let records = $state<McpCallRecord[]>([]);
  let listEl: HTMLDivElement | undefined;

  onMount(() => {
    getMcpCallLog()
      .then((rs) => {
        // 后端返回最旧在前,展示倒序(最新在上)
        records = rs.reverse();
      })
      .catch((e) => console.error('加载 MCP 调用记录失败:', e));
    const un = onMcpCall((rec) => {
      records = [rec, ...records];
      // 环形上限 200,前端也截一下避免无限增长
      if (records.length > 200) records = records.slice(0, 200);
    });
    return () => un.then((f) => f());
  });

  /** 时间只显示 时:分:秒.毫秒,日期省略(看的是刚发生的操作) */
  function fmtTime(iso: string): string {
    const d = new Date(iso);
    if (isNaN(d.getTime())) return iso;
    const p = (n: number, w = 2) => String(n).padStart(w, '0');
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
  }
</script>

<div class="flex flex-col h-full min-h-0">
  <!-- select-text + tabindex="-1":入参/报错要能拖选复制,点一下容器后 Ctrl+A 也只圈这块
       (范围逻辑在 App.svelte 的 handleSelectAll) -->
  <div
    bind:this={listEl}
    tabindex="-1"
    class="overflow-y-auto outline-none select-text"
    style="flex: 1 1 0%; min-height: 0;"
  >
    {#if records.length === 0}
      <div class="px-4 py-3 text-[12px]" style="color: var(--muted-foreground);">
        agent 经 MCP 调用工具(list_ports / connect / send …)时,这里实时显示每次调用的工具、入参、结果与耗时。
      </div>
    {:else}
      <table class="w-full text-[12px]" style="table-layout: fixed;">
        <thead class="sticky top-0" style="background: var(--background-elevated);">
          <tr class="text-[var(--muted-foreground)]" style="border-bottom: 1px solid var(--border);">
            <th class="px-2 py-1 text-left font-medium" style="width: 96px;">时间</th>
            <th class="px-2 py-1 text-left font-medium" style="width: 120px;">工具</th>
            <th class="px-2 py-1 text-left font-medium">入参</th>
            <th class="px-2 py-1 text-left font-medium" style="width: 52px;">耗时</th>
          </tr>
        </thead>
        <tbody>
          {#each records as r, i (i)}
            <tr style="border-bottom: 1px solid var(--border-subtle); {r.ok ? '' : 'background: var(--error-bg, transparent);'}">
              <td class="px-2 py-1 align-top whitespace-nowrap" style="font-family: var(--font-mono); color: var(--muted-foreground);">{fmtTime(r.ts)}</td>
              <td class="px-2 py-1 align-top" style="font-family: var(--font-mono); color: var(--foreground);">{r.tool}</td>
              <td class="px-2 py-1 align-top break-all" style="font-family: var(--font-mono); color: {r.ok ? 'var(--muted-foreground)' : 'var(--error)'};">
                {r.ok ? r.args : (r.error || r.args)}
              </td>
              <td class="px-2 py-1 align-top whitespace-nowrap" style="color: var(--muted-foreground);">{r.duration_ms}ms</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>
