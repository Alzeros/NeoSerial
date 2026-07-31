<script lang="ts">
  import {
    activeScriptModule,
    activeScriptPage,
    addScriptPage,
    currentModulePages,
    removeScriptPage,
    scriptLoopInterval,
    scriptModules,
    scriptRunCount,
    scriptRunning,
    switchScriptModule,
  } from '$lib/stores';
  import { openFileDialog, saveFileDialog, loadSequenceConfig, saveSequenceConfig, sequenceRun, sequenceStop } from '$lib/tauri';

  async function handleRun() {
    const pages = currentModulePages();
    const page = pages[activeScriptPage.value];
    if (!page) return;
    try {
      await sequenceRun(page.commands, scriptRunCount.value, scriptLoopInterval.value);
    } catch (e) {
      console.error('序列执行失败:', e);
    }
  }

  async function handleStop() {
    try {
      await sequenceStop();
    } catch (e) {
      console.error('停止失败:', e);
    }
  }

  async function handleSaveConfig() {
    const path = await saveFileDialog(
      '保存序列配置',
      'sequence.json',
      [{ name: 'JSON', extensions: ['json'] }]
    );
    if (!path) return;
    try {
      await saveSequenceConfig(path, scriptModules);
    } catch (e) {
      console.error('保存配置失败:', e);
    }
  }

  async function handleLoadConfig() {
    const path = await openFileDialog('加载序列配置', [
      { name: 'JSON', extensions: ['json'] }
    ]);
    if (!path) return;
    try {
      const modules = await loadSequenceConfig(path);
      if (modules && modules.length > 0) {
        scriptModules.length = 0;
        scriptModules.push(...modules);
        activeScriptModule.value = 0;
        activeScriptPage.value = 0;
      }
    } catch (e) {
      console.error('加载配置失败:', e);
    }
  }

  function handleClearConfig() {
    const pages = currentModulePages();
    const page = pages[activeScriptPage.value];
    if (!page) return;
    page.commands = page.commands.map((cmd: any, i: number) => ({
      enabled: true,
      command: '',
      hex: false,
      enter: true,
      delay_ms: i === 0 ? 2000 : 0,
    }));
  }

  // 右键页签：弹出"删除"菜单（多页时才允许删，单页禁用）
  let pageMenu = $state<{ open: boolean; x: number; y: number; index: number }>({
    open: false, x: 0, y: 0, index: -1,
  });
  // 删除二次确认浮层
  let confirmDelete = $state<{ open: boolean; index: number }>({ open: false, index: -1 });

  function handlePageContextMenu(e: MouseEvent, index: number) {
    e.preventDefault();
    pageMenu = { open: true, x: e.clientX, y: e.clientY, index };
  }

  function closePageMenu() {
    pageMenu.open = false;
  }

  // 点"删除页签" → 关右键菜单 → 开二次确认
  function handleDeletePageFromMenu() {
    const idx = pageMenu.index;
    closePageMenu();
    if (currentModulePages().length <= 1) return;
    confirmDelete = { open: true, index: idx };
  }

  function handleConfirmDelete() {
    const idx = confirmDelete.index;
    confirmDelete.open = false;
    removeScriptPage(idx);
  }

  function handleCancelDelete() {
    confirmDelete.open = false;
  }
</script>

<svelte:window on:click={closePageMenu} on:contextmenu={(e) => {
  // 点页签右键菜单外部时关闭（页签自身的 contextmenu 已 stopPropagation）
  if (pageMenu.open) {
    const t = e.target as HTMLElement;
    if (!t.closest('[data-page-menu]') && !t.closest('[data-page-tab]')) {
      closePageMenu();
    }
  }
}} />

