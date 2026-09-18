<script lang="ts">
  import { onMount } from 'svelte';
  import {
    clearMcpCallLog,
    getMcpCallLog,
    onMcpCall,
    onMcpCallLogCleared,
    type McpCallRecord,
  } from '$lib/tauri';

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
    // 清空是进程级的(后端记录 + 所有窗口),本窗口也靠这个事件清,不在点击处直接清:
    // 保证与别的窗口走同一条路径,且后端清失败时面板不会"看着清了、切回 tab 又冒出来"
    const unCleared = onMcpCallLogCleared(() => {
      records = [];
    });
    return () => {
      un.then((f) => f());
      unCleared.then((f) => f());
    };
  });

  // ============ 右键菜单(与主日志区同款):有选区时可复制;清除 = 控制台 clear ============
  let ctxMenu = $state<{ x: number; y: number; show: boolean; hasSelection: boolean }>({
    x: 0, y: 0, show: false, hasSelection: false,
  });

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    // 不冒泡到 App 的全局处理(那里对非输入框区域一律禁用右键)
    e.stopPropagation();
    const sel = window.getSelection();
    ctxMenu.hasSelection = !!(sel && sel.toString().length > 0);
    // 边界检测:靠近底部翻到上方,靠近右边左移
    const MW = 110, MH = 80;
    const vw = window.innerWidth, vh = window.innerHeight;
    ctxMenu.x = e.clientX + MW > vw ? Math.max(4, vw - MW - 4) : e.clientX;
    ctxMenu.y = e.clientY + MH > vh ? Math.max(4, e.clientY - MH) : e.clientY;
    ctxMenu.show = true;
  }

  function closeCtxMenu() {
    ctxMenu.show = false;
  }

  async function ctxCopy() {
    const sel = window.getSelection();
    if (sel) {
      try { await navigator.clipboard.writeText(sel.toString()); } catch { /* 剪贴板不可用 */ }
    }
    closeCtxMenu();
  }

  function ctxClear() {
    closeCtxMenu();
    clearMcpCallLog().catch((e) => console.error('清除 MCP 调用记录失败:', e));
  }

  $effect(() => {
    if (!ctxMenu.show) return;
    const close = () => { ctxMenu.show = false; };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') ctxMenu.show = false; };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', onKey);
    };
  });

  /** 时间只显示 时:分:秒.毫秒,日期省略(看的是刚发生的操作) */
  function fmtTime(iso: string): string {
    const d = new Date(iso);
    if (isNaN(d.getTime())) return iso;
    const p = (n: number, w = 2) => String(n).padStart(w, '0');
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
  }
</script>

<div class="flex flex-col h-full min-h-0" oncontextmenu={handleContextMenu}>
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

  {#if ctxMenu.show}
    <div
      class="fixed z-[200] py-1 rounded-md border shadow-lg min-w-[100px]"
      style="background: var(--background-elevated); border-color: var(--border); left: {ctxMenu.x}px; top: {ctxMenu.y}px;"
      onmousedown={(e) => e.stopPropagation()}
    >
      {#if ctxMenu.hasSelection}
        <button
          class="block w-full text-left px-3 py-1.5 text-[13px] hover:bg-[var(--border-subtle)] transition-colors cursor-pointer"
          onclick={ctxCopy}
        >复制</button>
      {/if}
      <button
        class="block w-full text-left px-3 py-1.5 text-[13px] hover:bg-[var(--border-subtle)] transition-colors cursor-pointer"
        onclick={ctxClear}
        disabled={records.length === 0}
        style={records.length === 0 ? 'opacity: 0.45; cursor: default;' : ''}
      >清除</button>
    </div>
  {/if}
</div>
