<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { scriptModules, activeScriptModule, activeScriptPage, currentModulePages } from '$lib/stores';
  import { loadSequenceAuto, saveSequenceAuto, onSequenceChanged } from '$lib/tauri';
  import { loadSequenceOnce } from '$lib/startup';
  import type { ScriptModule } from '$lib/types';

  // 无界面、每个串口窗口仅挂载一次。右侧 ScriptSequencer 可随时卸载，
  // 但草稿、防抖保存和跨窗口监听必须一直存在，否则重新展开会用磁盘旧值覆盖草稿。
  let loaded = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let lastPersistedJson: string | null = null;
  let savesInFlight = 0;
  let disposed = false;
  let reloadId = 0;

  function cancelTimer() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = null;
  }

  async function reload() {
    const id = ++reloadId;
    const before = JSON.stringify(scriptModules);
    try {
      const modules = await loadSequenceAuto();
      // 读盘期间又收到新通知/本地编辑时，不用晚到的旧数据覆盖它们。
      if (disposed || id !== reloadId || savesInFlight || JSON.stringify(scriptModules) !== before) return;
      if (modules.length > 0) {
        scriptModules.length = 0;
        scriptModules.push(...modules);
        lastPersistedJson = JSON.stringify(scriptModules);
        activeScriptModule.value = Math.min(activeScriptModule.value, scriptModules.length - 1);
        const pages = currentModulePages();
        activeScriptPage.value = Math.min(activeScriptPage.value, Math.max(0, pages.length - 1));
      }
    } catch (e) {
      console.error('自动加载序列配置失败:', e);
    }
  }

  $effect(() => {
    const json = JSON.stringify(scriptModules);
    if (!loaded) return;
    cancelTimer();
    // reload/保存的回声不再落盘，避免多窗口之间反复互相触发保存。
    if (json === lastPersistedJson) return;
    saveTimer = setTimeout(async () => {
      saveTimer = null;
      const snapshot = JSON.stringify(scriptModules);
      savesInFlight++;
      try {
        // 提交与确认使用同一份独立快照，IPC 等待期间的编辑留给下一次防抖保存。
        await saveSequenceAuto(JSON.parse(snapshot) as ScriptModule[]);
        if (!disposed) lastPersistedJson = snapshot;
      } catch (e) {
        console.error('自动保存序列配置失败:', e);
      } finally {
        savesInFlight--;
      }
    }, 800);
  });

  const myLabel = getCurrentWebview().label;
  onMount(() => {
    // 与 main.ts 的预取共享同一 Promise；侧栏显隐不会再次加载磁盘或重置草稿。
    loadSequenceOnce().then((persisted) => {
      if (disposed) return;
      lastPersistedJson = persisted;
      loaded = true;
    });
    const unlisten = onSequenceChanged((event) => {
      if (event.source === myLabel || !loaded) return;
      // 有待保存/保存中的本地编辑时维持原来的“最后保存者赢”语义。
      if (savesInFlight || JSON.stringify(scriptModules) !== lastPersistedJson) return;
      void reload();
    });
    return () => {
      disposed = true;
      cancelTimer();
      unlisten.then((stop) => stop());
    };
  });
</script>
