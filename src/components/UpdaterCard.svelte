<script lang="ts">
  import { check } from '@tauri-apps/plugin-updater';
  import { relaunch } from '@tauri-apps/plugin-process';
  import { RefreshCw, Download, Rocket, CheckCircle2, AlertCircle } from 'lucide-svelte';

  // 状态机:idle → checking → available(version,notes)/up-to-date → downloading → downloaded → installing
  type State =
    | { kind: 'idle' }
    | { kind: 'checking' }
    | { kind: 'up-to-date' }
    | { kind: 'available'; version: string; notes: string }
    | { kind: 'downloading'; contentLength: number; downloaded: number }
    | { kind: 'downloaded' }
    | { kind: 'installing' }
    | { kind: 'error'; message: string };

  let state = $state<State>({ kind: 'idle' });

  async function checkUpdate(silent = false) {
    state = { kind: 'checking' };
    try {
      const update = await check();
      if (update?.available) {
        state = { kind: 'available', version: update.version, notes: update.body ?? '' };
      } else {
        state = { kind: 'up-to-date' };
        if (silent) {
          // 静默检查为最新时不打扰,恢复 idle(3s 后)
          setTimeout(() => { state = { kind: 'idle' }; }, 3000);
        }
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      state = { kind: 'error', message: msg };
      if (silent) {
        // 静默检查失败不打扰,恢复 idle
        setTimeout(() => { state = { kind: 'idle' }; }, 3000);
      }
    }
  }

  async function downloadAndInstall() {
    if (state.kind !== 'available') return;
    try {
      // 重新 check 拿 update 对象(check 内部有缓存,重复调用无副作用)
      const update = await check();
      if (!update?.available) {
        state = { kind: 'up-to-date' };
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
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      state = { kind: 'error', message: msg };
    }
  }

  // 不自动检查——手动点"检查"按钮触发(避免每次开设置都请求 github,国内访问不稳定)

  // 进度百分比(contentLength=0 时显示不确定态)
  const percent = $derived(
    state.kind === 'downloading' && state.contentLength > 0
      ? Math.min(100, Math.round((state.downloaded / state.contentLength) * 100))
      : 0
  );
</script>

<div class="rounded-lg border p-4 mt-4 w-full max-w-sm mx-auto" style="border-color: var(--border);">
  {#if state.kind === 'idle'}
    <div class="flex items-center justify-between">
      <span class="text-[12px]" style="color: var(--muted-foreground);">检查更新</span>
      <button
        class="flex items-center gap-1.5 text-[12px] px-2.5 py-1 rounded-md transition-colors hover:opacity-80"
        style="color: #2563eb;"
        onclick={() => checkUpdate(false)}
      >
        <RefreshCw size={13} />
        检查
      </button>
    </div>
  {:else if state.kind === 'checking'}
    <div class="flex items-center gap-2 text-[12px]" style="color: var(--muted-foreground);">
      <RefreshCw size={13} class="animate-spin" />
      检查中…
    </div>
  {:else if state.kind === 'up-to-date'}
    <div class="flex items-center gap-2 text-[12px]" style="color: var(--muted-foreground);">
      <CheckCircle2 size={13} style="color: #16a34a;" />
      已是最新版本
      <button
        class="ml-auto text-[12px] opacity-70 hover:opacity-100"
        style="color: var(--muted-foreground);"
        onclick={() => checkUpdate(false)}
      >再次检查</button>
    </div>
  {:else if state.kind === 'available'}
    <div class="flex flex-col gap-2">
      <div class="flex items-center gap-2 text-[12px]">
        <Download size={13} style="color: #2563eb;" />
        <span style="color: var(--foreground);">发现新版本 v{state.version}</span>
      </div>
      {#if state.notes}
        <div class="text-[11px] leading-relaxed rounded-md p-2 max-h-24 overflow-auto whitespace-pre-wrap"
             style="background: #f3f4f6; color: var(--muted-foreground);">
          {state.notes}
        </div>
      {/if}
      <button
        class="flex items-center justify-center gap-1.5 text-[12px] font-medium px-3 py-1.5 rounded-md transition-colors hover:opacity-90"
        style="background: #2563eb; color: #ffffff;"
        onclick={downloadAndInstall}
      >
        <Download size={13} />
        下载并安装
      </button>
    </div>
  {:else if state.kind === 'downloading'}
    <div class="flex flex-col gap-1.5">
      <div class="flex items-center justify-between text-[12px]">
        <span style="color: var(--foreground);">下载中…</span>
        <span style="color: var(--muted-foreground);">
          {state.contentLength > 0 ? `${percent}%` : '下载中…'}
        </span>
      </div>
      <div class="h-1.5 rounded-full overflow-hidden" style="background: #e5e7eb;">
        {#if state.contentLength > 0}
          <div class="h-full rounded-full transition-all duration-150" style="width: {percent}%; background: #2563eb;"></div>
        {:else}
          <div class="h-full w-1/3 rounded-full animate-pulse" style="background: #2563eb;"></div>
        {/if}
      </div>
    </div>
  {:else if state.kind === 'downloaded' || state.kind === 'installing'}
    <div class="flex items-center gap-2 text-[12px]" style="color: #2563eb;">
      <Rocket size={13} class="animate-pulse" />
      安装中,即将重启…
    </div>
  {:else if state.kind === 'error'}
    <div class="flex flex-col gap-2">
      <div class="flex items-start gap-2 text-[12px]" style="color: #dc2626;">
        <AlertCircle size={13} class="mt-0.5 shrink-0" />
        <span class="break-all">{state.message}</span>
      </div>
      <button
        class="self-start text-[12px] px-2.5 py-1 rounded-md transition-colors hover:opacity-80"
        style="color: #2563eb;"
        onclick={() => checkUpdate(false)}
      >重试</button>
    </div>
  {/if}
</div>
