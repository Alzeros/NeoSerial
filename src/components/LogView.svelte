<script lang="ts">
  import { autoScroll, hexDisplay, logLines, logVersion, logSendContent, logDirLabelStyle, showTimestamp } from '$lib/stores';
  import type { LogLine } from '$lib/types';

  let scrollContainer: HTMLDivElement;

  $effect(() => {
    // 依赖 logVersion（始终递增）而非 logLines.length（缓冲区满后不变）
    logVersion.value;
    if (autoScroll.value && scrollContainer) {
      requestAnimationFrame(() => {
        scrollContainer.scrollTop = scrollContainer.scrollHeight;
      });
    }
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
    class="flex-1 overflow-y-auto overflow-x-auto font-mono px-5 py-4"
    style="font-size: var(--log-font-size); line-height: var(--log-line-height);"
  >
    {#each logLines as line, i (i)}
      <div class="flex hover:bg-[rgba(255,255,255,0.03)] px-2 py-px gap-4">
        <!-- 方向标记：记录发送开关关闭时，隐去标签列，内容朝左对齐（Tx 此时本就不显示） -->
        {#if logSendContent.value}
          <span class="shrink-0 text-right font-bold {dirColor(line.dir)}">
            {dirLabel(line.dir)}
          </span>
        {/if}
        <!-- 时间戳 -->
        {#if showTimestamp.value}
          <span class="shrink-0 text-[var(--muted-foreground)] tabular-nums">{line.ts}</span>
        {/if}
        <!-- 内容 -->
        <span class="break-all whitespace-pre-wrap {line.is_error ? 'text-[var(--error)]' : ''}">
          {renderLine(line)}
        </span>
      </div>
    {/each}
  </div>
</div>
