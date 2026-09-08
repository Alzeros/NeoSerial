<script lang="ts">
  import { getVersion } from '@tauri-apps/api/app';
  import { X } from 'lucide-svelte';
  import { presetBaudRates, cachedSettings, theme, themeMeta, customTheme, applyTheme, logFontSize, logLineHeight, applyLogFont, logDirLabelStyle, textEncoding, logFontLatin, logFontLatinPresets, logFontCJK, logFontCJKPresets } from '$lib/stores';
  import { defaultCustomTheme } from '$lib/customTheme';
  import { patchSettings, getMcpStatus, openUrl, openThemeEditor, exitApp, commandIndexRefresh, commandIndexRefreshDoc, commandIndexTestConnection, sendHistoryClear } from '$lib/tauri';
  import { commandIndex } from '$lib/commandIndex';
  import { defaultSuggestLimits } from '$lib/suggest';
  import { Github, Eye, EyeOff, Plug, Loader2, RefreshCw, ChevronDown, ChevronRight } from 'lucide-svelte';
  import UpdaterCard from '$components/UpdaterCard.svelte';
  import Collapsible from '$components/ui/Collapsible.svelte';
  import type { ManualDocument, Settings, SettingsPatch } from '$lib/types';
  // 应用图标：从 src/assets 引入，Vite 自动处理打包（src-tauri/icons 在 watch ignored 中，无法直接 import）
  import appIcon from '$assets/icon.png';

  let open = $state(false);

  // 左侧导航：当前激活的设置项
  type Section = 'about' | 'general' | 'appearance' | 'extensions';
  let activeSection = $state<Section>('about');
  const sections: { key: Section; label: string }[] = [
    { key: 'about', label: '关于' },
    { key: 'general', label: '通用' },
    { key: 'appearance', label: '外观' },
    { key: 'extensions', label: '扩展' },
  ];
  // 应用版本号（打开关于页时懒加载）
  let version = $state<{ value: string }>({ value: '' });

  // 主题编辑副本（打开时从 store 拷贝，取消不应用；选中即时预览）
  let editTheme = $state<string>('preset-1');
  // 自定义主题色板编辑副本（14 色 + 圆角），改动即时预览，保存才落 store
  let editCustom = $state<Record<string, string>>(defaultCustomTheme());
  // 日志字体编辑副本
  let editFontSize = $state(14);
  let editLineHeight = $state(1.6);
  let editDirLabel = $state<'short' | 'full'>('short');
  // 字体族编辑副本：'default' 或 CSS font-family 值（英文/中文分别设置）
  let editFontLatin = $state<string>('default');
  let editFontCJK = $state<string>('default');
  // 文本模式编码编辑副本（ASCII/UTF-8/GBK）
  let editTextEncoding = $state<'ascii' | 'utf8' | 'gbk'>('ascii');
  // 打开前的原值（取消时恢复，因部分预览直接改了 store）
  let origDirLabel: 'short' | 'full' = 'short';
  let origTextEncoding: 'ascii' | 'utf8' | 'gbk' = 'ascii';
  let origFontLatin: string = 'default';
  let origFontCJK: string = 'default';

  // 内置默认波特率：不可删除，只能在此基础上增删用户自定义项
  const DEFAULT_BAUD_RATES = [9600, 115200, 921600];
  const isDefault = (n: number) => DEFAULT_BAUD_RATES.includes(n);

  // 预设波特率编辑副本（打开时从 store 拷贝，取消不污染 store）
  let editBaudRates = $state<number[]>([]);
  let newBaud = $state('');
  // MCP 自动启动编辑副本（从 cachedSettings 拷贝；改后重启生效）
  let editMcpAutoStart = $state(true);
  // MCP 端口编辑副本（默认 34594;被占自动递增。改后需重新 claude mcp add）
  let editMcpPort = $state(34594);
  // "后台运行"编辑副本（从 cachedSettings 拷贝;兜底值与后端 default_background_mode 一致=关）
  let editBackgroundMode = $state(false);
  // 各模块的侧栏 tab 显隐(每模块独立设置项)。默认开(保现状)
  let editShowSuggestTab = $state(true);
  let editShowMcpTab = $state(true);
  // 快捷指令编辑区密度编辑副本(字号 + 输入框高度 + 行间距 + 字体族)
  let editQcFontSize = $state(13);
  let editQcInputHeight = $state(28);
  let editQcRowGap = $state(4);
  let editQcFontFamily = $state<string>('default');
  // 扩展页子导航:null=模块列表(文件夹视图),'suggest'/'mcp'=进入该模块设置子页
  let extModule = $state<'suggest' | 'mcp' | 'quick' | null>(null);
  // MCP server 当前运行状态（打开设置页/切到 MCP 页时拉取,显示实际端口）
  let mcpStatus = $state<{ running: boolean; port: number | null }>({ running: false, port: null });
  let mcpCopied = $state(false);
  // 指令联想编辑副本(从 cachedSettings.command_index 拷贝;保存后立即生效,不需重启)
  let editSuggestEnabled = $state(true);
  let editKbBaseUrl = $state('');
  let editKbApiKey = $state('');
  let editKbAutoRefresh = $state(true);
  let editDisabledDocIds = $state<number[]>([]);
  // 联想行为:弹出门槛(是否忽略 AT 前缀 + 最少字符数)与两类候选各自的上限
  let editSuggestMinChars = $state(defaultSuggestLimits.minChars);
  let editSuggestIgnoreAtPrefix = $state(defaultSuggestLimits.ignoreAtPrefix);
  let editSuggestMaxManual = $state(defaultSuggestLimits.maxManual);
  let editSuggestMaxHistory = $state(defaultSuggestLimits.maxHistory);
  // 发送历史留存条数上限(与 suggest 的"联想里显示几条"不同,这个管总共留几条)。
  // 兜底值与后端 send_history::SEND_HISTORY_DEFAULT_MAX 一致
  const DEFAULT_HISTORY_LIMIT = 200;
  let editHistoryLimit = $state(DEFAULT_HISTORY_LIMIT);
  let showApiKey = $state(false);
  let indexRefreshing = $state(false);
  // 正在单本刷新的手册 id(null=没有)。后端 REFRESHING 只允许一个写入者,所以一次只转一行
  let indexRefreshingDocId = $state<number | null>(null);
  // 连通性测试中(地址行按钮);与刷新独立,测试结果也写进 indexRefreshMsg 复用状态行展示
  let indexTesting = $state(false);
  /** 每本手册在本地缓存里实际有多少条指令。手册列表报的 cmd_count 是服务器侧的数,
   *  单本刷新之后会出现"服务器说 13 条、本地一条没缓存"的行,显示本地数才看得出该刷哪本。
   *  documents 可能几十本、commands 上千条,算一次 Map 而不是每行 filter 一遍。 */
  const cachedCmdCounts = $derived.by(() => {
    const m = new Map<number, number>();
    for (const c of commandIndex.commands) m.set(c.document_id, (m.get(c.document_id) ?? 0) + 1);
    return m;
  });
  // 刷新结果:ok 全部成功 / warn 部分手册失败沿用旧缓存 / error 整体失败
  let indexRefreshMsg = $state<{ kind: 'ok' | 'warn' | 'error'; text: string } | null>(null);
  let historyCleared = $state(false);
  const canRefreshIndex = $derived(editKbBaseUrl.trim().length > 0 && editKbApiKey.trim().length > 0 && !indexRefreshing);
  // 编辑副本的结构化快照:脏检测(与上次加载/应用时的快照比)和构建补丁(只提交变了的字段)共用。
  // 只比编辑副本本身,不依赖 cachedSettings 的字段完整性(避免后端版本差异导致 JSON 永远不等、应用按钮常亮)。
  type EditValues = {
    fontSize: number; lineHeight: number; dirLabel: 'short' | 'full'; fontLatin: string; fontCJK: string;
    textEncoding: 'ascii' | 'utf8' | 'gbk';
    backgroundMode: boolean; showSuggestTab: boolean; showMcpTab: boolean;
    qcFontSize: number; qcInputHeight: number; qcRowGap: number; qcFontFamily: string;
    baudRates: number[]; theme: string; custom: Record<string, string>;
    mcpAutoStart: boolean; mcpPort: number;
    kbBaseUrl: string; kbApiKey: string; disabledDocIds: number[]; kbAutoRefresh: boolean; suggestEnabled: boolean;
    suggestMinChars: number; suggestIgnoreAtPrefix: boolean; suggestMaxManual: number; suggestMaxHistory: number;
    historyLimit: number;
  };
  function currentEdits(): EditValues {
    // 波特率:内置默认 3 项保留 + 用户自定义(去重、排序)
    const userAdded = editBaudRates.filter((b) => !isDefault(b));
    const baudRates = [...new Set([...DEFAULT_BAUD_RATES, ...userAdded])].sort((a, b) => a - b);
    return {
      fontSize: editFontSize, lineHeight: editLineHeight, dirLabel: editDirLabel, fontLatin: editFontLatin, fontCJK: editFontCJK,
      textEncoding: editTextEncoding,
      backgroundMode: editBackgroundMode, showSuggestTab: editShowSuggestTab, showMcpTab: editShowMcpTab,
      qcFontSize: editQcFontSize, qcInputHeight: editQcInputHeight, qcRowGap: editQcRowGap, qcFontFamily: editQcFontFamily,
      baudRates, theme: editTheme, custom: { ...editCustom },
      mcpAutoStart: editMcpAutoStart, mcpPort: editMcpPort,
      kbBaseUrl: editKbBaseUrl.trim(), kbApiKey: editKbApiKey.trim(), disabledDocIds: [...editDisabledDocIds],
      kbAutoRefresh: editKbAutoRefresh, suggestEnabled: editSuggestEnabled,
      suggestMinChars: editSuggestMinChars, suggestIgnoreAtPrefix: editSuggestIgnoreAtPrefix,
      suggestMaxManual: editSuggestMaxManual, suggestMaxHistory: editSuggestMaxHistory,
      historyLimit: editHistoryLimit,
    };
  }
  function editsSnapshot(): string {
    return JSON.stringify(currentEdits());
  }
  // 上次加载/应用时的编辑值:脏检测基准,也是补丁的比对基准
  let lastAppliedEdits = $state<EditValues | null>(null);
  const hasUnsavedChanges = $derived(lastAppliedEdits !== null && editsSnapshot() !== JSON.stringify(lastAppliedEdits));
  // 保存失败的原因(值无效、写盘失败),显示在按钮栏;下次应用/保存或重开对话框时清掉
  let saveError = $state<string | null>(null);

  export function show(section: Section = 'about', extMod: 'suggest' | 'mcp' | 'quick' | null = null) {
    editBaudRates = [...presetBaudRates.value];
    editTheme = theme.value;
    editCustom = { ...customTheme.value };
    editFontSize = logFontSize.value;
    editLineHeight = logLineHeight.value;
    editDirLabel = logDirLabelStyle.value;
    origDirLabel = logDirLabelStyle.value;
    editFontLatin = logFontLatin.value;
    editFontCJK = logFontCJK.value;
    origFontLatin = logFontLatin.value;
    origFontCJK = logFontCJK.value;
    editTextEncoding = textEncoding.value;
    origTextEncoding = textEncoding.value;
    newBaud = '';
    editMcpAutoStart = cachedSettings.value?.mcp?.auto_start ?? true;
    editMcpPort = cachedSettings.value?.mcp?.port ?? 34594;
    editBackgroundMode = cachedSettings.value?.ui?.background_mode ?? false;
    editShowSuggestTab = cachedSettings.value?.ui?.show_suggest_tab ?? true;
    editShowMcpTab = cachedSettings.value?.ui?.show_mcp_tab ?? false;
    editQcFontSize = cachedSettings.value?.ui?.qc_font_size ?? 13;
    editQcInputHeight = cachedSettings.value?.ui?.qc_input_height ?? 28;
    editQcRowGap = cachedSettings.value?.ui?.qc_row_gap ?? 4;
    editQcFontFamily = cachedSettings.value?.ui?.qc_font_family ?? 'default';
    mcpCopied = false;
    const ci = cachedSettings.value?.command_index;
    editSuggestEnabled = ci?.suggest_enabled ?? true;
    editKbBaseUrl = ci?.base_url ?? '';
    editKbApiKey = ci?.api_key ?? '';
    editKbAutoRefresh = ci?.auto_refresh ?? true;
    editDisabledDocIds = [...(ci?.disabled_doc_ids ?? [])];
    editSuggestMinChars = ci?.suggest_min_chars ?? defaultSuggestLimits.minChars;
    editSuggestIgnoreAtPrefix = ci?.suggest_ignore_at_prefix ?? defaultSuggestLimits.ignoreAtPrefix;
    editSuggestMaxManual = ci?.suggest_max_manual ?? defaultSuggestLimits.maxManual;
    editSuggestMaxHistory = ci?.suggest_max_history ?? defaultSuggestLimits.maxHistory;
    editHistoryLimit = ci?.history_limit ?? DEFAULT_HISTORY_LIMIT;
    showApiKey = false;
    indexRefreshMsg = null;
    historyCleared = false;
    saveError = null;
    // 指令联想子页的分节折叠:每次开窗重置。知识库没配全 → 展开知识库那节做引导
    const kbConfigured = editKbBaseUrl.trim().length > 0 && editKbApiKey.trim().length > 0;
    openKb = !kbConfigured;
    openManuals = kbConfigured;
    openBehavior = false;
    openHistory = false;
    collapsedGroups = [];
    // 记录"加载态快照",作为脏检测与补丁的基准:之后编辑副本变 ≠ 此快照 = 有未应用改动
    lastAppliedEdits = currentEdits();
    activeSection = section;
    // 外部触发可带子模块:指令查询面板的齿轮按钮 → 直接进指令联想子页
    extModule = extMod;
    // 拉取 MCP 运行状态(显示实际端口)
    getMcpStatus().then((s) => (mcpStatus = s)).catch(() => {});
    open = true;
    // 进入关于页时懒加载版本号（仅首次拉取，失败兜底）
    if (section === 'about' && !version.value) {
      getVersion()
        .then((v) => (version.value = v))
        .catch(() => (version.value = '0.3.3'));
    }
  }

  // 选中主题：即时预览（应用到 <html>），但不落盘；取消则恢复原值
  function selectTheme(value: string) {
    editTheme = value;
    applyTheme(value, editCustom);
  }

  // 打开独立主题编辑器窗口（单例，已存在则聚焦）
  async function openThemeEditorWindow() {
    // 先把当前选中的 custom 落到 store（编辑器窗口会读 settings 初始化）
    // 这样在弹窗里选了 custom 卡片后再开编辑器，编辑器看到的是最新色板
    editTheme = 'custom';
    // theme.value 必须同步为 custom：<html> 已切到 custom，若 store 仍记 preset-1，
    // 后续任何 applyTheme(theme.value)（取消设置、theme-changed 广播）都会跳回预设
    theme.value = 'custom';
    customTheme.value = { ...editCustom };
    applyTheme('custom', editCustom);
    // 关闭设置弹窗，避免两层叠加
    open = false;
    try {
      await openThemeEditor();
    } catch (e) {
      console.error('打开主题编辑器失败:', e);
    }
  }

  // 字号/行高：即时预览（带上当前英文/中文字体）
  function changeFontSize(v: number) {
    editFontSize = v;
    applyLogFont(v, editLineHeight, editFontLatin, editFontCJK);
  }
  function changeLineHeight(v: number) {
    editLineHeight = v;
    applyLogFont(editFontSize, v, editFontLatin, editFontCJK);
  }
  // 方向标签样式：即时预览
  function changeDirLabel(v: 'short' | 'full') {
    editDirLabel = v;
    logDirLabelStyle.value = v;
  }
  // 英文字体：选预设即时预览
  function selectFontLatin(value: string) {
    editFontLatin = value;
    applyLogFont(editFontSize, editLineHeight, value, editFontCJK);
  }
  // 中文字体：选预设即时预览
  function selectFontCJK(value: string) {
    editFontCJK = value;
    applyLogFont(editFontSize, editLineHeight, editFontLatin, value);
  }

  function addBaud() {
    const n = Number(newBaud);
    // 后端是 u32 列表,小数/非数字保存时整份被拒,这里就挡掉
    if (!Number.isInteger(n) || n <= 0) return;
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

  /** 只把"相对上次加载/应用变了"的编辑项装进补丁。整份提交会把别处(另一窗口、agent、
   *  主题编辑器)在本对话框打开期间改过的字段用这里的旧副本冲回去;只交差异就互不干扰。 */
  function buildPatch(): SettingsPatch {
    const cur = currentEdits();
    const base = lastAppliedEdits;
    const changed = (k: keyof EditValues) => !base || JSON.stringify(cur[k]) !== JSON.stringify(base[k]);
    const patch: SettingsPatch = {};
    const ui: NonNullable<SettingsPatch['ui']> = {};
    if (changed('fontSize')) ui.log_font_size = cur.fontSize;
    if (changed('lineHeight')) ui.log_line_height = cur.lineHeight;
    if (changed('dirLabel')) ui.log_dir_label = cur.dirLabel;
    if (changed('fontLatin')) ui.log_font_latin = cur.fontLatin;
    if (changed('fontCJK')) ui.log_font_cjk = cur.fontCJK;
    if (changed('textEncoding')) ui.text_encoding = cur.textEncoding === 'utf8' ? 'Utf8' : cur.textEncoding === 'gbk' ? 'Gbk' : 'Ascii';
    if (changed('backgroundMode')) ui.background_mode = cur.backgroundMode;
    if (changed('showSuggestTab')) ui.show_suggest_tab = cur.showSuggestTab;
    if (changed('showMcpTab')) ui.show_mcp_tab = cur.showMcpTab;
    if (changed('qcFontSize')) ui.qc_font_size = cur.qcFontSize;
    if (changed('qcInputHeight')) ui.qc_input_height = cur.qcInputHeight;
    if (changed('qcRowGap')) ui.qc_row_gap = cur.qcRowGap;
    if (changed('qcFontFamily')) ui.qc_font_family = cur.qcFontFamily;
    if (Object.keys(ui).length) patch.ui = ui;
    const presets: NonNullable<SettingsPatch['presets']> = {};
    if (changed('baudRates')) presets.baud_rates = cur.baudRates;
    if (changed('theme')) presets.theme = cur.theme;
    if (changed('custom')) presets.custom_theme = cur.custom;
    if (Object.keys(presets).length) patch.presets = presets;
    const mcp: NonNullable<SettingsPatch['mcp']> = {};
    if (changed('mcpAutoStart')) mcp.auto_start = cur.mcpAutoStart;
    if (changed('mcpPort')) mcp.port = cur.mcpPort;
    if (Object.keys(mcp).length) patch.mcp = mcp;
    const ci: NonNullable<SettingsPatch['command_index']> = {};
    if (changed('kbBaseUrl')) ci.base_url = cur.kbBaseUrl;
    if (changed('kbApiKey')) ci.api_key = cur.kbApiKey;
    if (changed('disabledDocIds')) ci.disabled_doc_ids = cur.disabledDocIds;
    if (changed('kbAutoRefresh')) ci.auto_refresh = cur.kbAutoRefresh;
    if (changed('suggestEnabled')) ci.suggest_enabled = cur.suggestEnabled;
    if (changed('suggestMinChars')) ci.suggest_min_chars = cur.suggestMinChars;
    if (changed('suggestIgnoreAtPrefix')) ci.suggest_ignore_at_prefix = cur.suggestIgnoreAtPrefix;
    if (changed('suggestMaxManual')) ci.suggest_max_manual = cur.suggestMaxManual;
    if (changed('suggestMaxHistory')) ci.suggest_max_history = cur.suggestMaxHistory;
    if (changed('historyLimit')) ci.history_limit = cur.historyLimit;
    if (Object.keys(ci).length) patch.command_index = ci;
    return patch;
  }

  /** 应用:落盘 + 同步预览 store + 更新 cachedSettings,但不关窗。供"应用"按钮和"保存"共用。
   *  返回是否成功:失败(端口留空/越界、写盘出错)时 store 不动、原因显示在按钮栏,"保存"不关窗——
   *  原先失败只进 console,对话框照常关掉、界面按新值显示,重启后全部回到旧值。 */
  async function applyEdits(): Promise<boolean> {
    saveError = null;
    // 数字输入框清空会绑成 null,越界的端口后端也会拒;先在这里给出能看懂的原因
    if (!Number.isInteger(editMcpPort) || editMcpPort < 1024 || editMcpPort > 65535) {
      saveError = 'MCP 端口须为 1024–65535 的整数';
      return false;
    }
    const patch = buildPatch();
    if (Object.keys(patch).length === 0) return true;
    let next: Settings;
    try {
      next = await patchSettings(patch);
    } catch (e) {
      console.error('保存设置失败:', e);
      saveError = `保存失败:${e}`;
      return false;
    }
    // 落盘成功再把预览值同步进 store(主题/字体/编码/波特率),与 settings 一致
    presetBaudRates.value = next.presets.baud_rates;
    theme.value = editTheme;
    customTheme.value = { ...editCustom };
    logFontSize.value = editFontSize;
    logLineHeight.value = editLineHeight;
    logDirLabelStyle.value = editDirLabel;
    logFontLatin.value = editFontLatin;
    logFontCJK.value = editFontCJK;
    textEncoding.value = editTextEncoding;
    cachedSettings.value = next;
    // 已应用的值就是新的"打开前原值":之后再取消/Esc 只撤销这之后的预览,不能把已落盘的 4 项翻回去
    origDirLabel = editDirLabel;
    origFontLatin = editFontLatin;
    origFontCJK = editFontCJK;
    origTextEncoding = editTextEncoding;
    // 应用完成:把脏检测/补丁基准刷新到当前,应用按钮重新置灰
    lastAppliedEdits = currentEdits();
    return true;
  }

  /** 应用:保存但不关窗,便于继续调其他设置项。 */
  async function handleApply() {
    await applyEdits();
  }

  /** 保存:应用 + 关窗;保存失败留在对话框里让用户看到原因。 */
  async function handleSave() {
    if (await applyEdits()) open = false;
  }

  // 一键复制 MCP 连接指令到剪贴板
  async function copyMcpCommand() {
    if (!mcpStatus.port) return;
    const cmd = `claude mcp add --transport http neoserial http://localhost:${mcpStatus.port}/mcp`;
    try {
      await navigator.clipboard.writeText(cmd);
      mcpCopied = true;
      setTimeout(() => (mcpCopied = false), 1500);
    } catch {
      // 剪贴板不可用时静默
    }
  }

  // 刷新指令库:用编辑框里的地址/Key(不必先保存)。缓存更新后后端广播,commandIndex store 自己重载。
  // 后端是增量的:updated_at 与条数都没变的手册直接沿用缓存,所以文案带上"重拉了几本"。
  async function handleRefreshIndex() {
    if (!canRefreshIndex) return;
    indexRefreshing = true;
    indexRefreshMsg = null;
    try {
      const r = await commandIndexRefresh(editKbBaseUrl, editKbApiKey);
      const detail = r.refreshed === 0
        ? '各手册均无更新'
        : `重拉 ${r.refreshed} 本${r.skipped ? `,${r.skipped} 本无更新` : ''}`;
      indexRefreshMsg = r.failed.length
        ? { kind: 'warn', text: `${r.failed.length} 本手册更新失败,沿用旧缓存:${r.failed.join('、')}` }
        : { kind: 'ok', text: `已更新 · ${detail} · 共 ${r.doc_count} 本手册 ${r.cmd_count} 条指令` };
    } catch (e) {
      indexRefreshMsg = { kind: 'error', text: `刷新失败:${e}` };
    } finally {
      indexRefreshing = false;
    }
  }

  /** 手册行末尾的"刷新这一本":只拉这本,其余沿用缓存。与全量刷新共用后端那把标志,
   *  所以一次只能刷一本(其余行的按钮期间禁用)。 */
  async function handleRefreshDoc(id: number) {
    if (!canRefreshIndex || indexRefreshingDocId !== null || indexRefreshing) return;
    indexRefreshingDocId = id;
    indexRefreshMsg = null;
    try {
      const r = await commandIndexRefreshDoc(editKbBaseUrl, editKbApiKey, id);
      indexRefreshMsg = r.cmd_status === 'done'
        ? { kind: 'ok', text: `已更新《${r.title}》· ${r.cmd_count} 条指令` }
        : {
            kind: 'warn',
            text: `《${r.title}》${r.cmd_status === 'running' ? '知识库仍在提取指令,稍后再刷' : r.cmd_status === 'failed' ? '知识库提取失败,没有可用指令' : '知识库尚未提取指令'}`,
          };
    } catch (e) {
      indexRefreshMsg = { kind: 'error', text: `刷新失败:${e}` };
    } finally {
      indexRefreshingDocId = null;
    }
  }

  /** 地址行"测试"按钮:轻量探活(只请求手册列表接口),结果写进状态行复用展示,不落盘。 */
  async function handleTestConnection() {
    if (!canRefreshIndex || indexTesting) return;
    indexTesting = true;
    indexRefreshMsg = null;
    try {
      const msg = await commandIndexTestConnection(editKbBaseUrl, editKbApiKey);
      indexRefreshMsg = { kind: 'ok', text: msg };
    } catch (e) {
      indexRefreshMsg = { kind: 'error', text: `连通失败:${e}` };
    } finally {
      indexTesting = false;
    }
  }

  // 勾选 = 不在排除名单
  function toggleDoc(id: number, checked: boolean) {
    editDisabledDocIds = checked ? editDisabledDocIds.filter((d) => d !== id) : [...editDisabledDocIds, id];
  }

  // ===== 手册列表按知识库分组折叠 =====
  // 分组名来自手册列表接口的 group_names(管理员在知识库 Web 端归的组)。一本手册可属多个分组,
  // 则在每个分组下各出现一次——勾选绑的是 doc id,两处联动。排除名单存的始终是 doc id,
  // 分组只是批量勾选的入口:管理员那边调整分组时,已有的排除名单不会跟着漂移。
  const UNGROUPED = '未分组';
  /** null = 没有任何手册带分组(含旧缓存无此字段)→ 按原来的平铺渲染,不显示分组头。 */
  const docGroups = $derived.by((): { name: string; docs: ManualDocument[] }[] | null => {
    const docs = commandIndex.documents;
    if (!docs.some((d) => d.group_names.length)) return null;
    const map = new Map<string, ManualDocument[]>();
    for (const d of docs) {
      for (const name of d.group_names.length ? d.group_names : [UNGROUPED]) {
        const arr = map.get(name);
        if (arr) arr.push(d);
        else map.set(name, [d]);
      }
    }
    // 分组顺序按首次出现(即手册列表顺序,与"同名指令主记录取排前面那本"同一个依据),未分组垫底
    const groups = [...map].map(([name, ds]) => ({ name, docs: ds }));
    return [...groups.filter((g) => g.name !== UNGROUPED), ...groups.filter((g) => g.name === UNGROUPED)];
  });

  // ===== 指令联想子页的分节折叠 =====
  // 子页有 4 节(手册/知识库/联想行为/发送历史),铺开约 800px 而内容区只有 400px。
  // 默认只展开"参与联想的手册"(最常回来动的那节),其余收起、头部显示当前值摘要;
  // 知识库还没配置时改为展开知识库那节——第一次进来该被引导去填地址,而不是看一个空手册列表。
  // 展开状态只在本次开着设置页期间有效,每次 show() 重置(纯视图状态,不进 settings.json)。
  let openManuals = $state(true);
  let openKb = $state(false);
  let openBehavior = $state(false);
  let openHistory = $state(false);

  const kbSummary = $derived.by(() => {
    const base = editKbBaseUrl.trim();
    if (!base || !editKbApiKey.trim()) return '未配置';
    return base.replace(/^https?:\/\//, '').replace(/\/+$/, '');
  });
  const manualSummary = $derived.by(() => {
    const ready = commandIndex.documents.filter((d) => d.cmd_status === 'done');
    const on = ready.filter((d) => !editDisabledDocIds.includes(d.id)).length;
    return `已选 ${on}/${ready.length}`;
  });
  const behaviorSummary = $derived(
    `${editSuggestIgnoreAtPrefix ? '忽略 AT+ · ' : ''}≥${editSuggestMinChars} 字符 · 手册 ${editSuggestMaxManual} / 历史 ${editSuggestMaxHistory}`,
  );
  const historySummary = $derived(`已记录 ${commandIndex.history.length} 条 · 上限 ${editHistoryLimit}`);

  // 折叠起来的分组名。只在本次开着设置页期间有效,不落盘(分组是服务端的,记住折叠状态意义不大)
  let collapsedGroups = $state<string[]>([]);
  function toggleGroupCollapsed(name: string) {
    collapsedGroups = collapsedGroups.includes(name)
      ? collapsedGroups.filter((n) => n !== name)
      : [...collapsedGroups, name];
  }

  /** 分组头勾选框的三态依据:该组里可勾选(cmd_status=done)的有几本、已勾了几本。
   *  提取中/失败的手册本来就点不动,不计入分母。 */
  function groupCheck(docs: ManualDocument[]): { ready: number; checked: number } {
    const ready = docs.filter((d) => d.cmd_status === 'done');
    return { ready: ready.length, checked: ready.filter((d) => !editDisabledDocIds.includes(d.id)).length };
  }

  /** 分组头勾选:全选该组 / 全不选,只动 cmd_status=done 的那些。 */
  function toggleGroup(docs: ManualDocument[], checked: boolean) {
    const ids = docs.filter((d) => d.cmd_status === 'done').map((d) => d.id);
    editDisabledDocIds = checked
      ? editDisabledDocIds.filter((id) => !ids.includes(id))
      : [...new Set([...editDisabledDocIds, ...ids])];
  }

  async function handleClearHistory() {
    try {
      await sendHistoryClear();
      historyCleared = true;
      setTimeout(() => (historyCleared = false), 1500);
    } catch (e) {
      console.error('清空发送历史失败:', e);
    }
  }

  /** "上次更新 09-03 10:22" 用的短时间;解析不了就原样显示 */
  function formatFetchedAt(iso: string | null): string {
    if (!iso) return '';
    const d = new Date(iso);
    if (isNaN(d.getTime())) return iso;
    const p = (n: number) => String(n).padStart(2, '0');
    return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  }

  function handleCancel() {
    // 取消：恢复打开前的主题与字体（撤销预览，含自定义色板改动）
    applyTheme(theme.value, customTheme.value);
    applyLogFont(logFontSize.value, logLineHeight.value, logFontLatin.value, logFontCJK.value);
    logDirLabelStyle.value = origDirLabel;
    logFontLatin.value = origFontLatin;
    logFontCJK.value = origFontCJK;
    textEncoding.value = origTextEncoding;
    open = false;
  }
</script>

<svelte:window on:keydown={(e) => {
  // Esc 关闭设置弹窗（弹窗不再支持点遮罩关闭）
  if (e.key === 'Escape' && open) handleCancel();
}} />

{#if open}
  <div
    class="fixed inset-0 z-[100] flex items-center justify-center"
    style="background: rgba(0,0,0,0.35);"
  >
    <div
      class="rounded-lg shadow-xl w-[600px] border flex flex-col"
      style="background: var(--background-elevated); border-color: var(--border);"
      onclick={(e) => e.stopPropagation()}
    >
      <!-- 标题 + 关闭按钮 -->
      <div class="flex items-center justify-between px-5 py-2 border-b border-[var(--border)]">
        <div class="text-[15px] font-semibold text-[var(--foreground)]">设置</div>
        <button
          class="flex items-center justify-center w-6 h-6 -mr-1.5 rounded text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] cursor-pointer transition-colors"
          onclick={handleCancel}
          title="关闭 (Esc)"
        ><X size={15} /></button>
      </div>

      <!-- 左右分栏：左导航 + 右内容（固定高度，内容多时右栏独立滚动） -->
      <div class="flex" style="height: 400px;">
        <!-- 左侧导航 -->
        <nav class="w-[120px] flex-shrink-0 border-r border-[var(--border)] py-2">
          {#each sections as s}
            <button
              class="block w-full text-left px-4 py-2 text-[13px] transition-colors {activeSection === s.key
                ? 'bg-[var(--border-subtle)] text-[var(--primary)] font-medium border-l-2 border-[var(--primary)]'
                : 'text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] hover:text-[var(--foreground)] border-l-2 border-transparent'}"
              onclick={() => {
                activeSection = s.key;
                // 点左侧导航一律回到该页的顶层:扩展页停在上次进过的子页(指令联想/MCP…)时,
                // 再点"扩展"应该回到模块列表,而不是原地不动
                extModule = null;
              }}
            >{s.label}</button>
          {/each}
        </nav>

        <!-- 右侧内容区（独立滚动） -->
        <div class="flex-1 overflow-y-auto px-6 py-5">
          {#if activeSection === 'about'}
            <!-- 关于：图标 + 应用名 + 版本 -->
            <div class="flex flex-col items-center justify-center text-center" style="min-height: 280px;">
              <img src={appIcon} alt="NeoSerial" class="w-16 h-16 mb-3 rounded-lg shadow-sm" />
              <div class="text-[15px] font-semibold text-[var(--foreground)] mb-1">NeoSerial</div>
              <div class="text-[13px] text-[var(--muted-foreground)] mb-4">串口通信调试工具</div>
              <!-- GitHub 源码链接 -->
              <button
                class="mb-1 flex items-center gap-1.5 text-[12px] transition-opacity hover:opacity-70"
                style="color: var(--muted-foreground);"
                onclick={() => openUrl('https://github.com/Alzeros/NeoSerial')}
                title="在浏览器打开 GitHub 源码"
              >
                <Github size={13} />
                GitHub 源码
              </button>
              <!-- 更新检查卡片:版本号已懒加载,作 prop 传入(卡片内部展示) -->
              <UpdaterCard version={version.value} />
            </div>
          {:else if activeSection === 'general'}
            <!-- 通用：预设波特率 -->
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

            <!-- 分隔：日志显示 -->
            <div class="my-5 border-t border-[var(--border)]"></div>

            <!-- 文本编码 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">文本编码</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-3">
              HEX显示关闭时的文本模式解码方式。
            </div>
            <div class="flex items-center gap-3 mb-5">
              <span class="w-16 text-[13px] text-[var(--foreground)]">编码</span>
              <div class="flex gap-2">
                {#each [
                  { v: 'ascii', l: 'ASCII' },
                  { v: 'utf8', l: 'UTF-8' },
                  { v: 'gbk', l: 'GBK' },
                ] as enc}
                  <button
                    class="px-3 py-1 rounded-md border text-[13px] transition-colors {editTextEncoding === enc.v
                      ? 'border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]'
                      : 'border-[var(--border)] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] cursor-pointer'}"
                    onclick={() => (editTextEncoding = enc.v as 'ascii' | 'utf8' | 'gbk')}
                  >{enc.l}</button>
                {/each}
              </div>
            </div>

            <!-- 日志字体 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">日志字体</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-4">
              英文字体用于 ASCII/HEX 对齐（等宽），中文字体渲染中文内容，两者自动拼成回退栈，即时预览。
            </div>

            <!-- 英文字体 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">英文字体</span>
              <select
                class="flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
                value={editFontLatin}
                onchange={(e) => selectFontLatin((e.target as HTMLSelectElement).value)}
              >
                {#each logFontLatinPresets as p}
                  <option value={p.value}>{p.label}</option>
                {/each}
              </select>
            </div>

            <!-- 中文字体 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">中文字体</span>
              <select
                class="flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
                value={editFontCJK}
                onchange={(e) => selectFontCJK((e.target as HTMLSelectElement).value)}
              >
                {#each logFontCJKPresets as p}
                  <option value={p.value}>{p.label}</option>
                {/each}
              </select>
            </div>

            <!-- 字号 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">字号</span>
              <input
                type="range" min="10" max="22" step="1"
                class="flex-1 accent-[var(--primary)]"
                value={editFontSize}
                oninput={(e) => changeFontSize(Number((e.target as HTMLInputElement).value))}
              />
              <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editFontSize}px</span>
            </div>

            <!-- 行高 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">行高</span>
              <input
                type="range" min="1.0" max="3.0" step="0.1"
                class="flex-1 accent-[var(--primary)]"
                value={editLineHeight}
                oninput={(e) => changeLineHeight(Number((e.target as HTMLInputElement).value))}
              />
              <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editLineHeight.toFixed(1)}</span>
            </div>

            <!-- 方向标签 -->
            <div class="flex items-center gap-3 mb-4">
              <span class="w-16 text-[13px] text-[var(--foreground)]">方向标签</span>
              <div class="flex gap-2">
                <button
                  class="px-3 py-1 rounded-md border text-[13px] transition-colors {editDirLabel === 'short'
                    ? 'border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]'
                    : 'border-[var(--border)] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] cursor-pointer'}"
                  onclick={() => changeDirLabel('short')}
                >Tx / Rx</button>
                <button
                  class="px-3 py-1 rounded-md border text-[13px] transition-colors {editDirLabel === 'full'
                    ? 'border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]'
                    : 'border-[var(--border)] text-[var(--muted-foreground)] hover:bg-[var(--border-subtle)] cursor-pointer'}"
                  onclick={() => changeDirLabel('full')}
                >发送 / 接收</button>
              </div>
            </div>

            <!-- 预览 -->
            <div class="mt-4 p-3 rounded border" style="border-color: var(--border); background: var(--background-data); font-family: var(--log-font-family); font-size: {editFontSize}px; line-height: {editLineHeight};">
              <div>{editDirLabel === 'full' ? '发送' : 'Tx'} 10:39:47.362 AT</div>
              <div>{editDirLabel === 'full' ? '接收' : 'Rx'} 10:39:47.484 OK</div>
              <div>{editDirLabel === 'full' ? '发送' : 'Tx'} 10:39:47.545 AT+CSQ</div>
              <div>{editDirLabel === 'full' ? '接收' : 'Rx'} 10:39:47.612 模块就绪</div>
            </div>

            <!-- 分隔：窗口行为 -->
            <div class="my-5 border-t border-[var(--border)]"></div>

            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">后台运行</div>
            <div class="flex items-center gap-3">
              <label class="switch">
                <input type="checkbox" bind:checked={editBackgroundMode} />
                <span class="switch-track"></span>
                <span class="switch-label">后台运行（托盘常驻）</span>
              </label>
            </div>
            <div class="text-[12px] text-[var(--muted-foreground)] mt-2 leading-relaxed">
              {#if editBackgroundMode}
                开启：关闭窗口不断开串口连接，连接留在后台继续收发、存日志、跑序列；关掉最后一个窗口应用留在托盘，MCP 服务照常。
                托盘右键菜单可重新打开后台连接、切到已开窗口或退出应用。
              {:else}
                关闭：不显示托盘图标。关闭窗口会断开该窗口自己建立的连接（agent 建立的交还 agent）；关掉最后一个窗口即退出应用、停止 MCP 服务（若 agent 仍连着，会先确认）。
              {/if}
            </div>

            <div class="mt-4 flex items-center gap-3">
              <button class="btn btn-secondary" style="padding: 6px 14px;" onclick={() => exitApp()}>退出 NeoSerial</button>
              <span class="text-[12px] text-[var(--muted-foreground)]">断开所有连接并停止 MCP 服务（与托盘菜单"退出"相同）。</span>
            </div>
          {:else if activeSection === 'appearance'}
            <!-- 外观：主题预设 -->
            <div class="mb-2 text-[13px] font-medium text-[var(--foreground)]">主题</div>
            <div class="text-[12px] text-[var(--muted-foreground)] mb-3">
              选择应用的整体配色方案，点击即时预览。
            </div>
            <div class="grid grid-cols-2 gap-3">
              {#each themeMeta as t}
                <button
                  class="flex items-center gap-3 rounded-lg border-2 p-3 transition-all cursor-pointer {editTheme === t.key
                    ? 'border-[var(--primary)]'
                    : 'border-[var(--border)] hover:border-[var(--border-strong)]'}"
                  onclick={() => selectTheme(t.key)}
                >
                  <!-- 预览色块：背景色 + 强调色圆点 -->
                  <div class="relative w-10 h-10 rounded-md flex-shrink-0" style="background: {t.bg};">
                    <div class="absolute bottom-1 right-1 w-3 h-3 rounded-full" style="background: {t.accent};"></div>
                  </div>
                  <div class="flex flex-col items-start">
                    <span class="text-[13px] font-medium text-[var(--foreground)]">{t.label}</span>
                    <span class="text-[11px] text-[var(--muted-foreground)]">{t.accent}</span>
                  </div>
                </button>
              {/each}
              <!-- 自定义主题卡片：预览色块跟随当前编辑中的色板 -->
              <button
                class="flex items-center gap-3 rounded-lg border-2 p-3 transition-all cursor-pointer {editTheme === 'custom'
                  ? 'border-[var(--primary)]'
                  : 'border-[var(--border)] hover:border-[var(--border-strong)]'}"
                onclick={() => selectTheme('custom')}
              >
                <div class="relative w-10 h-10 rounded-md flex-shrink-0 border border-[var(--border)]" style="background: {editCustom['background']};">
                  <div class="absolute bottom-1 right-1 w-3 h-3 rounded-full" style="background: {editCustom['primary']};"></div>
                </div>
                <div class="flex flex-col items-start">
                  <span class="text-[13px] font-medium text-[var(--foreground)]">自定义</span>
                  <span class="text-[11px] text-[var(--muted-foreground)]">自己调配色板</span>
                </div>
              </button>
              <!-- 选中自定义时：右列空位放编辑器入口，与左侧卡片同尺寸 -->
              {#if editTheme === 'custom'}
                <div class="flex items-center gap-3 rounded-lg border-2 border-dashed p-3" style="border-color: var(--border);">
                  <div class="flex-1 min-w-0">
                    <div class="text-[13px] font-medium text-[var(--foreground)]">编辑配色</div>
                    <div class="text-[11px] text-[var(--muted-foreground)]">实时预览</div>
                  </div>
                  <button class="btn btn-primary flex-shrink-0" style="padding: 4px 10px; font-size: 12px; white-space: nowrap;" onclick={openThemeEditorWindow}>
                    打开 →
                  </button>
                </div>
              {/if}
            </div>
          {:else if activeSection === 'extensions'}
            {#if extModule === null}
              <!-- 文件夹视图:三个模块。快捷指令常驻开启(无开关),指令联想/MCP 各自独立开关 -->
              <div class="text-[12px] text-[var(--muted-foreground)] mb-4 leading-relaxed">
                扩展功能按模块独立管理。点开进入各自设置:快捷指令常驻开启,只调密度;指令联想与 MCP 可独立开关。
              </div>
              <button
                class="flex items-center w-full text-left px-3 py-3 rounded transition-colors hover:bg-[var(--border-subtle)] mb-2"
                style="border: 1px solid var(--border);"
                onclick={() => (extModule = 'quick')}
              >
                <span class="text-[14px] font-medium text-[var(--foreground)]">快捷指令</span>
                <span class="ml-2 text-[12px]" style="color: var(--primary);">常驻</span>
                <span class="ml-auto text-[var(--muted-foreground)]">›</span>
              </button>
              <button
                class="flex items-center w-full text-left px-3 py-3 rounded transition-colors hover:bg-[var(--border-subtle)] mb-2"
                style="border: 1px solid var(--border);"
                onclick={() => (extModule = 'suggest')}
              >
                <span class="text-[14px] font-medium text-[var(--foreground)]">指令联想</span>
                <span class="ml-2 text-[12px]" style="color: {editSuggestEnabled ? 'var(--primary)' : 'var(--muted-foreground)'};">{editSuggestEnabled ? '已启用' : '未启用'}</span>
                <span class="ml-auto text-[var(--muted-foreground)]">›</span>
              </button>
              <button
                class="flex items-center w-full text-left px-3 py-3 rounded transition-colors hover:bg-[var(--border-subtle)]"
                style="border: 1px solid var(--border);"
                onclick={() => (extModule = 'mcp')}
              >
                <span class="text-[14px] font-medium text-[var(--foreground)]">MCP 服务</span>
                <span class="ml-2 text-[12px]" style="color: {editMcpAutoStart ? 'var(--primary)' : 'var(--muted-foreground)'};">{editMcpAutoStart ? '已启用' : '未启用'}</span>
                <span class="ml-auto text-[var(--muted-foreground)]">›</span>
              </button>
            {:else if extModule === 'quick'}
              <!-- 快捷指令子页:常驻开启(无总开关),只有一组密度设置。
                   与指令联想页同一套页头(标题行 + 一行说明 + 分隔线收口),但项目少,不套折叠。 -->
              <button class="flex items-center gap-1 text-[13px] text-[var(--muted-foreground)] hover:text-[var(--foreground)] mb-3" onclick={() => (extModule = null)}>
                <span>‹</span><span>返回扩展</span>
              </button>
              <div class="flex items-center gap-3 mb-1">
                <div class="text-[13px] font-medium text-[var(--foreground)]">快捷指令</div>
                <!-- 这一页没有开关(常驻开启),用一枚标记占住右侧那列,与另两页的总开关同位置 -->
                <span class="ml-auto text-[12px]" style="color: var(--primary);">常驻开启</span>
              </div>
              <div class="text-[12px] text-[var(--muted-foreground)]">
                侧栏快捷指令编辑区的密度:字号、输入框高度、行间距与字体。
              </div>

              <!-- 上边框给页头收口,与指令联想页的状态行同法,不必再套一层 div -->
              <div class="flex items-center gap-3 mt-3 pt-2 mb-4" style="border-top: 1px solid var(--border-subtle);">
                <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">字号</span>
                <input
                  type="range" min="11" max="18" step="1"
                  class="flex-1 accent-[var(--primary)]"
                  value={editQcFontSize}
                  oninput={(e) => (editQcFontSize = Number((e.target as HTMLInputElement).value))}
                />
                <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editQcFontSize}px</span>
              </div>
              <div class="flex items-center gap-3 mb-4">
                <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">输入框高度</span>
                <input
                  type="range" min="22" max="40" step="1"
                  class="flex-1 accent-[var(--primary)]"
                  value={editQcInputHeight}
                  oninput={(e) => (editQcInputHeight = Number((e.target as HTMLInputElement).value))}
                />
                <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editQcInputHeight}px</span>
              </div>
              <div class="flex items-center gap-3 mb-4">
                <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">行间距</span>
                <input
                  type="range" min="0" max="10" step="1"
                  class="flex-1 accent-[var(--primary)]"
                  value={editQcRowGap}
                  oninput={(e) => (editQcRowGap = Number((e.target as HTMLInputElement).value))}
                />
                <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editQcRowGap}px</span>
              </div>
              <div class="flex items-center gap-3 mb-4">
                <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">字体</span>
                <select
                  class="flex-1 rounded border border-[var(--border)] bg-[var(--background-input)] px-2 py-1.5 text-[13px] focus-visible:outline-none focus-visible:border-[var(--primary)]"
                  value={editQcFontFamily}
                  onchange={(e) => (editQcFontFamily = (e.target as HTMLSelectElement).value)}
                >
                  {#each logFontLatinPresets as p}
                    <option value={p.value}>{p.label}</option>
                  {/each}
                </select>
              </div>

              <!-- 预览:两行样例,即时反映字号/高度/行间距/字体(只在此页生效,不联动主页) -->
              <div class="mt-3 pt-2" style="border-top: 1px solid var(--border-subtle);">
                <div class="text-[12px] text-[var(--muted-foreground)] mb-2">预览</div>
                <div style="font-size: {editQcFontSize}px;{editQcFontFamily !== 'default' ? ` font-family: ${editQcFontFamily};` : ''}">
                  {#each ['AT+CSQ?', 'AT+CGDCONT?'] as sample}
                    <div style="padding: {editQcRowGap}px 0;">
                      <div
                        class="rounded border border-[var(--border)] bg-[var(--background-input)] px-2 flex items-center text-[var(--foreground)]"
                        style="height: {editQcInputHeight}px;"
                      >{sample}</div>
                    </div>
                  {/each}
                </div>
              </div>
            {:else if extModule === 'suggest'}
              <!-- 指令联想子页:分节折叠(默认只展开一节),按自然流排布,超出由外层内容区滚动 -->
              <div>
                <button class="flex items-center gap-1 text-[13px] text-[var(--muted-foreground)] hover:text-[var(--foreground)] mb-3" onclick={() => (extModule = null)}>
                  <span>‹</span><span>返回扩展</span>
                </button>
                <!-- 标题与总开关同一行:开关管的是整页,不必再写一遍"启用输入联想"
                     (label 只包 track,标题不是它的 label;可点区域交给 aria-label/title) -->
                <div class="flex items-center gap-3 mb-1">
                  <div class="text-[13px] font-medium text-[var(--foreground)]">指令联想</div>
                  <label class="switch ml-auto" title={editSuggestEnabled ? '关闭输入联想' : '启用输入联想'}>
                    <input type="checkbox" aria-label="启用输入联想" bind:checked={editSuggestEnabled} />
                    <span class="switch-track"></span>
                  </label>
                </div>
                <div class="text-[12px] text-[var(--muted-foreground)]">
                  输入时弹出候选:知识库手册索引(带语法/参数/示例)与发送历史。
                </div>

                {#if editSuggestEnabled}
                  <!-- tab 显隐是"启用"的从属项,但层级靠"总开关关掉它就不渲染"表达就够了:
                       缩进 + 淡字反而跟谁都不对齐、比谁都淡,像掉在那儿的注释。
                       switch-row 让轨道与总开关的轨道右对齐成一列(否则一左一右像两套东西)。 -->
                  <label class="switch switch-row mt-2">
                    <input type="checkbox" bind:checked={editShowSuggestTab} />
                    <span class="switch-track"></span>
                    <span class="switch-label">显示"指令查询"tab</span>
                  </label>

                  <!-- 刷新状态行常驻(不进折叠节):单本刷新的按钮在"参与联想的手册"里,
                       结果消息若留在知识库那节,那节一折叠就等于点了刷新没有任何反馈。
                       纯文字不加底色:四节行都没有底色,少一种容器语言更整。
                       上边框顺带给页顶(标题/开关)收口,不必再套一层 div。 -->
                  <div
                    class="text-[12px] mt-3 pt-2 mb-2"
                    style="border-top: 1px solid var(--border-subtle); color: {indexRefreshMsg?.kind === 'error' ? 'var(--error)' : indexRefreshMsg?.kind === 'warn' ? 'var(--warning)' : 'var(--muted-foreground)'};"
                  >
                    {#if indexRefreshMsg}
                      {indexRefreshMsg.text}
                    {:else if !editKbBaseUrl.trim() || !editKbApiKey.trim()}
                      填写地址和 API Key 后可刷新;未配置时联想只用发送历史。
                    {:else if commandIndex.fetchedAt}
                      上次更新 {formatFetchedAt(commandIndex.fetchedAt)} · {commandIndex.documents.filter((d) => d.cmd_status === 'done').length} 本手册 · {commandIndex.commands.length} 条指令
                    {:else}
                      尚未拉取过,点"刷新指令库"。
                    {/if}
                  </div>

                  <!-- 手册行:平铺与分组两种排布共用,故抽成 snippet(同一本手册属多个分组时会渲染多次,
                       勾选绑 doc id 所以联动) -->
                  {#snippet docRow(d: ManualDocument)}
                    {@const ready = d.cmd_status === 'done'}
                    {@const cached = cachedCmdCounts.get(d.id) ?? 0}
                    {@const stale = ready && cached !== d.cmd_count}
                    <!-- 行不能整个是 <label>:点末尾的刷新按钮会被 label 转成"点了 checkbox",
                         连带切换勾选(与 CustomSelect 那处同一个 label 激活行为)。label 只包勾选+标题。 -->
                    <div class="flex items-center gap-2">
                      <label class="flex items-center gap-2 min-w-0 flex-1 select-none {ready ? 'cursor-pointer' : 'opacity-50 cursor-not-allowed'}">
                        <input
                          type="checkbox"
                          class="h-4 w-4 rounded accent-[var(--primary)] shrink-0"
                          disabled={!ready}
                          checked={ready && !editDisabledDocIds.includes(d.id)}
                          onchange={(e) => toggleDoc(d.id, (e.target as HTMLInputElement).checked)}
                        />
                        <span class="text-[13px] text-[var(--foreground)] truncate">{d.title}</span>
                        <!-- 条数显示本地缓存的数;与服务器报的不一致时补出"服务器 N",提示这本该刷新 -->
                        <span class="text-[12px] shrink-0" style="color: {stale ? 'var(--warning)' : 'var(--muted-foreground)'};">
                          {#if ready}
                            ({cached} 条指令{#if stale} · 服务器 {d.cmd_count}{/if})
                          {:else if d.cmd_status === 'running'}提取中{:else if d.cmd_status === 'failed'}提取失败{:else}未提取{/if}
                        </span>
                      </label>
                      <button
                        type="button"
                        class="btn btn-ghost shrink-0"
                        style="padding: 2px 6px;"
                        disabled={!canRefreshIndex || indexRefreshing || indexRefreshingDocId !== null}
                        title={canRefreshIndex ? `只刷新《${d.title}》,其余手册沿用缓存` : '填写地址和 API Key 后可刷新'}
                        onclick={() => handleRefreshDoc(d.id)}
                      >
                        {#if indexRefreshingDocId === d.id}<Loader2 size={13} class="animate-spin" />{:else}<RefreshCw size={13} />{/if}
                      </button>
                    </div>
                  {/snippet}

                  <!-- 分节折叠:四节全铺开约 800px,而内容区只有 400px。默认只展开一节,
                       收起的节头部显示当前值摘要;仍然可以整页上下滚,只是不必一次面对全部设置项。 -->
                  <div>
                    <Collapsible title="知识库服务器" summary={kbSummary} bind:open={openKb}>
                      <div class="flex items-center gap-3 mb-3">
                        <span class="w-16 text-[13px] text-[var(--foreground)] shrink-0">地址</span>
                        <input type="text" class="flex-1 min-w-0" style="padding: 6px 10px;" bind:value={editKbBaseUrl} placeholder="http://127.0.0.1:8200" spellcheck="false" />
                        <!-- 测试连通性:与下方 API Key 行的显示/隐藏按钮同尺寸对齐;探活结果写进状态行 -->
                        <button
                          type="button"
                          class="btn btn-ghost shrink-0"
                          style="padding: 4px 8px;"
                          disabled={!canRefreshIndex || indexTesting}
                          title={canRefreshIndex ? '测试与知识库服务器的连通性' : '填写地址和 API Key 后可测试'}
                          onclick={handleTestConnection}
                        >{#if indexTesting}<Loader2 size={14} class="animate-spin" />{:else}<Plug size={14} />{/if}</button>
                      </div>
                      <div class="flex items-center gap-3 mb-3">
                        <span class="w-16 text-[13px] text-[var(--foreground)] shrink-0">API Key</span>
                        <input type="text" class="flex-1 min-w-0 {showApiKey ? '' : 'masked-input'}" style="padding: 6px 10px;" bind:value={editKbApiKey} placeholder="kb_…" spellcheck="false" autocomplete="off" />
                        <button type="button" class="btn btn-ghost shrink-0" style="padding: 4px 8px;" title={showApiKey ? '隐藏' : '显示'} onclick={() => (showApiKey = !showApiKey)}>
                          {#if showApiKey}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
                        </button>
                      </div>
                      <div class="flex items-center gap-3">
                        <label class="flex items-center gap-2 cursor-pointer select-none">
                          <input type="checkbox" class="h-4 w-4 rounded accent-[var(--primary)]" bind:checked={editKbAutoRefresh} />
                          <span class="text-[13px] text-[var(--foreground)]">启动时自动刷新</span>
                        </label>
                        <button
                          class="btn btn-secondary ml-auto"
                          style="padding: 6px 14px;"
                          disabled={!canRefreshIndex}
                          title={canRefreshIndex || indexRefreshing ? '' : '填写地址和 API Key 后可刷新'}
                          onclick={handleRefreshIndex}
                        >{indexRefreshing ? '刷新中…' : '刷新指令库'}</button>
                      </div>
                    </Collapsible>

                    {#if commandIndex.documents.length}
                      <Collapsible title="参与联想的手册" summary={manualSummary} bind:open={openManuals}>
                        <!-- 内联滚动框:手册多到三四十本时不撑开整个设置页,固定高度内滚动 -->
                        <div
                          class="flex flex-col gap-1.5 overflow-y-auto px-1 py-1"
                          style="max-height: 200px; border: 1px solid var(--border); border-radius: var(--radius);"
                        >
                          {#if docGroups}
                            {#each docGroups as g (g.name)}
                              {@const st = groupCheck(g.docs)}
                              {@const collapsed = collapsedGroups.includes(g.name)}
                              <!-- 分组头同样不做成 <label>:点折叠按钮会连带切换该组勾选 -->
                              <div class="flex items-center gap-2">
                                <input
                                  type="checkbox"
                                  class="h-4 w-4 rounded accent-[var(--primary)] shrink-0"
                                  disabled={st.ready === 0}
                                  checked={st.ready > 0 && st.checked === st.ready}
                                  indeterminate={st.checked > 0 && st.checked < st.ready}
                                  title={st.ready === 0 ? '该分组没有已提取的手册' : '全选 / 全不选该分组'}
                                  onchange={(e) => toggleGroup(g.docs, (e.target as HTMLInputElement).checked)}
                                />
                                <button
                                  type="button"
                                  class="flex items-center gap-1 min-w-0 flex-1 text-left"
                                  style="color: var(--muted-foreground);"
                                  title={collapsed ? '展开' : '折叠'}
                                  onclick={() => toggleGroupCollapsed(g.name)}
                                >
                                  {#if collapsed}<ChevronRight size={13} />{:else}<ChevronDown size={13} />{/if}
                                  <span class="text-[13px] font-medium truncate" style="color: var(--foreground);">{g.name}</span>
                                  <span class="text-[12px] shrink-0">
                                    {st.ready === 0 ? '无已提取手册' : `${st.checked}/${st.ready}`}
                                  </span>
                                </button>
                              </div>
                              {#if !collapsed}
                                <!-- 组内手册缩进 + 左侧竖线,与分组头区分 -->
                                <div class="flex flex-col gap-1.5 ml-1.5 pl-2" style="border-left: 1px solid var(--border-subtle);">
                                  {#each g.docs as d (d.id)}
                                    {@render docRow(d)}
                                  {/each}
                                </div>
                              {/if}
                            {/each}
                          {:else}
                            {#each commandIndex.documents as d (d.id)}
                              {@render docRow(d)}
                            {/each}
                          {/if}
                        </div>
                      </Collapsible>
                    {/if}

                    <Collapsible title="联想行为" summary={behaviorSummary} bind:open={openBehavior}>
                      <!-- 键盘操作是"弹层怎么用",归在这一节;原先摊在页顶当引言,占三行还抢眼 -->
                      <div class="text-[12px] text-[var(--muted-foreground)] mb-3">
                        ↑ 进列表并选中最佳候选,继续 ↑ 向上翻;Tab / 有高亮时回车 = 填入,Esc 收起;无高亮时回车照旧发送。
                      </div>
                      <!-- 弹出时机:模组指令几乎全以 AT+ 开头,按输入字符数算门槛时
                           "AT""AT+"就命中整本手册,而它们是打任何指令的必经之路 -->
                      <label class="switch mb-3">
                        <input type="checkbox" bind:checked={editSuggestIgnoreAtPrefix} />
                        <span class="switch-track"></span>
                        <span class="switch-label">算长度时忽略 AT+ 前缀</span>
                      </label>
                      <div class="flex items-center gap-3 mb-2">
                        <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">最少输入</span>
                        <input
                          type="range" min="1" max="6" step="1"
                          class="flex-1 accent-[var(--primary)]"
                          value={editSuggestMinChars}
                          oninput={(e) => (editSuggestMinChars = Number((e.target as HTMLInputElement).value))}
                        />
                        <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editSuggestMinChars} 字符</span>
                      </div>
                      <div class="text-[12px] text-[var(--muted-foreground)] mb-4">
                        {#if editSuggestIgnoreAtPrefix}
                          {@const sample = 'AT+' + 'MIPLCREATE'.slice(0, editSuggestMinChars)}
                          去掉 AT/AT+/AT&amp; 前缀后够 {editSuggestMinChars} 个字符才弹:<code>{sample}</code> 弹,
                          <code>{sample.slice(0, -1)}</code> 不弹。
                        {:else}
                          按输入的字符数算:输满 {editSuggestMinChars} 个字符就弹(<code>AT</code>、<code>AT+</code> 也算)。
                        {/if}
                      </div>

                      <div class="flex items-center gap-3 mb-3">
                        <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">手册指令</span>
                        <input
                          type="range" min="1" max="50" step="1"
                          class="flex-1 accent-[var(--primary)]"
                          value={editSuggestMaxManual}
                          oninput={(e) => (editSuggestMaxManual = Number((e.target as HTMLInputElement).value))}
                        />
                        <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editSuggestMaxManual} 条</span>
                      </div>
                      <div class="flex items-center gap-3 mb-2">
                        <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">发送历史</span>
                        <input
                          type="range" min="0" max="30" step="1"
                          class="flex-1 accent-[var(--primary)]"
                          value={editSuggestMaxHistory}
                          oninput={(e) => (editSuggestMaxHistory = Number((e.target as HTMLInputElement).value))}
                        />
                        <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editSuggestMaxHistory} 条</span>
                      </div>
                      <div class="text-[12px] text-[var(--muted-foreground)]">
                        两类各自限量,历史再多也挤不掉手册卡片;被挡住的条数会显示在弹层顶部。
                        历史上限设 0 = 联想里不出历史;空输入按 ↑ 翻历史不受此限。
                      </div>
                    </Collapsible>

                    <Collapsible title="发送历史" summary={historySummary} bind:open={openHistory}>
                      <div class="flex items-center gap-3 mb-2">
                        <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">留存上限</span>
                        <input
                          type="range" min="50" max="1000" step="50"
                          class="flex-1 accent-[var(--primary)]"
                          value={editHistoryLimit}
                          oninput={(e) => (editHistoryLimit = Number((e.target as HTMLInputElement).value))}
                        />
                        <span class="w-12 text-center text-[13px] text-[var(--muted-foreground)]">{editHistoryLimit} 条</span>
                      </div>
                      <div class="text-[12px] text-[var(--muted-foreground)] mb-3">
                        输入框手动发过的内容留这么多条(超出的挤掉最旧的),存在 <code>send-history.json</code>。
                        调小后保存,多出来的旧记录当场就裁掉。<b>不是</b>联想里显示几条——那个在"联想行为"里。
                      </div>
                      <div class="flex items-center gap-3">
                        <span class="text-[12px] text-[var(--muted-foreground)]">已记录 {commandIndex.history.length} 条</span>
                        <button class="btn btn-secondary ml-auto" style="padding: 6px 14px;" disabled={commandIndex.history.length === 0} onclick={handleClearHistory}>{historyCleared ? '已清空' : '清空'}</button>
                      </div>
                    </Collapsible>
                  </div>
                {/if}
              </div>
            {:else if extModule === 'mcp'}
              <!-- MCP 服务子页:与指令联想页同一套页头(标题行带总开关 + 一行说明 + 从属开关
                   + 分隔线下的常驻状态行);项目少,不套折叠。 -->
              <button class="flex items-center gap-1 text-[13px] text-[var(--muted-foreground)] hover:text-[var(--foreground)] mb-3" onclick={() => (extModule = null)}>
                <span>‹</span><span>返回扩展</span>
              </button>
              <div class="flex items-center gap-3 mb-1">
                <div class="text-[13px] font-medium text-[var(--foreground)]">MCP 服务</div>
                <label class="switch ml-auto" title={editMcpAutoStart ? '关闭 MCP 服务' : '启用 MCP 服务'}>
                  <input type="checkbox" aria-label="启用 MCP 服务" bind:checked={editMcpAutoStart} />
                  <span class="switch-track"></span>
                </label>
              </div>
              <div class="text-[12px] text-[var(--muted-foreground)]">
                内嵌 MCP server,Claude Code 等 agent 经由它操作串口。开关与端口改后重启生效。
              </div>

              {#if editMcpAutoStart}
                <!-- tab 显隐是启用的从属项:功能关了 tab 必然关,开关也藏起来 -->
                <label class="switch switch-row mt-2">
                  <input type="checkbox" bind:checked={editShowMcpTab} />
                  <span class="switch-track"></span>
                  <span class="switch-label">显示"MCP 日志"tab</span>
                </label>

                <!-- 常驻状态行:纯文字 + 上边框收口,位置与指令联想页的"上次更新…"一致。
                     开着却没跑(端口被占/还没重启)用警示色,那是需要用户注意的状态。 -->
                <div
                  class="text-[12px] mt-3 pt-2 mb-3"
                  style="border-top: 1px solid var(--border-subtle); color: {mcpStatus.running ? 'var(--muted-foreground)' : 'var(--warning)'};"
                >
                  {#if mcpStatus.running && mcpStatus.port}
                    服务运行中 · 端口 {mcpStatus.port}
                  {:else}
                    未运行:端口 {editMcpPort} 被占或尚未启动,保存后重启生效。
                  {/if}
                </div>

                <div class="flex items-center gap-3 mb-3">
                  <span class="w-20 text-[13px] text-[var(--foreground)] shrink-0">端口</span>
                  <input type="number" class="w-24 px-2 py-1 text-[13px] rounded border border-[var(--border)] bg-[var(--background-input)] text-[var(--foreground)]" bind:value={editMcpPort} min="1024" max="65535" />
                  <span class="ml-auto text-[12px] text-[var(--muted-foreground)]">被占时自动向上找空闲端口</span>
                </div>

                {#if mcpStatus.running && mcpStatus.port}
                  <div class="text-[12px] text-[var(--muted-foreground)] mb-2">
                    在 Claude Code 里粘贴执行,或在终端运行:
                  </div>
                  <div class="flex items-center gap-2">
                    <!-- select-text:除了右边的复制按钮,也允许手动选一段(全局默认不可选) -->
                    <code class="flex-1 text-[12px] px-2.5 py-1.5 rounded bg-[var(--border-subtle)] text-[var(--foreground)] overflow-x-auto whitespace-nowrap select-text">
                      claude mcp add --transport http neoserial http://localhost:{mcpStatus.port}/mcp
                    </code>
                    <button
                      class="shrink-0 px-2.5 py-1.5 rounded text-[12px] font-medium transition-colors {mcpCopied
                        ? 'bg-[var(--primary)] text-[var(--primary-foreground)]'
                        : 'bg-[var(--border-subtle)] text-[var(--foreground)] hover:bg-[var(--border)]'}"
                      onclick={copyMcpCommand}
                      title="复制到剪贴板"
                    >{mcpCopied ? '已复制' : '复制'}</button>
                  </div>
                {/if}
              {/if}
            {/if}
          {/if}
        </div>
      </div>

      <!-- 底部按钮:取消(撤销+关) | 应用(保存不关,无改动时置灰) | 保存(应用+关);保存失败原因显示在左侧 -->
      <div class="flex items-center gap-2 px-5 pb-3 border-t border-[var(--border)] pt-2">
        {#if saveError}
          <!-- select-text:报错要能拷出去问人(显示被 truncate 截断,选中拿到的是完整那句) -->
          <span class="flex-1 min-w-0 truncate text-[12px] select-text" style="color: var(--error);" title={saveError}>{saveError}</span>
        {:else}
          <span class="flex-1"></span>
        {/if}
        <button class="btn btn-ghost" style="padding: 4px 12px;" onclick={handleCancel}>取消</button>
        <button
          class="btn btn-secondary"
          style="padding: 4px 12px;"
          disabled={!hasUnsavedChanges}
          title={hasUnsavedChanges ? '保存但不关闭,可继续调整其他设置' : '没有可应用的改动'}
          onclick={handleApply}
        >应用</button>
        <button class="btn btn-primary" style="padding: 4px 12px;" onclick={handleSave}>保存</button>
      </div>
    </div>
  </div>
{/if}
