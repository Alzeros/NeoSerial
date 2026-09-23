<script lang="ts">
  import { onDestroy } from 'svelte';
  import CustomSelect from './ui/CustomSelect.svelte';
  import { CODEC_OPERATIONS, convertCodec, codecSendPayload, type CodecConfig, type CodecResult } from '$lib/codec';
  import { hexSend, requestSuggestFill } from '$lib/stores';

  let { config, onconfigchange }: {
    config: CodecConfig;
    onconfigchange: (config: CodecConfig) => void;
  } = $props();

  let result = $state<CodecResult | null>(null);
  let resultSnapshot = $state('');
  let failure = $state<{ snapshot: string; message: string } | null>(null);
  let feedback = $state('');
  let feedbackTimer: ReturnType<typeof setTimeout> | undefined;
  const snapshot = $derived(JSON.stringify(config));
  const stale = $derived(result !== null && resultSnapshot !== snapshot);
  const error = $derived(failure?.snapshot === snapshot ? failure.message : '');
  const usable = $derived(result !== null && !stale && !error);
  const payload = $derived(result ? codecSendPayload(result) : null);
  const isBase64 = $derived(config.operation.startsWith('base64_'));
  const inputFormat = $derived(config.operation === 'base64_decode' ? 'Base64'
    : config.operation === 'hex_to_text' || (config.operation === 'base64_encode' && config.format === 'hex') ? 'Hex' : '文本（UTF-8）');
  const outputFormat = $derived(config.operation === 'base64_encode' ? 'Base64'
    : config.operation === 'text_to_hex' || (config.operation === 'base64_decode' && config.format === 'hex') ? 'Hex' : '文本（UTF-8）');
  const formatOptions = [{ label: '文本（UTF-8）', value: 'text' }, { label: 'Hex', value: 'hex' }];
  const actionLabel = $derived(config.operation === 'base64_encode' ? '编码' : config.operation === 'base64_decode' ? '解码' : '转换');
  const fillHint = $derived(payload?.hex ? '按 Hex 字节填入，保留原始数据' : '按文本填入发送框');

  function update(patch: Partial<CodecConfig>) {
    onconfigchange({ ...config, ...patch });
    feedback = '';
  }

  function run() {
    feedback = '';
    try {
      result = convertCodec(config);
      resultSnapshot = snapshot;
      failure = null;
    } catch (e) {
      failure = { snapshot, message: e instanceof Error ? e.message : String(e) };
      if (result) resultSnapshot = '';
    }
  }

  function showFeedback(message: string) {
    if (feedbackTimer) clearTimeout(feedbackTimer);
    feedback = message;
    feedbackTimer = setTimeout(() => (feedback = ''), 2000);
  }

  async function copy() {
    if (!usable || !result) return;
    const copiedResult = result;
    const copiedSnapshot = snapshot;
    try {
      await navigator.clipboard.writeText(copiedResult.value);
      if (result === copiedResult && snapshot === copiedSnapshot) showFeedback('已复制');
    } catch {
      showFeedback('复制失败，请在结果区选中后复制');
    }
  }

  function fill() {
    if (!usable || !payload || !result?.value) return;
    hexSend.value = payload.hex;
    requestSuggestFill(payload.text);
    showFeedback(payload.hex ? '已按 Hex 填入' : '已按文本填入');
  }

  onDestroy(() => { if (feedbackTimer) clearTimeout(feedbackTimer); });
</script>

