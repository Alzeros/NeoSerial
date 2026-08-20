# NeoSerial

本机自用的串口调试工具，面向通信模组嵌入式开发。经典工程软件风格（参考 SSCOM/XCOM）。Tauri 2 + Svelte 5 + Rust + serialport。

**单 exe 多窗口** + **内嵌 MCP server**（Claude Code 等 AI agent 经由它操作串口）+ **自动更新**。

## 构建

```bash
npm install          # 安装前端依赖
npm run tauri dev    # 开发模式（同时启动 Vite + Cargo）
npm run tauri build  # 发布构建（生成 NSIS 安装包）
```

## 技术栈

- **前端**: Svelte 5（$state 响应式）+ TypeScript + Tailwind CSS + Vite + lucide-svelte（图标）
- **后端**: Rust + Tauri 2 + serialport 4.9 + crossbeam-channel + rmcp 3.1（MCP streamable HTTP）
- **插件**: tauri-plugin-dialog / fs / shell / updater / process
- **通信**: Tauri IPC（invoke + 定向 emit_to）+ MCP HTTP

## 多窗口 + MCP 共享串口

核心特性：一个 exe 开多个独立窗口，每个窗口连一个串口；同时内嵌一个 MCP server，AI agent 与人**共享同一串口连接**——agent 发的命令显示在 GUI 窗口，人发的命令 agent 能读到历史。

- **多窗口**：点标题栏 `+` 开新窗口（完整界面的复制品），各窗口独立选端口连接，并发收发不阻塞。窗口 label 用 `win-{自增}`，与连接解耦。
- **MCP 共享连接**：agent `connect` 一个已被 GUI 连的端口 → 复用，不报冲突。`ConnectionHandle.window_label`（`Arc<RwLock>`）记录连接归属窗口，GUI 接管 MCP 连接时改此值，reader/writer 线程下次 emit 自动用新 label，无需重启线程。
- **事件定向**：所有事件（rx-line/tx-line/sequence-progress 等）用 `emit_to(window_label)` 定向到归属窗口，前端用 `getCurrentWebview().listen` 接收，多窗口不串流。
- **能力（capability）**：`capabilities/default.json` 的 `windows: ["main", "win-*"]` 用通配符授权所有窗口的 listen/start-dragging/close 等权限。

### MCP 工具（15 个）

| 工具 | 能力 |
|------|------|
| `list_ports` | 列可用串口 |
| `connect` / `disconnect` | 连接/断开（连接复用，不报冲突） |
| `get_status` | 查连接状态 + rx 序号 |
| `send` | 发数据（不读） |
| `send_and_read` | 发后阻塞读响应（静默间隙超时） |
| `get_history_since` | 拉收发历史（tx+rx，带方向，seq 基准） |
| `reset_stats` | 清零收发字节统计 |
| `send_file` | 发送文件（二进制安全，固件/烧录） |
| `start_logging` / `stop_logging` | 开始/停止存盘（全局） |
| `sequence_run` / `sequence_stop` | 脚本序列（批量命令+循环+延时，自动化测试） |
| `get_settings` / `save_settings` | 读/写配置（持久化） |

agent 发现实例：读 `%APPDATA%/neoserial/mcp-registry.json`，按 COM 口找已连实例，或从默认端口 34594 起 `get_status` 探测。Claude Code 配置：

```
claude mcp add --transport http neoserial http://localhost:34594/mcp
```

约定见项目根 `CLAUDE.md`。

## 界面布局

自定义无边框窗口，左主区 + 右脚本面板（可拖拽调宽、可折叠），信息密度高、一屏完成常用操作：

```
┌──────────────────────────────────────────────────────────────┐
│ 标题栏：应用名 | + 新窗口 | 置顶 | 脚本折叠 | 设置 | 窗口控制   │
├──────────────────────────────────────────────┬───────────────┤
│ 串口配置（端口/波特率/数据位/校验/停止位/流控）│               │
├──────────────────────────────────────────────┤    脚本序列    │
│                                              │   侧边栏       │
│                  数据显示区                   │  （可折叠/     │
│              （HEX/ASCII、时间戳、            │   可拖拽调宽） │
│               方向标签、错误高亮）             │               │
│                                              │  模块 → 页签   │
├──────────────────────────────────────────────┤  → 命令表格    │
│ 工具条 + 开关组 + 发送输入                    │               │
│ 文件发送 / 日志保存（可折叠）                  │               │
├──────────────────────────────────────────────┤               │
│ 状态栏：连接状态 / Tx / Rx / 清零             │               │
└──────────────────────────────────────────────┴───────────────┘
```

## 架构

连接为中心：每个 ConnectionHandle 是自包含单元（串口句柄 + 读/写线程 + per-conn 收发历史 + window_label），UI 是它的视图。

### 数据通路

```
设备 → serialport read → LineAssembler(跨 read 拼行)
     → LogLine(预存 raw/ascii/hex) → emit_to(window_label, "rx-line") → 该窗口 logLines[]
                                  → 喂 per-conn RxHistory(供 MCP 阻塞读/历史查询)
                                  → FileLogger channel → 存盘线程写文件
```

