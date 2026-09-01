<script lang="ts">
  import { ChevronUp, ChevronDown, X, WholeWord } from 'lucide-svelte';
  import { displayMode, textEncoding, logLines, logVersion, logSendContent, logDirLabelStyle, showTimestamp, showLineIndex, scrollContainerRef, clearLogLines, paused } from '$lib/stores';
  import type { LogLine } from '$lib/types';

  let scrollContainer: HTMLDivElement;
  // 同步给 App.svelte 用的容器引用（兜底 + 兼容旧调用）
  $effect(() => {
    scrollContainerRef.el = scrollContainer;
    return () => {
      if (scrollContainerRef.el === scrollContainer) {
        scrollContainerRef.el = null;
      }
    };
  });

  /**
   * 自动滚动：日志有更新就跳到最新一条（无条件跟随）
   * 搜索打开时暂停自动滚动，避免与搜索定位冲突。
   */
  $effect(() => {
    logVersion.value;
    if (!scrollContainer) return;
    if (searchOpen) return;
    scrollContainer.scrollTop = scrollContainer.scrollHeight;
    requestAnimationFrame(() => {
      if (scrollContainer) {
        scrollContainer.scrollTop = scrollContainer.scrollHeight;
      }
    });
  });

  // ============ 搜索 ============
  let searchOpen = $state(false);
  let searchQuery = $state('');
  let searchCaseSensitive = $state(false);
  let searchWholeWord = $state(false);
  let searchInput: HTMLInputElement;
  /** 匹配行的索引列表（logLines 中的下标） */
  let matchIndices = $state<number[]>([]);
  /** 当前定位的匹配在 matchIndices 中的下标 */
  let currentMatch = $state(0);
  // 上一轮的查询条件，用于区分"查询变了"和"只是 logLines 变了"
  let lastQuery = '';
  let lastCs = false;
  let lastWw = false;
  let lastHex = false;
  let lastEnc = 'ascii';

  const matchSet = $derived(new Set(matchIndices));

  /**
   * 共享搜索正则：行匹配(testRe, 无 g)与高亮(globalRe, 有 g)共用同一 pattern，
   * 避免两套逻辑各算各的导致"匹配命中行但高亮没描边"。全字匹配用 \b 包裹。
   * 查询词首尾本身为非词字符时 \b 不成立，会命中不了——这是 VSCode 同款行为，
   * 遇此情况用户应关闭全字匹配。
   */
  const searchMatcher = $derived.by(() => {
    if (!searchQuery) return null;
    const escaped = searchQuery.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const pattern = searchWholeWord ? `\\b${escaped}\\b` : escaped;
    const flags = searchCaseSensitive ? '' : 'i';
    try {
      return {
        testRe: new RegExp(pattern, flags),
        globalRe: new RegExp(pattern, flags + 'g')
      };
    } catch {
      return null;
    }
  });

  function openSearch() {
    // 已开着再按 Ctrl/F3:只 focus 回搜索框,不动当前查询(避免覆盖正在编辑的词)。
    if (searchOpen) {
      searchInput?.focus();
      searchInput?.select();
      return;
    }
    // 首次打开:Ctrl+F 前若在日志区选中了文本,带进搜索框(浏览器/VSCode 惯例)。
    // window.getSelection() 拿 DOM 选区;input 内选区不归它管,故只处理"日志区选中文本"。
    // 多行/超长选区(如选中整片日志)不塞进搜索框——限长 200 字符且无换行才带。
    const sel = window.getSelection();
    const picked = sel ? sel.toString().trim() : '';
    if (picked && picked.length <= 200 && !picked.includes('\n')) {
      searchQuery = picked;
    }
    searchOpen = true;
    requestAnimationFrame(() => {
      searchInput?.focus();
      // 全选便于直接覆盖输入;空查询时光标定位到末尾即可
      searchInput?.select();
    });
  }

  function closeSearch() {
    searchOpen = false;
    searchQuery = '';
    matchIndices = [];
    currentMatch = 0;
  }

  function handleGlobalKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
      e.preventDefault();
      openSearch();
    }
    if (e.key === 'Escape') {
      if (ctxMenu.show) { ctxMenu.show = false; return; }
      if (searchOpen) { closeSearch(); return; }
    }
    if (searchOpen && e.key === 'F3') {
      e.preventDefault();
      if (e.shiftKey) prevMatch(); else nextMatch();
    }
  }

  // ============ 右键菜单 ============
  let ctxMenu = $state<{ x: number; y: number; show: boolean; hasSelection: boolean }>({
    x: 0, y: 0, show: false, hasSelection: false,
  });

  function handleContextMenu(e: MouseEvent) {
    // 输入框/文本域保留浏览器默认菜单（复制/粘贴）
    const target = e.target as HTMLElement;
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;
    e.preventDefault();
    e.stopPropagation();
    const sel = window.getSelection();
    ctxMenu.hasSelection = !!(sel && sel.toString().length > 0);
    // 边界检测：靠近底部翻到上方，靠近右边左移
    const MW = 110, MH = 140;
    const vw = window.innerWidth, vh = window.innerHeight;
    ctxMenu.x = e.clientX + MW > vw ? Math.max(4, vw - MW - 4) : e.clientX;
    ctxMenu.y = e.clientY + MH > vh ? Math.max(4, e.clientY - MH) : e.clientY;
    ctxMenu.show = true;
  }

  function closeCtxMenu() {
    ctxMenu.show = false;
  }

  async function ctxCopy() {
    const sel = window.getSelection();
    if (sel) {
      try { await navigator.clipboard.writeText(sel.toString()); } catch { /* 剪贴板不可用 */ }
    }
    closeCtxMenu();
  }

  function ctxClear() {
    clearLogLines();
    closeCtxMenu();
  }

  function ctxTogglePause() {
    paused.value = !paused.value;
    closeCtxMenu();
  }

  function ctxSearch() {
    openSearch();
    closeCtxMenu();
  }

  $effect(() => {
    if (!ctxMenu.show) return;
    const close = () => { ctxMenu.show = false; };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  });

  // Ctrl+A 限定在日志区范围:容器是普通 div,不可聚焦,浏览器 Ctrl+A 默认全选
  // 整个文档(顶栏+日志+底栏全选,即"选中整个软件框")。给容器加 tabindex=-1 使
  // 其可鼠标点击聚焦,再拦截 Ctrl+A 用 Range 手动选区只覆盖日志区内容——符合
  // "按模块框/输入框划定范围"的预期。用 -1 不进 Tab 序列,不干扰输入框键盘导航;
  // focus:outline-none 去掉点击聚焦后的默认焦点环(选中高亮已足够提示)。
  // 焦点在输入框时此 handler 不触发,输入框 Ctrl+A 仍是浏览器默认选自身。
  function handleLogKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && (e.key === 'a' || e.key === 'A')) {
      e.preventDefault();
      const sel = window.getSelection();
      if (!sel || !scrollContainer) return;
      const range = document.createRange();
      range.selectNodeContents(scrollContainer);
      sel.removeAllRanges();
      sel.addRange(range);
    }
  }

  function handleSearchInputKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (e.shiftKey) prevMatch(); else nextMatch();
    }
  }

  function onSearchInput() {
    currentMatch = 0;
    requestAnimationFrame(() => scrollToMatch());
  }

  function nextMatch() {
    if (matchIndices.length === 0) return;
    currentMatch = (currentMatch + 1) % matchIndices.length;
    scrollToMatch();
  }

  function prevMatch() {
    if (matchIndices.length === 0) return;
    currentMatch = (currentMatch - 1 + matchIndices.length) % matchIndices.length;
    scrollToMatch();
  }

  function scrollToMatch() {
    const lineIdx = matchIndices[currentMatch];
    if (lineIdx === undefined) return;
    const el = scrollContainer?.querySelector(`[data-idx="${lineIdx}"]`);
    el?.scrollIntoView({ block: 'center' });
  }

  /** 重新计算匹配行。查询/大小写/全字/显示模式/编码变化时重置到第一个匹配；
   *  仅 logLines 变化时保留当前位置（夹紧到有效范围）。 */
  $effect(() => {
    const q = searchQuery;
    const cs = searchCaseSensitive;
    const ww = searchWholeWord;
    const lines = logLines;
    const hex = displayMode.value === 'hex';
    // 编码变化也会改变 renderLine 输出，纳入依赖触发重算
    const enc = textEncoding.value;
    const matcher = searchMatcher;

    if (!q || !matcher) {
      matchIndices = [];
      currentMatch = 0;
      lastQuery = '';
      lastCs = cs;
      lastWw = ww;
      lastHex = hex;
      lastEnc = enc;
      return;
    }

    const queryChanged = q !== lastQuery || cs !== lastCs || ww !== lastWw || hex !== lastHex || enc !== lastEnc;
    lastQuery = q;
    lastCs = cs;
    lastWw = ww;
    lastHex = hex;
    lastEnc = enc;

    const { testRe } = matcher;
    const indices: number[] = [];
    for (let i = 0; i < lines.length; i++) {
      const text = renderLine(lines[i]);
      if (testRe.test(text)) {
        indices.push(i);
      }
    }
    matchIndices = indices;

    if (queryChanged) {
      currentMatch = indices.length > 0 ? 0 : -1;
      if (indices.length > 0) {
        requestAnimationFrame(() => scrollToMatch());
      }
    } else if (currentMatch >= indices.length) {
      currentMatch = Math.max(0, indices.length - 1);
    }
  });

  /** 把文本按搜索词切分为片段，标记匹配段用于高亮渲染。
   *  使用共享正则 searchMatcher.globalRe，与行匹配同一 pattern，保证高亮范围与命中行一致。 */
  function highlightSegments(text: string, regex: RegExp): { text: string; match: boolean }[] {
    regex.lastIndex = 0;
    const segments: { text: string; match: boolean }[] = [];
    let lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = regex.exec(text)) !== null) {
      if (m.index > lastIndex) {
        segments.push({ text: text.slice(lastIndex, m.index), match: false });
      }
      segments.push({ text: m[0], match: true });
      lastIndex = m.index + m[0].length;
      if (m.index === regex.lastIndex) regex.lastIndex++;
    }
    if (lastIndex < text.length) {
      segments.push({ text: text.slice(lastIndex), match: false });
    }
    if (segments.length === 0) {
      return [{ text, match: false }];
    }
    return segments;
  }

  function isCurrentMatch(i: number): boolean {
    return matchIndices[currentMatch] === i;
  }

  // ============ 渲染 ============

  /** 解码器缓存：按编码 label 复用 TextDecoder，避免每条日志都新建（开销小但没必要） */
  const decoderCache = new Map<string, TextDecoder>();
  function getDecoder(label: string): TextDecoder {
    let d = decoderCache.get(label);
    if (!d) {
      d = new TextDecoder(label, { fatal: false });
      decoderCache.set(label, d);
    }
    return d;
  }

  /**
   * 解码原始字节为文本显示。根据 textEncoding 选择编码：
   * - 'ascii'：只保留可打印 ASCII（0x20-0x7E），过滤控制字符——即 LogLine.ascii
   * - 'utf8'：UTF-8 解码，非法字节用 � 替换，过滤控制字符
   * - 'gbk'：GBK 解码（TextDecoder label 'gbk'，WebView2/Chromium 支持）
   */
  function renderLine(line: LogLine): string {
    if (displayMode.value === 'hex') {
      return renderHex(line);
    }
    if (textEncoding.value === 'ascii') {
      return line.ascii;
    }
    const enc = textEncoding.value === 'gbk' ? 'gbk' : 'utf-8';
    const s = getDecoder(enc).decode(new Uint8Array(line.raw));
    // 过滤控制字符（C0/C1 + DEL），避免 CR/LF/TAB 扰乱日志行排版。
    // 用 charCodeAt 判断：0x20 空格、0x7E 以内可打印，0x7F DEL 及 C1 区(0x80-0x9F)过滤，
    // 中文等可打印 Unicode（码点 >0x9F）保留。
    return Array.from(s)
      .filter((c) => {
        const code = c.codePointAt(0)!;
        return code >= 0x20 && code !== 0x7f && !(code >= 0x80 && code <= 0x9f);
      })
      .join('');
  }

  /**
   * HEX 显示：hex dump + ASCII 解析列，让用户直观看到字节对应的字符。
   * 格式: "41 54 0D 0A  AT.."
   */
  function renderHex(line: LogLine): string {
    if (line.raw.length === 0) return '';
    let out = '';
    for (let i = 0; i < line.raw.length; i += 16) {
      const chunk = line.raw.slice(i, i + 16);
      let hexPart = '';
      for (let j = 0; j < 16; j++) {
        if (j < chunk.length) {
          hexPart += chunk[j].toString(16).toUpperCase().padStart(2, '0') + ' ';
        } else {
          hexPart += '   ';
        }
        if (j === 7) hexPart += ' '; // 两组 8 字节间额外空格
      }
      const ascii = chunk
        .map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.'))
        .join('');
      out += `${hexPart} ${ascii}\n`;
    }
    return out.trimEnd();
  }

  function dirColor(dir: string): string {
    switch (dir) {
      case 'rx':
        return 'text-[var(--rx)]';
      case 'tx':
        return 'text-[var(--tx)]';
      default:
        return 'text-[var(--muted-foreground)]';
    }
  }

  function dirLabel(dir: string): string {
    const full = logDirLabelStyle.value === 'full';
    switch (dir) {
      case 'rx':
        return full ? '接收' : 'Rx';
      case 'tx':
        return full ? '发送' : 'Tx';
      default:
        return 'Sys';
    }
  }
