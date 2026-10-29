<script lang="ts">
  import type { LogFilter } from '$lib/logDiagnostics';

  let {
    filter = $bindable(), selectionMode = $bindable(false), context = $bindable(10), marker = $bindable(''),
    selectedCount, elapsed, hasRows, hasErrors, busy, onreset, onerror, onclearSelection, onmark, oncopy, onsave,
  }: {
    filter: LogFilter;
    selectionMode: boolean;
    context: number;
    marker: string;
    selectedCount: number;
    elapsed: number | null;
    hasRows: boolean;
    hasErrors: boolean;
    busy: boolean;
    onreset: () => void;
    onerror: (step: number) => void;
    onclearSelection: () => void;
    onmark: () => void;
    oncopy: () => void;
    onsave: () => void;
  } = $props();
</script>

<div data-log-tools class="tools border-b border-[var(--border)] px-3 py-2" style="background: var(--background-elevated);">
  <div class="tool-row">
    <label>方向
      <select data-log-direction bind:value={filter.direction} aria-label="日志方向">
        <option value="all">全部</option><option value="rx">仅接收</option><option value="tx">仅发送</option>
      </select>
    </label>
    <label><input data-log-errors type="checkbox" bind:checked={filter.errorsOnly} />仅错误</label>
    <input data-log-include aria-label="包含关键词" placeholder="包含关键词（字面匹配）" bind:value={filter.include} />
    <input data-log-exclude aria-label="排除关键词" placeholder="排除关键词" bind:value={filter.exclude} />
    <button data-log-filter-reset onclick={onreset}>重置筛选</button>
  </div>
  <div class="tool-row">
    <button disabled={!hasErrors} onclick={() => onerror(-1)} title="跳到当前筛选范围内的上一个错误">上个错误</button>
    <button data-log-next-error disabled={!hasErrors} onclick={() => onerror(1)} title="跳到当前筛选范围内的下一个错误">下个错误</button>
    <label><input data-log-select-toggle type="checkbox" bind:checked={selectionMode} />选择记录（最多两条）</label>
    {#if selectedCount > 0}
      <span>已选 {selectedCount} 条</span>
      <button onclick={onclearSelection}>取消选择</button>
    {/if}
    {#if selectedCount === 2}
      <span data-log-elapsed>时间差：{elapsed === null ? '时间戳缺失' : `${elapsed.toLocaleString()} ms`}</span>
    {/if}
  </div>
  <div class="tool-row">
    <input data-log-marker-input aria-label="人工标记" maxlength="120" placeholder="为选中首条或最新记录添加标记" bind:value={marker} />
    <button data-log-marker-add disabled={!hasRows || !marker.trim()} onclick={onmark}>添加标记</button>
  </div>
  <div class="tool-row">
    <label>前后各
      <input aria-label="上下文行数" type="number" min="0" max="1000" step="1" bind:value={context} class="context-count" />
      条上下文
    </label>
    <button data-log-snippet-copy disabled={!hasRows || busy} onclick={oncopy}>复制片段</button>
    <button data-log-snippet-save disabled={!hasRows || busy} onclick={onsave}>{busy ? '处理中…' : '导出 TXT'}</button>
    <span class="hint">{selectedCount ? '选中范围含原始上下文（含被筛选隐藏的记录）' : '未选择时导出当前筛选结果'}</span>
  </div>
</div>

<style>
  .tools { display: flex; flex-direction: column; gap: 6px; font-size: 12px; color: var(--muted-foreground); max-height: 42%; overflow-y: auto; flex-shrink: 0; }
  .tool-row { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 8px; }
  label { display: inline-flex; align-items: center; gap: 5px; }
  button, select, input:not([type="checkbox"]) { min-height: 28px; border: 1px solid var(--border); border-radius: 5px; background: var(--background); color: var(--foreground); padding: 3px 7px; font: inherit; }
  button { cursor: pointer; white-space: nowrap; }
  button:hover:not(:disabled) { background: var(--overlay-hover); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  input:not([type="checkbox"]) { width: 150px; min-width: 0; }
  input[data-log-marker-input] { flex: 1; min-width: 140px; }
  input.context-count { width: 62px; }
  .hint { font-size: 11px; }
</style>
