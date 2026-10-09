import { getVersion } from '@tauri-apps/api/app';
import { check } from '@tauri-apps/plugin-updater';
import { getSettings, patchSettings, onSettingsChanged } from './tauri';
import { defaultUpdaterSettings, UpdateReminder, type UpdaterSettings, type UpdateInfo } from './updateReminder';

export const updateView = $state({
  settings: defaultUpdaterSettings(), currentVersion: '', ready: false, error: '',
});

let manager: UpdateReminder | undefined;
let generation = 0;
let saveTail: Promise<unknown> = Promise.resolve();

export function watchUpdates(automatic: boolean): () => void {
  const token = ++generation;
  let unlisten: (() => void) | undefined;
  let refreshId = 0;
  updateView.ready = false;
  updateView.error = '';
  manager?.dispose();
  manager = undefined;

  async function refresh() {
    const request = ++refreshId;
    try {
      await saveTail;
      const settings = await getSettings();
      if (token !== generation || request !== refreshId) return;
      manager?.sync({ ...defaultUpdaterSettings(), ...settings.updater });
    } catch {
      if (token === generation) updateView.error = '读取更新设置失败，请重新打开设置重试';
    }
  }

  void (async () => {
    try {
      const listener = await onSettingsChanged(() => { void refresh(); });
      if (token !== generation) { listener(); return; }
      unlisten = listener;
      const version = await getVersion();
      if (token !== generation) return;
      updateView.currentVersion = version;
      manager = new UpdateReminder(version, automatic, {
        now: Date.now,
        changed: settings => {
          updateView.settings = settings;
          updateView.ready = true;
          updateView.error = '';
        },
        save: patch => {
          const task = saveTail.then(() => patchSettings({ updater: patch }));
          saveTail = task.catch(() => {});
          return task.then(settings => ({ ...defaultUpdaterSettings(), ...settings.updater }));
        },
        check: async () => {
          const update = await check({ timeout: 15000 });
          try {
            return update ? { version: update.version, body: update.body ?? '' } : null;
          } finally {
            await update?.close();
          }
        },
      });
      await refresh();
    } catch {
      if (token === generation) updateView.error = '初始化更新设置失败，请重新打开设置重试';
    }
  })();

  return () => {
    if (token !== generation) return;
    generation++;
    manager?.dispose();
    manager = undefined;
    unlisten?.();
  };
}

export async function setUpdatePreferences(patch: Partial<UpdaterSettings>): Promise<void> {
  if (!manager || !updateView.ready) throw Error('更新设置尚未加载');
  await manager.update(patch);
}

export async function recordUpdate(update: UpdateInfo | null): Promise<void> {
  if (!manager || !updateView.ready) throw Error('更新设置尚未加载');
  await manager.record(update);
}
