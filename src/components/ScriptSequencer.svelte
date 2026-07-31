<script lang="ts">
  import {
    activeScriptModule,
    activeScriptPage,
    addScriptPage,
    addScriptRow,
    currentModulePages,
    reorderScriptRow,
    removeScriptPage,
    removeScriptRow,
    scriptLoopInterval,
    scriptModules,
    scriptRunCount,
    scriptRunning,
    switchScriptModule,
  } from '$lib/stores';

  // 顺序调整模式：开启后行可拖拽排序
  let orderMode = $state<{ value: boolean }>({ value: false });

  // ---- 指针事件拖拽（不依赖 HTML5 DnD，WebView2 里更可靠）----
  let tbodyEl: HTMLElement | null = null;
  let dragState: {
    from: number;
    startY: number;
    moved: boolean;
    targetIndex: number;
    ghost: HTMLElement | null;
  } | null = null;
  let activeMove: ((e: PointerEvent) => void) | null = null;
  let activeUp: ((e: PointerEvent) => void) | null = null;

  function getRowEls(): HTMLElement[] {
    return Array.from(tbodyEl?.querySelectorAll('tr[data-row]') ?? []) as HTMLElement[];
  }

  function onPointerDown(e: PointerEvent, index: number) {
    if (!orderMode.value) return;
    if (e.button !== 0) return;
    e.preventDefault();
    tbodyEl = (e.currentTarget as HTMLElement).closest('tbody') as HTMLElement;
    dragState = { from: index, startY: e.clientY, moved: false, targetIndex: index, ghost: null };
    activeMove = onPointerMove;
    activeUp = onPointerUp;
    window.addEventListener('pointermove', activeMove, { passive: false });
    window.addEventListener('pointerup', activeUp, { passive: false });
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragState) return;
    if (!dragState.moved && Math.abs(e.clientY - dragState.startY) < 3) return;
    if (!dragState.moved) {
      dragState.moved = true;
      const src = getRowEls()[dragState.from];
      if (src) {
        const g = src.cloneNode(true) as HTMLElement;
        const r = src.getBoundingClientRect();
        g.style.position = 'fixed';
        g.style.left = r.left + 'px';
        g.style.top = r.top + 'px';
        g.style.width = r.width + 'px';
        g.style.opacity = '0.85';
        g.style.pointerEvents = 'none';
        g.style.zIndex = '9999';
        g.style.background = 'var(--background-elevated)';
        g.style.border = '1px solid var(--primary)';
        g.style.boxShadow = '0 4px 12px rgba(0,0,0,0.25)';
        document.body.appendChild(g);
        dragState.ghost = g;
      }
      getRowEls().forEach((r) => { r.style.opacity = '0.3'; });
    }
    if (dragState.ghost) {
      dragState.ghost.style.top = (e.clientY - 18) + 'px';
    }
    const rows = getRowEls();
    let target = dragState.from;
    const y = e.clientY;
    for (let i = 0; i < rows.length; i++) {
      const rr = rows[i].getBoundingClientRect();
      if (y < rr.top + rr.height / 2) { target = i; break; }
      if (i === rows.length - 1) target = i;
    }
    dragState.targetIndex = target;
    rows.forEach((r, i) => {
      r.style.borderTop = (i === target && target !== dragState!.from)
        ? '2px solid var(--primary)' : '';
    });
    e.preventDefault();
  }

  function onPointerUp(_e: PointerEvent) {
    if (activeMove) window.removeEventListener('pointermove', activeMove);
    if (activeUp) window.removeEventListener('pointerup', activeUp);
    activeMove = null;
    activeUp = null;
    if (dragState) {
      getRowEls().forEach((r) => { r.style.opacity = ''; r.style.borderTop = ''; });
      if (dragState.ghost) dragState.ghost.remove();
      if (dragState.moved && dragState.targetIndex !== dragState.from) {
        reorderScriptRow(dragState.from, dragState.targetIndex);
      }
      dragState = null;
    }
  }

  import { openFileDialog, saveFileDialog, loadSequenceConfig, saveSequenceConfig, sequenceRun, sequenceStop, send } from '$lib/tauri';
  import { connected } from '$lib/stores';

  // 单行发送：点编号按钮即发送该行（用本行 hex/enter 设置）
  async function sendOne(index: number) {
    if (!connected.value) return;
    const page = currentModulePages()[activeScriptPage.value];
    const cmd = page?.commands[index];
    if (!cmd || !cmd.command.trim()) return;
    try {
      await send(cmd.command, cmd.enter ? 'Crlf' : 'None', cmd.hex);
    } catch (e) {
      console.error('单行发送失败:', e);
    }
  }

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
      delay_ms: 0,
      note: '',
    }));
  }

  // 右键页签：弹出"重命名/删除"菜单（删除：多页才允许删，单页禁用）
  let pageMenu = $state<{ open: boolean; x: number; y: number; index: number }>({
    open: false, x: 0, y: 0, index: -1,
  });
  // 删除二次确认浮层
  let confirmDelete = $state<{ open: boolean; index: number }>({ open: false, index: -1 });
  // 重命名浮层
  let renameState = $state<{ open: boolean; index: number; name: string }>({ open: false, index: -1, name: '' });

  function handlePageContextMenu(e: MouseEvent, index: number) {
    e.preventDefault();
    pageMenu = { open: true, x: clampMenuX(e.clientX), y: clampMenuY(e.clientY), index };
  }

  function closePageMenu() {
    pageMenu.open = false;
  }

  // 点"重命名页签" → 关右键菜单 → 开重命名浮层（预填当前名）
  function handleRenamePageFromMenu() {
    const idx = pageMenu.index;
    const pages = currentModulePages();
    const cur = pages[idx]?.name ?? '';
    closePageMenu();
    renameState = { open: true, index: idx, name: cur };
  }

  function handleConfirmRename() {
    const idx = renameState.index;
    const pages = currentModulePages();
    const page = pages[idx];
    if (page) page.name = renameState.name.trim() || page.name;
    renameState.open = false;
  }

  function handleCancelRename() {
    renameState.open = false;
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

  // 命令行右键：删除当前行（至少保留 1 行）
  let rowMenu = $state<{ open: boolean; x: number; y: number; index: number }>({
    open: false, x: 0, y: 0, index: -1,
  });

  function handleRowContextMenu(e: MouseEvent, index: number) {
    e.preventDefault();
    rowMenu = { open: true, x: clampMenuX(e.clientX), y: clampMenuY(e.clientY), index };
  }

  function closeRowMenu() {
    rowMenu.open = false;
  }

  // 右键菜单边界检测：靠近右/下边缘时向左/上偏移，避免超出窗口不可见
  // 菜单预估宽 140、高 120，留 8px 安全边距
  const MENU_W = 140;
  const MENU_H = 120;
  const SAFE = 8;
  function clampMenuX(x: number): number {
    const vw = window.innerWidth;
    return x + MENU_W + SAFE > vw ? Math.max(SAFE, x - MENU_W) : x;
  }
  function clampMenuY(y: number): number {
    const vh = window.innerHeight;
    return y + MENU_H + SAFE > vh ? Math.max(SAFE, y - MENU_H) : y;
  }

  function handleDeleteRowFromMenu() {
    const idx = rowMenu.index;
    closeRowMenu();
    removeScriptRow(idx);
  }

  // 行右键：编辑注释（弹窗，默认空，预填当前 note）
  let noteEdit = $state<{ open: boolean; index: number; text: string }>({ open: false, index: -1, text: '' });

  function handleEditNoteFromMenu() {
    const idx = rowMenu.index;
    const page = currentModulePages()[activeScriptPage.value];
    const cur = page?.commands[idx]?.note ?? '';
    closeRowMenu();
    noteEdit = { open: true, index: idx, text: cur };
  }

  function handleConfirmNote() {
    const idx = noteEdit.index;
    const page = currentModulePages()[activeScriptPage.value];
    if (page?.commands[idx]) page.commands[idx].note = noteEdit.text;
    noteEdit.open = false;
  }

  function handleCancelNote() {
    noteEdit.open = false;
  }
</script>

<svelte:window on:click={() => { closePageMenu(); closeRowMenu(); }} on:contextmenu={(e) => {
  // 点菜单外部时关闭
  const t = e.target as HTMLElement;
  if ((pageMenu.open && !t.closest('[data-page-menu]') && !t.closest('[data-page-tab]'))
      || (rowMenu.open && !t.closest('[data-row-menu]') && !t.closest('[data-row]'))) {
    closePageMenu();
    closeRowMenu();
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
    <table class="w-full text-[13px] table-fixed">
      <thead class="sticky top-0" style="background: var(--background-elevated);">
        <tr class="text-[var(--muted-foreground)]">
          <th class="w-8 px-1 py-2 text-center font-medium">
            <button
              class="w-full rounded px-1 py-0.5 text-[12px] font-medium transition-colors {(currentModulePages()[activeScriptPage.value]?.commands.every((c: any) => c.enabled) ?? false)
                ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)]'}"
              title="全选/取消选中"
              onclick={() => {
                const page = currentModulePages()[activeScriptPage.value];
                if (!page || page.commands.length === 0) return;
                const allOn = page.commands.every((c: any) => c.enabled);
                page.commands.forEach((c: any) => (c.enabled = !allOn));
              }}
            >#</button>
          </th>
          <th class="px-2 py-2 text-center font-medium">命令</th>
          <th class="w-10 px-1 py-2 text-center font-medium">
            <button
              class="w-full rounded px-1 py-0.5 text-[12px] font-medium transition-colors {(currentModulePages()[activeScriptPage.value]?.commands.every((c: any) => c.hex) ?? false)
                ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)]'}"
              title="全选/取消 Hex"
              onclick={() => {
                const page = currentModulePages()[activeScriptPage.value];
                if (!page || page.commands.length === 0) return;
                const allOn = page.commands.every((c: any) => c.hex);
                page.commands.forEach((c: any) => (c.hex = !allOn));
              }}
            >Hex</button>
          </th>
          <th class="w-8 px-1 py-2 text-center font-medium">
            <button
              class="w-full rounded px-1 py-0.5 text-[12px] font-medium transition-colors {(currentModulePages()[activeScriptPage.value]?.commands.every((c: any) => c.enter) ?? false)
                ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)]'}"
              title="全选/取消 回车"
              onclick={() => {
                const page = currentModulePages()[activeScriptPage.value];
                if (!page || page.commands.length === 0) return;
                const allOn = page.commands.every((c: any) => c.enter);
                page.commands.forEach((c: any) => (c.enter = !allOn));
              }}
            >↩</button>
          </th>
          <th class="w-[48px] px-1 py-2 text-center font-medium">Delay</th>
          <th class="w-[95px] px-2 py-2 text-center font-medium">注释</th>
        </tr>
      </thead>
      <tbody>
        {#each currentModulePages()[activeScriptPage.value]?.commands as cmd, i (cmd)}
          <tr
            data-row
            class="border-t border-[var(--border-subtle)] hover:bg-[var(--border-subtle)]"
            oncontextmenu={(e) => handleRowContextMenu(e, i)}
          >
            <td class="px-1 py-1 text-center text-[var(--muted-foreground)]">
              {#if orderMode.value}
                <span
                  class="select-none cursor-grab inline-flex items-center justify-center w-6 h-6 rounded-md border border-[var(--primary)] bg-[var(--primary)]/10 text-[var(--primary)] text-[14px] font-bold transition-colors hover:bg-[var(--primary)]/20"
                  title="拖动调整顺序"
                  onpointerdown={(e) => onPointerDown(e, i)}
                >⠿</span>
              {:else}
                <button
                  class="inline-flex items-center justify-center w-6 h-6 rounded-md border text-[12px] font-medium transition-colors cursor-pointer {cmd.enabled
                    ? 'border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]'
                    : 'border-[var(--border)] bg-[var(--border-subtle)] text-[var(--muted-foreground)] hover:border-[var(--primary)] hover:text-[var(--primary)]'}"
                  title={cmd.enabled ? '已选中（点击取消）' : '未选中（点击选中）'}
                  onclick={() => (cmd.enabled = !cmd.enabled)}
                >{i + 1}</button>
              {/if}
            </td>
            <td class="px-2 py-1">
              <input
                class="w-full rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
                bind:value={cmd.command}
                disabled={orderMode.value}
              />
            </td>
            <td class="px-1 py-1 text-center">
              <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" bind:checked={cmd.hex} disabled={orderMode.value} />
            </td>
            <td class="px-1 py-1 text-center">
              <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" bind:checked={cmd.enter} disabled={orderMode.value} />
            </td>
            <td class="px-1 py-1">
              <input
                type="number"
                class="w-full rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1 text-[13px] text-center focus-visible:outline-none focus-visible:border-[var(--primary)]"
                bind:value={cmd.delay_ms}
                disabled={orderMode.value}
              />
            </td>
            <td class="px-2 py-1">
              <button
                class="w-full rounded border px-2 py-1 text-[13px] transition-colors truncate text-center flex items-center justify-center {connected.value
                  ? 'border-[var(--border)] bg-[var(--border-subtle)] text-[var(--foreground)] hover:bg-[var(--primary)] hover:text-[var(--primary-foreground)] hover:border-[var(--primary)] cursor-pointer'
                  : 'border-[var(--border-subtle)] text-[var(--muted-foreground)] opacity-40 cursor-not-allowed'}"
                style="padding: 2px 3px; line-height: 1;"
                title={cmd.note ? `发送：${cmd.note}` : (connected.value ? '点击发送此行（右键编辑注释）' : '未连接')}
                disabled={!connected.value}
                onclick={() => sendOne(i)}
              >{cmd.note || '发送'}</button>
            </td>
          </tr>
        {/each}
      </tbody>
      <!-- 第 N+1 行：大 + 号，点击新增一行 -->
      <tfoot>
        <tr>
          <td colspan="6" class="px-2 py-1">
            <button
              data-add-row
              class="w-full py-1 text-[13px] font-medium text-[var(--muted-foreground)] hover:bg-[var(--primary)]/10 hover:text-[var(--primary)] border border-dashed border-[var(--border)] hover:border-[var(--primary)] cursor-pointer transition-colors rounded-md"
              onclick={addScriptRow}
              title="新增一行"
            >+ 新增一行</button>
          </td>
        </tr>
      </tfoot>
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
    <div class="flex gap-2 items-center">
      <button class="btn btn-ghost" onclick={handleSaveConfig}>保存</button>
      <button class="btn btn-ghost" onclick={handleLoadConfig}>加载</button>
      <button class="btn btn-ghost" onclick={handleClearConfig}>清空</button>
      <label class="switch ml-auto">
        <input type="checkbox" bind:checked={orderMode.value} />
        <span class="switch-track"></span>
        <span class="switch-label">调整顺序</span>
      </label>
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
      class="flex items-center w-full px-3 py-1.5 text-[13px] text-left text-[var(--foreground)] hover:bg-[var(--border-subtle)] cursor-pointer"
      onclick={handleRenamePageFromMenu}
    >
      重命名页签
    </button>
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