</script>

<svelte:window on:keydown={handleGlobalKeydown} />

<!-- 数据显示区：最干净的纸白，视觉重心 -->
<div class="relative h-full overflow-hidden flex flex-col" style="background: var(--background-data);" oncontextmenu={handleContextMenu}>
  <!-- 搜索栏：右上角浮动，与浏览器 find bar 类似 -->
  {#if searchOpen}
    <div
      class="absolute top-0 right-0 z-20 flex items-center gap-1 px-2 py-1.5 border-b border-l rounded-bl-md shadow-md"
      style="background: var(--background-elevated); border-color: var(--border);"
    >
      <input
        bind:this={searchInput}
        bind:value={searchQuery}
        onkeydown={handleSearchInputKeydown}
        oninput={onSearchInput}
        placeholder="搜索日志..."
        style="width: 160px; height: 28px; font-size: 13px; padding: 0 8px; border-radius: var(--radius-sm);"
      />
      <button
        class="flex items-center justify-center w-7 h-7 rounded text-[13px] font-medium transition-colors {searchCaseSensitive
          ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
          : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)]'}"
        onclick={() => (searchCaseSensitive = !searchCaseSensitive)}
        title="区分大小写"
      >Aa</button>
      <button
        class="flex items-center justify-center w-7 h-7 rounded transition-colors {searchWholeWord
          ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
          : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)]'}"
        onclick={() => (searchWholeWord = !searchWholeWord)}
        title="全字匹配"
      ><WholeWord size={15} /></button>
      <button
        class="flex items-center justify-center w-7 h-7 rounded text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
        onclick={prevMatch}
        disabled={matchIndices.length === 0}
        title="上一个 (Shift+Enter)"
      ><ChevronUp size={15} /></button>
      <button
        class="flex items-center justify-center w-7 h-7 rounded text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
        onclick={nextMatch}
        disabled={matchIndices.length === 0}
        title="下一个 (Enter)"
      ><ChevronDown size={15} /></button>
      <span class="text-[12px] text-[var(--muted-foreground)] tabular-nums min-w-[48px] text-center">
        {matchIndices.length > 0 ? `${currentMatch + 1}/${matchIndices.length}` : '0/0'}
      </span>
      <button
        class="flex items-center justify-center w-7 h-7 rounded text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] transition-colors"
        onclick={closeSearch}
        title="关闭 (Esc)"
      ><X size={15} /></button>
    </div>
  {/if}

  <div
    bind:this={scrollContainer}
    tabindex="-1"
    role="log"
    aria-label="串口日志"
    onkeydown={handleLogKeydown}
    class="flex-1 overflow-y-auto overflow-x-auto px-3 py-4 outline-none"
    style="font-family: var(--log-font-family); font-size: var(--log-font-size); line-height: var(--log-line-height);"
  >
    {#each logLines as line, i (i)}
      <div
        data-idx={i}
        class="flex px-1 py-px {matchSet.has(i)
          ? (isCurrentMatch(i)
            ? 'bg-[rgba(196,138,46,0.18)]'
            : 'bg-[rgba(196,138,46,0.06)]')
          : 'hover:bg-[rgba(255,255,255,0.03)]'}"
      >
        <!-- 行号(本次连接期间 index,最左列,等宽数字右对齐) -->
        {#if showLineIndex.value}
          <div class="shrink-0 pr-2 mr-2 border-r border-[var(--border)] text-right tabular-nums text-[var(--muted-foreground)]" style="min-width: 3em;">
            {line.line_index || ''}
          </div>
        {/if}
        <!-- 方向 + 时间戳：包在一个框里，右边框与日志内容分隔 -->
        {#if logSendContent.value || showTimestamp.value}
          <div class="flex items-baseline gap-2 shrink-0 pr-2 mr-2 border-r border-[var(--border)]">
            {#if logSendContent.value}
              <span class="text-right font-bold {dirColor(line.dir)}">
                {dirLabel(line.dir)}
              </span>
            {/if}
            {#if showTimestamp.value}
              <span class="text-[var(--muted-foreground)] tabular-nums">{line.ts}</span>
            {/if}
          </div>
        {/if}
        <!-- 内容：搜索匹配时高亮关键词片段 -->
        <span class="break-all whitespace-pre-wrap {line.is_error ? 'text-[var(--error)]' : ''}">
          {#if searchQuery && matchSet.has(i) && searchMatcher}
            {#each highlightSegments(renderLine(line), searchMatcher.globalRe) as seg}
              {#if seg.match}
                <mark style="background: rgba(196,138,46,0.35); color: inherit; border-radius: 2px; padding: 0 1px;">{seg.text}</mark>
              {:else}
                {seg.text}
              {/if}
            {/each}
          {:else}
            {renderLine(line)}
          {/if}
        </span>
      </div>
    {/each}
  </div>

  {#if ctxMenu.show}
    <div
      class="fixed z-[200] py-1 rounded-md border shadow-lg min-w-[100px]"
      style="background: var(--background-elevated); border-color: var(--border); left: {ctxMenu.x}px; top: {ctxMenu.y}px;"
      onmousedown={(e) => e.stopPropagation()}
    >
      {#if ctxMenu.hasSelection}
        <button
          class="block w-full text-left px-3 py-1.5 text-[13px] hover:bg-[var(--border-subtle)] transition-colors cursor-pointer"
          onclick={ctxCopy}
        >复制</button>
      {/if}
      <button
        class="block w-full text-left px-3 py-1.5 text-[13px] hover:bg-[var(--border-subtle)] transition-colors cursor-pointer"
        onclick={ctxClear}
      >清空</button>
      <button
        class="block w-full text-left px-3 py-1.5 text-[13px] hover:bg-[var(--border-subtle)] transition-colors cursor-pointer"
        onclick={ctxTogglePause}
      >{paused.value ? '继续' : '暂停'}</button>
      <button
        class="block w-full text-left px-3 py-1.5 text-[13px] hover:bg-[var(--border-subtle)] transition-colors cursor-pointer"
        onclick={ctxSearch}
      >搜索</button>
    </div>
  {/if}
</div>
