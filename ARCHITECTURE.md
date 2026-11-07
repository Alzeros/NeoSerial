# NeoSerial 架构说明

面向贡献者，记录 README 里不展开的实现细节。功能与使用见 [README.md](README.md)。

## 总览

连接是核心：每个 `ConnectionHandle` 是一个自包含单元，包含串口句柄、读/写线程、这个连接自己的收发历史和归属窗口 label。窗口只是连接的视图，MCP 和 GUI 操作的是同一批连接。

```
┌────────────── NeoSerial 进程（单实例）──────────────┐
│  窗口 main / win-N（Svelte）   MCP server（rmcp）    │
│        │ Tauri IPC                 │ 127.0.0.1:34594 │
│        └──────────┬────────────────┘                 │
│          connections: HashMap<port, ConnectionHandle>│
│            ├─ 读线程 / 写线程 / 监控线程             │
│            ├─ RxHistory（tx+rx，10,000 行）          │
│            └─ window_label: Arc<RwLock<String>>      │
│          FileLogger（全局存盘线程）                   │
└──────────────────────────────────────────────────────┘
```

## 目录结构

```
src/                      前端（Svelte 5 + TypeScript）
  App.svelte              主界面装配、事件订阅、历史回填、开关持久化
  components/             界面组件（连接栏、日志区、发送区、快捷指令、数据处理、设置、主题编辑器…）
  lib/
    stores.svelte.ts      全局 $state store
    tauri.ts              invoke / 事件封装
    suggest.ts            指令联想的候选合并与排序（纯函数）
    dataProcessing.ts     帧构造器默认值、类型守卫、模板
    codec.ts / sms.ts     编解码、短信 PDU（纯函数）
    logRetention.ts       日志行数 + 字节双上限淘汰
    mcpSetup.ts           MCP 客户端接入向导模板
src-tauri/src/            后端（Rust）
  lib.rs                  应用装配、窗口生命周期、MCP 启动与 registry 心跳
  state.rs                AppState
  tray.rs                 托盘与后台运行
  window_sizing.rs        Win32 子类化 WM_GETMINMAXINFO 约束最小尺寸
  connection/             WinPort（overlapped I/O）、读/写线程、拼行、文件分块发送
  buffer/                 LogLine、环形缓冲、RxHistory
  commands/               Tauri command（连接、发送、序列、配置、日志、指令库、帧构造、MCP 日志）
  mcp/                    MCP server、工具实现、registry、调用记录、自检
  config/                 settings、发送历史、指令缓存、DPAPI 凭据、数据目录与迁移
  dataproc/               帧构造与校验算法
  logging/                FileLogger
  util/                   编码、hex、时间、端口友好名（SetupAPI）
tests/                    node --test 单元测试；browser/ 为无头浏览器回归
```

## 数据通路

```
设备 → WinPort overlapped read → LineAssembler（跨 read 拼行）
     → LogLine（预存 raw/ascii/hex）→ emit_to(window_label, "rx-line") → 该窗口 logLines
                                    → 写入该连接的 RxHistory（供 MCP 阻塞读与历史查询）
                                    → FileLogger channel → 存盘线程写文件
```

TX 回显统一走 `emit_tx_line`：手动发送、脚本序列、文件发送、MCP 四条路径都经过它，MCP 的 tx 历史也在这里写入。

## 线程模型

- **读线程**：overlapped read（20ms 超时，4KB 缓冲）→ `LineAssembler` 按 `\r\n` / `\n` / `\r` 切行 → 构造 `LogLine` → 批量 `emit_to`（攒够 16 行或超过 5ms 发一批），同时写入 `RxHistory`。读超时会把没有换行的残尾刷出来。
- **写线程**：crossbeam channel（容量 64）接收 `WriteCommand`，逐条 `write_all`。
  - overlapped write 的等待上限按"数据长度 / 波特率"推算；短写会循环补写；连续没有进展累计 3 秒才判定设备卡住；不合并积压的命令。
  - 文件发送用 `FileChunk` 加 `FileSendTracker`：写线程每写完一块就累加实际写出的字节数，`send_file` 据此报告进度，并等最后一块写完才返回。某块写失败后，同一文件余下的块直接丢弃。
  - 手动发送、脚本序列和 MCP 用 `SendSilent`：发起方已经先回显，避免写阻塞拖慢节奏。
- **存盘线程**：`FileLogger` 独立线程 + `BufWriter`，全局只有一份。开始、停止、写盘失败都会广播 `logging-changed`，各窗口据此同步状态。停止记录和退出应用都会等缓冲写完；写失败时摘掉 logger，并把原因显示在按钮上。
- **监控线程**：等读、写线程都退出后清理状态，通知归属窗口连接已断开，并唤醒在 `disconnect` 里等待的调用方（exit Condvar）。
- **CTS/DSR 监视线程**：只在 Windows 且硬件流控时创建，用 `WaitCommEvent` 事件驱动，电平变化时 emit `modem-status`。

