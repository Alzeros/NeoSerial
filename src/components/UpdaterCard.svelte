<script lang="ts">
  import { check } from '@tauri-apps/plugin-updater';
  import { relaunch } from '@tauri-apps/plugin-process';
  import { RefreshCw, Download, Rocket } from 'lucide-svelte';

  // 版本号由父组件(SettingsDialog 已懒加载)传入,避免卡片重复请求。
  let { version = '' }: { version?: string } = $props();

  // 状态机:idle → checking → available(→ downloading → installing);失败回落 error。
  //
  // idle 与"检查为最新"合并:up-to-date 不做成独立状态机节点,而是 idle 上的
  // latest 子标记——检查完成且为最新时置 true,显示"已是最新版本"绿点反馈,
  // 3s 后回落 false(无状态行)。idle 与 latest 共用同一布局(版本号 + 检查更新
  // 按钮),只是状态行有则显示无则塌陷,减少状态复杂度。
  type State =
    | { kind: 'idle'; latest: boolean }
    | { kind: 'checking' }
    | { kind: 'available'; version: string }
    | { kind: 'downloading'; contentLength: number; downloaded: number }
    | { kind: 'installing' }
    | { kind: 'error' };

  let state = $state<State>({ kind: 'idle', latest: false });

  // latest 反馈态的回落定时器(检查为最新后 3s 回到无状态行 idle)。
  let flashTimer: ReturnType<typeof setTimeout> | undefined;

  // 置 latest 反馈态,3s 后回落。多处复用(检查为最新 / 下载时发现已无更新)。
  function flashLatest() {
    if (flashTimer) clearTimeout(flashTimer);
    state = { kind: 'idle', latest: true };
    flashTimer = setTimeout(() => {
      if (state.kind === 'idle' && state.latest) {
        state = { kind: 'idle', latest: false };
      }
    }, 3000);
  }

  async function checkUpdate() {
    // checking 态忽略重复触发(不 disabled——保持可点 + 文字反馈,弱网下用户多点
    // 不会因 disabled 无反馈而焦躁;重复请求被忽略即可,实现成本最低)。
    if (state.kind === 'checking') return;
    state = { kind: 'checking' };
    try {
      const update = await check();
      if (update?.available) {
        state = { kind: 'available', version: update.version };
      } else {
        flashLatest();
      }
    } catch {
      // 统一文案,不区分网络/其他错误类型(投入产出比不划算,后续按反馈再细分)
      state = { kind: 'error' };
    }
  }

  async function downloadAndInstall() {
    if (state.kind !== 'available') return;
    try {
      // 重新 check 拿 update 对象(check 内部有缓存,重复调用无副作用)
      const update = await check();
      if (!update?.available) {
        flashLatest();
        return;
      }
      state = { kind: 'downloading', contentLength: 0, downloaded: 0 };
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started':
            state = { kind: 'downloading', contentLength: event.data.contentLength ?? 0, downloaded: 0 };
            break;
          case 'Progress':
            if (state.kind === 'downloading') {
              state = { kind: 'downloading', contentLength: state.contentLength, downloaded: state.downloaded + event.data.chunkLength };
            }
            break;
          case 'Finished':
            break;
        }
      });
      state = { kind: 'installing' };
      await relaunch();
    } catch {
      state = { kind: 'error' };
    }
  }

  // 不自动检查——手动点按钮触发(避免每次开设置都请求 github,国内访问不稳定)

  // 进度百分比(contentLength=0 时显示不确定态)
  const percent = $derived(
    state.kind === 'downloading' && state.contentLength > 0
      ? Math.min(100, Math.round((state.downloaded / state.contentLength) * 100))
      : 0
  );
</script>

<!-- 一张卡:外壳不变,内部内容随状态切换(不做成多个不同布局)。
     浅灰背景(非纯白卡片 + 边框);有更新时 2px accent 边框强调,更显眼。
     border 恒为 2px(available 时 accent 色,其余 transparent),避免状态切换时尺寸跳动。 -->
<div
  class="rounded-lg p-4 mt-4 w-full max-w-[280px] mx-auto transition-colors"
  style="background: var(--border-subtle); border: 2px solid {state.kind === 'available' ? 'var(--primary)' : 'transparent'};"
