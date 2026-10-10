<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { checkMcpConnection, getMcpStatus, openUrl, type McpStatus } from '$lib/tauri';
  import { MCP_CLIENTS, MCP_VERIFY_PROMPT, mcpEndpoint, mcpSetupGuide, type SetupBlock, type McpClient, type McpScope } from '$lib/mcpSetup';

  import Collapsible from '$components/ui/Collapsible.svelte';

  let showAlternative = $state(false);
  let showTroubleshooting = $state(false);
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
    showAlternative = false;
    showTroubleshooting = false;
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
      checkMessage = `本机服务正常 · ${result.tool_count} 个工具可读取。客户端是否接入请在客户端确认。`;
    } catch (error) {
      if (!alive || current !== requestId) return;
      checkFailed = true;
      checkMessage = `自检失败：${error}`;
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
      <button class="btn btn-secondary text-[12px] shrink-0" onclick={check} disabled={checking}>{checking ? '自检中…' : '本机服务自检'}</button>
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
        <label class="text-[12px]" for="mcp-scope">生效范围</label>
        <select id="mcp-scope" bind:value={scope} class="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[12px]">
          <option value="user">{client === 'vscode' ? '当前 VS Code 配置文件' : '当前用户 · 各项目可用'}</option>
          <option value="project">{client === 'claude' ? '当前项目 · 仅自己' : '仅当前项目'}</option>
        </select>
      </div>
    {/if}
    {#snippet configBlock(block: SetupBlock, index: number)}
      <div class="mt-2 min-w-0">
        <p class="text-[12px] leading-relaxed text-[var(--muted-foreground)] select-text break-words"><span class="text-[var(--foreground)]">{block.merge ? '配置位置：' : '执行位置：'}</span>{block.location}</p>
        {#if block.merge}
          <p class="mt-1 text-[12px] leading-relaxed text-[var(--muted-foreground)]">将以下内容合并到现有配置，保留其他服务；已有 neoserial 时更新该项，不要覆盖整个文件。</p>
        {/if}
        <div class="mt-2 rounded border border-[var(--border)] overflow-hidden">
          <div class="flex items-center justify-between gap-2 px-2 py-1 bg-[var(--overlay-hover)]">
            <span class="text-[12px] text-[var(--muted-foreground)]">{block.label}</span>
            <button class="text-[12px] px-1 py-0.5 hover:text-[var(--primary)]" aria-label={`复制${block.label}`} onclick={() => copy(`block-${index}`,block.code)}>{copied === `block-${index}` ? '已复制' : '复制'}</button>
          </div>
          <textarea readonly aria-label={block.label} value={block.code} rows={Math.min(8,Math.max(2,block.code.split('\n').length))}
            class="mcp-code block w-full resize-none bg-[var(--background-input)] p-2 font-mono text-[12px] leading-5 outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-[var(--primary)]" spellcheck="false"></textarea>
        </div>
      </div>
    {/snippet}

    <section class="setup-step" aria-label="添加到客户端">
      <h4 class="step-heading"><span class="step-number" aria-hidden="true">1</span>添加到客户端</h4>
      {#if !guide.blocks[0]?.merge}
        <p class="text-[12px] leading-relaxed text-[var(--muted-foreground)] select-text">{guide.instructions}</p>
      {/if}
      {#if client === 'codex' && scope === 'project'}
        <p class="text-[12px] leading-relaxed text-[var(--muted-foreground)]">需先在 Codex 中信任该项目，项目配置才会生效。</p>
      {/if}
      {#if guide.blocks[0]}
        {@render configBlock(guide.blocks[0], 0)}
      {/if}
      {#if guide.blocks.length > 1}
        <div class="mt-2">
          <Collapsible title="手动配置（替代命令）" bind:open={showAlternative}>
            <p class="text-[12px] text-[var(--muted-foreground)]">与上方命令二选一，无需重复配置。</p>
            {#each guide.blocks.slice(1) as block, index (`${client}-${scope}-${index}`)}
              {@render configBlock(block, index + 1)}
            {/each}
          </Collapsible>
        </div>
      {/if}
    </section>

    <section class="setup-step" aria-label="验证接入">
      <h4 class="step-heading"><span class="step-number" aria-hidden="true">2</span>验证接入</h4>
      <p class="text-[12px] leading-relaxed select-text">{guide.verification}</p>
      <div class="mt-2 rounded border border-[var(--border)] p-2">
        <div class="flex flex-wrap items-center justify-between gap-2 mb-1">
          <span class="text-[12px] text-[var(--muted-foreground)]">在客户端对话中发送</span>
          <button class="text-[12px] text-[var(--primary)] hover:underline" onclick={() => copy('verify', MCP_VERIFY_PROMPT)}>{copied === 'verify' ? '已复制' : '复制验证提示词'}</button>
        </div>
        <p class="text-[12px] leading-relaxed select-text">{MCP_VERIFY_PROMPT}</p>
      </div>
    </section>

    <div class="mt-3">
      <Collapsible title="接入遇到问题？" bind:open={showTroubleshooting}>
        <div class="space-y-2 text-[12px] leading-relaxed text-[var(--muted-foreground)] select-text">
          <p>{guide.notes}</p>
          <p><span class="text-[var(--foreground)]">本机自检通过，客户端仍连不上：</span>自检仅验证 NeoSerial 的服务，不会替客户端注册配置。请核对配置位置、生效范围，再按上方步骤验证。</p>
          <p><span class="text-[var(--foreground)]">端口变化：</span>请使用上方当前服务地址，更新客户端中的旧地址。</p>
          <p><span class="text-[var(--foreground)]">WSL、容器或远程环境：</span>当前地址用于本机接入；需先确认客户端能够访问 NeoSerial 所在电脑的服务。</p>
        </div>
      </Collapsible>
    </div>
    <button class="mt-2 text-[12px] text-[var(--primary)] hover:underline" onclick={() => openUrl(guide.docs)}>查看官方接入说明 ↗</button>
  {/if}
  {#if copyError}<p role="alert" class="mt-2 text-[12px] text-[var(--error)]">{copyError}</p>{/if}
</section>


<style>
  .setup-step {
    margin-top: 16px;
    padding-top: 12px;
    border-top: 1px solid var(--border-subtle);
  }
  .step-heading {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    font-size: 13px;
    font-weight: 500;
  }
  .step-number {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--overlay-hover);
    color: var(--primary);
    font-size: 12px;
  }
</style>
