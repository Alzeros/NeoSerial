<script lang="ts">
  import { presetBaudRates, cachedSettings } from '$lib/stores';
  import { saveSettings } from '$lib/tauri';
  import type { Settings } from '$lib/types';

  let open = $state(false);

  // 内置默认波特率：不可删除，只能在此基础上增删用户自定义项
  const DEFAULT_BAUD_RATES = [9600, 115200, 921600];
  const isDefault = (n: number) => DEFAULT_BAUD_RATES.includes(n);

  // 预设波特率编辑副本（打开时从 store 拷贝，取消不污染 store）
  let editBaudRates = $state<number[]>([]);
  let newBaud = $state('');

  export function show() {
    editBaudRates = [...presetBaudRates.value];
    newBaud = '';
    open = true;
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
    // 落盘：基于缓存 settings 透传，仅更新 presets
    const base = cachedSettings.value;
    if (base) {
      const next: Settings = { ...base, presets: { baud_rates: rates } };
      try {
        await saveSettings(next);
        cachedSettings.value = next;
      } catch (e) {
        console.error('保存预设波特率失败:', e);
      }
    }
    open = false;
  }

  function handleCancel() {
    open = false;
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center"
    style="background: rgba(0,0,0,0.35);"
    onclick={handleCancel}
  >
    <div
      class="rounded-lg shadow-xl w-[380px] border"
      style="background: var(--background-elevated); border-color: var(--border);"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="px-6 py-4 border-b border-[var(--border)]">
        <div class="text-[15px] font-semibold text-[var(--foreground)]">设置</div>
      </div>

      <div class="px-6 py-5">
        <!-- 预设波特率 -->
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
      </div>

      <div class="flex justify-end gap-2 px-4 pb-4 border-t border-[var(--border)] pt-3">
        <button class="btn btn-ghost" style="padding: 6px 14px;" onclick={handleCancel}>取消</button>
        <button class="btn btn-primary" style="padding: 6px 14px;" onclick={handleSave}>保存</button>
      </div>
    </div>
  </div>
{/if}
