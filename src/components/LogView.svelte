<script lang="ts">
  import { hexDisplay, logLines, logVersion, logSendContent, logDirLabelStyle, showTimestamp, scrollContainerRef } from '$lib/stores';
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
   * - $effect 依赖 logVersion（每次 appendLogLine 都 +1，必然触发）
   * - 直接设置 scrollTop：Svelte 5 中 $effect 在 DOM 提交后运行，scrollHeight 已是新值
   * - 再补一次 requestAnimationFrame 兜底，确保任何布局时序下都滚到底
   * - 不再受 autoScroll 开关限制：settings.json 曾持久化 auto_scroll=false 且界面无开关可重开，
   *   导致日志更新后视图停在顶部。按设计意图，有更新就无条件跳到最新。
   */
  $effect(() => {
    // 明确读取以建立依赖：logVersion 每次 appendLogLine 都 +1
    logVersion.value;
    if (!scrollContainer) return;

    // 立即滚到底
    scrollContainer.scrollTop = scrollContainer.scrollHeight;
    // 下一帧兜底一次，防止浏览器尚未完成布局导致读取到旧 scrollHeight
    requestAnimationFrame(() => {
      if (scrollContainer) {
        scrollContainer.scrollTop = scrollContainer.scrollHeight;
      }
    });
  });

  function renderLine(line: LogLine): string {
    if (hexDisplay.value) {
      return renderHex(line);
    }
    return line.ascii;
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

<!-- 数据显示区：最干净的纸白，视觉重心 -->
<div class="h-full overflow-hidden flex flex-col" style="background: var(--background-data);">
  <div
    bind:this={scrollContainer}
    class="flex-1 overflow-y-auto overflow-x-auto font-mono px-3 py-4"
    style="font-size: var(--log-font-size); line-height: var(--log-line-height);"
  >
    {#each logLines as line, i (i)}
      <div class="flex hover:bg-[rgba(255,255,255,0.03)] px-1 py-px">
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
        <!-- 内容 -->
        <span class="break-all whitespace-pre-wrap {line.is_error ? 'text-[var(--error)]' : ''}">
          {renderLine(line)}
        </span>
      </div>
    {/each}
  </div>
</div>
