<script lang="ts">
  import { onDestroy } from 'svelte';
  import CustomSelect from './ui/CustomSelect.svelte';
  import { generateSms, parseSmsBatch, smsDefaultsFromSettings, restoreSmsDefaults,
    SMS_ENCODING_OPTIONS as encodingOptions, SMS_VALIDITY_OPTIONS as validityOptions,
    type SmsConfig, type SmsGenerated, type SmsEncoding } from '$lib/sms';
  import { cachedSettings } from '$lib/stores';

  let { config, onconfigchange }: { config: SmsConfig; onconfigchange: (config: SmsConfig) => void } = $props();
  type Parsed = ReturnType<typeof parseSmsBatch>;
  let parsed = $state<Parsed | null>(null);
  let generated = $state<SmsGenerated | null>(null);
  let parsedSnapshot = $state('');
  let generatedSnapshot = $state('');
  let generationError = $state<{ snapshot: string; message: string } | null>(null);
  let feedback = $state('');
  let timer: ReturnType<typeof setTimeout> | undefined;
  const parseSnapshot = $derived(config.pdu_input);
  const generateSnapshot = $derived(JSON.stringify([config.recipient, config.smsc, config.text, config.encoding, config.reference, config.validity_period ?? null]));
  const isParse = $derived(config.mode === 'parse');
  const hasResult = $derived(isParse ? parsed !== null : generated !== null);
  const stale = $derived(isParse ? parsed !== null && parsedSnapshot !== parseSnapshot : generated !== null && generatedSnapshot !== generateSnapshot);
  const error = $derived(generationError?.snapshot === generateSnapshot ? generationError.message : '');
  const canCopy = $derived(hasResult && !stale && (isParse ? !!parsed?.messages.length : !error));
  const encodingLabel = (encoding: SmsEncoding) => encoding === 'gsm7' ? 'GSM 7-bit' : encoding === 'ucs2' ? 'UCS-2' : '8-bit 二进制';
  const byteHex = (value: number) => '0x' + value.toString(16).padStart(2, '0').toUpperCase();

  function update(patch: Partial<SmsConfig>) { onconfigchange({...config, ...patch}); feedback = ''; }
  function restoreDefaults() {
    update(restoreSmsDefaults(config, smsDefaultsFromSettings(cachedSettings.value?.ui)));
    feedback = '已恢复默认参数';
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => (feedback = ''), 2000);
  }
  function run() {
    feedback = '';
    if (isParse) {
      parsed = parseSmsBatch(config.pdu_input);
      parsedSnapshot = parseSnapshot;
    } else {
      try {
        generated = generateSms(config);
        generatedSnapshot = generateSnapshot;
        generationError = null;
      } catch (e) {
        generationError = { snapshot: generateSnapshot, message: e instanceof Error ? e.message : String(e) };
        if (generated) generatedSnapshot = '';
      }
    }
  }
  async function copy(value: string) {
    if (!canCopy) return;
    try { await navigator.clipboard.writeText(value); feedback = '已复制'; }
    catch { feedback = '复制失败，请选中结果后复制'; }
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => (feedback = ''), 2000);
  }
  function copyReport() {
    if (!parsed) return;
    const groups = parsed.groups.map(group => `长短信 ${group.peer} · 参考号 ${group.reference} · ${group.received}/${group.total} 段\n${group.complete ? group.text ?? group.dataHex : `缺段：${group.missing.join(', ') || '无'}；冲突段：${group.conflicts.join(', ') || '无'}`}`);
    const records = parsed.messages.map((message, index) => `第 ${index+1} 条 · ${message.type}\n号码：${message.peer}\n短信中心：${message.smsc || '使用模组设置'}\n时间：${message.timestamp ?? '无'}\n编码：${encodingLabel(message.encoding)}\n${message.text ?? message.dataHex}\nPDU：${message.pdu}`);
    void copy([...groups, ...records, ...parsed.errors].join('\n\n'));
  }
  onDestroy(() => { if (timer) clearTimeout(timer); });
</script>

