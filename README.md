# NeoSerial

本机自用的串口调试工具，面向通信模组嵌入式开发。经典工程软件风格（参考 SSCOM/XCOM）。Tauri 2 + Svelte 5 + Rust + serialport。

## 构建

```bash
npm install          # 安装前端依赖
npm run tauri dev    # 开发模式（同时启动 Vite + Cargo）
npm run tauri build  # 发布构建（生成 NSIS 安装包）
```

## 技术栈

- **前端**: Svelte 5（$state 响应式）+ TypeScript + Tailwind CSS + Vite + lucide-svelte（图标）
- **后端**: Rust + Tauri 2 + serialport 4.9 + crossbeam-channel
- **插件**: tauri-plugin-dialog / tauri-plugin-fs / tauri-plugin-shell
- **通信**: Tauri IPC（invoke / listen）+ 批量 Event 推送

## 界面布局

自定义无边框窗口，左主区 + 右脚本面板（可拖拽调宽、可折叠），信息密度高、一屏完成常用操作：

```
┌──────────────────────────────────────────────────────────────┐
│ 标题栏：应用名 | 置顶 | 脚本折叠 | 设置 | 窗口控制(最小/最大/关闭)│
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

连接为中心：每个 Connection 是自包含单元（串口句柄 + 读线程 + 写线程），UI 是它的视图。

### 数据通路

```
设备 → serialport read → LineAssembler(跨 read 拼行)
     → LogLine(预存 raw/ascii/hex) → 批量 emit "rx-line" → 前端 logLines[]
                                  → FileLogger channel → 存盘线程写文件
```

### 线程模型

- **读线程**: 阻塞 read（50ms 超时）→ `LineAssembler` 按 `\r\n` / `\n` / `\r` 切行（跨 read 边界安全）→ 构造 `LogLine`（预存 raw/ascii/hex 三份，切换显示模式无需重新转换）→ 批量 emit Tauri Event（攒够 16 行或超过 5ms）。超时时 flush 不换行的残尾（如 shell 提示符 `> `）。
- **写线程**: 通过 crossbeam channel（容量 64）接收 `WriteCommand`，`write_all` + `flush` 确保数据立即传输。手动/文件发送用 `Send`（写 + emit tx-line）；脚本序列用 `SendSilent`（只写不 emit，序列线程自行按发起时刻 emit，避免被写阻塞拖慢节奏）。
- **存盘线程**: `FileLogger` 独立线程 + `BufWriter`，通过 unbounded channel 接收行文本，`send` 纳秒级不阻塞读线程。
- **监控线程**: 等待读/写线程都退出后清理状态并通知前端断开。

### 连接模式自动降级

1. 首选 `port.try_clone()` → 独立句柄模式（读/写各持有一个端口，无锁，性能最优）
2. 克隆失败（某些 USB 转串口芯片不支持）→ `Arc<Mutex<>>` 共享端口模式（降级，前端弹提示）

### 前端

- Svelte 5 `$state` 响应式 store 管理全部 UI 状态
- `logLines` 数组上限 10,000 行，超出时丢弃最旧行
- 自动滚动：日志有更新即跳到最新一条（`$effect` + `requestAnimationFrame` 兜底）
- IME 组合处理：中文输入法首次 Enter 用于确认上屏时，`compositionend` 补发发送

### 配置与持久化

| 文件 | 路径 | 说明 |
|------|------|------|
| 设置 | `%APPDATA%\neoserial\settings.json` | 串口默认值、UI 偏好、命令组、错误关键词、预设波特率、主题 |
| 脚本序列 | `%APPDATA%\neoserial\sequence.json` | 脚本模块/页签/命令（防抖 800ms 自动保存） |
| 日志文件 | `%APPDATA%\neoserial\logs\` | 默认存盘目录（文件名 `YYYYMMDD_HHMMSS.log`） |

- 配置文件损坏时自动备份为 `.bad` 并回退默认值
- 支持旧格式迁移（`quick_commands` → `command_groups`）
- 时间戳固定 UTC+8

## 功能

### 串口通信

- 串口参数配置（端口/波特率/数据位 5-8/校验 None·Odd·Even/停止位 1·2/流控 None·Soft·Hard）
- 手动发送（HEX/ASCII、行尾选择 CR/LF/CRLF/None、回车发送、发送 Ctrl-Z 即 0x1A）
- 文件发送（1024 字节分块读取 + 实时进度条）
- 串口热插拔检测（未连接时每 2 秒轮询端口列表）

### 数据显示

- ASCII / HEX 显示切换（HEX 模式带 ASCII 解析列）
- 时间戳（HH:MM:SS.mmm）
- 方向标签（Tx/Rx 或 发送/接收，可在设置中切换）
- 错误关键词高亮（默认 ERROR / FAIL / +CME ERROR / +CMS ERROR，可自定义）
- 暂停/继续接收、清空日志
- 日志区字号、行高可调（设置中实时预览）

### 日志记录

- 可选记录发送内容（记录发送开关即时同步后端）
- 支持新建覆盖 / 停止后续写同一文件
- 存盘格式：不可打印字节转 `\xHH`，保留全部原始信息

### 脚本序列

- 模块 → 页签 → 命令行三级结构（当前预置"快捷指令"模块，最多 6 个页签）
- 每行：启用/命令/HEX/回车/延时(ms)/注释
- 单行发送（点行号按钮即发送）、全选/全选Hex/全选回车批量切换
- 拖拽排序（指针事件实现，不依赖 HTML5 DnD）
- 批量运行：运行次数、循环间隔(ms)、实时进度条、执行状态（轮次/条数/用时）
- 右键菜单：重命名页签、删除页签、编辑注释、删除行
- 配置自动保存/加载（`sequence.json`），另存为/手动加载 JSON

### 主题与外观

- 4 套预设主题（暖白青 / 雾灰松 / 深海夜航 / 暖砂陶），点击即时预览
- 自定义标题栏（窗口置顶、脚本面板折叠、设置菜单、关于弹窗）
- 窗口最小尺寸硬约束（无边框窗口下防 resize grip 突破下限）

### 统计

- 收发字节数实时统计（Tx/Rx），支持清零

## 测试

```bash
cargo test           # Rust 单元测试（LineAssembler / RingBuffer / LogLine / Settings / FileLogger / codec / hex_fmt / time_fmt）
npm run check        # Svelte/TypeScript 类型检查
```
