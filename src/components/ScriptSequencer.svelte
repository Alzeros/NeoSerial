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
    scriptCurrentRow,
    scriptLoopInterval,
    scriptModules,
    scriptRunCount,
    scriptRunning,
    scriptRunState,
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

  import { openFileDialog, saveFileDialog, loadSequenceConfig, saveSequenceConfig, loadSequenceAuto, saveSequenceAuto, sequenceRun, sequenceStop, send, onSequenceChanged } from '$lib/tauri';
  import { connected, windowPort } from '$lib/stores';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { onMount } from 'svelte';

  // 单行发送：点编号按钮即发送该行（用本行 hex/enter 设置）
  async function sendOne(index: number) {
    if (!connected.value) return;
    const page = currentModulePages()[activeScriptPage.value];
    const cmd = page?.commands[index];
    if (!cmd || !cmd.command.trim()) return;
    try {
      await send(windowPort.value!, cmd.command, cmd.enter ? 'Crlf' : 'None', cmd.hex);
    } catch (e) {
      console.error('单行发送失败:', e);
    }
  }

  async function handleRun() {
    const pages = currentModulePages();
    const page = pages[activeScriptPage.value];
    if (!page) return;
    // 初始化运行状态：总发送数=勾选行数×轮数，记录起始时刻
    const enabledCount = page.commands.filter((c: any) => c.enabled).length;
    scriptRunState.total = enabledCount * scriptRunCount.value;
    scriptRunState.sent = 0;
    scriptRunState.round = 1;
    scriptRunState.startedAt = Date.now();
    scriptRunState.finished = '';
    scriptRunning.value = true;
    try {
      await sequenceRun(windowPort.value!, page.commands, scriptRunCount.value, scriptLoopInterval.value);
    } catch (e) {
      // 启动失败（如未连接串口、序列已在运行）：复位 UI 运行态。
      // 后端在失败路径已复位 SEQUENCE_RUNNING 标志，这里把前端状态同步回去，
      // 否则 scriptRunning 卡在 true，停止按钮失效、也无法再次运行。
      console.error('序列执行失败:', e);
      scriptRunning.value = false;
      scriptCurrentRow.value = -1;
      scriptRunState.finished = '';
      scriptRunState.sent = 0;
      scriptRunState.total = 0;
      scriptRunState.round = 1;
    }
  }

  async function handleStop() {
    try {
      await sequenceStop(windowPort.value!);
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
      { name: '配置文件', extensions: ['json', 'ini'] },
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

  // ---- 自动加载 & 自动保存 ----
  // 启动时从默认路径加载；数据变化时防抖自动保存
  let autoSaveLoaded = $state(false);
  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null;

  async function autoLoad() {
    try {
      const modules = await loadSequenceAuto();
      if (modules.length > 0) {
        scriptModules.length = 0;
        scriptModules.push(...modules);
        activeScriptModule.value = 0;
        activeScriptPage.value = 0;
      }
    } catch (e) {
      console.error('自动加载序列配置失败:', e);
    } finally {
      autoSaveLoaded = true;
    }
  }

  // 防抖自动保存：数据变化 800ms 后写盘
  $effect(() => {
    // 深度追踪 scriptModules 的变化
    JSON.stringify(scriptModules);
    if (!autoSaveLoaded) return;
    if (autoSaveTimer) clearTimeout(autoSaveTimer);
    autoSaveTimer = setTimeout(async () => {
      try {
        await saveSequenceAuto(scriptModules);
      } catch (e) {
        console.error('自动保存序列配置失败:', e);
      }
    }, 800);
  });

  // 组件挂载时自动加载
  $effect(() => {
    if (!autoSaveLoaded) autoLoad();
  });

  // 多窗口快捷指令同步:其他窗口改了 sequence.json 并保存时,广播 sequence-changed。
  // 本窗口收到(非自己触发的)→ reload 同步。若自己有未保存改动(autoSaveTimer pending)
  // → 跳过(等自己保存,最后保存的赢;改动频率低,冲突概率极小)。
  const myLabel = getCurrentWebview().label;
  onMount(() => {
    const unlisten = onSequenceChanged((e) => {
      if (e.source === myLabel) return; // 自己触发的跳过
      if (autoSaveTimer) return;        // 自己有未保存改动,跳过避免丢失
      autoLoad();                        // reload 同步
    });
    return () => { unlisten.then((f) => f()); };
  });

  // 清空需二次确认：点"清空"只弹确认浮层，确认后才真正清空
  let confirmClear = $state<{ open: boolean }>({ open: false });

  function handleClearConfig() {
    confirmClear.open = true;
  }

  function handleConfirmClear() {
    confirmClear.open = false;
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

  function handleCancelClear() {
    confirmClear.open = false;
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
    // 输入框右键交给 App 的全局输入菜单，不弹页签菜单
    const t = e.target as HTMLElement;
    if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA') return;
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
    // 文本输入框（命令栏）右键交给 App 的输入菜单（复制/剪切/粘贴/全选）
    const t = e.target as HTMLElement;
    if (t.tagName === 'INPUT') {
      const type = (t as HTMLInputElement).type;
      if (type === 'text' || type === '') return;
    }
    if (t.tagName === 'TEXTAREA') return;
    // 其余（checkbox/number/button 等）触发行菜单
    e.preventDefault();
    e.stopPropagation();
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

  // ---- 执行状态区：三态文案 + 圆点 ----
  // 就绪态：N=表格总行数，M=勾选行数，实时更新
  const totalRows = $derived(
    currentModulePages()[activeScriptPage.value]?.commands.length ?? 0
  );
  const enabledRows = $derived(
    currentModulePages()[activeScriptPage.value]?.commands.filter((c: any) => c.enabled).length ?? 0
  );
  // 勾选行 Delay 之和(秒，保留一位小数)——空闲态"约 t s/轮"用
  const enabledDelaySec = $derived.by(() => {
    const cmds = currentModulePages()[activeScriptPage.value]?.commands ?? [];
    const sum = cmds.filter((c: any) => c.enabled).reduce((acc: number, c: any) => acc + (c.delay_ms || 0), 0);
    return Math.round(sum / 100) / 10;
  });
  // 能否运行：至少勾选一条
  const canRun = $derived(connected.value && enabledRows > 0 && !scriptRunning.value);

  // 结束态淡回：done/aborted 后 3 秒切回就绪态展示
  let finishedVisible = $state(false);
  let finishTimer: ReturnType<typeof setTimeout> | null = null;
  // 用时：结束触发时定格（finishedElapsed）
  let finishedElapsed = $state(0);

  $effect(() => {
    const f = scriptRunState.finished;
    if (f === 'done' || f === 'aborted') {
      // 定格用时
      finishedElapsed = scriptRunState.startedAt > 0
        ? Math.max(0, Math.round((Date.now() - scriptRunState.startedAt) / 1000))
        : 0;
      finishedVisible = true;
      if (finishTimer) clearTimeout(finishTimer);
      finishTimer = setTimeout(() => {
        finishedVisible = false;
        scriptRunState.finished = '';
      }, 3000);
    }
  });

  // 进度条比例：已发送 / 总发送（运行中）
  const progressPct = $derived(
    scriptRunState.total > 0
      ? Math.min(100, Math.round((scriptRunState.sent / scriptRunState.total) * 100))
      : 0
  );

  // 状态区显示态：'running' | 'finished' | 'idle'
  const statusMode = $derived(
    scriptRunning.value ? 'running' : (finishedVisible ? 'finished' : 'idle')
  );
  // 状态区文案（圆点颜色与按钮形态已表达的状态不再用文字重复）
  const statusText = $derived.by(() => {
    if (statusMode === 'running') {
      // 运行中：i=当前轮内已发送序号，N=单轮勾选条数
      const perRound = scriptRunCount.value > 0 ? scriptRunState.total / scriptRunCount.value : 0;
      const iInRound = perRound > 0
        ? ((scriptRunState.sent - 1) % perRound) + 1
        : scriptRunState.sent;
      // y=1 时省略轮次段
      if (scriptRunCount.value <= 1) return `${iInRound}/${perRound}条`;
      return `第${scriptRunState.round}/${scriptRunCount.value}轮 · ${iInRound}/${perRound}条`;
    }
    if (statusMode === 'finished') {
      // 中断（用户主动停止）单独标"已停止"；正常结束"已完成"
      const label = scriptRunState.finished === 'aborted' ? '已停止' : '已完成';
      return `${label} ${scriptRunState.sent}条 · ${finishedElapsed}s`;
    }
    // 空闲态：按勾选情况区分
    if (enabledRows === 0) return '未勾选指令';
    if (enabledRows === totalRows) return `待发送 ${totalRows} 条`;
    return `待发送 ${enabledRows}/${totalRows} 条`;
  });
  // 空闲态末尾"约 t s/轮"后缀（勾选行 Delay 之和；0 时不显示）
  const idleSuffix = $derived(
    statusMode === 'idle' && enabledDelaySec > 0 ? ` · 约${enabledDelaySec}s/轮` : ''
  );
</script>

<svelte:window on:click={() => { closePageMenu(); closeRowMenu(); }} on:contextmenu={(e) => {
  // 点菜单外部时关闭
  const t = e.target as HTMLElement;
  if ((pageMenu.open && !t.closest('[data-page-menu]') && !t.closest('[data-page-tab]'))
      || (rowMenu.open && !t.closest('[data-row-menu]') && !t.closest('[data-row]'))) {
    closePageMenu();
    closeRowMenu();
  }
}} on:keydown={(e) => {
  // Esc 关闭所有弹窗（弹窗不再支持点遮罩关闭，Esc 是键盘退出途径）
  if (e.key === 'Escape') {
    if (confirmDelete.open) handleCancelDelete();
    if (confirmClear.open) handleCancelClear();
    if (renameState.open) handleCancelRename();
    if (noteEdit.open) handleCancelNote();
    closePageMenu();
    closeRowMenu();
  }
}} />

<div class="flex h-full flex-col border-l border-[var(--border)]" data-theme-target="background-elevated" style="background: var(--background-elevated);">
  <!-- 模块切换栏：预置功能标题，文字风格（非 tag），当前项加粗+下划线区分 -->
  <div class="flex items-center gap-4 border-b border-[var(--border)] px-4 py-1" style="background: var(--background);">
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
  <!-- 不限页数:页签多到排不下时横向滚动,"+"按钮固定末尾(shrink-0)不被挤掉 -->
  <div class="flex items-center gap-1 border-b border-[var(--border)] px-3 py-2 overflow-x-auto">
    {#each currentModulePages() as page, i}
      <button
        data-page-tab
        class="shrink-0 rounded px-3 py-1.5 text-[13px] font-medium transition-colors cursor-pointer {i === activeScriptPage.value
          ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
          : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)]'}"
        onclick={() => (activeScriptPage.value = i)}
        oncontextmenu={(e) => handlePageContextMenu(e, i)}
        title="右键可编辑此页签"
      >
        {page.name}
      </button>
    {/each}
    <button
      class="shrink-0 rounded px-2 py-1.5 text-[13px] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] cursor-pointer"
      onclick={addScriptPage}
      title="新增页签"
    >
      +
    </button>
  </div>

  <!-- 命令序列表格（独立滚动，不撑开外部布局） -->
  <div class="script-list">
    <table class="w-full text-[13px] table-fixed">
      <thead class="sticky top-0" style="background: var(--background-elevated);">
        <tr class="text-[var(--muted-foreground)]">
          <th class="w-8 px-1 py-1 text-center font-medium">
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
          <th class="px-2 py-1 text-center font-medium">命令</th>
          <th class="w-10 px-1 py-1 text-center font-medium">
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
          <th class="w-8 px-1 py-1 text-center font-medium">
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
          <th class="w-[48px] px-1 py-1 text-center font-medium">Delay</th>
          <th class="w-[95px] px-2 py-1 text-center font-medium">注释</th>
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
                  class="select-none cursor-grab inline-flex items-center justify-center w-5 h-5 rounded-md border border-[var(--primary)] bg-[var(--primary)]/10 text-[var(--primary)] text-[12px] font-bold transition-colors hover:bg-[var(--primary)]/20"
                  title="拖动调整顺序"
                  onpointerdown={(e) => onPointerDown(e, i)}
                >☰</span>
              {:else}
                <button
                  data-row-toggle
                  class="inline-flex items-center justify-center w-5 h-5 rounded-md border text-[10px] font-medium transition-colors cursor-pointer {cmd.enabled
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
                onkeydown={(e) => {
                  // Enter 发送（与 BottomPanel 一致：用 e.code 兼容中文输入法组合中的物理回车）
                  if (e.code === 'Enter' && !e.shiftKey) {
                    e.preventDefault();
                    sendOne(i);
                  }
                }}
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
                class="w-full tnum rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1 text-[13px] text-center focus-visible:outline-none focus-visible:border-[var(--primary)]"
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

  <!-- 执行控制：两层布局，上弱（次级操作）下强（主操作），两层左右缘对齐 -->
  <div class="border-t border-[var(--border)]" style="background: var(--background-elevated);">
    <!-- 第一层：次级操作（ghost 文字按钮 + 调整顺序开关） -->
    <div class="flex items-center gap-2 px-3 py-2">
      <button
        class="h-7 -ml-2 px-3 rounded text-[12px] font-medium border border-[var(--border-strong)] bg-[var(--background-input)] text-[var(--foreground-secondary)] hover:text-[var(--foreground)] hover:border-[var(--primary)] transition-colors"
        onclick={handleSaveConfig}
      >另存为</button>
      <button
        class="h-7 px-3 rounded text-[12px] font-medium border border-[var(--border-strong)] bg-[var(--background-input)] text-[var(--foreground-secondary)] hover:text-[var(--foreground)] hover:border-[var(--primary)] transition-colors"
        onclick={handleLoadConfig}
      >加载</button>
      <div class="w-px h-4 bg-[var(--border)] mx-1"></div>
      <button
        class="h-7 px-3 rounded text-[12px] font-medium border border-[var(--border-strong)] bg-[var(--background-input)] text-[var(--foreground-secondary)] hover:text-[var(--error)] hover:border-[var(--error)] transition-colors"
        onclick={handleClearConfig}
      >清空</button>
      <label class="switch ml-auto {scriptRunning.value ? 'opacity-50 pointer-events-none' : ''}">
        <input type="checkbox" bind:checked={orderMode.value} disabled={scriptRunning.value} />
        <span class="switch-track"></span>
        <span class="switch-label">调整顺序</span>
      </label>
    </div>
    <!-- 两层分割线 -->
    <div class="border-t border-[var(--border-subtle)]"></div>
    <!-- 第二层：主操作（运行 + 状态区 + 参数组），底座底色 -->
    <div class="relative flex items-center gap-3 px-3 py-2.5" data-theme-target="background-deep" style="background: var(--background-deep);">
      {#if scriptRunning.value}
        <button
          class="btn btn-danger-solid h-9 leading-none inline-flex items-center gap-1.5"
          onclick={handleStop}
          title="停止"
        ><span class="text-[12px]">■</span>停止</button>
      {:else}
        <button
          class="btn btn-primary h-9 leading-none inline-flex items-center gap-1.5"
          onclick={handleRun}
          disabled={!canRun}
          title={!connected.value ? '未连接串口' : (enabledRows === 0 ? '未勾选任何指令' : '运行')}
        >
          <span class="text-[12px]">▶</span>运行
        </button>
      {/if}
      <!-- 执行状态区：左对齐紧跟运行按钮，flex-1 撑开 + min-w-0 允许省略号截断，不挤压右侧参数组 -->
      <div class="flex items-center gap-1.5 min-w-0 flex-1 tnum">
        <span
          class="h-2 w-2 rounded-full flex-shrink-0 {statusMode === 'running'
            ? 'bg-[var(--rx)] status-dot-pulse'
            : 'bg-[var(--muted-foreground)]'}"
        ></span>
        <span class="text-[12px] text-[var(--muted-foreground)] truncate">{statusText}{idleSuffix}</span>
      </div>
      <!-- 参数组：整体右对齐，单位移到输入框右侧；两组间 12px -->
      <div class="flex items-center gap-3 flex-shrink-0">
        <div class="flex items-center gap-1">
          <span class="text-[12px] text-[var(--muted-foreground)]">次数</span>
          <input
            type="number"
            class="no-spin tnum rounded border border-[var(--border)] bg-[var(--background-input)] text-[13px] text-center focus-visible:outline-none focus-visible:border-[var(--primary)]"
            style="height: 32px; width: 52px; padding: 0 8px;"
            bind:value={scriptRunCount.value}
            min="1"
            disabled={scriptRunning.value}
            oncontextmenu={(e) => { e.preventDefault(); e.stopPropagation(); }}
          />
          <span class="text-[12px] text-[var(--muted-foreground)]">次</span>
        </div>
        <div class="flex items-center gap-1">
          <span class="text-[12px] text-[var(--muted-foreground)]">间隔</span>
          <input
            type="number"
            class="no-spin tnum rounded border border-[var(--border)] bg-[var(--background-input)] text-[13px] text-center focus-visible:outline-none focus-visible:border-[var(--primary)]"
            style="height: 32px; width: 60px; padding: 0 8px;"
            bind:value={scriptLoopInterval.value}
            min="0"
            disabled={scriptRunCount.value <= 1 || scriptRunning.value}
            oncontextmenu={(e) => { e.preventDefault(); e.stopPropagation(); }}
          />
          <span class="text-[12px] text-[var(--muted-foreground)]">ms</span>
        </div>
      </div>
      <!-- 底部 2px 进度条：仅运行中显示，进度=已发送/总发送 -->
      {#if scriptRunning.value}
        <div class="absolute left-0 right-0 bottom-0 h-0.5 bg-[var(--border-subtle)]">
          <div
            class="h-full bg-[var(--primary)] transition-[width] duration-150"
            style="width: {progressPct}%;"
          ></div>
        </div>
      {/if}
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

<!-- 清空指令二次确认弹窗 -->
{#if confirmClear.open}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center"
    style="background: rgba(0,0,0,0.35);"
  >
    <div
      class="rounded-lg shadow-xl w-[300px] border"
      style="background: var(--background-elevated); border-color: var(--border);"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="px-6 py-5">
        <div class="text-[14px] font-medium text-[var(--foreground)] mb-2">清空指令</div>
        <div class="text-[13px] text-[var(--muted-foreground)]">
          确定清空当前页签的所有指令？所有命令内容将被重置为空。
        </div>
      </div>
      <div class="flex justify-end gap-2 px-4 pb-4">
        <button class="btn btn-ghost" style="padding: 6px 14px;" onclick={handleCancelClear}>取消</button>
        <button
          class="btn cursor-pointer"
          style="padding: 6px 14px; background: var(--error); color: white; border-color: var(--error);"
          onclick={handleConfirmClear}
        >清空</button>
      </div>
    </div>
  </div>
{/if}
