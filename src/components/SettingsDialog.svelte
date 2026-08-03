<script lang="ts">
  import { presetBaudRates, cachedSettings, themeColor, themeColorMeta, themeColorHex, applyThemeColor, logFontSize, logLineHeight, applyLogFont, logDirLabelStyle } from '$lib/stores';
  import { saveSettings } from '$lib/tauri';
  import type { Settings } from '$lib/types';

  let open = $state(false);

  // 左侧导航：当前激活的设置项
  type Section = 'general' | 'appearance' | 'log';
  let activeSection = $state<Section>('general');
  const sections: { key: Section; label: string }[] = [
    { key: 'general', label: '通用' },
    { key: 'appearance', label: '外观' },
    { key: 'log', label: '日志显示' },
  ];

  // 主题色编辑副本（打开时从 store 拷贝，取消不应用；选中即时预览）
  let editThemeColor = $state<string>('blue');
  // 日志字体编辑副本
  let editFontSize = $state(14);
  let editLineHeight = $state(1.6);
  let editDirLabel = $state<'short' | 'full'>('short');
  // 打开前的原值（取消时恢复，因部分预览直接改了 store）
  let origDirLabel: 'short' | 'full' = 'short';

  // 内置默认波特率：不可删除，只能在此基础上增删用户自定义项
  const DEFAULT_BAUD_RATES = [9600, 115200, 921600];
  const isDefault = (n: number) => DEFAULT_BAUD_RATES.includes(n);

  // 预设波特率编辑副本（打开时从 store 拷贝，取消不污染 store）
  let editBaudRates = $state<number[]>([]);
  let newBaud = $state('');

  export function show() {
    editBaudRates = [...presetBaudRates.value];
    editThemeColor = themeColor.value;
    editFontSize = logFontSize.value;
    editLineHeight = logLineHeight.value;
    editDirLabel = logDirLabelStyle.value;
    origDirLabel = logDirLabelStyle.value;
    newBaud = '';
    activeSection = 'general';
    open = true;
  }

  // 选中主题色：即时预览（应用到 <html>），但不落盘；取消则恢复原值
  function selectThemeColor(value: string) {
    editThemeColor = value;
    applyThemeColor(value);
  }

  // 字号/行高：即时预览
  function changeFontSize(v: number) {
    editFontSize = v;
    applyLogFont(v, editLineHeight);
  }
  function changeLineHeight(v: number) {
    editLineHeight = v;
    applyLogFont(editFontSize, v);
  }
  // 方向标签样式：即时预览
  function changeDirLabel(v: 'short' | 'full') {
    editDirLabel = v;
    logDirLabelStyle.value = v;
  }

  // 自定义色：用原生颜色选择器，选后存为 'custom:#RRGGBB'
  function selectCustomColor(e: Event) {
    const input = e.target as HTMLInputElement;
    const hex = input.value.toUpperCase();
    selectThemeColor(`custom:${hex}`);
  }

  // 当前是否自定义色
  function isCustom(value: string): boolean {
    return value.startsWith('custom:');
  }

  function addBaud() {
    const n = Number(newBaud);
    if (!n || n <= 0) return;
    if (editBaudRates.includes(n)) {
      newBaud = '';
      return;
    }
    editBaudRates = [...editBaudRates, n].sort((a, b) => a - b);
    newBaud = '';
  }

  function removeBaud(n: number) {
    // 内置默认波特率不可删除
    if (isDefault(n)) return;
    editBaudRates = editBaudRates.filter((b) => b !== n);
  }

  async function handleSave() {
    // 合并：内置默认 3 项一定保留 + 用户自定义项（去重、排序）
    const userAdded = editBaudRates.filter((b) => !isDefault(b));
    const rates = [...new Set([...DEFAULT_BAUD_RATES, ...userAdded])].sort((a, b) => a - b);
    presetBaudRates.value = rates;
    // 主题色：预览值落盘到 store（<html> 已在选中时应用）
    themeColor.value = editThemeColor;
    // 日志字体：预览值落盘到 store
    logFontSize.value = editFontSize;
    logLineHeight.value = editLineHeight;
    logDirLabelStyle.value = editDirLabel;
    // 落盘：基于缓存 settings 透传，仅更新 presets 与 ui 字体
    const base = cachedSettings.value;
    if (base) {
      const next: Settings = {
        ...base,
        ui: { ...base.ui, log_font_size: editFontSize, log_line_height: editLineHeight, log_dir_label: editDirLabel },
        presets: { baud_rates: rates, theme_color: editThemeColor },
      };
      try {
        await saveSettings(next);
        cachedSettings.value = next;
      } catch (e) {
        console.error('保存设置失败:', e);
      }
    }
    open = false;
  }

  function handleCancel() {
    // 取消：恢复打开前的主题色与字体（撤销预览）
    applyThemeColor(themeColor.value);
    applyLogFont(logFontSize.value, logLineHeight.value);
    logDirLabelStyle.value = origDirLabel;
    open = false;
  }