<!-- 重命名弹窗 -->
{#if renameState.open}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center"
    style="background: rgba(0,0,0,0.35);"
    onclick={handleCancelRename}
  >
    <div
      class="rounded-lg shadow-xl w-[300px] border"
      style="background: var(--background-elevated); border-color: var(--border);"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="px-6 py-5">
        <div class="text-[14px] font-medium text-[var(--foreground)] mb-2">重命名页签</div>
        <input
          class="w-full rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
          bind:value={renameState.name}
          onkeydown={(e) => { if (e.key === 'Enter') handleConfirmRename(); if (e.key === 'Escape') handleCancelRename(); }}
        />
      </div>
      <div class="flex justify-end gap-2 px-4 pb-4">
        <button class="btn btn-ghost" style="padding: 6px 14px;" onclick={handleCancelRename}>取消</button>
        <button
          class="btn btn-primary cursor-pointer"
          style="padding: 6px 14px;"
          onclick={handleConfirmRename}
        >确定</button>
      </div>
    </div>
  </div>
{/if}

{#if rowMenu.open}
  <div
    data-row-menu
    class="fixed z-[60] min-w-[120px] border rounded-md shadow-lg py-1"
    style="left: {rowMenu.x}px; top: {rowMenu.y}px; background: var(--background-elevated); border-color: var(--border);"
    onclick={(e) => e.stopPropagation()}
  >
    <button
      class="flex items-center w-full px-3 py-1.5 text-[13px] text-left text-[var(--foreground)] hover:bg-[var(--border-subtle)] cursor-pointer"
      onclick={handleEditNoteFromMenu}
    >
      编辑注释
    </button>
    <button
      class="flex items-center w-full px-3 py-1.5 text-[13px] text-left text-[var(--error)] hover:bg-[var(--border-subtle)] cursor-pointer {currentModulePages()[activeScriptPage.value]?.commands.length <= 1 ? 'opacity-40 cursor-not-allowed' : ''}"
      disabled={currentModulePages()[activeScriptPage.value]?.commands.length <= 1}
      onclick={handleDeleteRowFromMenu}
    >
      删除此行
    </button>
  </div>
{/if}

<!-- 编辑注释弹窗 -->
{#if noteEdit.open}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center"
    style="background: rgba(0,0,0,0.35);"
    onclick={handleCancelNote}
  >
    <div
      class="rounded-lg shadow-xl w-[320px] border"
      style="background: var(--background-elevated); border-color: var(--border);"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="px-6 py-5">
        <div class="text-[14px] font-medium text-[var(--foreground)] mb-2">编辑注释</div>
        <input
          class="w-full rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
          bind:value={noteEdit.text}
          placeholder="为该行写点说明..."
          onkeydown={(e) => { if (e.key === 'Enter') handleConfirmNote(); if (e.key === 'Escape') handleCancelNote(); }}
        />
      </div>
      <div class="flex justify-end gap-2 px-4 pb-4">
        <button class="btn btn-ghost" style="padding: 6px 14px;" onclick={handleCancelNote}>取消</button>
        <button
          class="btn btn-primary cursor-pointer"
          style="padding: 6px 14px;"
          onclick={handleConfirmNote}
        >确定</button>
      </div>
    </div>
  </div>
{/if}
