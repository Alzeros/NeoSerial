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

  // 改色时实时广播到主窗口预览（applyTheme 仅作用于编辑器自身窗口，
  // emit 让主窗口也 applyTheme 同步，用户直接看主窗口效果）
  function previewBroadcast() {
    emit('theme-preview', { custom: editCustom });
  }

  // 悬停颜色项时广播到主窗口，主窗口把该色临时覆盖为高对比色，
  // 用户直接看到哪些区域受影响——比画框直观得多
  function highlightField(field: string | null) {
    emit('theme-highlight', { field });
  }

  function changeVar(key: string, value: string) {
    editCustom[key] = value.toUpperCase();
    applyTheme('custom', editCustom);
    previewBroadcast();
    dirty = true;
  }

  function startFromPreset(key: string) {
    editCustom = { ...presetPalettes[key] };
    applyTheme('custom', editCustom);
    previewBroadcast();
    dirty = true;
  }

  function resetToDefault() {
    editCustom = defaultCustomTheme();
    applyTheme('custom', editCustom);
    previewBroadcast();
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
      previewBroadcast();
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
    previewBroadcast();
    dirty = false;
  }

  async function handleClose() {
    // 关闭：清除高亮 + 广播 null 让主窗口从 settings 重载已保存的主题
    highlightField(null);
    emit('theme-preview', { custom: null });
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
        previewBroadcast(); // 广播到主窗口，让主窗口也进入预览模式
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
  <div class="te-toolbar flex items-center gap-2 px-3 py-2 border-b flex-shrink-0 flex-wrap min-w-0" style="background: var(--background-elevated); border-color: var(--border);">
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

  <!-- 主区域：调色面板（全宽，实时预览在主窗口） -->
  <div class="te-scroll flex-1 overflow-y-auto px-3 py-2 min-h-0">
    <div class="text-[12px] font-medium text-[var(--muted-foreground)] mb-1.5">背景颜色</div>
    <div class="grid grid-cols-3 gap-1 mb-3">
      {#each customThemeFields.filter(f => f.key.startsWith('background')) as f}
        <label class="flex items-center gap-1.5 cursor-pointer rounded-md px-1 py-0.5 hover:bg-[var(--overlay-hover)] transition-colors"
          onmouseenter={() => highlightField(f.key)}
          onmouseleave={() => highlightField(null)}
        >
          <input
            type="color"
            class="w-5 h-5 flex-shrink-0 rounded border border-[var(--border)] cursor-pointer bg-transparent p-0"
            value={editCustom[f.key]}
            oninput={(e) => changeVar(f.key, (e.target as HTMLInputElement).value)}
          />
          <span class="flex flex-col min-w-0">
            <span class="text-[11px] text-[var(--foreground)] whitespace-nowrap">{f.label}</span>
            <span class="text-[10px] text-[var(--muted-foreground)] tnum">{editCustom[f.key]}</span>
          </span>
        </label>
      {/each}
    </div>

    <div class="text-[12px] font-medium text-[var(--muted-foreground)] mb-1.5">文字颜色</div>
    <div class="grid grid-cols-3 gap-1 mb-3">
      {#each customThemeFields.filter(f => f.key.startsWith('foreground') || f.key === 'muted-foreground') as f}
        <label class="flex items-center gap-1.5 cursor-pointer rounded-md px-1 py-0.5 hover:bg-[var(--overlay-hover)] transition-colors"
          onmouseenter={() => highlightField(f.key)}
          onmouseleave={() => highlightField(null)}
        >
          <input
            type="color"
            class="w-5 h-5 flex-shrink-0 rounded border border-[var(--border)] cursor-pointer bg-transparent p-0"
            value={editCustom[f.key]}
            oninput={(e) => changeVar(f.key, (e.target as HTMLInputElement).value)}
          />
          <span class="flex flex-col min-w-0">
            <span class="text-[11px] text-[var(--foreground)] whitespace-nowrap">{f.label}</span>
            <span class="text-[10px] text-[var(--muted-foreground)] tnum">{editCustom[f.key]}</span>
          </span>
        </label>
      {/each}
    </div>

    <div class="text-[12px] font-medium text-[var(--muted-foreground)] mb-1.5">功能色</div>
    <div class="grid grid-cols-3 gap-1 mb-3">
      {#each customThemeFields.filter(f => !f.key.startsWith('background') && !f.key.startsWith('foreground') && f.key !== 'muted-foreground' && f.key !== 'border') as f}
        <label class="flex items-center gap-1.5 cursor-pointer rounded-md px-1 py-0.5 hover:bg-[var(--overlay-hover)] transition-colors"
          onmouseenter={() => highlightField(f.key)}
          onmouseleave={() => highlightField(null)}
        >
          <input
            type="color"
            class="w-5 h-5 flex-shrink-0 rounded border border-[var(--border)] cursor-pointer bg-transparent p-0"
            value={editCustom[f.key]}
            oninput={(e) => changeVar(f.key, (e.target as HTMLInputElement).value)}
          />
          <span class="flex flex-col min-w-0">
            <span class="text-[11px] text-[var(--foreground)] whitespace-nowrap">{f.label}</span>
            <span class="text-[10px] text-[var(--muted-foreground)] tnum">{editCustom[f.key]}</span>
          </span>
        </label>
      {/each}
    </div>

    <div class="text-[12px] font-medium text-[var(--muted-foreground)] mb-1.5">边框</div>
    <div class="grid grid-cols-3 gap-1 mb-3">
      {#each customThemeFields.filter(f => f.key === 'border') as f}
        <label class="flex items-center gap-1.5 cursor-pointer rounded-md px-1 py-0.5 hover:bg-[var(--overlay-hover)] transition-colors"
          onmouseenter={() => highlightField(f.key)}
          onmouseleave={() => highlightField(null)}
        >
          <input
            type="color"
            class="w-5 h-5 flex-shrink-0 rounded border border-[var(--border)] cursor-pointer bg-transparent p-0"
            value={editCustom[f.key]}
            oninput={(e) => changeVar(f.key, (e.target as HTMLInputElement).value)}
          />
          <span class="flex flex-col min-w-0">
            <span class="text-[11px] text-[var(--foreground)] whitespace-nowrap">{f.label}</span>
            <span class="text-[10px] text-[var(--muted-foreground)] tnum">{editCustom[f.key]}</span>
          </span>
        </label>
      {/each}
    </div>

    <!-- 圆角 -->
    <div class="mt-3 px-1">
      <div class="text-[13px] font-medium text-[var(--foreground)] mb-1.5">圆角</div>
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

    <div class="mt-2 px-1 text-[11px] text-[var(--muted-foreground)] leading-relaxed">
      悬停可在主窗口高亮对应区域，改色实时预览。
    </div>
  </div>

  <!-- 底部按钮栏 -->
  <div class="flex items-center justify-between gap-2 px-3 py-2 border-t flex-shrink-0" style="background: var(--background-elevated); border-color: var(--border);">
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
  @media (max-width: 420px) {
    .te-label-text {
      display: none;
    }
  }
</style>
