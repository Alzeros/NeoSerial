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
    normalizeFrameConfig,
    defaultRandomTextSpec,
    type FrameBuilderTool,
    type DataSpec,
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
  const formatOpts = [
    { label: 'Hex', value: 'hex' },
    { label: '文本', value: 'text' },
  ];
  const sourceOpts = [
    { label: '随机字符', value: 'random_text' },
    { label: '随机字节', value: 'random_bytes' },
    { label: '手动输入', value: 'content' },
    { label: '全 00', value: 'zeros' },
    { label: '全 FF', value: 'ff' },
    { label: '递增', value: 'increment' },
    { label: '自定义循环', value: 'custom_loop' },
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

  // 兼容当前内存中的旧配置和后续模板切换，迁移函数可重复调用。
  $effect(() => {
    normalizeFrameConfig(tool.config);
  });
  const rt = $derived(tool.config.data.random_text ?? defaultRandomTextSpec());
  let characterSettingsOpen = $state(false);
  const isRandomText = $derived(tool.config.data.mode === 'fill' && tool.config.data.pattern === 'random_text');

  // 四段展开/收起状态:不勾 = 该段不参与构帧(数据域必勾,恒展开)
  // 初始状态从配置推导:header_hex 非空 → 帧头开;length.size !== 'none' → 长度开;checksum.algo !== 'none' → 校验开
  const secOpen = $state({
    header: false,
    length: false,
    checksum: false,
  });
  // 顶部开关除了收起 UI,也必须把该段配置清零,否则"收起"等价于"还在用上次残留值"
  $effect(() => {
    secOpen.header = tool.config.header_hex !== '';
    secOpen.length = tool.config.length.size !== 'none';
    secOpen.checksum = tool.config.checksum.algo !== 'none';
  });
  function secSet(name: 'header' | 'length' | 'checksum', on: boolean) {
    secOpen[name] = on;
    if (!on) {
      if (name === 'header') tool.config.header_hex = '';
      else if (name === 'length') tool.config.length.size = 'none';
      else tool.config.checksum.algo = 'none';
    }
  }

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
  const sourceB = bindSel<'content' | DataSpec['pattern']>(
    () => tool.config.data.mode === 'content' ? 'content' : tool.config.data.pattern,
    (v) => {
      if (v === 'content') tool.config.data.mode = 'content';
      else {
        tool.config.data.mode = 'fill';
        tool.config.data.pattern = v;
        tool.config.data.length_basis = 'data_field';
      }
    },
  );
  const dataFormatB = bindSel(
    () => tool.config.data.format,
    (v) => (tool.config.data.format = v),
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

  // ---- 按键生成:点「生成」才调 build_frame;配置变更只打过期标,不自动重构 ----
  let built = $state<FrameResult | null>(null);
  let buildErr = $state<{ field: string | null; message: string } | null>(null);
  let sending = $state(false);
  let building = $state(false);
  let seq = 0;
  // 生成成功时的配置快照;之后配置一变,快照对不上即旧预览「已过期」
  let builtJson = $state('');
  const isStale = $derived(built !== null && builtJson !== JSON.stringify(tool.config));
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
    building = true;
    try {
      const r = await buildFrame(tool.config);
      if (mySeq !== seq) return; // 过期响应丢弃
      built = r;
      expanded = false;
      builtJson = JSON.stringify(tool.config);
      buildErr = null;
    } catch (e) {
      if (mySeq !== seq) return;
      // 保留上一次成功结果并标过期,而非直接清空,避免报错时预览闪没
      if (built) builtJson = '';
      else built = null;
      buildErr = normalizeBuildErr(e);
    } finally {
      if (mySeq === seq) building = false;
    }
  }

  function fieldErr(name: string) {
    return buildErr?.field === name;
  }

  /** 预览分段:先按 breakdown 切段;收起时裁到首 64 字节,展开显全帧,末段按截断边界收敛。
   *  在模板里现算容易越界,统一在这里算成纯数据。收起/展开状态驱动截断阈值。 */
  // 展开显全帧;收起只显前 64 字节。生成新帧时复位为收起。
  const COLLAPSED_BYTES = 64;
  let expanded = $state(false);
  const preview = $derived.by(() => {
    const r = built; // 闭包里直接用 built 会丢 null 收窄,先落局部量
    if (!r) return null;
    const totalBytes = r.total_len;
    const shown = expanded ? totalBytes : Math.min(totalBytes, COLLAPSED_BYTES);
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

  // 预览模式:hex 显原始字节流;text 把数据域段按 ASCII 回显(不可显字符显 ·)
  // 默认跟随数据域模式:文本内容/随机字符 → text,其他 → hex
  let previewMode = $state<'hex' | 'text'>('hex');
  $effect(() => {
    const d = tool.config.data;
    const isTexty = (d.mode === 'content' && d.format === 'text') || (d.mode === 'fill' && d.pattern === 'random_text');
    previewMode = isTexty ? 'text' : 'hex';
  });

  $effect(() => {
    if (!expanded) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') expanded = false;
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  /** 把 hex 片段转 ASCII 回显:可显字符(0x20-0x7E)原样,其余显 · */
  function hexToAscii(hex: string): string {
    return hex.split(' ').map((h) => {
      const b = parseInt(h, 16);
      return b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '·';
    }).join('');
  }

  /** 未连接 / 构帧中 / 构帧出错 / 预览已过期 → 发送与填入都禁用 */
  const canUseFrame = $derived(!sending && !building && !!built && !buildErr && !isStale);

  // ---- 发送:发预览那一帧(所见即所发),hex 模式 + 行尾 None ----
  async function handleSend() {
    if (!canUseFrame || !connected.value) return;
    sending = true;
    try {
      // 不给 windowPort 加空值守卫:main 窗口恒为 null,后端 resolve_port(None) 恰好
      // 解析到那条唯一连接(同 QuickCommands 的 sendOne 写法)。
      await send(windowPort.value!, built!.hex, 'None', true);
    } catch (e) {
      console.error('发送失败:', e);
    } finally {
      sending = false;
    }
  }

  /** 整帧可无损转文本才返回字符串:任一字节不可显(帧头/长度/校验等二进制段)就返回 null */
  function frameAsText(hex: string): string | null {
    const bytes = hex.split(' ').filter(Boolean);
    if (bytes.length === 0) return null;
    const chars: string[] = [];
    for (const h of bytes) {
      const b = parseInt(h, 16);
      if (!(b >= 0x20 && b <= 0x7e)) return null;
      chars.push(String.fromCharCode(b));
    }
    return chars.join('');
  }

  // ---- 填入发送框:整帧全可显 ASCII 就填文本(关 HEX 开关),否则填 hex 并开 HEX 开关 ----
  // 两条路都得显式设 hexSend:否则用户回车会把 hex 串当文本发、或把文本当 hex 解析。
  function handleFill() {
    if (!canUseFrame) return;
    const asText = frameAsText(built!.hex);
    if (asText !== null) {
      hexSend.value = false;
      requestSuggestFill(asText);
    } else {
      hexSend.value = true;
      requestSuggestFill(built!.hex);
    }
  }

  // ---- 复制整帧 hex:项目通用 navigator.clipboard 写法,复制成功 1.5s 显「已复制」 ----
  let copied = $state(false);
  async function copyHex() {
    if (!built || isStale) return;
    try {
      await navigator.clipboard.writeText(built.hex);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // 剪贴板不可用时静默(与 SettingsDialog 一致)
    }
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

<div class="relative flex h-full min-h-0 flex-1 flex-col">
  <!-- 段开关:复选框控制四段展开/收起;不勾 = 该段不参与构帧(数据域必勾) -->
  <div class="grid grid-cols-4 border-b border-[var(--border)] text-[13px]" style="background: var(--background-elevated);">
    <label class="flex items-center justify-center gap-1.5 py-2 cursor-pointer select-none border-r border-[var(--border)] {secOpen.header ? 'text-[var(--foreground)]' : 'text-[var(--muted-foreground)]'}">
      <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" checked={secOpen.header} onchange={(e) => secSet('header', (e.target as HTMLInputElement).checked)} />
      帧头
    </label>
    <label class="flex items-center justify-center gap-1.5 py-2 cursor-pointer select-none border-r border-[var(--border)] {secOpen.length ? 'text-[var(--foreground)]' : 'text-[var(--muted-foreground)]'}">
      <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" checked={secOpen.length} onchange={(e) => secSet('length', (e.target as HTMLInputElement).checked)} />
      长度
    </label>
    <label class="flex items-center justify-center gap-1.5 py-2 cursor-not-allowed select-none border-r border-[var(--border)] text-[var(--foreground)] opacity-50">
      <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" checked disabled />
      数据
    </label>
    <label class="flex items-center justify-center gap-1.5 py-2 cursor-pointer select-none {secOpen.checksum ? 'text-[var(--foreground)]' : 'text-[var(--muted-foreground)]'}">
      <input type="checkbox" class="h-3.5 w-3.5 rounded accent-[var(--primary)]" checked={secOpen.checksum} onchange={(e) => secSet('checksum', (e.target as HTMLInputElement).checked)} />
      校验
    </label>
  </div>

  <!-- 表单区:独立滚动,不挤压底部预览/操作条 -->
  <div class="flex-1 min-h-0 overflow-y-auto px-4 py-3 space-y-3 text-[13px] {expanded && preview ? 'hidden' : ''}">
  <!-- 展开全部 -->

    {#if secOpen.header}
      <section class="rounded-md border border-[var(--border)] bg-[var(--background-elevated)] p-3 space-y-2">
        <div class="text-[12px] font-medium text-[var(--muted-foreground)]">帧头</div>
        <div class="flex items-center gap-2">
          <input
            class="h-8 flex-1 min-w-0 rounded border bg-[var(--background-input)] px-2 font-mono text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
            style="border-color: {fieldErr('header') ? 'var(--error)' : 'var(--border)'}; color: var(--foreground);"
            bind:value={tool.config.header_hex}
            placeholder="十六进制,如 AA 55;留空则无帧头"
          />
        </div>
      </section>
    {/if}

    {#if secOpen.length}
      <section class="rounded-md border border-[var(--border)] bg-[var(--background-elevated)] p-3 space-y-2">
        <div class="text-[12px] font-medium text-[var(--muted-foreground)]">长度域</div>
        <div class="flex flex-wrap items-center gap-2">
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
    {/if}

    <!-- 常用操作固定两列:左边选生成方式,右边填数据域长度。 -->
    <section class="data-card rounded-md border border-[var(--border)] bg-[var(--background-elevated)] p-3">
      <div class="mb-3 flex h-6 items-center justify-between">
        <span class="text-[13px] font-medium text-[var(--foreground)]">数据域</span>
        {#if isRandomText}
          <button
            type="button"
            class="flex items-center gap-1 rounded px-1.5 py-1 text-[12px] text-[var(--primary)] hover:bg-[var(--overlay-hover)] cursor-pointer"
            aria-expanded={characterSettingsOpen}
            onclick={() => (characterSettingsOpen = !characterSettingsOpen)}
          >
            <svg class="transition-transform {characterSettingsOpen ? 'rotate-180' : ''}" width="9" height="6" viewBox="0 0 10 6" fill="none" aria-hidden="true">
              <path d="M1 1l4 4 4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
            字符设置
          </button>
        {/if}
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="min-w-0 space-y-1.5">
          <div class="text-[12px] text-[var(--muted-foreground)]">生成方式</div>
          <CustomSelect bind:value={sourceB.get, sourceB.set} options={sourceOpts} width="100%" />
        </div>
        {#if tool.config.data.mode === 'fill'}
          <label class="min-w-0 space-y-1.5">
            <span class="block text-[12px] text-[var(--muted-foreground)]">数据长度</span>
            <div class="relative">
              <input type="number" min="0" max="65536" step="1" class="data-input pr-12" style:border-color={fieldErr('length') ? 'var(--error)' : undefined} bind:value={tool.config.data.length} oninput={() => (tool.config.data.length_basis = 'data_field')} aria-label="数据长度（字节）" />
              <span class="pointer-events-none absolute right-3 top-0 flex h-8 items-center text-[12px] text-[var(--muted-foreground)]">字节</span>
            </div>
          </label>
        {:else}
          <div class="min-w-0 space-y-1.5">
            <div class="text-[12px] text-[var(--muted-foreground)]">内容格式</div>
            <CustomSelect bind:value={dataFormatB.get, dataFormatB.set} options={formatOpts} width="100%" />
          </div>
        {/if}
      </div>
      {#if isRandomText && characterSettingsOpen}
        <div class="mt-3 border-t border-[var(--border)] pt-3">
          <div class="mb-2 text-[12px] text-[var(--muted-foreground)]">包含字符</div>
          <div class="grid grid-cols-2 gap-x-3 gap-y-2 text-[13px]">
            <label class="flex items-center gap-2 cursor-pointer"><input type="checkbox" class="accent-[var(--primary)]" bind:checked={rt.upper} />大写 A–Z</label>
            <label class="flex items-center gap-2 cursor-pointer"><input type="checkbox" class="accent-[var(--primary)]" bind:checked={rt.lower} />小写 a–z</label>
            <label class="flex items-center gap-2 cursor-pointer"><input type="checkbox" class="accent-[var(--primary)]" bind:checked={rt.digits} />数字 0–9</label>
            <label class="flex items-center gap-2 cursor-pointer"><input type="checkbox" class="accent-[var(--primary)]" bind:checked={rt.special} />特殊字符</label>
          </div>
          <div class="mt-3 grid grid-cols-2 gap-3">
            <label class="min-w-0 space-y-1.5">
              <span class="block text-[12px] text-[var(--muted-foreground)]">特殊字符集合</span>
              <input class="data-input font-mono" bind:value={rt.special_chars} disabled={!rt.special} placeholder="!@#$%^&*" style:border-color={fieldErr('charset') ? 'var(--error)' : undefined} />
            </label>
            <div class="grid grid-cols-2 gap-2">
              <label class="min-w-0 space-y-1.5">
                <span class="block truncate text-[12px] text-[var(--muted-foreground)]">数字最少</span>
                <input type="number" min="0" step="1" class="data-input tnum" bind:value={rt.min_digits} disabled={!rt.digits} />
              </label>
              <label class="min-w-0 space-y-1.5">
                <span class="block truncate text-[12px] text-[var(--muted-foreground)]">特殊最少</span>
                <input type="number" min="0" step="1" class="data-input tnum" bind:value={rt.min_special} disabled={!rt.special} />
              </label>
            </div>
          </div>
          {#if fieldErr('charset')}<p class="mt-2 text-[12px] text-[var(--error)]">{buildErr?.message}</p>{/if}
        </div>
      {/if}
      {#if tool.config.data.mode === 'content'}
        <label class="mt-3 block space-y-1.5">
          <span class="block text-[12px] text-[var(--muted-foreground)]">数据内容</span>
          <input class="data-input {tool.config.data.format === 'hex' ? 'font-mono' : ''}" style:border-color={fieldErr('data') ? 'var(--error)' : undefined} bind:value={tool.config.data.value} placeholder={tool.config.data.format === 'hex' ? '十六进制，如 61 62' : '输入文本内容'} />
        </label>
      {:else if tool.config.data.pattern === 'increment'}
        <label class="mt-3 block space-y-1.5">
          <span class="block text-[12px] text-[var(--muted-foreground)]">起始值（0–255）</span>
          <input type="number" min="0" max="255" step="1" class="data-input tnum" bind:value={tool.config.data.start} />
        </label>
      {:else if tool.config.data.pattern === 'custom_loop'}
        <div class="mt-3 grid grid-cols-[96px_minmax(0,1fr)] gap-3">
          <div class="space-y-1.5"><div class="text-[12px] text-[var(--muted-foreground)]">循环格式</div><CustomSelect bind:value={customFormatB.get, customFormatB.set} options={formatOpts} width="100%" /></div>
          <label class="min-w-0 space-y-1.5"><span class="block text-[12px] text-[var(--muted-foreground)]">循环内容</span><input class="data-input {tool.config.data.custom_format === 'hex' ? 'font-mono' : ''}" style:border-color={fieldErr('custom') ? 'var(--error)' : undefined} bind:value={tool.config.data.custom_value} placeholder={tool.config.data.custom_format === 'hex' ? '如 00 FF' : '输入循环文本'} /></label>
        </div>
      {/if}
    </section>

    {#if secOpen.checksum}
      <section class="rounded-md border border-[var(--border)] bg-[var(--background-elevated)] p-3 space-y-2 relative z-20">
        <div class="text-[12px] font-medium text-[var(--muted-foreground)]">校验</div>
        <div class="flex flex-wrap items-center gap-2">
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
    {/if}
  </div>

  <!-- 生成结果:只渲染算好的 preview,不在模板里现算切片(>64 字节时末段会越界) -->
  {#if expanded && preview}
    <div class="flex min-h-0 flex-1 flex-col border-t border-[var(--border)] px-4 py-2.5" style="background: var(--background-deep);">
      <div class="mb-1 flex items-center gap-2">
        <span class="text-[12px] text-[var(--muted-foreground)]">完整结果 · 共 {preview.totalBytes} 字节</span>
        <button class="ml-auto px-1.5 py-0.5 rounded text-[12px] text-[var(--muted-foreground)] hover:text-[var(--foreground)] cursor-pointer" onclick={() => (expanded = false)}>返回设置</button>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto break-all font-mono text-[13px] leading-relaxed {(isStale || buildErr) ? 'opacity-40' : ''}" title={(isStale || buildErr) ? '该帧不对应当前设置' : ''}>
        {#each preview.spans as s, i (i)}
          {@const displayText = previewMode === 'text' && s.kind === 'data' ? hexToAscii(s.text) : s.text}
          <span title="{KIND_NAME[s.kind] ?? s.kind} {s.bytes} 字节" style="color: {KIND_COLOR[s.kind] ?? 'var(--foreground)'}">{displayText}</span>{' '}
        {/each}
      </div>
    </div>
  {/if}

  <div class="border-t border-[var(--border)] px-4 py-2.5 relative z-0 {expanded && preview ? 'hidden' : ''}" style="background: var(--background-deep);">
    <div class="mb-1 flex items-center gap-2">
      <button
        class="btn btn-primary h-7 px-3 leading-none text-[12px]"
        disabled={building}
        title="按当前设置生成一帧(随机模式每次生成新样本)"
        onclick={() => doBuild()}
      >{building ? '生成中…' : '生成'}</button>
      {#if isStale}
        <span class="text-[12px] text-[var(--warning)]">已过期 · 配置已改,请重新生成</span>
      {:else if sending}
        <span class="text-[12px] text-[var(--muted-foreground)]">发送中…</span>
      {/if}
      <!-- Hex/文本 预览切换:文本模式把数据域段按 ASCII 回显,不可显字符显 · -->
      {#if built}
        <button
          class="ml-auto px-1.5 py-0.5 rounded cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed {copied ? 'text-[var(--primary)] font-medium' : 'text-[var(--muted-foreground)] hover:text-[var(--foreground)]'}"
          disabled={isStale}
          title={isStale ? '帧已过期,请重新生成后再复制' : '复制整帧 hex 到剪贴板'}
          onclick={copyHex}
        >{copied ? '已复制' : '复制'}</button>
      {/if}
      <div class="{built ? '' : 'ml-auto '}flex items-center gap-1 text-[12px]">
        <button
          class="px-1.5 py-0.5 rounded cursor-pointer {previewMode === 'hex' ? 'text-[var(--primary)] font-medium' : 'text-[var(--muted-foreground)] hover:text-[var(--foreground)]'}"
          onclick={() => (previewMode = 'hex')}
        >Hex</button>
        <button
          class="px-1.5 py-0.5 rounded cursor-pointer {previewMode === 'text' ? 'text-[var(--primary)] font-medium' : 'text-[var(--muted-foreground)] hover:text-[var(--foreground)]'}"
          onclick={() => (previewMode = 'text')}
        >文本</button>
      </div>
    </div>
    {#if buildErr}
      <div
        class="flex items-start gap-1.5 rounded border border-[var(--error)]/40 bg-[var(--error)]/10 px-2 py-1 text-[12px] text-[var(--error)]"
        title={buildErr.message}
      >
        <span class="shrink-0 leading-4">⚠</span>
        <span class="min-w-0 flex-1 truncate leading-4">{buildErr.message}</span>
      </div>
    {/if}
    {#if preview}
      <div class="break-all font-mono text-[13px] leading-relaxed max-h-[120px] overflow-y-auto {(isStale || buildErr) ? 'opacity-40' : ''}" title={(isStale || buildErr) ? '该帧不对应当前设置' : ''}>
        {#each preview.spans as s, i (i)}
          {@const displayText = previewMode === 'text' && s.kind === 'data' ? hexToAscii(s.text) : s.text}
          <span title="{KIND_NAME[s.kind] ?? s.kind} {s.bytes} 字节" style="color: {KIND_COLOR[s.kind] ?? 'var(--foreground)'}">{displayText}</span>{' '}
        {/each}
        {#if preview.truncated}
          <button class="text-[var(--primary)] hover:underline cursor-pointer" onclick={() => (expanded = true)}>… 展开全部(共 {preview.totalBytes} 字节)</button>
        {/if}
      </div>
    {:else if !buildErr}
      <div class="text-[12px] text-[var(--muted-foreground)]">调好设置后点「生成」,这里显示构好的帧</div>
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
    <!-- 模板列表:单击载入,悬停右上角显删除。
         overflow-x-auto 会把纵向也计算成 auto 并裁剪,删除按钮 × 向项框外溢出 6px 会被切掉,
         故留 6px padding 容纳溢出,再用负 margin 抵消,底栏高度不变。 -->
    <div class="flex min-w-0 flex-1 items-center justify-end gap-1.5 overflow-x-auto px-1.5 py-1.5 -mx-1.5 -my-1.5">
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

<style>
  .data-input {
    width: 100%;
    min-width: 0;
    height: 32px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--background-input);
    color: var(--foreground);
    padding: 0 12px;
    font-size: 13px;
    box-shadow: var(--shadow-sm);
  }
  .data-input.pr-12 { padding-right: 48px; }
  .data-input:focus-visible { outline: none; border-color: var(--primary); }
  .data-input:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