<div class="codec-panel flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
  <div class="min-h-0 flex-1 overflow-y-auto px-4 py-3">
    <div class="codec-content flex h-full flex-col gap-3">
      <div class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 text-[12px]">
        <span class="text-[var(--muted-foreground)]">处理方式</span>
        <CustomSelect options={CODEC_OPERATIONS} width="155px"
          bind:value={() => config.operation, (value) => update({ operation: value as CodecConfig['operation'] })} />
        {#if isBase64}
          <span class="text-[var(--muted-foreground)]">{config.operation === 'base64_encode' ? '输入格式' : '输出格式'}</span>
          <CustomSelect options={formatOptions} width="135px"
            bind:value={() => config.format, (value) => update({ format: value as CodecConfig['format'] })} />
        {/if}
      </div>

      <div class="flex min-h-0 flex-1 flex-col gap-1.5">
        <div class="flex shrink-0 items-center justify-between gap-2 text-[12px]">
          <label for="codec-input" class="font-medium">输入 · {inputFormat}</label>
          <button type="button" class="text-[var(--muted-foreground)] hover:text-[var(--foreground)] disabled:opacity-40"
            disabled={!config.input} onclick={() => update({ input: '' })}>清空</button>
        </div>
        <textarea id="codec-input" aria-label="编解码输入" class="codec-text flex-1" spellcheck="false"
          value={config.input} oninput={(event) => update({ input: event.currentTarget.value })}
          placeholder={inputFormat === 'Hex' ? '例如 48 65 6C 6C 6F，支持空白分隔或连续 Hex' : inputFormat === 'Base64' ? '粘贴标准 Base64 数据' : '输入需要处理的文本，使用 UTF-8 编码'}></textarea>
      </div>

      <div class="flex shrink-0 flex-wrap items-center gap-2">
        <button type="button" class="btn btn-primary h-7 px-4 text-[12px] leading-none" onclick={run}>{actionLabel}</button>
        {#if stale}<span class="text-[12px] text-[var(--warning)]">已过期 · 输入或设置已改，请重新{actionLabel}</span>{/if}
      </div>
      {#if error}<p role="alert" class="shrink-0 break-words text-[12px] text-[var(--error)]">{error}</p>{/if}

      <div class="flex min-h-0 flex-1 flex-col gap-1.5">
        <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 text-[12px]">
          <label for="codec-result" class="font-medium">结果 · {result ? result.format === 'text' ? '文本（UTF-8）' : result.format === 'hex' ? 'Hex' : 'Base64' : outputFormat}</label>
          {#if result}<span class="text-[var(--muted-foreground)]">{result.byteLength} 字节</span>{/if}
        </div>
        <textarea id="codec-result" aria-label="编解码结果" class="codec-text flex-1 {stale ? 'opacity-50' : ''}" readonly spellcheck="false"
          value={result?.value ?? ''} placeholder={result ? '结果为空（0 字节）' : `点击“${actionLabel}”后显示结果`}></textarea>
      </div>
    </div>
  </div>

  <div class="shrink-0 border-t border-[var(--border)] bg-[var(--background-elevated)] px-4 py-2">
    <div class="flex flex-wrap items-center gap-2">
      <button type="button" class="codec-action" disabled={!usable} onclick={copy}>复制结果</button>
      <button type="button" class="codec-action" disabled={!usable || !result?.value} title={fillHint} onclick={fill}>填入发送框</button>
      <span role="status" class="text-[12px] text-[var(--primary)]">{feedback}</span>
    </div>
    {#if usable && result?.format === 'text' && payload?.hex}
      <p class="mt-1 text-[11px] text-[var(--muted-foreground)]">含非 ASCII 字符或空白控制字符，填入时使用 UTF-8 Hex 字节。</p>
    {/if}
  </div>
</div>

<style>
  .codec-content { min-height: 330px; }
  .codec-text {
    width: 100%; min-height: 70px; resize: none; overflow: auto;
    user-select: text; -webkit-user-select: text;
    border: 1px solid var(--border); border-radius: 6px;
    background: var(--background-input); color: var(--foreground);
    padding: 8px 10px; font: 13px/1.6 var(--font-mono);
    overflow-wrap: anywhere;
  }
  .codec-text:focus { outline: none; border-color: var(--primary); }
  .codec-action {
    height: 30px; padding: 0 12px; border-radius: 6px; font-size: 12px;
    border: 1px solid var(--border-strong); background: var(--background-input);
    color: var(--foreground-secondary); cursor: pointer;
  }
  .codec-action:hover:not(:disabled) { border-color: var(--primary); color: var(--foreground); }
  .codec-action:disabled { opacity: .4; cursor: not-allowed; }
</style>
