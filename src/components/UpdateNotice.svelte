<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { ArrowUpCircle, X } from 'lucide-svelte';
  import { openSettingsWindow } from '$lib/tauri';
  import { updateView, watchUpdates, setUpdatePreferences } from '$lib/updates.svelte';
  import { DAY, noticeVersion } from '$lib/updateReminder';

  let { currentVersion = '' }: {
    currentVersion?: string;
  } = $props();

  const primary = getCurrentWindow().label === 'main';
  let now = $state(Date.now());
  let error = $state('');
  let saving = $state(false);
  const displayVersion = $derived(currentVersion || updateView.currentVersion);
  const version = $derived(primary && updateView.ready
    ? noticeVersion(updateView.settings, updateView.currentVersion, now) : '');

  onMount(() => {
    if (!primary) return;
    const stop = watchUpdates(true);
    const timer = setInterval(() => { now = Date.now(); }, 60000);
    return () => { clearInterval(timer); stop(); };
  });

  async function snooze() {
    if (saving) return;
    saving = true;
    error = '';
    try {
      await setUpdatePreferences({ snoozed_version: version, snooze_until: Date.now() + 7 * DAY });
    } catch {
      error = '暂缓提醒未保存，请重试';
    } finally {
      saving = false;
    }
  }

  async function openUpdate() {
    try {
      await openSettingsWindow({ section: 'about' });
      error = '';
    } catch {
      error = '打开更新设置失败，请重试';
    }
  }
</script>

{#if version && displayVersion}
  <div class="flex items-center shrink-0 rounded-md text-[11px]" style="background: var(--border-subtle); color: var(--primary);" data-update-notice>
    <button class="flex items-center gap-1 px-2 py-1 cursor-pointer" onclick={openUpdate} title={error || `发现新版本 v${version}，点击查看更新说明，不会自动下载安装`}>
      <span data-app-version>v{displayVersion}</span>
      <ArrowUpCircle size={13} />
      <span>{error ? '请重试' : '可更新'}</span>
    </button>
    <button class="p-1 mr-1 cursor-pointer" onclick={snooze} disabled={saving} aria-label="稍后提醒" title="此版本 7 天内不再提醒">
      <X size={12} />
    </button>
  </div>
{:else if displayVersion}
  <span data-app-version data-tauri-drag-region class="shrink-0 px-2 py-1 text-[11px] text-[var(--muted-foreground)]">v{displayVersion}</span>
{/if}
