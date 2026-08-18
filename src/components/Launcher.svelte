<script lang="ts">
  import { onMount } from 'svelte';
  import TitleBar from '$components/TitleBar.svelte';
  import { listPorts, openPortWindow, getMcpStatus, type McpStatus } from '$lib/tauri';

  let ports = $state<string[]>([]);
  let mcp = $state<McpStatus | null>(null);
  let openingPort = $state<string | null>(null);
  let errorMsg = $state<string | null>(null);

  async function refreshPorts() {
    try {
      ports = await listPorts();
    } catch (e) {
      console.error('获取端口列表失败:', e);
    }
  }

  async function refreshMcp() {
    try {
      mcp = await getMcpStatus();
    } catch (e) {
      console.error('获取 MCP 状态失败:', e);
    }
  }

  async function handleOpen(port: string) {
    openingPort = port;
    errorMsg = null;
    try {
      await openPortWindow(port);
    } catch (e) {
      errorMsg = String(e);
    } finally {
      openingPort = null;
    }
  }

  onMount(() => {
    refreshPorts();
    refreshMcp();
    // 定时轮询端口列表(检测热插拔),2s
    const timer = setInterval(refreshPorts, 2000);
    return () => clearInterval(timer);
  });
</script>

<div class="flex h-screen w-screen flex-col overflow-hidden">
  <TitleBar />

  <div class="flex-1 overflow-auto px-8 py-6" style="background: var(--background);">
    <div class="mx-auto" style="max-width: 720px;">
      <!-- 标题 -->
      <div class="mb-6">
        <h1 class="text-2xl font-semibold" style="color: var(--foreground);">
          NeoSerial
        </h1>
        <p class="mt-1 text-[13px]" style="color: var(--muted-foreground);">
          选择串口打开独立窗口。每个窗口连接一个串口,多窗口并发收发,共用一个 MCP server。
        </p>
      </div>

      <!-- MCP 状态卡 -->
      <div class="mb-6 rounded-lg border p-4" style="border-color: var(--border); background: var(--background-elevated);">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <span class="text-[13px] font-medium" style="color: var(--foreground);">MCP Server</span>
            <span
              class="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium"
              style="background: {mcp?.running ? 'var(--accent-soft)' : 'var(--muted)'}; color: {mcp?.running ? 'var(--accent)' : 'var(--muted-foreground)'};"
            >
              <span class="inline-block h-1.5 w-1.5 rounded-full" style="background: currentColor;"></span>
              {mcp?.running ? '运行中' : '未运行'}
            </span>
          </div>
          {#if mcp?.running && mcp.port}
            <code class="text-[12px]" style="color: var(--muted-foreground);">
              http://localhost:{mcp.port}/mcp
            </code>
          {/if}
        </div>
      </div>

      <!-- 端口列表 -->
      <div class="mb-3 flex items-center justify-between">
        <span class="text-[13px] font-medium" style="color: var(--foreground);">可用串口</span>
        <button
          class="text-[12px] transition-opacity hover:opacity-70"
          style="color: var(--muted-foreground);"
          onclick={refreshPorts}
        >刷新</button>
      </div>

      {#if errorMsg}
        <div class="mb-3 rounded-md px-3 py-2 text-[12px]" style="background: var(--destructive-soft); color: var(--destructive);">
          {errorMsg}
        </div>
      {/if}

      {#if ports.length === 0}
        <div class="rounded-lg border border-dashed p-8 text-center text-[13px]" style="border-color: var(--border); color: var(--muted-foreground);">
          未检测到串口。插入设备后会自动刷新。
        </div>
      {:else}
        <div class="grid gap-2">
          {#each ports as port (port)}
            <button
              class="flex items-center justify-between rounded-lg border px-4 py-3 text-left transition-colors hover:bg-[var(--accent-soft)]"
              style="border-color: var(--border); background: var(--background-elevated);"
              disabled={openingPort === port}
              onclick={() => handleOpen(port)}
            >
              <div class="flex items-center gap-3">
                <span class="inline-flex h-7 w-7 items-center justify-center rounded-md text-[12px] font-semibold"
                      style="background: var(--accent-soft); color: var(--accent);">
                  ⟶
                </span>
                <span class="text-[14px] font-medium" style="color: var(--foreground);">{port}</span>
              </div>
              <span class="text-[12px]" style="color: var(--muted-foreground);">
                {openingPort === port ? '打开中…' : '打开窗口'}
              </span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
