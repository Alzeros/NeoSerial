<script lang="ts">
  // 帧构造器表单 + 实时预览 + 发送/填入/模板。
  // 表单直接 bind tool.config.xxx($state 深响应):ScriptSequencer 的自动保存 effect 深度
  // 订阅 scriptModules,改动即落盘,这里不做额外 clone。
  import CustomSelect from './ui/CustomSelect.svelte';
  import { buildFrame, send, type FrameResult } from '$lib/tauri';
  import {
    addTemplate,
    removeTemplate,
    loadTemplate,
    type FrameBuilderTool,
  } from '$lib/dataProcessing';
  import { connected, windowPort, hexSend, requestSuggestFill } from '$lib/stores';

  let { tool }: { tool: FrameBuilderTool } = $props();

  // ---- 下拉选项 ----
  const lenSizeOpts = [
    { label: '不含', value: 'none' },
    { label: '1 字节', value: 'one' },
    { label: '2 字节', value: 'two' },
  ];
  const endianOpts = [
    { label: '小端 (LE)', value: 'le' },
    { label: '大端 (BE)', value: 'be' },
  ];
  const coverageOpts = [
    { label: '仅数据域', value: 'data' },
    { label: '数据域+校验', value: 'data_plus_checksum' },
    { label: '整帧', value: 'whole_frame' },
  ];
  const dataModeOpts = [
    { label: '内容', value: 'content' },
    { label: '填充', value: 'fill' },
  ];
  const formatOpts = [
    { label: 'Hex', value: 'hex' },
    { label: '文本', value: 'text' },
  ];
  const patternOpts = [
    { label: '全 00', value: 'zeros' },
    { label: '全 FF', value: 'ff' },
    { label: '随机字节', value: 'random_bytes' },
    { label: '随机字符', value: 'random_text' },
    { label: '递增', value: 'increment' },
    { label: '自定义循环', value: 'custom_loop' },
  ];
  const lenBasisOpts = [
    { label: '按数据域', value: 'data_field' },
    { label: '按整帧', value: 'whole_frame' },
  ];
  const algoOpts = [
    { label: '无', value: 'none' },
    { label: 'CRC16 Modbus', value: 'crc16_modbus' },
    { label: 'CRC16 CCITT-FALSE', value: 'crc16_ccitt_false' },
    { label: 'CRC16 XModem', value: 'crc16_xmodem' },
    { label: 'CRC16 ARC', value: 'crc16_arc' },
    { label: 'CRC32', value: 'crc32' },
    { label: 'SUM8', value: 'sum8' },
    { label: 'XOR8', value: 'xor8' },
  ];

  // CustomSelect 的 value 是 string,配置字段是字面量联合:用 get/set 函数绑定对双向转型
  // (同 ConnectionBar 波特率下拉的写法)。闭包每次现读 tool.config,loadTemplate 整体
  // 换 config 引用后依然生效。
  function bindSel<T extends string>(get: () => T, set: (v: T) => void) {
    return { get: () => get(), set: (v: string) => set(v as T) };
  }
  const lenSizeB = bindSel(
    () => tool.config.length.size,
    (v) => (tool.config.length.size = v),
  );
  const lenEndianB = bindSel(
    () => tool.config.length.endian,
    (v) => (tool.config.length.endian = v),
  );
  const coverageB = bindSel(
    () => tool.config.length.coverage,
    (v) => (tool.config.length.coverage = v),
  );
  const dataModeB = bindSel(
    () => tool.config.data.mode,
    (v) => (tool.config.data.mode = v),
  );
  const dataFormatB = bindSel(
    () => tool.config.data.format,
    (v) => (tool.config.data.format = v),
  );
  const patternB = bindSel(
    () => tool.config.data.pattern,
    (v) => (tool.config.data.pattern = v),
  );
  const lenBasisB = bindSel(
    () => tool.config.data.length_basis,
    (v) => (tool.config.data.length_basis = v),
  );
  const customFormatB = bindSel(
    () => tool.config.data.custom_format,
    (v) => (tool.config.data.custom_format = v),
  );
  const ckAlgoB = bindSel(
    () => tool.config.checksum.algo,
    (v) => (tool.config.checksum.algo = v),
  );
  const ckEndianB = bindSel(
    () => tool.config.checksum.endian,
    (v) => (tool.config.checksum.endian = v),
  );

  // ---- 实时预览:配置变化 debounce 200ms 调 build_frame,带序号丢过期响应 ----
  let built = $state<FrameResult | null>(null);
  let buildErr = $state<{ field: string | null; message: string } | null>(null);
  let sending = $state(false);
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function scheduleBuild() {
    if (timer) clearTimeout(timer);
    timer = setTimeout(doBuild, 200);
  }
  /** 后端 Err(FrameBuildError) 经 Tauri 序列化后 catch 到的 e 可能是 JSON 字符串
   *  ("{\"field\":\"length\",\"message\":\"…\"}"),也可能是对象或纯文本:统一归一。 */
  function normalizeBuildErr(e: unknown): { field: string | null; message: string } {
    const o = typeof e === 'string' ? tryJsonParse(e) : e;
    if (o && typeof o === 'object' && typeof (o as any).message === 'string') {
      const f = (o as any).field;
      return { field: typeof f === 'string' ? f : null, message: (o as any).message };
    }
    return { field: null, message: String(e) };
  }
  function tryJsonParse(s: string): unknown {
    try { return JSON.parse(s); } catch { return null; }
  }
  async function doBuild() {
    const mySeq = ++seq;
    try {
      const r = await buildFrame(tool.config);
      if (mySeq !== seq) return; // 过期响应丢弃
      built = r;
      buildErr = null;
    } catch (e) {
      if (mySeq !== seq) return;
      built = null;
      buildErr = normalizeBuildErr(e);
    }
  }
  // 配置任一字段变化都触发(深度):JSON 快照比对。cleanup 顺手清 timer,防卸载后仍触发。
  let lastJson = '';
  $effect(() => {
    const j = JSON.stringify(tool.config);
    if (j !== lastJson) {
      lastJson = j;
      scheduleBuild();
    }
    return () => {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
    };
  });

  function fieldErr(name: string) {
    return buildErr?.field === name;
  }

  /** 预览分段:先按 breakdown 切段,再整体裁到首 64 字节,末段按截断边界收敛。
   *  在模板里现算容易越界,统一在这里算成纯数据。 */
  const PREVIEW_BYTES = 64;
  const preview = $derived.by(() => {
    const r = built; // 闭包里直接用 built 会丢 null 收窄,先落局部量
    if (!r) return null;
    const totalBytes = r.total_len;
    const shown = Math.min(totalBytes, PREVIEW_BYTES);
    const clip = (s: { kind: string; offset: number; len: number }) => {
      const a = Math.max(0, Math.min(s.offset, shown));
      const b = Math.max(a, Math.min(s.offset + s.len, shown));
      return b > a ? { kind: s.kind, text: r.hex.slice(3 * a, 3 * b - 1), bytes: b - a } : null;
    };
    return {
      spans: r.breakdown.map(clip).filter((x): x is NonNullable<typeof x> => x !== null),
      truncated: totalBytes > shown,
      totalBytes,
    };
  });

  // 分段四色:头/长度/数据/校验。title 供悬停核对每段字节数。
  const KIND_NAME: Record<string, string> = { header: '帧头', length: '长度', data: '数据', checksum: '校验' };
  const KIND_COLOR: Record<string, string> = {
    header: 'var(--primary)',
    length: 'var(--warning)',
    data: 'var(--foreground)',
    checksum: 'var(--rx)',
  };

  /** 未连接 / 构帧中 / 构帧出错 → 发送与填入都禁用(spec:错误时禁用发送/填入) */
  const canUseFrame = $derived(!sending && !!built && !buildErr);

  // ---- 发送:发预览那一帧(所见即所发),hex 模式 + 行尾 None,发完重构刷新预览 ----
  async function handleSend() {
    if (!canUseFrame || !connected.value) return;
    sending = true;
    try {
      // 不给 windowPort 加空值守卫:main 窗口恒为 null,后端 resolve_port(None) 恰好
      // 解析到那条唯一连接(同 QuickCommands 的 sendOne 写法)。
      await send(windowPort.value!, built!.hex, 'None', true);
      await doBuild(); // 随机模式重构,预览刷新
    } catch (e) {
      console.error('发送失败:', e);
    } finally {
      sending = false;
    }
  }

  // ---- 填入发送框:必须同时切 hex 模式,否则用户回车把 hex 字符串当文本发 ----
  function handleFill() {
    if (!canUseFrame) return;
    hexSend.value = true;
    requestSuggestFill(built!.hex);
  }

  // ---- 模板:存为模板弹小浮层输名;同名确认按钮变「覆盖」;单击载入;悬停显删除 ----
  let saveOpen = $state(false);
  let saveName = $state('');
  const nameExists = $derived(saveName.trim() !== '' && tool.templates.some((t) => t.name === saveName.trim()));
  function openSave() {
    saveName = '';
    saveOpen = !saveOpen;
  }
  function cancelSave() {
    saveOpen = false;
    saveName = '';
  }
  function confirmSave() {
    const n = saveName.trim();
    if (!n) return;
    addTemplate(tool, n);
    cancelSave();
  }