<div class="sms-panel flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
  <div class="min-h-0 flex-1 overflow-y-auto px-4 py-3">
    <div class="sms-content flex h-full flex-col gap-3">
      <div role="group" aria-label="短信处理方式" class="flex shrink-0 items-center gap-2 text-[12px]">
        {#each [{value:'parse',label:'解析 PDU'}, {value:'generate',label:'生成 PDU'}] as mode}
          <button type="button" aria-pressed={config.mode === mode.value}
            class="h-7 rounded border px-3 leading-none transition-colors {config.mode === mode.value ? 'border-[var(--primary)] bg-[var(--focus-ring)] text-[var(--primary)]' : 'border-[var(--border)] text-[var(--muted-foreground)] hover:border-[var(--border-strong)] hover:text-[var(--foreground)]'}"
            onclick={() => update({mode: mode.value as SmsConfig['mode']})}>{mode.label}</button>
        {/each}
      </div>

      {#if isParse}
        <div class="shrink-0 space-y-1.5">
          <div class="flex items-center justify-between text-[12px]">
            <label for="sms-pdu-input" class="font-medium">完整 PDU</label>
            <button type="button" class="text-[var(--muted-foreground)]" onclick={() => update({pdu_input:''})}>清空</button>
          </div>
          <textarea id="sms-pdu-input" aria-label="短信 PDU 输入" class="sms-text" rows="4" spellcheck="false" value={config.pdu_input}
            oninput={(event) => update({pdu_input:event.currentTarget.value})}
            placeholder="每条 PDU 一行，包含 SMSC 长度字段；未指定短信中心时以 00 开头。"></textarea>
          <p class="text-[11px] text-[var(--muted-foreground)]">支持 PDU 模式的 +CMGR / +CMT / +CMGL 返回内容；可粘贴多段长短信一起解析。</p>
        </div>
      {:else}
        <div class="shrink-0 space-y-2 text-[12px]">
          <div class="flex flex-wrap items-center gap-2">
            <label for="sms-recipient">接收号码</label>
            <input id="sms-recipient" aria-label="短信接收号码" class="sms-input min-w-0 flex-1" type="text" placeholder="例如 +8613800138000"
              value={config.recipient} oninput={(event) => update({recipient:event.currentTarget.value})} />
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <span>短信编码</span>
            <CustomSelect width="160px" options={encodingOptions}
              bind:value={() => config.encoding, (value) => update({encoding:value as SmsConfig['encoding']})} />
            <span class="text-[11px] text-[var(--muted-foreground)]">自动优先 GSM 7-bit</span>
          </div>
          <label for="sms-text-input" class="block">短信正文</label>
          <textarea id="sms-text-input" aria-label="短信正文输入" class="sms-text" rows="3" spellcheck="false" value={config.text}
            oninput={(event) => update({text:event.currentTarget.value})} placeholder="输入短信内容，超长正文自动分段"></textarea>
          <details class="sms-details">
            <summary>更多参数</summary>
            <div class="mt-2 flex flex-wrap items-center gap-2">
              <span>有效期</span>
              <CustomSelect width="160px" options={validityOptions}
                selectedFallbackLabel={config.validity_period == null ? undefined : `自定义（${config.validity_period}）`}
                bind:value={() => config.validity_period == null ? 'none' : String(config.validity_period),
                  (value) => update({validity_period: value === 'none' ? null : Number(value)})} />
            </div>
            <p class="mt-1 text-[11px] text-[var(--muted-foreground)]">从短信中心收到短信时起计算；不指定时不携带有效期字段。</p>
            <div class="mt-2 flex flex-wrap items-center gap-2">
              <label for="sms-center">短信中心</label>
              <input id="sms-center" class="sms-input min-w-0 flex-1" placeholder="留空使用模组设置" value={config.smsc}
                oninput={(event) => update({smsc:event.currentTarget.value})} />
            </div>
            <div class="mt-2 flex flex-wrap items-center gap-2">
              <label for="sms-reference">长短信参考号</label>
              <input id="sms-reference" class="sms-input w-20" type="number" min="0" max="255" step="1" value={config.reference}
                oninput={(event) => update({reference:event.currentTarget.value === '' ? NaN : Number(event.currentTarget.value)})} />
              <span class="text-[11px] text-[var(--muted-foreground)]">0–255，同一条短信各段共用；不同长短信请更换。</span>
            </div>
            <button type="button" class="sms-copy mt-2" onclick={restoreDefaults}
              title="恢复设置中的短信中心、编码和有效期，保留接收号码、正文及参考号">恢复默认参数</button>
          </details>
        </div>
      {/if}

      <div class="flex shrink-0 flex-wrap items-center gap-2">
        <button type="button" class="btn btn-primary h-7 px-4 text-[12px] leading-none" onclick={run}>{isParse ? '解析' : '生成'}</button>
        {#if stale}<span class="text-[12px] text-[var(--warning)]">已过期 · 输入已改，请重新{isParse ? '解析' : '生成'}</span>{/if}
      </div>

      <div class="sms-results min-h-[100px] flex-1 overflow-y-auto space-y-3 pr-1" aria-label="短信处理结果">
        {#if isParse}
          {#if parsed}
            {#each parsed.errors as message}<p role="alert" class="break-words text-[12px] text-[var(--error)]">{message}</p>{/each}
            <div class="space-y-3 {stale ? 'opacity-50' : ''}">
              {#each parsed.groups as group, index}
                <div class="sms-card" data-sms-assembly>
                  <div class="mb-1 flex flex-wrap items-center justify-between gap-2">
                    <span class="font-medium">长短信 · {group.complete ? '已合并' : '未合并'} · {group.received}/{group.total} 段</span>
                    <button type="button" class="sms-copy" disabled={!canCopy || !group.complete} onclick={() => copy(group.text ?? group.dataHex ?? '')}>复制完整{group.encoding === '8bit' ? '数据' : '正文'}</button>
                  </div>
                  <p class="mb-1 break-all text-[11px] text-[var(--muted-foreground)]">{group.peer} · 参考号 {group.reference}</p>
                  {#if group.complete}
                    <textarea class="sms-text" aria-label={`合并短信 ${index+1}`} readonly rows="3" value={group.text ?? group.dataHex ?? ''}></textarea>
                  {:else}
                    <p class="text-[var(--warning)]">{#if group.missing.length}缺少第 {group.missing.join('、')} 段。{/if}{#if group.conflicts.length}第 {group.conflicts.join('、')} 段存在内容冲突。{/if}</p>
                  {/if}
                </div>
              {/each}
              {#each parsed.messages as message, index}
                <div class="sms-card" data-sms-message>
                  <div class="mb-1 flex flex-wrap items-center justify-between gap-2">
                    <span class="font-medium">第 {index+1} 条 · {message.type === 'SMS-DELIVER' ? '接收短信' : '发送短信'}{#if message.concat} · {message.concat.sequence}/{message.concat.total} 段{/if}</span>
                    <button type="button" class="sms-copy" disabled={!canCopy} onclick={() => copy(message.text ?? message.dataHex)}>复制{message.text === null ? 'Hex' : '正文'}</button>
                  </div>
                  <p class="mb-1 break-all text-[11px] text-[var(--muted-foreground)]">{message.peer} · {encodingLabel(message.encoding)}</p>
                  <textarea class="sms-text" aria-label={`短信正文 ${index+1}`} readonly rows="3" value={message.text ?? message.dataHex} placeholder="空正文"></textarea>
                  <details class="sms-details mt-2">
                    <summary>协议详情</summary>
                    <dl class="sms-fields mt-2">
                      <dt>类型</dt><dd>{message.type}</dd>
                      <dt>短信中心</dt><dd>{message.smsc || '未指定（使用模组设置）'}</dd>
                      <dt>{message.type === 'SMS-DELIVER' ? '发送号码' : '接收号码'}</dt><dd>{message.peer}</dd>
                      {#if message.timestamp}<dt>短信时间</dt><dd>{message.timestamp}</dd>{/if}
                      <dt>首字节 / PID / DCS</dt><dd>{byteHex(message.firstOctet)} / {byteHex(message.pid)} / {byteHex(message.dcs)}</dd>
                      <dt>UDL</dt><dd>{message.udl} {message.encoding === 'gsm7' ? 'septets' : '字节'}</dd>
                      <dt>TPDU 长度</dt><dd>{message.tpduLength} 字节</dd>
                      {#if message.messageReference !== null}<dt>消息参考号</dt><dd>{message.messageReference}</dd>{/if}
                      {#if message.validity}<dt>有效期</dt><dd>{message.validity}</dd>{/if}
                      {#if message.concat}<dt>长短信参考号</dt><dd>{message.concat.reference}（{message.concat.bits} 位）</dd>{/if}
                      {#each message.udh as ie}<dt>UDH · {byteHex(ie.id)}</dt><dd>{ie.dataHex}</dd>{/each}
                    </dl>
                    <textarea class="sms-text mt-2" aria-label={`原始 PDU ${index+1}`} readonly rows="3" value={message.pdu}></textarea>
                  </details>
                </div>
              {/each}
            </div>
          {:else}<p class="text-[12px] text-[var(--muted-foreground)]">点击“解析”后显示短信正文和协议详情。</p>{/if}
        {:else}
          {#if error}<p role="alert" class="text-[12px] text-[var(--error)]">{error}</p>{/if}
          {#if generated}
            <div class="space-y-3 {stale ? 'opacity-50' : ''}">
              <p class="text-[12px] text-[var(--muted-foreground)]">{encodingLabel(generated.encoding)} · 共 {generated.parts.length} 段 · SMS-SUBMIT</p>
              {#each generated.parts as part}
                <div class="sms-card" data-sms-generated>
                  <div class="mb-2 flex flex-wrap items-center justify-between gap-2">
                    <span class="font-medium">第 {part.sequence}/{part.total} 段</span>
                    <button type="button" class="sms-copy" disabled={!canCopy} onclick={() => copy(part.pdu)}>复制 PDU</button>
                  </div>
                  <p class="mb-2 select-text font-mono">AT+CMGS={part.tpduLength}</p>
                  <textarea class="sms-text" aria-label={`生成 PDU ${part.sequence}`} readonly rows="3" value={part.pdu}></textarea>
                  <details class="sms-details mt-2"><summary>本段正文</summary><textarea class="sms-text mt-2" readonly rows="3" aria-label={`分段正文 ${part.sequence}`} value={part.text} placeholder="空正文"></textarea></details>
                </div>
              {/each}
              <p class="text-[11px] text-[var(--muted-foreground)]">CMGS 长度不含短信中心地址部分。PDU 在模组输入提示符后以文本字符输入。</p>
            </div>
          {:else if !error}<p class="text-[12px] text-[var(--muted-foreground)]">填写号码和正文，点击“生成”。</p>{/if}
        {/if}
      </div>
    </div>
  </div>
  <div class="flex shrink-0 flex-wrap items-center gap-2 border-t border-[var(--border)] bg-[var(--background-elevated)] px-4 py-2">
    {#if isParse}
      <button type="button" class="sms-action" disabled={!canCopy} onclick={copyReport}>复制解析结果</button>
    {:else}
      <button type="button" class="sms-action" disabled={!canCopy} onclick={() => generated && copy(generated.parts.map(part => part.pdu).join('\n'))}>复制全部 PDU</button>
    {/if}
    <span role="status" class="text-[12px] text-[var(--primary)]">{feedback}</span>
  </div>
</div>

<style>
  .sms-content { min-height: 420px; }
  .sms-text, .sms-input {
    width: 100%; border: 1px solid var(--border); border-radius: 6px;
    background: var(--background-input); color: var(--foreground); padding: 6px 9px;
    user-select: text; -webkit-user-select: text; font: 12px/1.6 var(--font-mono);
  }
  .sms-input { width: auto; }
  .sms-text { display: block; resize: none; overflow: auto; overflow-wrap: anywhere; }
  .sms-text:focus, .sms-input:focus { outline: none; border-color: var(--primary); }
  .sms-card { border: 1px solid var(--border); border-radius: 6px; padding: 10px; font-size: 12px; background: var(--background-data); }
  .sms-copy { color: var(--primary); cursor: pointer; }
  .sms-copy:disabled, .sms-action:disabled { opacity: .4; cursor: not-allowed; }
  .sms-details { font-size: 12px; }
  .sms-details summary { color: var(--muted-foreground); cursor: pointer; }
  .sms-fields { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 4px 12px; user-select: text; }
  .sms-fields dt { color: var(--muted-foreground); }
  .sms-fields dd { overflow-wrap: anywhere; }
  .sms-action { height: 30px; padding: 0 12px; border: 1px solid var(--border-strong); border-radius: 6px; background: var(--background-input); font-size: 12px; cursor: pointer; }
  .sms-action:hover:not(:disabled) { border-color: var(--primary); }
</style>
