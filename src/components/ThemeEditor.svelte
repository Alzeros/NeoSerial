<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { emit } from '@tauri-apps/api/event';
  import { Palette, Download, Upload, RotateCcw, Check, X } from 'lucide-svelte';
  import { theme, customTheme, applyTheme, themeMeta, cachedSettings } from '$lib/stores';
  import {
    customThemeFields,
    defaultCustomTheme,
    presetPalettes,
    customRadius,
    buildThemeFile,
    parseThemeFile,
    normalizeCustomTheme,
  } from '$lib/customTheme';
  import { getSettings, saveSettings, saveFileDialog, openFileDialog, exportThemeFile, importThemeFile } from '$lib/tauri';
  import type { Settings } from '$lib/types';

  const appWindow = getCurrentWindow();

  // 编辑副本（改色即时预览，保存才落 store）
  let editCustom = $state<Record<string, string>>(defaultCustomTheme());
  // 打开时的初始快照（"恢复"按钮回到这里）
  let initialCustom = $state<Record<string, string>>(defaultCustomTheme());
  let dirty = $state(false);
  let importError = $state('');
  let saved = $state(false);

  function changeVar(key: string, value: string) {
    editCustom[key] = value.toUpperCase();
    applyTheme('custom', editCustom);
    dirty = true;
  }

  function startFromPreset(key: string) {
    editCustom = { ...presetPalettes[key] };
    applyTheme('custom', editCustom);
    dirty = true;
  }

  function resetToDefault() {
    editCustom = defaultCustomTheme();
    applyTheme('custom', editCustom);
    dirty = true;
  }

  async function exportTheme() {
    const path = await saveFileDialog('导出主题', 'neoserial-theme.json', [
      { name: 'NeoSerial 主题', extensions: ['json'] },
    ]);
    if (!path) return;
    try {
      await exportThemeFile(path, buildThemeFile(editCustom));
    } catch (e) {
      importError = `导出失败: ${e}`;
    }
  }

  async function importTheme() {
    const path = await openFileDialog('导入主题', [
      { name: 'NeoSerial 主题', extensions: ['json'] },
    ]);
    if (!path) return;
    try {
      const parsed = parseThemeFile(await importThemeFile(path));
      if (!parsed) {
        importError = '主题文件格式无效';
        return;
      }
      importError = '';
      editCustom = parsed;
      applyTheme('custom', editCustom);
      dirty = true;
    } catch (e) {
      importError = `导入失败: ${e}`;
    }
  }

  async function handleSave() {
    customTheme.value = { ...editCustom };
    theme.value = 'custom';
    // 基于 cachedSettings 落盘；若编辑器窗口加载设置失败则现取一次
    let base = cachedSettings.value;
    if (!base) {
      base = await getSettings();
      cachedSettings.value = base;
    }
    const next: Settings = {
      ...base,
      presets: { ...base.presets, theme: 'custom', custom_theme: { ...editCustom } },
    };
    try {
      await saveSettings(next);
      cachedSettings.value = next;
      // 广播给其他窗口(main + win-*)重新应用主题
      await emit('theme-changed', {});
    } catch (e) {
      console.error('保存主题失败:', e);
      return;
    }
    // 保存成功：更新初始快照(恢复基准前移)、清 dirty、短暂反馈
    initialCustom = { ...editCustom };
    dirty = false;
    saved = true;
    setTimeout(() => (saved = false), 1500);
  }

  function handleRestore() {
    // 恢复到打开时的状态（撤销所有预览改动）
    editCustom = { ...initialCustom };
    applyTheme('custom', editCustom);
    dirty = false;
  }

  async function handleClose() {
    // 关闭：放弃未保存的预览改动，恢复到上次保存的主题
    applyTheme(theme.value, customTheme.value);
    await appWindow.close();
  }

  async function handleMinimize() {
    await appWindow.minimize();
  }

  async function handleToggleMaximize() {
    await appWindow.toggleMaximize();
  }

  // ESC 关闭（有未保存改动时不拦截，用户想走就走，改动不落盘）
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') handleClose();
  }

  onMount(() => {
    // 主题编辑器窗口：加载设置，把主题切到 custom 并应用当前色板
    getSettings()
      .then((s) => {
        cachedSettings.value = s;
        const c = normalizeCustomTheme(s.presets?.custom_theme);
        editCustom = c;
        initialCustom = { ...c };
        theme.value = 'custom';
        applyTheme('custom', editCustom);
      })
      .catch((e) => console.error('加载设置失败:', e));
  });