<div class="flex h-full flex-col border-l border-[var(--border)]" style="background: var(--background-elevated);">
  <!-- 模块切换栏：预置功能标题，文字风格（非 tag），当前项加粗+下划线区分 -->
  <div class="flex items-center gap-4 border-b border-[var(--border)] px-4 py-2" style="background: var(--background);">
    {#each scriptModules as m, i}
      <button
        class="text-[13px] font-medium transition-colors cursor-pointer pb-0.5 border-b-2 {i === activeScriptModule.value
          ? 'text-[var(--foreground)] border-[var(--primary)]'
          : 'text-[var(--muted-foreground)] border-transparent hover:text-[var(--foreground)]'}"
        onclick={() => switchScriptModule(i)}
      >
        {m.name}
      </button>
    {/each}
  </div>

  <!-- 页签栏（当前模块的 Page0/Page1...）右键页签可删除 -->
  <div class="flex items-center gap-1 border-b border-[var(--border)] px-3 py-2">
    {#each currentModulePages() as page, i}
      <button
        data-page-tab
        class="rounded px-3 py-1.5 text-[13px] font-medium transition-colors cursor-pointer {i === activeScriptPage.value
          ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
          : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)]'}"
        onclick={() => (activeScriptPage.value = i)}
        oncontextmenu={(e) => handlePageContextMenu(e, i)}
        title="右键可删除此页签"
      >
        {page.name}
      </button>
    {/each}
    {#if currentModulePages().length < 6}
      <button
        class="rounded px-2 py-1.5 text-[13px] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] cursor-pointer"
        onclick={addScriptPage}
        title="新增页签"
      >
        +
      </button>
    {/if}
  </div>

  <!-- 命令序列表格（独立滚动，不撑开外部布局） -->
  <div class="script-list">
    <table class="w-full text-[13px]">
      <thead class="sticky top-0" style="background: var(--background-elevated);">
        <tr class="text-[var(--muted-foreground)]">
          <th class="w-8 px-2 py-2 text-center">
            <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]"
              checked={currentModulePages()[activeScriptPage.value]?.commands.every((c: any) => c.enabled) ?? false}
              onchange={(e) => {
                const page = currentModulePages()[activeScriptPage.value];
                if (page) page.commands.forEach((c: any) => (c.enabled = (e.target as HTMLInputElement).checked));
              }}
            />
          </th>
          <th class="px-2 py-2 text-left font-medium">命令</th>
          <th class="w-10 px-2 py-2 text-center font-medium">Hex</th>
          <th class="w-10 px-2 py-2 text-center font-medium">↩</th>
          <th class="w-8 px-2 py-2 text-center font-medium">#</th>
          <th class="w-16 px-2 py-2 text-center font-medium">Delay</th>
        </tr>
      </thead>
      <tbody>
        {#each currentModulePages()[activeScriptPage.value]?.commands as cmd, i}
          <tr class="border-t border-[var(--border-subtle)] hover:bg-[var(--border-subtle)]">
            <td class="px-2 py-1 text-center">
              <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" bind:checked={cmd.enabled} />
            </td>
            <td class="px-2 py-1">
              <input
                class="w-full rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
                bind:value={cmd.command}
              />
            </td>
            <td class="px-2 py-1 text-center">
              <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" bind:checked={cmd.hex} />
            </td>
            <td class="px-2 py-1 text-center">
              <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" bind:checked={cmd.enter} />
            </td>
            <td class="px-2 py-1 text-center text-[var(--muted-foreground)]">{i + 1}</td>
            <td class="px-2 py-1">
              <input
                type="number"
                class="w-full rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1 text-[13px] text-center focus-visible:outline-none focus-visible:border-[var(--primary)]"
                bind:value={cmd.delay_ms}
              />
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  <!-- 执行控制 -->
  <div class="border-t border-[var(--border)] px-4 py-3 space-y-2">
    <div class="flex items-center gap-2">
      {#if scriptRunning.value}
        <button class="btn btn-secondary" onclick={handleStop}>■ 停止</button>
      {:else}
        <button class="btn btn-primary" onclick={handleRun}>▶ 运行</button>
      {/if}
      <div class="flex items-center gap-1 text-[13px] text-[var(--muted-foreground)]">
        <span>次数:</span>
        <input type="number" class="w-14" bind:value={scriptRunCount.value} min="1" />
      </div>
      <div class="flex items-center gap-1 text-[13px] text-[var(--muted-foreground)]">
        <span>间隔:</span>
        <input type="number" class="w-16" bind:value={scriptLoopInterval.value} min="0" />
        <span>ms</span>
      </div>
    </div>
    <div class="flex gap-2">
      <button class="btn btn-ghost" onclick={handleSaveConfig}>保存</button>
      <button class="btn btn-ghost" onclick={handleLoadConfig}>加载</button>
      <button class="btn btn-ghost" onclick={handleClearConfig}>清空</button>
    </div>
  </div>
</div>

<!-- 页签右键菜单 -->
{#if pageMenu.open}
  <div
    data-page-menu
    class="fixed z-[60] min-w-[120px] border rounded-md shadow-lg py-1"
    style="left: {pageMenu.x}px; top: {pageMenu.y}px; background: var(--background-elevated); border-color: var(--border);"
    onclick={(e) => e.stopPropagation()}
  >
    <button
      class="flex items-center w-full px-3 py-1.5 text-[13px] text-left text-[var(--error)] hover:bg-[var(--border-subtle)] cursor-pointer {currentModulePages().length <= 1 ? 'opacity-40 cursor-not-allowed' : ''}"
      disabled={currentModulePages().length <= 1}
      onclick={handleDeletePageFromMenu}
    >
      删除页签
    </button>
  </div>
{/if}

<!-- 删除二次确认弹窗 -->
{#if confirmDelete.open}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center"
    style="background: rgba(0,0,0,0.35);"
    onclick={handleCancelDelete}
  >
    <div
      class="rounded-lg shadow-xl w-[300px] border"
      style="background: var(--background-elevated); border-color: var(--border);"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="px-6 py-5">
        <div class="text-[14px] font-medium text-[var(--foreground)] mb-2">删除页签</div>
        <div class="text-[13px] text-[var(--muted-foreground)]">
          确定删除此页签？该页签内的所有命令将被清除。
        </div>
      </div>
      <div class="flex justify-end gap-2 px-4 pb-4">
        <button class="btn btn-ghost" style="padding: 6px 14px;" onclick={handleCancelDelete}>取消</button>
        <button
          class="btn cursor-pointer"
          style="padding: 6px 14px; background: var(--error); color: white; border-color: var(--error);"
          onclick={handleConfirmDelete}
        >删除</button>
      </div>
    </div>
  </div>
{/if}