## 串口打开方式

`WinPort::open()`（`CreateFileW` + `FILE_FLAG_OVERLAPPED`）后用 `DuplicateHandle` 克隆，读、写各持一个独立句柄。

- `WriteFile` 立即返回、不阻塞。超时后先 `CancelIo`，再用 `GetOverlappedResult(bWait=TRUE)` 等 I/O 真正完成后才释放缓冲，否则 OVERLAPPED 结构会在 I/O 完成前被释放。
- DCB 显式设置 `fBinary` 和 DTR/RTS `*_CONTROL_ENABLE`；硬件流控用 `fOutxCtsFlow` + `RTS_CONTROL_HANDSHAKE`。驱动级写超时关闭，读超时为 20ms。
- 这样做是为了绕开 serialport 的同步 `WriteFile`：USB 驱动缓冲满时，同步写会阻塞好几秒。非 Windows 平台仍走 serialport 路径。

## 连接建立不阻塞其他端口

打开串口和拉高 DTR/RTS 是阻塞的驱动调用，实测可能卡好几秒。

- `connect` 只在持有 `connections` 锁期间做检查，并往 `connecting` 集合插入占位（`ConnectingGuard`，RAII，任何失败路径都会清掉）。阻塞的打开操作放在锁外，完成后重新取锁再插入连接。
- 占位挡住了 TOCTOU：并发的第二个 `connect` 会被挡回，不会重复创建连接。同时，打开某个端口期间，其他已连端口的 send/disconnect/get_status 不受影响。
- MCP 侧的阻塞工具（`connect`、`list_ports` 等）经 `spawn_blocking` 执行，避免占住 tokio worker、拖住整个 MCP server。

## 多窗口与事件路由

- 窗口 label：启动时的窗口是 `main`，**+** 开出的窗口是 `win-{自增}`，与端口无关。另有 `settings`（设置，单例、隐藏复用）和 `theme-editor`。
- `ConnectionHandle.window_label` 是 `Arc<RwLock<String>>`，所有事件（rx-line / tx-line / sequence-progress / modem-status 等）都用 `emit_to(label)` 定向发送，前端用 `getCurrentWebview().listen` 接收，多窗口之间不串数据。
- agent 先建立的连接，label 为 `mcp-{port}`（不建窗口）。之后某个 GUI 窗口连同一端口时会**接管**：改写 RwLock 里的值，读写线程下次 emit 就自动发到新窗口，不用重启线程。窗口挂上已有连接时，用 `get_window_history` 回填历史日志，按 `line_index` 去重。
- 能力授权：`capabilities/default.json` 的 `windows` 用 `["main", "win-*", "theme-editor", "settings"]` 通配。**新窗口无法交互（拖不动、收不到事件）时，先检查这里。**
- 会创建窗口的 command 必须是 `async fn`：同步 command 在主线程里调用 `WebviewWindowBuilder::build()` 会死锁。

## 每个连接的收发历史

每个 `ConnectionHandle` 持有一个 `Arc<RxHistory>`，存 `(seq, Dir, LogLine)`，tx 和 rx 共用同一个序号；等待通知用 `tokio::sync::Notify`。`send_and_read` 和 `get_history_since` 都只读该连接自己的历史，多个连接互不干扰。

`get_history_since` 的游标约定：

- 返回的 `latest_seq` 是**本次实际返回的最后一行**的序号，不是全量最大序号。
- 被 `max_lines` 截断时 `truncated=true`，用 `latest_seq` 接着拉就能无缝续读。如果返回的是全量最大序号，被截掉的那批行就会被游标跳过、永久丢失。
- 缓冲（10,000 行）溢出、游标之后有行已被挤出时 `dropped=true`，让 agent 能区分"数据不完整"和"没有新数据"。

## MCP

- rmcp Streamable HTTP server，`bind_port` 从配置端口起最多尝试 20 个端口，返回已绑定的 listener 交给 tokio，避免先 drop 再 rebind 的竞态。
- MCP 和 GUI 共享同一个 `connections` Arc，这是"人和 agent 共用连接"的前提。
- registry（`%APPDATA%\neoserial\mcp-registry.json`）：每个条目记录 `pid`、`port`、`connections[{com, baud}]`、`started_at`、`heartbeat_at`；每 5 秒心跳一次，写入时先写临时文件再改名；退出时注销。
- 调用记录（`mcp/call_log.rs`）：进程内环形缓冲，最多 200 条，参数截断到 500 字符，只存内存，供 MCP 日志页签显示。
- 本机自检（`mcp/diagnostics.rs`）：只检测本进程实际绑定的地址，做 `initialize` + `tools/list`，不执行串口工具。