</script>

<svelte:window on:keydown={onKeydown} />

<!-- 根容器：严格等于窗口客户区（h-full/w-full + #app{height:100%}）。
     不要再写 min-width/min-height —— 窗口最小尺寸只由后端 set_min_size 一处决定，
     CSS 再设一个下限就会出现"两个下限"，拖到临界点时互相拉扯导致尺寸抖动。 -->
<div class="flex h-full w-full flex-col overflow-hidden" style="background: var(--background);">
  <!-- 自定义标题栏（decorations:false） -->
  <div
    class="flex items-center h-8 border-b select-none flex-shrink-0"
    style="background: var(--background-elevated); border-color: var(--border);"
  >
    <div
      data-tauri-drag-region
      class="flex-1 h-full flex items-center px-3 text-[13px] font-medium text-[var(--muted-foreground)]"
      ondblclick={handleToggleMaximize}
    >
      主题编辑器
    </div>
    <button
      class="flex items-center justify-center w-12 h-full text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
      onclick={handleMinimize}
      title="最小化"
    >
      <svg width="11" height="11" viewBox="0 0 12 12" fill="none"><rect x="1" y="5.5" width="10" height="1" fill="currentColor" /></svg>
    </button>
    <button
      class="flex items-center justify-center w-12 h-full text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
      onclick={handleToggleMaximize}
      title="最大化/还原"
    >
      <svg width="11" height="11" viewBox="0 0 12 12" fill="none"><rect x="1.5" y="1.5" width="9" height="9" stroke="currentColor" stroke-width="1" fill="none" rx="1" /></svg>
    </button>
    <button
      class="flex items-center justify-center w-12 h-full text-[var(--muted-foreground)] hover:bg-[var(--error)] hover:text-white cursor-pointer transition-colors"
      onclick={handleClose}
      title="关闭 (Esc)"
    >
      <svg width="11" height="11" viewBox="0 0 12 12" fill="none"><path d="M1 1L11 11M11 1L1 11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg>
    </button>
  </div>

  <!-- 工具栏 -->
  <!-- flex-wrap 是安全阀：窗口被拖到极窄时宁可换行，也绝不让工具条把窗口撑住。
       正常宽度（>=720）下不会换行，所以不会因换行改变工具条高度。 -->
  <div class="te-toolbar flex items-center gap-2 px-4 py-2.5 border-b flex-shrink-0 flex-wrap min-w-0" style="background: var(--background-elevated); border-color: var(--border);">
    <span class="te-toolbar-label text-[12px] text-[var(--muted-foreground)] whitespace-nowrap flex items-center gap-1" title="从预设载入整套配色再微调">
      <Palette size={13} /> <span class="te-label-text">从预设载入</span>
    </span>
    {#each themeMeta as t}
      <button
        class="inline-flex items-center gap-1.5 rounded-md border px-2 py-1 text-[12px] cursor-pointer transition-colors border-[var(--border)] text-[var(--foreground)] hover:border-[var(--border-strong)] hover:bg-[var(--border-subtle)]"
        title="载入「{t.label}」整套配色再微调"
        onclick={() => startFromPreset(t.key)}
      >
        <span class="relative w-3.5 h-3.5 rounded-sm border border-[var(--border)]" style="background: {t.bg};">
          <span class="absolute bottom-0 right-0 w-1.5 h-1.5 rounded-full" style="background: {t.accent};"></span>
        </span>
        {t.label}
      </button>
    {/each}
    <div class="flex-1"></div>
    <button class="btn btn-ghost" style="padding: 4px 10px; font-size: 12px;" onclick={importTheme} title="从 JSON 文件导入主题">
      <Upload size={13} /> 导入
    </button>
    <button class="btn btn-ghost" style="padding: 4px 10px; font-size: 12px;" onclick={exportTheme} title="导出当前色板为 JSON 文件">
      <Download size={13} /> 导出
    </button>
    <button class="btn btn-ghost" style="padding: 4px 10px; font-size: 12px;" onclick={resetToDefault} title="恢复到默认底稿">
      <RotateCcw size={13} /> 重置
    </button>
  </div>

  {#if importError}
    <div class="mx-4 mt-2 px-3 py-2 rounded text-[12px] flex-shrink-0" style="color: var(--error); background: var(--danger-overlay);">
      {importError}
    </div>
  {/if}

  <!-- 主区域：左颜色编辑 + 右实时预览 -->
  <div class="flex flex-1 min-h-0 min-w-0">
    <!-- 左侧：颜色项 -->
    <div class="te-scroll overflow-y-auto px-4 py-3" style="width: 320px; max-width: 46%; min-width: 264px; flex-shrink: 0; border-right: 1px solid var(--border);">
      <div class="text-[13px] font-medium text-[var(--foreground)] mb-3">基础色板</div>
      <div class="grid grid-cols-1 gap-2">
        {#each customThemeFields as f}
          <label class="flex items-center gap-3 cursor-pointer rounded-md px-2 py-1.5 hover:bg-[var(--overlay-hover)] transition-colors">
            <input
              type="color"
              class="w-8 h-8 flex-shrink-0 rounded border border-[var(--border)] cursor-pointer bg-transparent p-0"
              value={editCustom[f.key]}
              oninput={(e) => changeVar(f.key, (e.target as HTMLInputElement).value)}
            />
            <span class="flex flex-col min-w-0">
              <span class="text-[12px] text-[var(--foreground)] whitespace-nowrap">{f.label}</span>
              <span class="text-[11px] text-[var(--muted-foreground)] tnum">{editCustom[f.key]}</span>
            </span>
          </label>
        {/each}
      </div>

      <!-- 圆角 -->
      <div class="mt-4 px-2">
        <div class="text-[13px] font-medium text-[var(--foreground)] mb-2">圆角</div>
        <div class="flex items-center gap-3">
          <input
            type="range" min="0" max="24" step="1"
            class="flex-1 accent-[var(--primary)]"
            value={customRadius(editCustom)}
            oninput={(e) => changeVar('radius', (e.target as HTMLInputElement).value)}
          />
          <span class="w-10 text-center text-[12px] text-[var(--muted-foreground)] tnum">{customRadius(editCustom)}px</span>
        </div>
      </div>

      <div class="mt-4 px-2 text-[11px] text-[var(--muted-foreground)] leading-relaxed">
        悬停色、阴影、滚动条等衍生色会按以上基础色自动推导。整个窗口即实时预览——改任意色，背景、按钮、文字立刻变色。
      </div>
    </div>

    <!-- 右侧：实时预览面板（min-width:0 打破 flex 默认 min-width:auto，
         否则内部 select/input 的自然宽度会撑住窗口无法缩窄） -->
    <div class="te-scroll flex-1 overflow-y-auto p-5" style="background: var(--background-deep); min-width: 0; min-height: 0;">
      <div class="text-[13px] font-medium text-[var(--muted-foreground)] mb-3">实时预览</div>

      <!-- 模拟卡片：用 div + 简短文字演示按钮/输入框，不引真实 select/input
           —— 真实控件带全局 min-width 约束，会撑出不可压缩的卡片宽度。 -->
      <div class="rounded-lg p-4 mb-4 border" style="background: var(--background-elevated); border-color: var(--border); box-shadow: var(--shadow-md);">
        <div class="text-[14px] font-semibold mb-2" style="color: var(--foreground);">连接配置</div>
        <div class="flex flex-wrap items-center gap-2 mb-3">
          <span class="px-2 py-1 rounded text-[12px] border" style="background: var(--background-input); border-color: var(--border); color: var(--foreground);">COM3</span>
          <span class="px-2 py-1 rounded text-[12px] border" style="background: var(--background-input); border-color: var(--border); color: var(--foreground);">115200</span>
          <button class="btn btn-primary" style="padding: 6px 14px;">连接</button>
          <button class="btn btn-danger" style="padding: 6px 14px;">断开</button>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <span class="flex-1 min-w-[80px] px-2 py-1 rounded text-[12px] border truncate" style="background: var(--background-input); border-color: var(--border); color: var(--muted-foreground);">输入 AT 指令</span>
          <button class="btn btn-secondary" style="padding: 6px 14px;">发送</button>
          <button class="btn btn-clear-hover" style="padding: 6px 14px;">清空</button>
        </div>
      </div>

      <!-- 模拟日志区 -->
      <div class="rounded-lg border mb-4 overflow-hidden" style="background: var(--background-data); border-color: var(--border);">
        <div class="px-3 py-1.5 border-b text-[12px] font-medium" style="border-color: var(--border); color: var(--muted-foreground);">日志</div>
        <div class="p-3 font-mono text-[13px]" style="line-height: 1.8; font-family: var(--log-font-family);">
          <div><span class="dir-tx">Tx</span> <span style="color: var(--muted-foreground);">10:39:47</span> AT</div>
          <div><span class="dir-rx">Rx</span> <span style="color: var(--muted-foreground);">10:39:47</span> OK</div>
          <div><span class="dir-tx">Tx</span> <span style="color: var(--muted-foreground);">10:39:48</span> AT+CSQ</div>
          <div><span class="dir-rx">Rx</span> <span style="color: var(--muted-foreground);">10:39:48</span> +CSQ: 28,0</div>
          <div class="is-error">ERROR</div>
          <div><span class="dir-rx">Rx</span> <span style="color: var(--muted-foreground);">10:39:49</span> 模块就绪</div>
        </div>
      </div>

      <!-- 模拟状态栏 -->
      <div class="flex items-center gap-4 px-3 py-2 rounded-md border text-[12px]" style="background: var(--background-elevated); border-color: var(--border);">
        <span class="flex items-center gap-1.5" style="color: var(--foreground);">
          <span class="w-2 h-2 rounded-full status-dot-pulse" style="background: var(--primary);"></span>
          已连接 COM3
        </span>
        <span style="color: var(--muted-foreground);">Tx: <span class="tnum" style="color: var(--tx);">1,024</span></span>
        <span style="color: var(--muted-foreground);">Rx: <span class="tnum" style="color: var(--rx);">8,192</span></span>
        <span style="color: var(--warning);">⚠ 警告</span>
      </div>

      <!-- 色板一览：列数随宽度自适应，窗口窄时收缩为 2-3 列 -->
      <div class="mt-4 grid gap-2" style="grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));">
        {#each customThemeFields as f}
          <div class="rounded-md border p-2 text-center" style="background: var(--background-elevated); border-color: var(--border);">
            <div class="w-full h-8 rounded mb-1 border" style="background: {editCustom[f.key]}; border-color: var(--border);"></div>
            <div class="text-[10px] text-[var(--muted-foreground)] truncate">{f.label}</div>
          </div>
        {/each}
      </div>
    </div>
  </div>

  <!-- 底部按钮栏 -->
  <div class="flex items-center justify-between gap-2 px-4 py-3 border-t flex-shrink-0" style="background: var(--background-elevated); border-color: var(--border);">
    <div class="text-[12px] text-[var(--muted-foreground)]">
      {#if dirty}<span style="color: var(--warning);">● 有未保存改动</span>{:else if saved}<span style="color: var(--primary);">✓ 已应用</span>{:else}<span style="color: var(--muted-foreground);">已保存</span>{/if}
    </div>
    <div class="te-actions flex gap-2">
      <button class="btn btn-ghost" style="padding: 6px 14px;" onclick={handleRestore} disabled={!dirty}>恢复上次保存</button>
      <button class="btn btn-secondary" style="padding: 6px 14px;" onclick={handleClose}>关闭</button>
      <button class="btn btn-primary" style="padding: 6px 14px;" onclick={handleSave}>
        {#if saved}<Check size={14} class="inline" /> 已应用{:else}保存并应用{/if}
      </button>
    </div>
  </div>
</div>

<style>
  /* 滚动槽位常驻：滚动条出现/消失不再改变内容宽度，
     避免"内容变高→出滚动条→变窄→内容更高"的临界抖动。 */
  .te-scroll {
    scrollbar-gutter: stable;
  }

  /* 底部按钮永不换行，宽度不足时整组不动，让左侧状态文字去挤压 */
  .te-actions {
    flex-shrink: 0;
    white-space: nowrap;
  }

  /* 窄窗口下收起"从预设载入"文字（只留图标 + tooltip），
     让工具条在最小宽度 720 下仍能保持单行，不因换行改变高度。 */
  @media (max-width: 900px) {
    .te-label-text {
      display: none;
    }
  }
</style>