</script>

<svelte:window on:keydown={(e) => {
  // Esc 关闭设置弹窗（弹窗不再支持点遮罩关闭）
  if (e.key === 'Escape' && open) handleCancel();
}} />

{#if open}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center"
    style="background: rgba(0,0,0,0.35);"
  >
    <div
      class="rounded-lg shadow-xl w-[560px] border flex flex-col"
      style="background: var(--background-elevated); border-color: var(--border);"
      onclick={(e) => e.stopPropagation()}
    >
      <!-- 标题 -->
      <div class="px-6 py-4 border-b border-[var(--border)]">
        <div class="text-[15px] font-semibold text-[var(--foreground)]">设置</div>
      </div>

      <!-- 左右分栏：左导航 + 右内容（固定高度，避免切换设置项时弹窗跳动） -->
      <div class="flex" style="height: 340px;">
        <!-- 左侧导航 -->
        <nav class="w-[120px] flex-shrink-0 border-r border-[var(--border)] py-2">
          {#each sections as s}
            <button
              class="block w-full text-left px-4 py-2 text-[13px] transition-colors {activeSection === s.key
                ? 'bg-[var(--border-subtle)] text-[var(--primary)] font-medium border-l-2 border-[var(--primary)]'
                : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] border-l-2 border-transparent'}"
              onclick={() => (activeSection = s.key)}
            >{s.label}</button>
          {/each}
        </nav>

        <!-- 右侧内容区（独立滚动） -->
        <div class="flex-1 overflow-y-auto px-6 py-5">
          {#if activeSection === 'general'}
            <!-- 通用：预设波特率 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">预设波特率</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-3">
              添加后可在连接栏波特率下拉中选择。
            </div>

            <div class="flex flex-wrap gap-2 mb-3 min-h-[28px]">
              {#each editBaudRates as b}
                <span
                  class="inline-flex items-center gap-1 rounded-md border px-2 py-1 text-[13px]"
                  style="border-color: var(--border); background: var(--border-subtle); color: var(--foreground);"
                >
                  {b}
                  <button
                    class="leading-none {isDefault(b)
                      ? 'text-[var(--muted-foreground)] opacity-30 cursor-not-allowed'
                      : 'text-[var(--muted-foreground)] hover:text-[var(--error)] cursor-pointer'}"
                    title={isDefault(b) ? '内置预设，不可删除' : '移除'}
                    disabled={isDefault(b)}
                    onclick={() => removeBaud(b)}
                  >×</button>
                </span>
              {:else}
                <span class="text-[12px] text-[var(--muted-foreground)] italic">暂无预设</span>
              {/each}
            </div>

            <div class="flex items-center gap-2">
              <input
                type="number"
                class="flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
                bind:value={newBaud}
                placeholder="输入波特率，如 4800"
                onkeydown={(e) => { if (e.key === 'Enter') addBaud(); }}
              />
              <button class="btn btn-secondary" style="padding: 6px 14px;" onclick={addBaud}>添加</button>
            </div>
          {:else if activeSection === 'appearance'}
            <!-- 外观：主题色 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">主题色</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-3">
              选择应用图标的强调色，点击即时预览。
            </div>
            <div class="flex items-start gap-3">
              {#each themeColorMeta as t}
                <div class="flex flex-col items-center gap-1">
                  <span class="text-[11px] text-[var(--muted-foreground)]">{t.label}</span>
                  <button
                    class="w-8 h-8 rounded-full border-2 transition-transform cursor-pointer {editThemeColor === t.key
                      ? 'border-[var(--foreground)] scale-110'
                      : 'border-transparent hover:scale-105'}"
                    style="background: {t.color};"
                    title={t.label}
                    onclick={() => selectThemeColor(t.key)}
                  ></button>
                </div>
              {/each}
              <!-- 自定义色：点击触发隐藏的 color input -->
              <div class="flex flex-col items-center gap-1">
                <span class="text-[11px] text-[var(--muted-foreground)]">自定义</span>
                <label
                  class="relative w-8 h-8 rounded-full border-2 transition-transform cursor-pointer flex items-center justify-center {isCustom(editThemeColor)
                    ? 'border-[var(--foreground)] scale-110'
                    : 'border-transparent hover:scale-105'}"
                  style={isCustom(editThemeColor) ? `background: ${themeColorHex(editThemeColor)};` : 'background: conic-gradient(from 0deg, #ff0000, #ffff00, #00ff00, #00ffff, #0000ff, #ff00ff, #ff0000);'}
                  title="自定义颜色"
                >
                  <input
                    type="color"
                    class="absolute inset-0 opacity-0 cursor-pointer"
                    value={isCustom(editThemeColor) ? themeColorHex(editThemeColor) : '#3F51C5'}
                    oninput={selectCustomColor}
                  />
                </label>
              </div>
            </div>
          {:else if activeSection === 'log'}
            <!-- 日志显示：字号 + 行高 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">日志字体</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-4">
              调节日志区文字大小与行间距，即时预览。
            </div>

            <!-- 字号 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">字号</span>
              <input
                type="range" min="10" max="22" step="1"
                class="flex-1 accent-[var(--primary)]"
                value={editFontSize}
                oninput={(e) => changeFontSize(Number((e.target as HTMLInputElement).value))}
              />
              <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editFontSize}px</span>
            </div>

            <!-- 行高 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">行高</span>
              <input
                type="range" min="1.0" max="3.0" step="0.1"
                class="flex-1 accent-[var(--primary)]"
                value={editLineHeight}
                oninput={(e) => changeLineHeight(Number((e.target as HTMLInputElement).value))}
              />
              <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editLineHeight.toFixed(1)}</span>
            </div>

            <!-- 方向标签 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">方向标签</span>
              <div class="flex gap-2">
                <button
                  class="px-3 py-1 rounded-md border text-[13px] transition-colors {editDirLabel === 'short'
                    ? 'border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]'
                    : 'border-[var(--border)] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] cursor-pointer'}"
                  onclick={() => changeDirLabel('short')}
                >Tx / Rx</button>
                <button
                  class="px-3 py-1 rounded-md border text-[13px] transition-colors {editDirLabel === 'full'
                    ? 'border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]'
                    : 'border-[var(--border)] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] cursor-pointer'}"
                  onclick={() => changeDirLabel('full')}
                >发送 / 接收</button>
              </div>
            </div>

            <!-- 预览 -->
            <div class="mt-4 p-3 rounded border" style="border-color: var(--border); background: var(--background-data); font-family: var(--font-mono); font-size: {editFontSize}px; line-height: {editLineHeight};">
              <div>{editDirLabel === 'full' ? '发送' : 'Tx'} 10:39:47.362 AT</div>
              <div>{editDirLabel === 'full' ? '接收' : 'Rx'} 10:39:47.484 OK</div>
              <div>{editDirLabel === 'full' ? '发送' : 'Tx'} 10:39:47.545 AT+CSQ</div>
            </div>
          {/if}
        </div>
      </div>

      <!-- 底部按钮 -->
      <div class="flex justify-end gap-2 px-4 pb-4 border-t border-[var(--border)] pt-3">
        <button class="btn btn-ghost" style="padding: 6px 14px;" onclick={handleCancel}>取消</button>
        <button class="btn btn-primary" style="padding: 6px 14px;" onclick={handleSave}>保存</button>
      </div>
    </div>
  </div>
{/if}