>
  <!-- 顶行:左标题 + 右按钮(标题加粗 13px;available 时标题用 accent 色) -->
  <div class="flex items-center justify-between gap-3">
    {#if state.kind === 'available'}
      <span class="text-[13px] font-semibold" style="color: var(--primary);">发现新版本 v{state.version}</span>
    {:else}
      <span class="text-[13px] font-semibold" style="color: var(--foreground);">
        当前版本 {version || '0.2.6'}
      </span>
    {/if}

    <!-- 右侧按钮:按状态切换文案/样式。downloading/installing 不显示按钮
         (提示移到下方进度/安装行),其余态:
           - idle(含 latest 反馈)/ error:检查更新 / 重试,次要样式(非强调色)
           - checking:文字变"检查中…"保持可点(忽略重复触发)
           - available:立即更新,accent 强调色,与次要按钮形成视觉区分 -->
    {#if state.kind === 'idle'}
      <button
        class="flex items-center gap-1.5 text-[12px] px-2.5 py-1 rounded-md transition-colors hover:opacity-80 shrink-0"
        style="color: var(--primary);"
        onclick={checkUpdate}
      >
        <RefreshCw size={13} />
        检查更新
      </button>
    {:else if state.kind === 'checking'}
      <button
        class="flex items-center gap-1.5 text-[12px] px-2.5 py-1 rounded-md shrink-0"
        style="color: var(--muted-foreground);"
        onclick={checkUpdate}
      >
        <RefreshCw size={13} class="animate-spin" />
        检查中…
      </button>
    {:else if state.kind === 'available'}
      <button
        class="flex items-center justify-center gap-1.5 text-[12px] font-medium px-3 py-1.5 rounded-md transition-colors hover:opacity-90 shrink-0"
        style="background: var(--primary); color: var(--primary-foreground);"
        onclick={downloadAndInstall}
      >
        <Download size={13} />
        立即更新
      </button>
    {:else if state.kind === 'error'}
      <button
        class="flex items-center gap-1.5 text-[12px] px-2.5 py-1 rounded-md transition-colors hover:opacity-80 shrink-0"
        style="color: var(--primary);"
        onclick={checkUpdate}
      >
        <RefreshCw size={13} />
        重试
      </button>
    {/if}
  </div>

  <!-- 状态行:仅 checking / idle.latest / error 显示(圆点 + 文字,11px 次要文字色;
       available 时状态行用 accent 色承载在标题上,此处不再显示)。
       idle(未检查)/ available 不显示该行,高度塌陷不留占位——idle 态无操作,
       强占空白无信息量。有内容时卡片自然增高,属预期。 -->
  {#if state.kind === 'checking'}
    <div class="flex items-center gap-1.5 mt-1.5 text-[11px]" style="color: var(--muted-foreground);">
      <span class="w-2 h-2 rounded-full inline-block animate-pulse shrink-0" style="background: #9ca3af;"></span>
      正在检查更新…
    </div>
  {:else if state.kind === 'idle' && state.latest}
    <div class="flex items-center gap-1.5 mt-1.5 text-[11px]" style="color: var(--muted-foreground);">
      <span class="w-2 h-2 rounded-full inline-block shrink-0" style="background: #16a34a;"></span>
      已是最新版本
    </div>
  {:else if state.kind === 'error'}
    <div class="flex items-center gap-1.5 mt-1.5 text-[11px]" style="color: var(--muted-foreground);">
      <span class="w-2 h-2 rounded-full inline-block shrink-0" style="background: #dc2626;"></span>
      更新失败,请重试
    </div>
  {/if}

  <!-- 下载/安装子状态(available 点击"立即更新"后):顶行标题"发现新版本"保留,
       右侧按钮不再渲染,下方显示进度条 / 安装提示。复用同一张卡框架。 -->
  {#if state.kind === 'downloading'}
    <div class="flex flex-col gap-1.5 mt-3">
      <div class="flex items-center justify-between text-[11px]" style="color: var(--muted-foreground);">
        <span>下载中…</span>
        <span>{state.contentLength > 0 ? `${percent}%` : '下载中…'}</span>
      </div>
      <div class="h-1.5 rounded-full overflow-hidden" style="background: var(--border);">
        {#if state.contentLength > 0}
          <div class="h-full rounded-full transition-all duration-150" style="width: {percent}%; background: var(--primary);"></div>
        {:else}
          <div class="h-full w-1/3 rounded-full animate-pulse" style="background: var(--primary);"></div>
        {/if}
      </div>
    </div>
  {:else if state.kind === 'installing'}
    <div class="flex items-center gap-1.5 mt-3 text-[11px]" style="color: var(--primary);">
      <Rocket size={12} class="animate-pulse" />
      安装中,即将重启…
    </div>
  {/if}
</div>