## 前端

- Svelte 5 的 `$state` store 管理全部 UI 状态（`.svelte.ts` 后缀）。
- 日志缓存 `LogRetentionBuffer` 同时按行数和原始字节数淘汰，只在追加和淘汰时增减计数，不会每次都扫描全部缓存；行渲染按记录身份复用。
- 自动滚动：日志有更新就跳到最新（`$effect` + `requestAnimationFrame`）。
- 输入法处理：中文输入法用第一次 Enter 确认上屏时，在 `compositionend` 补发送。
- 挂载前先预取设置和快捷指令（`preloadBeforeMount`），避免开窗口时布局跳变。

### 前端诊断

`main.ts` 在 UI 加载前安装通用观察器，页面按 session/seq、原生按 native_seq 关联。应用自己的 HTML 页面默认启用；新控件使用 DOM 观察，新命令/插件使用 Tauri 公共传输入口，新事件通过统一回调观察，不依赖已知故障清单。原有串口下拉和设置窗口的语义阶段记录仅作补充。

真实 Tauri 的 invoke/回调入口不可写。`bridge-bootstrap.js` 通过 Builder 的 `append_invoke_initialization_script` 在 core 定义前接入，仅在定义这三个入口时包装函数，保留只读描述符并立即恢复 `Object.defineProperty`；未完整初始化也会自动恢复。页面观察器连接到独立客户端，不改写已锁定入口。`diagnosticBootstrap.test.ts` 使用锁定版本的官方 core/IPC 初始化脚本验证 Windows/macOS/Linux URL 形式、冻结原型、普通/插件/Channel 调用、错误、传输回退和断开；升级 Cargo.lock 中的 Tauri 时必须刷新对应 fixture。浏览器业务回归也使用同样的只读入口，并默认开启诊断。

| 层 | 实现与自动覆盖 | 边界 |
|---|---|---|
| 操作与界面 | `frontendReplay.ts` 用 rrweb 2.2.1 记录 DOM、属性/CSS、滚动和交互；指针日志带 rrweb 节点 ID | DOM 回放，不是原生屏幕视频；iframe/Canvas/媒体和显式私有区域不录内部 |
| 通信 | `diagnosticBridge.ts` 包装 `__TAURI_INTERNALS__` 的 invoke/回调；`diagnosticNetwork.ts` 观察 HTTP、WebSocket、EventSource、Worker | 记录结构、状态、时间、样本/计数，不存业务正文；Tauri 内部接口升级需跑回归 |
| 共享状态 | `diagnosticState.ts` 自动枚举 store 导出对象，在操作后和每秒检查摘要变化 | 不遍历任意组件闭包或 JS 堆；大集合仅计数和首尾结构，凭据/正文脱敏 |
| 性能与错误 | 全局异常、Promise、资源错误、Long Tasks、内存摘要和 JS 心跳 | 浏览器未提供的性能字段标为空；崩溃后无法继续执行页面代码 |
| 窗口与存活 | `logging/runtime.rs` 独立检查页面心跳，`logging/window.rs` 对所有窗口采样原生状态 | 排除无前端脚本的保活/外部页面；心跳缺失仅是观察事实，不自动认定死锁 |
| Windows 引擎 | `logging/webview.rs` 接导航、ProcessFailed、BrowserProcessExited；内部 Runtime/Log 通知覆盖页面采集器启动前的异常/资源错误，`logging/cdp.rs` 筛选错误、位置和脱敏摘要 | 按窗口记录接入和启用结果；不打开远程调试端口，不展开控制台远程对象，不自动收集内存转储；Mac 不提供这些 WebView2 事件 |

记录层自身的失败不会修改操作返回值、错误对象或取消语义。普通日志运行时每 10 秒最多接收 1000 条、待写队列最多 1000 条；丢弃数显式记录。真实输入使用独立的有界原生提交路径，避免先前写入等待 ACK 时，点击处理函数立即卡死而丢失最后操作。导出确认会等待普通写入、真实输入写入和回放片段提交完成。