</script>

<div class="flex h-full flex-col">
  <!-- 表单区:独立滚动,不挤压底部预览/操作条 -->
  <div class="flex-1 overflow-y-auto px-4 py-3 space-y-4 text-[13px]">
    <!-- 帧头 -->
    <section class="space-y-1.5">
      <div class="text-[12px] font-medium text-[var(--muted-foreground)]">帧头</div>
      <div class="flex items-center gap-2">
        <input
          class="flex-1 rounded border bg-[var(--background-input)] px-2 py-1.5 font-mono text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
          style="border-color: {fieldErr('header') ? 'var(--error)' : 'var(--border)'}; color: var(--foreground);"
          bind:value={tool.config.header_hex}
          placeholder="十六进制,如 AA 55;留空则无帧头"
        />
      </div>
    </section>

    <!-- 长度 -->
    <section class="space-y-1.5">
      <div class="text-[12px] font-medium text-[var(--muted-foreground)]">长度域</div>
      <div class="flex items-center gap-2">
        <CustomSelect bind:value={lenSizeB.get, lenSizeB.set} options={lenSizeOpts} width="90px" />
        <CustomSelect
          bind:value={lenEndianB.get, lenEndianB.set}
          options={endianOpts}
          width="110px"
          disabled={tool.config.length.size === 'none' || tool.config.length.size === 'one'}
        />
        <CustomSelect bind:value={coverageB.get, coverageB.set} options={coverageOpts} width="130px" />
      </div>
    </section>

    <!-- 数据域 -->
    <section class="space-y-1.5">
      <div class="text-[12px] font-medium text-[var(--muted-foreground)]">数据域</div>
      <div class="flex items-center gap-2">
        <CustomSelect bind:value={dataModeB.get, dataModeB.set} options={dataModeOpts} width="90px" />
        {#if tool.config.data.mode === 'content'}
          <CustomSelect bind:value={dataFormatB.get, dataFormatB.set} options={formatOpts} width="76px" />
          <input
            class="flex-1 rounded border bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)] {tool.config.data.format === 'hex' ? 'font-mono' : ''}"
            style="border-color: {fieldErr('data') ? 'var(--error)' : 'var(--border)'}; color: var(--foreground);"
            bind:value={tool.config.data.value}
            placeholder={tool.config.data.format === 'hex' ? '十六进制,如 61 62' : '文本内容'}
          />
        {:else}
          <CustomSelect bind:value={patternB.get, patternB.set} options={patternOpts} width="110px" />
          {#if tool.config.data.pattern === 'random_text'}
            <input
              class="w-36 rounded border bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
              style="border-color: {fieldErr('charset') ? 'var(--error)' : 'var(--border)'}; color: var(--foreground);"
              bind:value={tool.config.data.charset}
              title="随机字符集(ASCII)"
              placeholder="字符集"
            />
          {:else if tool.config.data.pattern === 'increment'}
            <!-- 数字字段用原生 number 输入(同快捷指令 Delay 列):值是 number 不是 string -->
            <input
              type="number"
              min="0"
              max="255"
              class="tnum w-20 rounded border bg-[var(--background-input)] px-2 py-1.5 text-center text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
              style="border-color: var(--border); color: var(--foreground);"
              bind:value={tool.config.data.start}
              title="递增起始值(0-255)"
            />
          {:else if tool.config.data.pattern === 'custom_loop'}
            <CustomSelect bind:value={customFormatB.get, customFormatB.set} options={formatOpts} width="76px" />
            <input
              class="flex-1 rounded border bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)] {tool.config.data.custom_format === 'hex' ? 'font-mono' : ''}"
              style="border-color: {fieldErr('custom') ? 'var(--error)' : 'var(--border)'}; color: var(--foreground);"
              bind:value={tool.config.data.custom_value}
              placeholder={tool.config.data.custom_format === 'hex' ? '循环内容 hex,如 00 FF' : '循环内容文本'}
            />
          {/if}
          <!-- 填充长度 + 口径:只在 fill 模式显示 -->
          <input
            type="number"
            min="0"
            class="tnum w-24 rounded border bg-[var(--background-input)] px-2 py-1.5 text-center text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
            style="border-color: {fieldErr('length') ? 'var(--error)' : 'var(--border)'}; color: var(--foreground);"
            bind:value={tool.config.data.length}
            title="填充字节数"
          />
          <CustomSelect bind:value={lenBasisB.get, lenBasisB.set} options={lenBasisOpts} width="100px" />
        {/if}
      </div>
    </section>

    <!-- 校验 -->
    <section class="space-y-1.5">
      <div class="text-[12px] font-medium text-[var(--muted-foreground)]">校验</div>
      <div class="flex items-center gap-3">
        <CustomSelect bind:value={ckAlgoB.get, ckAlgoB.set} options={algoOpts} width="170px" />
        <CustomSelect
          bind:value={ckEndianB.get, ckEndianB.set}
          options={endianOpts}
          width="110px"
          disabled={tool.config.checksum.algo === 'none'}
        />
        <!-- algo 为 none 时开关置灰:值保留、语义忽略,切回算法后原勾选还在 -->
        <label class="flex items-center gap-1.5 text-[13px] {tool.config.checksum.algo === 'none' ? 'text-[var(--muted-foreground)] opacity-50' : 'text-[var(--foreground)] cursor-pointer select-none'}">
          <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" bind:checked={tool.config.checksum.include_header} disabled={tool.config.checksum.algo === 'none'} />
          含帧头
        </label>
        <label class="flex items-center gap-1.5 text-[13px] {tool.config.checksum.algo === 'none' ? 'text-[var(--muted-foreground)] opacity-50' : 'text-[var(--foreground)] cursor-pointer select-none'}">
          <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" bind:checked={tool.config.checksum.include_length} disabled={tool.config.checksum.algo === 'none'} />
          含长度
        </label>
      </div>
    </section>
  </div>

  <!-- 预览区:只渲染算好的 preview,不在模板里现算切片(>64 字节时末段会越界) -->
  <div class="border-t border-[var(--border)] px-4 py-2.5" style="background: var(--background-deep);">
    <div class="mb-1 flex items-center gap-2">
      <span class="text-[12px] text-[var(--muted-foreground)]">实时预览</span>
      {#if sending}<span class="text-[12px] text-[var(--muted-foreground)]">发送中…</span>{/if}
    </div>
    {#if buildErr}
      <div class="text-[12px] text-[var(--error)]">{buildErr.message}</div>
    {:else if preview}
      <div class="break-all font-mono text-[13px] leading-relaxed">
        {#each preview.spans as s, i (i)}
          <span title="{KIND_NAME[s.kind] ?? s.kind} {s.bytes} 字节" style="color: {KIND_COLOR[s.kind] ?? 'var(--foreground)'}">{s.text}</span>{' '}
        {/each}
        {#if preview.truncated}
          <span class="text-[var(--muted-foreground)]">… 共 {preview.totalBytes} 字节</span>
        {/if}
      </div>
    {:else}
      <div class="text-[12px] text-[var(--muted-foreground)]">构帧中…</div>
    {/if}
  </div>

  <!-- 操作条:发送 / 填入 / 存模板 / 模板列表 -->
  <div class="flex items-center gap-2 border-t border-[var(--border)] px-3 py-2" style="background: var(--background-elevated);">
    <button
      class="btn btn-primary h-8 leading-none"
      disabled={!canUseFrame || !connected.value || sending}
      title={!connected.value ? '未连接串口' : '发送预览中的帧'}
      onclick={handleSend}
    >发送</button>
    <button
      class="h-8 px-3 rounded text-[12px] font-medium border border-[var(--border-strong)] bg-[var(--background-input)] text-[var(--foreground-secondary)] hover:text-[var(--foreground)] hover:border-[var(--primary)] transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:pointer-events-none"
      disabled={!canUseFrame}
      title="把整帧 hex 填入底部发送框(自动切 HEX 模式)"
      onclick={handleFill}
    >填入发送框</button>
    <div class="relative">
      <button
        class="h-8 px-3 rounded text-[12px] font-medium border border-[var(--border-strong)] bg-[var(--background-input)] text-[var(--foreground-secondary)] hover:text-[var(--foreground)] hover:border-[var(--primary)] transition-colors"
        onclick={openSave}
      >存为模板</button>
      {#if saveOpen}
        <div
          class="absolute bottom-full right-0 z-[300] mb-1.5 w-56 rounded-md border p-2.5 shadow-lg"
          style="background: var(--background-elevated); border-color: var(--border);"
        >
          <input
            class="w-full rounded border bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
            style="border-color: var(--border); color: var(--foreground);"
            bind:value={saveName}
            placeholder="模板名称"
            onkeydown={(e) => {
              if (e.key === 'Enter') confirmSave();
              if (e.key === 'Escape') cancelSave();
            }}
          />
          {#if nameExists}
            <div class="mt-1 text-[11px] text-[var(--warning)]">同名模板将被覆盖</div>
          {/if}
          <div class="mt-2 flex justify-end gap-1.5">
            <button class="btn btn-ghost" style="padding: 4px 10px;" onclick={cancelSave}>取消</button>
            <button
              class="btn btn-primary cursor-pointer"
              style="padding: 4px 10px;"
              disabled={!saveName.trim()}
              onclick={confirmSave}
            >{nameExists ? '覆盖' : '保存'}</button>
          </div>
        </div>
      {/if}
    </div>
    <!-- 模板列表:单击载入,悬停右上角显删除 -->
    <div class="flex min-w-0 flex-1 items-center justify-end gap-1.5 overflow-x-auto">
      {#if tool.templates.length === 0}
        <span class="text-[12px] text-[var(--muted-foreground)]">暂无模板</span>
      {/if}
      {#each tool.templates as t (t.name)}
        <div class="group relative shrink-0">
          <button
            class="max-w-32 truncate rounded border border-[var(--border)] bg-[var(--border-subtle)] px-2 py-1 text-[12px] text-[var(--foreground)] transition-colors hover:border-[var(--primary)] hover:text-[var(--primary)] cursor-pointer"
            title="载入模板「{t.name}」"
            onclick={() => loadTemplate(tool, t.name)}
          >{t.name}</button>
          <button
            class="absolute -right-1.5 -top-1.5 hidden h-4 w-4 items-center justify-center rounded-full border text-[10px] leading-none text-[var(--error)] group-hover:flex cursor-pointer"
            style="background: var(--background-elevated); border-color: var(--error);"
            title="删除模板「{t.name}」"
            onclick={() => removeTemplate(tool, t.name)}
          >×</button>
        </div>
      {/each}
    </div>
  </div>
</div>
