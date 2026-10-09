export const DAY = 24 * 60 * 60 * 1000;

export interface UpdaterSettings {
  auto_check: boolean;
  last_check_at: number;
  checked_version: string;
  available_version: string;
  release_notes: string;
  ignored_version: string;
  snoozed_version: string;
  snooze_until: number;
}

export function defaultUpdaterSettings(): UpdaterSettings {
  return {
    auto_check: true, last_check_at: 0, checked_version: '', available_version: '',
    release_notes: '', ignored_version: '', snoozed_version: '', snooze_until: 0,
  };
}

export function checkDue(settings: UpdaterSettings, current: string, now: number): boolean {
  return settings.auto_check && (settings.checked_version !== current || !settings.last_check_at
    || now < settings.last_check_at || now - settings.last_check_at >= DAY);
}

export function noticeVersion(settings: UpdaterSettings, current: string, now: number): string {
  const version = settings.available_version;
  if (!settings.auto_check || !version || settings.checked_version !== current
    || version === current || settings.ignored_version === version
    || (settings.snoozed_version === version && now < settings.snooze_until)) return '';
  return version;
}

export type UpdateInfo = { version: string; body: string };
type Dependencies = {
  now: () => number;
  save: (patch: Partial<UpdaterSettings>) => Promise<UpdaterSettings>;
  check: () => Promise<UpdateInfo | null>;
  changed: (settings: UpdaterSettings) => void;
};

export class UpdateReminder {
  settings = defaultUpdaterSettings();
  private current: string;
  private automatic: boolean;
  private deps: Dependencies;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private epoch = 0;
  private disposed = false;
  private running = false;

  constructor(current: string, automatic: boolean, deps: Dependencies) {
    this.current = current;
    this.automatic = automatic;
    this.deps = deps;
  }

  sync(settings: UpdaterSettings) {
    if (this.disposed) return;
    if (this.settings.auto_check !== settings.auto_check) this.epoch++;
    this.settings = settings;
    this.deps.changed(settings);
    clearTimeout(this.timer);
    if (this.automatic && !this.running && checkDue(settings, this.current, this.deps.now())) {
      this.timer = setTimeout(() => { void this.runAutomatic(); }, 5000);
    }
  }

  async update(patch: Partial<UpdaterSettings>) {
    const previous = this.settings;
    this.epoch++;
    this.sync({ ...previous, ...patch });
    const epoch = this.epoch;
    try {
      const saved = await this.deps.save(patch);
      if (!this.disposed && epoch === this.epoch) this.sync(saved);
    } catch (error) {
      if (!this.disposed && epoch === this.epoch) this.sync(previous);
      throw error;
    }
  }

  async record(update: UpdateInfo | null) {
    const epoch = ++this.epoch;
    const saved = await this.deps.save(this.resultPatch(update));
    if (!this.disposed && epoch === this.epoch) this.sync(saved);
  }

  private resultPatch(update: UpdateInfo | null): Partial<UpdaterSettings> {
    return {
      checked_version: this.current, last_check_at: this.deps.now(),
      available_version: update?.version ?? '', release_notes: update?.body ?? '',
    };
  }

  async runAutomatic() {
    if (this.disposed || this.running || !this.automatic
      || !checkDue(this.settings, this.current, this.deps.now())) return;
    clearTimeout(this.timer);
    this.running = true;
    const epoch = this.epoch;
    const valid = () => !this.disposed && epoch === this.epoch && this.settings.auto_check;
    try {
      const saved = await this.deps.save({
        last_check_at: this.deps.now(), checked_version: this.current,
        ...(this.settings.checked_version !== this.current ? { available_version: '', release_notes: '' } : {}),
      });
      if (!valid()) return;
      this.sync(saved);
      const update = await this.deps.check();
      if (!valid()) return;
      const result = await this.deps.save(this.resultPatch(update));
      if (valid()) this.sync(result);
    } catch {
    } finally {
      this.running = false;
    }
  }

  dispose() {
    this.disposed = true;
    this.epoch++;
    clearTimeout(this.timer);
  }
}