网络观察排除 Tauri 的 `ipc:`/`ipc.localhost` 原生传输，由命令观察器统一记录，避免日志写入经 `fetch` 再次触发日志。原生生命周期记录使用单个后台写入线程，待写队列最多 256 条，磁盘阻塞时计数丢弃，恢复后写入缺失标记；导出最多等待原生队列 2 秒，结果写入 `native_queue`。事件/回放写入先异步等待各自的写入许可，避免慢磁盘下大量线程阻塞在存储锁上。页面心跳最多一个请求在途，避免后端阻塞时持续积累回调。异常退出的日志尾部先分行再追加，保留坏行标记和新启动记录。导出拒绝写入诊断存储目录，并限制同一时刻只有一个导出任务。`replay-backpressure.mjs` 覆盖在途大批次延迟失败、重试超过缓冲限额时的缺失标记及新检查点恢复。

回放约每 30 秒或 1000 个事件生成检查点；`logging/replay.rs` 以独立片段存储，单片段上限 8 MiB、总额 64 MiB，淘汰整片段。前端待写队列限 8 MiB/4000 事件，单事件超过 4 MiB或反复录制失败会降级并注明，不反复生成超大快照。检查点携带构建/浏览器环境，即使较早事件日志淘汰也保留版本上下文。`replayPrivacy.ts` 同时处理完整节点和增量属性数组，保留样式差量/属性移除的格式；未知渲染属性默认保留，避免未来组件的 data-state、主题、方向或表格属性被白名单丢弃。UI 固定文字由 Svelte 模板 AST 自动提取，不读取脚本常量、属性或表达式。动态文本和输入脱敏，已知用户文字/凭据属性及外部资源 URL 去除；技术属性仍可能包含业务信息，导出前需检查。回放查看器只在禁脚本 iframe 中重建界面，不运行原来的串口或应用代码。

`logging/frontend.rs` 保存最多 8 × 4 MiB 的事件日志，与串口 FileLogger 独立。导出前收集所有受观察页面的提交确认，最多等 6 秒，未响应/失败窗口在报告中注明；坏行/不完整片段保留缺失标记。JSON 以流式写出，避免再分配整份导出文本。后台模式托盘提供原生导出入口，所有页面失效时也能导出先前记录。

回归 `tests/browser/flight-recorder.mjs` 在运行时创建没有专门记录代码的新控件，验证新控件、未知命令/插件/事件、状态变化、业务脱敏和 2000 行私有区域压力；再从导出的 JSON 在沙箱中还原显隐/尺寸变化。`ui-failure-diagnostics.mjs` 另覆盖遮罩、禁用、隐藏、立即关闭及设置读取阻塞。原生测试验证检查点轮换、损坏处理、看门狗、显示器坐标和导出保护。Windows COM 接口另做 Windows 目标类型检查；实际 Windows WebView2 故障触发仍需实机验证。

发行构建的 `npm run build` 会生成隐藏 source map，并在 Tauri 打包前移到 `frontend-symbols/<build-id>/`，不嵌入安装包。该目录包含资源文件名/hash 清单，构建标识也写入页面启动和回放检查点。工作流保存 Actions artifact（90 天），同时将 `NeoSerial-frontend-symbols.zip` 附在该版本 Release，保留与版本相同的生命周期。用诊断文件的构建标识和压缩 JS 文件名选择 source map，恢复堆栈位置，避免长期使用版本丢失符号。

## 配置持久化

- 写设置统一走 `patch_settings`：以后端内存中的状态为底做深合并。
  - 主界面开关改了，只回写本窗口改动的字段（防抖 200ms）；连接成功时记下 `last_port` 和串口参数。
  - 设置页和主题编辑器只提交改过的项。这样多个窗口、agent、主题编辑器同时修改时互不覆盖。
- 跨窗口同步的设置：主题、字体、编码、预设波特率，以及时间戳 / 行号 / 记录发送三个开关。各窗口独立的设置：端口下拉、HEX 显示、回车换行。
- 文件写入先写 `.tmp` 再改名；文件损坏时改名为 `.bad` 并使用默认值。
- 旧版本升级时自动迁移：
  - 指令缓存从配置目录搬到缓存目录；搬不动就当作缓存丢掉，下次刷新时重建。
  - `settings.json` 里的明文 `api_key` 搬进 DPAPI 凭据文件。原文件先备份为 `settings.json.bak.pre-secret` 再重写，明文立刻从磁盘上消失。
- 数据目录优先级：`NEOSERIAL_DATA_DIR` > exe 同目录的 `neoserial.portable` > 系统标准位置。registry 例外，路径始终固定。

## 开发注意

- Tauri 2 IPC 参数会自动转成 camelCase，前端 invoke 必须传 camelCase（如 `{ baudRate, dataBits }`）。
- `.gitignore` 忽略了 `*.png`，只对 `src/assets/`、`assets/`、`src-tauri/icons/` 开了例外。新增图片要放在这几个目录下，否则不会入库。
