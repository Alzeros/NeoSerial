<script lang="ts">
  import { getVersion } from '@tauri-apps/api/app';
  import { X } from 'lucide-svelte';
  import { presetBaudRates, cachedSettings, theme, themeMeta, applyTheme, logFontSize, logLineHeight, applyLogFont, logDirLabelStyle, textEncoding, logFontLatin, logFontLatinPresets, logFontCJK, logFontCJKPresets } from '$lib/stores';
  import { saveSettings, getMcpStatus } from '$lib/tauri';
  import type { Settings } from '$lib/types';
  // 应用图标：从 src/assets 引入，Vite 自动处理打包（src-tauri/icons 在 watch ignored 中，无法直接 import）
  import appIcon from '$assets/icon.png';

  let open = $state(false);

  // 左侧导航：当前激活的设置项
  type Section = 'about' | 'general' | 'appearance' | 'mcp';
  let activeSection = $state<Section>('about');
  const sections: { key: Section; label: string }[] = [
    { key: 'about', label: '关于' },
    { key: 'general', label: '通用' },
    { key: 'appearance', label: '外观' },
    { key: 'mcp', label: 'MCP 服务' },
  ];
  // 应用版本号（打开关于页时懒加载）
  let version = $state<{ value: string }>({ value: '' });

  // 主题编辑副本（打开时从 store 拷贝，取消不应用；选中即时预览）
  let editTheme = $state<string>('preset-1');
  // 日志字体编辑副本
  let editFontSize = $state(14);
  let editLineHeight = $state(1.6);
  let editDirLabel = $state<'short' | 'full'>('short');
  // 字体族编辑副本：'default' 或 CSS font-family 值（英文/中文分别设置）
  let editFontLatin = $state<string>('default');
  let editFontCJK = $state<string>('default');
  // 文本模式编码编辑副本（ASCII/UTF-8/GBK）
  let editTextEncoding = $state<'ascii' | 'utf8' | 'gbk'>('ascii');
  // 打开前的原值（取消时恢复，因部分预览直接改了 store）
  let origDirLabel: 'short' | 'full' = 'short';
  let origTextEncoding: 'ascii' | 'utf8' | 'gbk' = 'ascii';
  let origFontLatin: string = 'default';
  let origFontCJK: string = 'default';

  // 内置默认波特率：不可删除，只能在此基础上增删用户自定义项
  const DEFAULT_BAUD_RATES = [9600, 115200, 921600];
  const isDefault = (n: number) => DEFAULT_BAUD_RATES.includes(n);

  // 预设波特率编辑副本（打开时从 store 拷贝，取消不污染 store）
  let editBaudRates = $state<number[]>([]);
  let newBaud = $state('');
  // MCP 自动启动编辑副本（从 cachedSettings 拷贝；改后重启生效）
  let editMcpAutoStart = $state(true);
  // MCP 端口编辑副本（默认 34594;被占自动递增。改后需重新 claude mcp add）
  let editMcpPort = $state(34594);
  // MCP server 当前运行状态（打开设置页/切到 MCP 页时拉取,显示实际端口）
  let mcpStatus = $state<{ running: boolean; port: number | null }>({ running: false, port: null });
  let mcpCopied = $state(false);

  export function show(section: Section = 'about') {
    editBaudRates = [...presetBaudRates.value];
    editTheme = theme.value;
    editFontSize = logFontSize.value;
    editLineHeight = logLineHeight.value;
    editDirLabel = logDirLabelStyle.value;
    origDirLabel = logDirLabelStyle.value;
    editFontLatin = logFontLatin.value;
    editFontCJK = logFontCJK.value;
    origFontLatin = logFontLatin.value;
    origFontCJK = logFontCJK.value;
    editTextEncoding = textEncoding.value;
    origTextEncoding = textEncoding.value;
    newBaud = '';
    editMcpAutoStart = cachedSettings.value?.mcp?.auto_start ?? true;
    editMcpPort = cachedSettings.value?.mcp?.port ?? 34594;
    mcpCopied = false;
    activeSection = section;
    // 拉取 MCP 运行状态(显示实际端口)
    getMcpStatus().then((s) => (mcpStatus = s)).catch(() => {});
    open = true;
    // 进入关于页时懒加载版本号（仅首次拉取，失败兜底）
    if (section === 'about' && !version.value) {
      getVersion()
        .then((v) => (version.value = v))
        .catch(() => (version.value = '0.1.3'));
    }
  }

  // 选中主题：即时预览（应用到 <html>），但不落盘；取消则恢复原值
  function selectTheme(value: string) {
    editTheme = value;
    applyTheme(value);
  }

  // 字号/行高：即时预览（带上当前英文/中文字体）
  function changeFontSize(v: number) {
    editFontSize = v;
    applyLogFont(v, editLineHeight, editFontLatin, editFontCJK);
  }
  function changeLineHeight(v: number) {
    editLineHeight = v;
    applyLogFont(editFontSize, v, editFontLatin, editFontCJK);
  }
  // 方向标签样式：即时预览
  function changeDirLabel(v: 'short' | 'full') {
    editDirLabel = v;
    logDirLabelStyle.value = v;
  }
  // 英文字体：选预设即时预览
  function selectFontLatin(value: string) {
    editFontLatin = value;
    applyLogFont(editFontSize, editLineHeight, value, editFontCJK);
  }
  // 中文字体：选预设即时预览
  function selectFontCJK(value: string) {
    editFontCJK = value;
    applyLogFont(editFontSize, editLineHeight, editFontLatin, value);
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
    // 主题：预览值落盘到 store（<html> 已在选中时应用）
    theme.value = editTheme;
    // 日志字体：预览值落盘到 store
    logFontSize.value = editFontSize;
    logLineHeight.value = editLineHeight;
    logDirLabelStyle.value = editDirLabel;
    logFontLatin.value = editFontLatin;
    logFontCJK.value = editFontCJK;
    textEncoding.value = editTextEncoding;
    // 落盘：基于缓存 settings 透传，仅更新 presets 与 ui 字体
    const base = cachedSettings.value;
    if (base) {
      const next: Settings = {
        ...base,
        ui: { ...base.ui, log_font_size: editFontSize, log_line_height: editLineHeight, log_dir_label: editDirLabel, log_font_latin: editFontLatin, log_font_cjk: editFontCJK, text_encoding: editTextEncoding === 'utf8' ? 'Utf8' : editTextEncoding === 'gbk' ? 'Gbk' : 'Ascii' },
        presets: { baud_rates: rates, theme: editTheme },
        mcp: { auto_start: editMcpAutoStart, port: editMcpPort },
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

  // 一键复制 MCP 连接指令到剪贴板
  async function copyMcpCommand() {
    if (!mcpStatus.port) return;
    const cmd = `claude mcp add --transport http neoserial http://localhost:${mcpStatus.port}/mcp`;
    try {
      await navigator.clipboard.writeText(cmd);
      mcpCopied = true;
      setTimeout(() => (mcpCopied = false), 1500);
    } catch {
      // 剪贴板不可用时静默
    }
  }

  function handleCancel() {
    // 取消：恢复打开前的主题与字体（撤销预览）
    applyTheme(theme.value);
    applyLogFont(logFontSize.value, logLineHeight.value, logFontLatin.value, logFontCJK.value);
    logDirLabelStyle.value = origDirLabel;
    logFontLatin.value = origFontLatin;
    logFontCJK.value = origFontCJK;
    textEncoding.value = origTextEncoding;
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
      <!-- 标题 + 关闭按钮 -->
      <div class="flex items-center justify-between px-6 py-4 border-b border-[var(--border)]">
        <div class="text-[15px] font-semibold text-[var(--foreground)]">设置</div>
        <button
          class="flex items-center justify-center w-7 h-7 -mr-2 rounded text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
          onclick={handleCancel}
          title="关闭 (Esc)"
        ><X size={16} /></button>
      </div>

      <!-- 左右分栏：左导航 + 右内容（固定高度，内容多时右栏独立滚动） -->
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
          {#if activeSection === 'about'}
            <!-- 关于：图标 + 应用名 + 版本 -->
            <div class="flex flex-col items-center justify-center text-center" style="min-height: 280px;">
              <img src={appIcon} alt="NeoSerial" class="w-16 h-16 mb-3 rounded-lg shadow-sm" />
              <div class="text-[15px] font-semibold text-[var(--foreground)] mb-1">NeoSerial</div>
              <div class="text-[13px] text-[var(--muted-foreground)] mb-4">串口通信调试工具</div>
              <div class="text-[13px] text-[var(--muted-foreground)]">
                版本 <span class="text-[var(--foreground)] font-medium">{version.value || '0.1.3'}</span>
              </div>
            </div>
          {:else if activeSection === 'general'}
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

            <!-- 分隔：日志显示 -->
            <div class="my-5 border-t border-[var(--border)]"></div>

            <!-- 文本编码 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">文本编码</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-3">
              HEX显示关闭时的文本模式解码方式。
            </div>
            <div class="flex items-center gap-3 mb-5">
              <span class="w-16 text-[13px] text-[var(--foreground)]">编码</span>
              <div class="flex gap-2">
                {#each [
                  { v: 'ascii', l: 'ASCII' },
                  { v: 'utf8', l: 'UTF-8' },
                  { v: 'gbk', l: 'GBK' },
                ] as enc}
                  <button
                    class="px-3 py-1 rounded-md border text-[13px] transition-colors {editTextEncoding === enc.v
                      ? 'border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]'
                      : 'border-[var(--border)] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] cursor-pointer'}"
                    onclick={() => (editTextEncoding = enc.v as 'ascii' | 'utf8' | 'gbk')}
                  >{enc.l}</button>
                {/each}
              </div>
            </div>

            <!-- 日志字体 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">日志字体</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-4">
              英文字体用于 ASCII/HEX 对齐（等宽），中文字体渲染中文内容，两者自动拼成回退栈，即时预览。
            </div>

            <!-- 英文字体 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">英文字体</span>
              <select
                class="flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
                value={editFontLatin}
                onchange={(e) => selectFontLatin((e.target as HTMLSelectElement).value)}
              >
                {#each logFontLatinPresets as p}
                  <option value={p.value}>{p.label}</option>
                {/each}
              </select>
            </div>

            <!-- 中文字体 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">中文字体</span>
              <select
                class="flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
                value={editFontCJK}
                onchange={(e) => selectFontCJK((e.target as HTMLSelectElement).value)}
              >
                {#each logFontCJKPresets as p}
                  <option value={p.value}>{p.label}</option>
                {/each}
              </select>
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
            <div class="mt-4 p-3 rounded border" style="border-color: var(--border); background: var(--background-data); font-family: var(--log-font-family); font-size: {editFontSize}px; line-height: {editLineHeight};">
              <div>{editDirLabel === 'full' ? '发送' : 'Tx'} 10:39:47.362 AT</div>
              <div>{editDirLabel === 'full' ? '接收' : 'Rx'} 10:39:47.484 OK</div>
              <div>{editDirLabel === 'full' ? '发送' : 'Tx'} 10:39:47.545 AT+CSQ</div>
              <div>{editDirLabel === 'full' ? '接收' : 'Rx'} 10:39:47.612 模块就绪</div>
            </div>
          {:else if activeSection === 'appearance'}
            <!-- 外观：主题预设 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">主题</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-3">
              选择应用的整体配色方案，点击即时预览。
            </div>
            <div class="grid grid-cols-2 gap-3">
              {#each themeMeta as t}
                <button
                  class="flex items-center gap-3 rounded-lg border-2 p-3 transition-all cursor-pointer {editTheme === t.key
                    ? 'border-[var(--primary)]'
                    : 'border-[var(--border)] hover:border-[var(--border-strong)]'}"
                  onclick={() => selectTheme(t.key)}
                >
                  <!-- 预览色块：背景色 + 强调色圆点 -->
                  <div class="relative w-10 h-10 rounded-md flex-shrink-0" style="background: {t.bg};">
                    <div class="absolute bottom-1 right-1 w-3 h-3 rounded-full" style="background: {t.accent};"></div>
                  </div>
                  <div class="flex flex-col items-start">
                    <span class="text-[13px] font-medium text-[var(--foreground)]">{t.label}</span>
                    <span class="text-[11px] text-[var(--muted-foreground)]">{t.accent}</span>
                  </div>
                </button>
              {/each}
            </div>
          {:else if activeSection === 'mcp'}
            <!-- MCP 服务：自动启动开关 + 连接指令 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">MCP 服务</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-4">
              内嵌 MCP server 让 Claude Code 经由 NeoSerial 操作串口（不占端口）。
            </div>
            <label class="flex items-center gap-3 cursor-pointer select-none">
              <input
                type="checkbox"
                class="h-4 w-4 rounded accent-[var(--primary)]"
                bind:checked={editMcpAutoStart}
              />
              <span class="text-[13px] text-[var(--foreground)]">打开软件时自动启动 MCP server</span>
            </label>
            <div class="text-[12px] text-[var(--muted-foreground)] mt-1 ml-7 mb-5">
              关闭后启动时不占用端口（需重启软件生效）。
            </div>

            <!-- 监听端口 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">监听端口</div>
            <div class="flex items-center gap-2 mb-1">
              <input
                type="number"
                class="w-24 px-2 py-1 text-[13px] rounded border border-[var(--border)] bg-[var(--background-elevated)] text-[var(--foreground)]"
                bind:value={editMcpPort}
                min="1024"
                max="65535"
              />
              <span class="text-[12px] text-[var(--muted-foreground)]">默认 34594,被占自动递增</span>
            </div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-5">
              <code class="px-1 rounded bg-[var(--border-subtle)]">claude mcp add</code> 配实际端口一次,后续会话自动连接。端口被占时自动 +1 找下一个(变了要重配,但默认端口没规律极少冲突)。
            </div>

            <!-- 当前运行状态 + 连接指令 -->
            {#if mcpStatus.running && mcpStatus.port}
              <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">连接 Claude Code</div>
              <div class="text-[12px] text-[var(--muted-foreground)] mb-2">
                MCP server 运行中，端口 <span class="text-[var(--foreground)] font-medium">{mcpStatus.port}</span>。在终端执行一次即可,后续会话自动连接：
              </div>
              <div class="flex items-center gap-2">
                <code class="flex-1 text-[12px] px-2.5 py-1.5 rounded bg-[var(--border-subtle)] text-[var(--foreground)] overflow-x-auto whitespace-nowrap">
                  claude mcp add --transport http neoserial http://localhost:{mcpStatus.port}/mcp
                </code>
                <button
                  class="shrink-0 px-2.5 py-1.5 rounded text-[12px] font-medium transition-colors {mcpCopied
                    ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                    : 'bg-[var(--border-subtle)] text-[var(--foreground)] hover:bg-[var(--border)]'}"
                  onclick={copyMcpCommand}
                  title="复制到剪贴板"
                >{mcpCopied ? '已复制' : '复制'}</button>
              </div>
            {:else}
              <div class="text-[12px] text-[var(--muted-foreground)] mt-4 px-3 py-2 rounded bg-[var(--border-subtle)]">
                MCP server 未运行{!editMcpAutoStart ? '（已关闭自动启动）' : `（端口 ${editMcpPort} 可能被占用,改端口或释放占用后重启）`}。
              </div>
            {/if}
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