### 线程模型

- **读线程**: 阻塞 read（50ms 超时）→ `LineAssembler` 按 `\r\n` / `\n` / `\r` 切行 → 构造 `LogLine` → 批量 `emit_to`（攒够 16 行或超过 5ms）+ push per-conn `RxHistory`。超时时 flush 不换行的残尾。
- **写线程**: crossbeam channel（容量 64）接收 `WriteCommand`，`write_all` + `emit_to` tx-line/tx-update。手动/文件发送用 `Send`；脚本序列用 `SendSilent`（序列线程自行 emit，避免被写阻塞拖慢节奏）。
- **存盘线程**: `FileLogger` 独立线程 + `BufWriter`，全局单份（多连接收发进同一文件）。
- **监控线程**: 等待读/写线程都退出后清理状态并通知归属窗口断开。

### 连接模式自动降级

1. 首选 `port.try_clone()` → 独立句柄模式（读/写各持有一个端口，无锁，性能最优）
2. 克隆失败 → `Arc<Mutex<>>` 共享端口模式（降级，前端弹提示）

### per-connection 收发历史隔离

每个 `ConnectionHandle` 持 `Arc<RxHistory>`（存 (seq, Dir, LogLine)，tx+rx 共用序号）。reader 喂 Rx、`emit_tx_line` 喂 Tx。`send_and_read` 从该连接 history 读、`get_history_since` 返回该连接的，多连接不串数据。

### 前端

- Svelte 5 `$state` 响应式 store 管理全部 UI 状态
- `logLines` 上限 10,000 行，超出丢弃最旧
- 自动滚动：日志有更新即跳最新（`$effect` + `requestAnimationFrame` 兜底）
- IME 组合处理：中文输入法首次 Enter 用于确认上屏时，`compositionend` 补发发送
- 端口下拉默认显示上次连的端口（不在可用列表则取第一个）

### 配置与持久化

| 文件 | 路径 | 说明 |
|------|------|------|
| 设置 | `%APPDATA%\neoserial\settings.json` | 串口默认值、UI 偏好、命令组、错误关键词、预设波特率、主题、MCP 设置 |
| 脚本序列 | `%APPDATA%\neoserial\sequence.json` | 脚本模块/页签/命令（防抖自动保存） |
| 日志文件 | `%APPDATA%\neoserial\logs\` | 默认存盘目录 |
| MCP registry | `%APPDATA%\neoserial\mcp-registry.json` | 多实例 port→COM 映射（供 agent 发现） |

- 配置损坏自动备份为 `.bad` 并回退默认值
- 时间戳固定 UTC+8

## 功能

### 串口通信

- 串口参数配置（端口/波特率/数据位 5-8/校验/停止位 1·2/流控）
- 手动发送（HEX/ASCII、行尾 CR/LF/CRLF/None、回车发送、Ctrl-Z）
- 文件发送（1024 字节分块 + 进度条）
- 端口冲突提示（同一端口已连时显示淡红提示）
- 串口热插拔检测（未连接时每 2 秒轮询端口列表）

### 数据显示

- ASCII / HEX 显示切换
- 时间戳（HH:MM:SS.mmm）
- 方向标签（Tx/Rx 或 发送/接收，可切换）
- 错误关键词高亮（默认 ERROR/FAIL/+CME ERROR/+CMS ERROR，可自定义）
- 暂停/继续、清空、字号行高可调

### 日志记录

- 可选记录发送内容（开关即时同步后端）
- 新建覆盖 / 续写同一文件
- 存盘格式：不可打印字节转 `\xHH`

### 脚本序列

- 模块 → 页签 → 命令行结构（预置"快捷指令"模块）
- 每行：启用/命令/HEX/回车/延时/注释
- 单行发送、批量切换、拖拽排序
- 批量运行：运行次数、循环间隔、进度条、执行状态
- 右键菜单：重命名/删除页签、编辑注释、删除行
- 自动保存/加载，另存为/手动加载 JSON

### 主题与外观

- 4 套预设主题，点击即时预览
- 自定义标题栏（新窗口、置顶、脚本折叠、设置、关于）
- 窗口最小尺寸硬约束

### 自动更新

- 设置 → 关于 → 检查更新（手动，避免每次开设置都请求）
- 主源 `dl.neo.loc.cc`（自建服务器，国内可达）+ GitHub release fallback
- minisign 签名验证，下载安装后自动 relaunch
- CI：打 `v*` tag 触发 GitHub Actions（tauri-action）构建+签名+发布，latest.json 同步到镜像

### 统计

- 收发字节数实时统计（Tx/Rx），支持清零

## 测试

```bash
cargo test           # Rust 单元测试（111 passed）
npm run check        # Svelte/TypeScript 类型检查
```

## 源码

[github.com/Alzeros/NeoSerial](https://github.com/Alzeros/NeoSerial)
