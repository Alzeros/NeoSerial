<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { checkMcpConnection, getMcpStatus, openUrl, type McpStatus } from '$lib/tauri';
  import { MCP_CLIENTS, mcpEndpoint, mcpSetupGuide, type McpClient, type McpScope } from '$lib/mcpSetup';

  let client = $state<McpClient>('generic');
  let scope = $state<McpScope>('user');
  let status = $state<McpStatus>({running:false,port:null});
  let loading = $state(true);
  let checking = $state(false);
  let statusError = $state('');
  let checkMessage = $state('');
  let checkFailed = $state(false);
  let copied = $state('');
  let copyError = $state('');
  let alive = true;
  let requestId = 0;
  let copyId = 0;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  const endpoint = $derived(mcpEndpoint(status));
  const guide = $derived(mcpSetupGuide(client, scope, endpoint ?? ''));

  function resetCopy() {
    copyId++;
    clearTimeout(copyTimer);
    copied = '';
    copyError = '';
  }

  $effect(() => {
    client; scope; endpoint;
    resetCopy();
  });

  async function refresh() {
    const current = ++requestId;
    loading = true;
    checking = false;
    statusError = '';
    checkMessage = '';
    try {
      const next = await getMcpStatus();
      if (alive && current === requestId) status = next;
    } catch (error) {
      if (alive && current === requestId) {
        status = {running:false,port:null};
        statusError = `读取服务状态失败：${error}`;
      }
    } finally {
      if (alive && current === requestId) loading = false;
    }
  }

  async function check() {
    const current = ++requestId;
    checking = true;
    checkMessage = '';
    checkFailed = false;
    try {
      const result = await checkMcpConnection();
      if (!alive || current !== requestId) return;
      status = {running:true,port:result.port};
      checkMessage = `服务检测通过 · ${result.tool_count} 个工具可读取`;
    } catch (error) {
      if (!alive || current !== requestId) return;
      checkFailed = true;
      checkMessage = `检测失败：${error}`;
    } finally {
      if (alive && current === requestId) checking = false;
    }
  }

  async function copy(key: string, text: string) {
    const current = ++copyId;
    clearTimeout(copyTimer);
    copied = '';
    copyError = '';
    try {
      await navigator.clipboard.writeText(text);
      if (!alive || current !== copyId) return;
      copied = key;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {copied = '';},1500);
    } catch {
      if (alive && current === copyId) copyError = '复制失败，可直接选中下方内容手动复制。';
    }
  }

  onMount(() => { void refresh(); });
  onDestroy(() => {alive = false; requestId++; copyId++; clearTimeout(copyTimer);});
</script>

<section class="mcp-setup mt-3 pt-3 border-t border-[var(--border-subtle)] min-w-0" aria-label="MCP 客户端接入">
  <div class="flex items-center justify-between gap-2 mb-2">
    <h3 class="text-[13px] font-medium">客户端接入</h3>
    <button class="text-[12px] text-[var(--muted-foreground)] hover:text-[var(--foreground)] disabled:opacity-50" onclick={refresh} disabled={loading || checking}>刷新状态</button>
  </div>
  <div class="text-[12px] text-[var(--muted-foreground)] mb-2" role="status">
    {#if loading}正在读取服务状态…
    {:else if statusError}<span class="text-[var(--error)] select-text">{statusError}</span>
    {:else if endpoint}服务运行中 · Streamable HTTP · 仅本机
    {:else}服务未运行。启用 MCP 服务并保存后，重启应用，再刷新状态。
    {/if}
  </div>

  {#if endpoint && !loading}
    <div class="flex items-center gap-2 min-w-0">
      <input aria-label="MCP 服务地址" readonly value={endpoint}
        class="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[12px] font-mono" />
      <button class="btn btn-secondary shrink-0 text-[12px]" onclick={() => copy('address',endpoint!)}>{copied === 'address' ? '已复制' : '复制地址'}</button>
    </div>
    <div class="flex items-center gap-2 mt-2">
      <button class="btn btn-secondary text-[12px] shrink-0" onclick={check} disabled={checking}>{checking ? '检测中…' : '检测服务'}</button>
      <span class="text-[12px] text-[var(--muted-foreground)]">检查协议与工具列表，不操作串口。</span>
    </div>
    {#if checkMessage}
      <p role="status" class="mt-2 text-[12px] break-words select-text" style="color: {checkFailed ? 'var(--error)' : 'var(--primary)'}">{checkMessage}</p>
    {/if}

    <div class="flex flex-wrap items-center gap-2 mt-4 mb-2">
      <label class="text-[12px]" for="mcp-client">客户端</label>
      <select id="mcp-client" bind:value={client} class="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[12px]">
        {#each MCP_CLIENTS as option}<option value={option.value}>{option.label}</option>{/each}
      </select>
    </div>
    {#if client !== 'generic'}
      <div class="flex items-center gap-2 mb-2">
        <label class="text-[12px]" for="mcp-scope">作用范围</label>
        <select id="mcp-scope" bind:value={scope} class="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[12px]">
          <option value="user">当前用户 · 各项目可用</option>
          <option value="project">仅当前项目</option>
        </select>
      </div>
    {/if}
    <p class="text-[12px] leading-relaxed text-[var(--muted-foreground)] select-text">{guide.instructions}</p>
    {#each guide.blocks as block, index (`${client}-${scope}-${index}`)}
      <div class="mt-2 rounded border border-[var(--border)] overflow-hidden">
        <div class="flex items-center justify-between gap-2 px-2 py-1 bg-[var(--border-subtle)]">
          <span class="text-[12px] text-[var(--muted-foreground)]">{block.label}</span>
          <button class="text-[12px] px-1 py-0.5 hover:text-[var(--primary)]" aria-label={`复制${block.label}`} onclick={() => copy(`block-${index}`,block.code)}>{copied === `block-${index}` ? '已复制' : '复制'}</button>
        </div>
        <textarea readonly aria-label={block.label} value={block.code} rows={Math.min(8,Math.max(2,block.code.split('\n').length))}
          class="mcp-code block w-full resize-none bg-[var(--background-input)] p-2 font-mono text-[12px] leading-5 outline-none" spellcheck="false"></textarea>
      </div>
    {/each}
    <p class="mt-3 text-[12px] leading-relaxed select-text"><span class="font-medium">验证方法：</span>{guide.verification}</p>
    <p class="mt-2 text-[12px] leading-relaxed text-[var(--muted-foreground)] select-text">{guide.notes}</p>
    <p class="mt-2 text-[12px] leading-relaxed text-[var(--muted-foreground)]">服务检测通过后，仍需在客户端完成配置。服务端口变化后请更新客户端地址。</p>
    <button class="mt-2 text-[12px] text-[var(--primary)] hover:underline" onclick={() => openUrl(guide.docs)}>查看官方接入说明 ↗</button>
  {/if}
  {#if copyError}<p role="alert" class="mt-2 text-[12px] text-[var(--error)]">{copyError}</p>{/if}
</section>
