# NeoSerial 串口调试工具 · 设计文档

- **日期**：2026-07-14
- **状态**：已确认，待生成实现计划
- **作者**：用户 + Claude（brainstorming 协作）

## 1. 背景与目标

用户从事通信模组嵌入式开发，市面上的串口工具总有不顺手之处，希望自己实现一个可长期维护、可自行调整的串口调试工具。

技术栈选定：**Rust + Slint + serialport**。理由：

- **serialport 4.9**：Rust 生态最成熟的串口库，跨平台，Windows 一等公民，标准串口参数与流控全覆盖。
- **Slint 1.17**：声明式 UI、Rust 原生、轻量（无 webview 依赖），适合信息密集 + 实时刷新的桌面工具。
- **Rust**：内存安全 + 无 GC 抖动，长时间挂机看 log 稳定。

### 目标用户与交付

- 仅本机自用（Windows 11），源码自行 build，不打包分发。
- MVP 阶段不做安装包、自动更新、驱动提示。

### 成功标准

1. 能稳定打开/关闭串口，收发 AT 命令并查看模组返回。
2. 高频 debug log（偶发几十 KB/s）刷屏时 UI 不卡、不丢显示。
3. 可选存盘，开启后即使高频也不丢字节。
4. 串口热插拔 / 打开失败 / 断开等异常不导致 app 崩溃。
5. 配置（串口参数 + 快捷命令）持久化，重启后恢复。

## 2. MVP 范围

**MVP = 单串口完整体验**：AT 收发 + log 查看共用一个数据视图。架构从第一天就按"多连接"预留，但 UI 先只暴露一个连接。

### MVP 包含

- 单串口连接：端口枚举、参数配置、打开/关闭、状态显示。
- 数据视图：收发着色 + 每行时间戳 + ASCII/HEX 切换。
- 发送：单行输入 + 快捷命令按钮 + 发送历史。
- 高频不卡：环形缓冲 + 50fps（20ms/帧）节流。
- 可选存盘：按需开启，独立线程不阻塞串口读。
- 暂停 / 清屏 / 自动滚动。
- 配置持久化：串口设置 + 快捷命令 + 窗口尺寸。
- 错误处理：打开失败/断开/写失败/存盘失败等不崩溃。

### MVP 不包含（留后续版本）

- v2：多串口标签页（同时观察多个串口）。
- v2：HEX 十六进制网格双面板（偏移/字节/ASCII 三列）。
- v2：多行脚本批量发送。
- v2：日志搜索/过滤。
- v2：右键菜单（复制行/复制 HEX）。
- v2：协议解析层（AT/URC 结构化解析）。
- 后续：XMODEM/YMODEM 固件升级。

### 明确排除（YAGNI）

- 跨平台编译分发（仅本机）。
- 自动更新机制。
- 运行时文件日志（tracing）——MVP 用 `eprintln!` 到 stderr 即可。
- 配置即时保存——MVP 仅窗口关闭时全量写。
- 历史完整回看（暂停时不冻结缓冲）——见 §4 暂停语义。

## 3. 总体架构

### 3.1 架构骨架：连接为中心

核心是一个 **Connection** 抽象：每个串口连接是自包含单元，内含串口句柄 + 环形缓冲 + 读写线程。UI 只是连接的"视图"。

`App` 持有 `Vec<ConnectionHandle>`。MVP 时 Vec 里只有一个元素；加多串口时往 Vec 塞多个、UI 改造成标签页即可，核心逻辑不动。加协议解析时在 reader 后插一层 parser，也不动 Connection 骨架。

### 3.2 线程模型（三线程，职责分离）

```
┌─────────────────────────────────────────────────────────┐
│  UI 线程（主线程，跑 Slint 事件循环）                      │
│  - 持有 ConnectionHandle（弱引用）                        │
│  - 一个 20ms Timer：从环形缓冲取新行 → 刷新 VecModel      │
│  - 用户操作（打开/关闭/发送）→ 通过 handle 下指令          │
└───────────────▲──────────────────────────────────────────┘
                │ invoke_from_event_loop
                │ (新数据/状态变更通知)
┌───────────────┴──────────────────────────────────────────┐
│  Connection 后台线程（每连接一个，阻塞读）                 │
│  - loop: bytes_to_read → read → 行累加 → 推环形缓冲       │
│  - 命令 channel 取出 open/close/send → 执行              │
│  - 异常/断开 → 通知 UI                                   │
└──────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────┐
│  存盘线程（按需 spawn，开启存盘时才启动）                  │
│  - 通过无界 channel 接收原始字节副本（send 纳秒级）       │
│  - 慢慢写文件，磁盘 I/O 全隔离，不阻塞读线程              │
└─────────────────────────────────────────────────────────┘
```

**存盘线程独立的关键理由**：如果在读线程内同步写文件，磁盘 I/O 阻塞会让串口驱动接收缓冲溢出 → 丢数据。而开存盘恰恰是为留底，"以为存了其实丢了"比不开更糟。读线程甩字节副本到 channel 是纳秒级内存操作，永不阻塞。

**存盘按需启停**：不开启存盘 → 不创建存盘线程；开启 → spawn，关闭 → flush + join。不常驻。

### 3.3 Rust ↔ Slint 桥接

- `.slint` 声明：`callback open-connection(...)`、`callback send(string)`、`callback close()`、`in-out property <[LogLine]> log-model`，其中 `LogLine = { ts: string, text: string, color: color, dir: int }`。
- Rust 端 `slint::include_modules!()` 生成类型；主线程 set 回调 → 转发到 ConnectionHandle；Timer 拉数据 → 更新 VecModel。
- 所有 UI↔后台交互都经 `app.rs` 桥接，避免散落的 `invoke_from_event_loop` 调用。

## 4. 核心数据流

### 4.1 接收路径（RX）

```
串口硬件
  │ read(buf)  ── 后台读线程（阻塞读 + 10ms timeout）
  ▼
原始字节块 chunk（Vec<u8>，可能跨行/半行）
  │
  ├──► [若开存盘] channel.send(chunk.clone()) ──► 存盘线程 ──► 文件（原始字节，不切行）
  │
  ▼
行累加器 LineAssembler（维护"未结束的半行"缓冲）
  │ 按 \r\n / \n / \r 切分；未遇换行的尾段留存，等下次拼接
  ▼
完整行 Vec<RawLine>（含时间戳、方向=RX、原始字节）
  │
  ▼
环形缓冲 RingBuffer<LogLine>（cap 5000，满了丢最旧）
  │ LogLine = { ts, dir, raw: Vec<u8>, ascii: String, hex: String, color }
  ▼
UI Timer（20ms 一帧）：取"自上次刷新以来的新增行"
  │
  ▼
VecModel<LogLineView>（{ ts, text, color, dir }）
  │ ASCII 模式 → text=ascii；HEX 模式 → text=hex（16字节折行）
  ▼
ListView + 自定义 delegate（虚拟化，只渲染可见行）
```

**关键设计点：**

1. **行累加器必须存在**。串口 `read` 一次返回的字节边界与换行符边界无关——一次读到 `"AT+\r"`，下次才读到 `"CFUN=1,1\r\nOK\r\n"`。LineAssembler 负责跨 read 拼接。

2. **时间戳打在"行完成时刻"**，非字节到达时刻。一行可能跨多次 read，打在完成时更稳定、更有意义。

3. **LogLine 同时存 raw + ascii + hex**。ASCII↔HEX 切换是高频操作，若每次切换对 5000 行重新转换会有延迟。预存三份，切换只改 VecModel 的 text 字段，瞬间完成。内存代价：5000 行 × 几十字节 ≈ 几百 KB，可忽略。

4. **HEX 模式显示规则**：一行原始字节按 16 字节折成多行，格式 `0000:  41 54 2B 43 46 55 4E 3D  31 2C 31 0D 0A        AT+CFUN=1,1\r\n`（偏移 + 十六进制 + ASCII 旁注）。

5. **着色规则**：RX 浅色（`#c8d3e0`）、TX 绿色（`#7ee787`）、命中 error_keywords 的行红色（`#ff7b72`）。颜色从 model 的 color 字段读。着色默认开启不做开关（零开销：color 是 model 字段，渲染本就要用）。

### 4.2 发送路径（TX）

```
UI 输入框 / 快捷按钮
  │ 用户按回车或点按钮
  ▼
Rust 回调 on_send(text, mode, line_ending)
  │ mode 决定：ASCII 直接 encode；HEX 解析空格分隔的 hex 对
  │ 按设置追加行尾：CR / LF / CRLF / 无
  ▼
channel.send(SendCommand) ──► 后台线程
  │
  ▼
port.write_all(bytes)
  │
  ▼
回显：构造 LogLine(dir=Tx, raw=bytes) 推入环形缓冲
  │ 发送内容进同一日志流，用 TX 绿色区分
  ▼
发送历史栈 push（最近 50 条，上下箭头调出）
  │
  ▼
输入框内容保留（便于微调后连发，如 AT+CSQ → AT+CSQN）
```

**关键点**：发送内容**回显进同一日志流**，与 RX 用同一套着色/时间戳。"我发了什么、模组回了什么"在同一时间轴上对照看，避免眼睛在两个框间来回跳。

### 4.3 暂停与清屏

- **暂停 = 只冻结屏幕，后台照收照丢旧**。暂停不停止读线程（否则缓冲溢出），只暂停 UI Timer 刷新。暂停期间环形缓冲照常丢旧。UI 在暂停按钮旁显示"⏸ 已暂停（后台继续接收）"小字提示，明确语义。这不是"冻结数据流"，是"冻结屏幕看当前内容"。
- **清屏**：清空环形缓冲 + VecModel，不重置时间戳。无二次确认（高频操作，确认很烦；隐含安全感：开了存盘则已存）。

### 4.4 自动滚动

- toggle，默认开。新数据来了若开着就滚到底。
- **用户手动往上滚时自动关闭自动滚动**（否则往上看历史、新数据一来又被拽到底）。滚回底部时自动重新开启。这是体验关键。

## 5. 项目结构与模块划分

### 5.1 目录结构

```
cmserial/
├── Cargo.toml
├── build.rs                      # slint_build::compile("ui/app.slint")
├── ui/                           # Slint UI 声明
│   ├── app.slint                 # 入口：主窗口，组合各面板
│   ├── components/
│   │   ├── connection-bar.slint  # 顶部：端口/波特率/参数/打开按钮
│   │   ├── log-view.slint        # 中部：ListView 日志流
│   │   ├── send-bar.slint        # 底部：输入框 + 快捷按钮 + 发送
│   │   └── toolbar.slint         # 工具栏：ASCII/HEX切换、暂停、清屏、存盘
│   └── styles/
│       └── theme.slint           # 颜色/字号常量
│
└── src/
    ├── main.rs                   # 入口：创建 App、set 回调、跑事件循环
    ├── app.rs                    # App 胶水层：桥接 UI 回调 ↔ Connection
    │
    ├── connection/               # 【核心】连接为中心
    │   ├── mod.rs                # Connection：自包含单元，spawn 读线程
    │   ├── handle.rs             # ConnectionHandle：UI 持有的命令句柄
    │   ├── reader.rs             # 读线程：阻塞 read + 行累加 + 推缓冲
    │   ├── writer.rs             # 写：从命令 channel 取 → write_all
    │   └── line_assembler.rs     # 跨 read 拼行的状态机
    │
    ├── buffer/
    │   ├── ring_buffer.rs        # 环形缓冲<LogLine>，cap 可配，满了丢旧
    │   └── log_line.rs           # LogLine 结构（ts/dir/raw/ascii/hex/color）
    │
    ├── logging/                  # 存盘（按需）
    │   └── file_logger.rs        # 存盘线程：接 channel 写文件
    │
    ├── config/                   # 持久化
    │   └── settings.rs           # 串口设置 + 快捷命令 ↔ JSON
    │
    └── util/
        ├── codec.rs              # ASCII/HEX 互转、行尾处理
        ├── hex_fmt.rs            # 16字节折行 HEX 格式化
        └── time_fmt.rs           # HH:MM:SS.mmm（手写 + 硬编码 +8）
```

### 5.2 模块职责与依赖边界

| 模块 | 职责 | 依赖 | 不依赖 |
|------|------|------|--------|
| `connection` | 一个串口连接的全部生命周期 | serialport, buffer, line_assembler | UI（不 import slint） |
| `handle` | UI→连接的单向命令通道 | crossbeam-channel | UI |
| `buffer` | 纯数据结构，环形缓冲 | 无 | 串口、UI |
| `logging` | 字节流→文件 | std fs | 串口、UI、缓冲 |
| `config` | 设置读写 JSON | serde, serde_json | UI |
| `app` | 胶水：UI 回调↔handle 命令，缓冲数据推回 UI | slint, connection, buffer | 不直接碰 serialport |
| `util` | 无状态纯函数 | 无 | 一切 |

### 5.3 关键隔离原则

1. **`connection` 完全不依赖 Slint**。它是纯 Rust 库式模块，可脱离 UI 单测（喂假字节验缓冲/切行）。
2. **UI 线程不直接调 serialport**。UI 回调 → `app.rs` → `handle.send_command()` → channel → 读线程。任何阻塞都在后台。
3. **`buffer` 与 `logging` 互相独立**。环形缓冲管"屏幕看什么"，存盘管"文件留什么"，数据都来自读线程的同一 chunk 副本，走不同路径、互不阻塞。
4. **`app.rs` 是唯一桥**。所有 UI↔后台交互经它，便于理解和维护。

### 5.4 依赖选型（5 个 crate）

```toml
[dependencies]
slint = "1.17"              # UI
serialport = "4.9"          # 串口
serde = { version = "1", features = ["derive"] }  # 配置序列化
serde_json = "1"            # 配置文件
crossbeam-channel = "0.5"   # 命令/数据 channel（select! 未来用得上）
```

- **不用 chrono**：时间戳手写 `HH:MM:SS.mmm` + 硬编码 +8（本机自用、永远 UTC+8）。`time_fmt.rs` 顶部带 `TZ_OFFSET_HOURS: i64 = 8` 常量及"若跨时区使用需改此处"注释。
- **crossbeam-channel 优于 std mpsc**：多 `select!`，未来加多连接 select 时用得上，API 更顺手。

## 6. UI 布局与交互

### 6.1 主窗口（单视图三段式）

```
┌───────────────────────────────────────────────────────────────┐
│ ▼ 连接栏 ConnectionBar                                          │ ~48px
│  端口[COM3 ▼] 波特率[115200▼] 数据位[8▼] 校验[None▼]            │
│  停止位[1▼] 流控[None▼]      [打开] [关闭]    状态:●已连接         │
├───────────────────────────────────────────────────────────────┤
│ ▼ 工具栏 Toolbar                                                │ ~32px
│  [ASCII/HEX ▼] [⏸暂停] [🗑清屏] [📁存盘:关] [↓自动滚动:开]       │
├───────────────────────────────────────────────────────────────┤
│                                                                 │
│  日志视图 LogView（ListView，虚拟化）                             │ flex:1
│  09:23:45.123  AT+CFUN=1,1                    ← TX 绿色          │
│  09:23:45.240  OK                             ← RX 浅色          │
│  09:23:50.300  +CME ERROR: 100                 ← 红色(关键字)     │
│                                                                 │
├───────────────────────────────────────────────────────────────┤
│ ▼ 发送栏 SendBar                                                │ ~80px
│  [AT▼历史] 输入框[AT+CSQ______________________] [CR▼] [发送]    │
│  快捷: [AT] [ATI] [AT+CSQ] [AT+CFUN=1,1] [AT+COPS?] [+ ▾]      │
└───────────────────────────────────────────────────────────────┘
```

### 6.2 各区交互细节

**连接栏**
- 端口下拉：启动时 `serialport::available_ports()` 枚举，点开下拉时刷新（支持热插拔）。
- 波特率下拉：预设常用值（9600/19200/38400/57600/115200/230400/460800/921600）+ 允许手输任意值（裸 u32，非标准值靠驱动）。
- 数据位 5/6/7/8、校验 None/Even/Odd、停止位 1/1.5/2、流控 None/Software/Hardware，下拉。
- 状态灯：●红未连接 / ●绿已连接 / ●黄正在打开。打开失败用状态栏红字显示原因，不做弹窗（避免打断）。

**工具栏**
- ASCII/HEX 切换：单选下拉，切换瞬间生效（LogLine 预存 ascii+hex，只改 model 的 text 字段）。
- 暂停：toggle，按下高亮 + 标题区显示"⏸ 已暂停（后台继续接收）"。
- 清屏：清缓冲 + model，无二次确认。
- 存盘开关：toggle，开启时弹文件保存对话框选路径，关闭时 flush+关闭文件，开关上显示当前文件名。
- 自动滚动：toggle 默认开。用户手动上滚时自动关闭，滚回底部自动重开。

**发送栏**
- 输入框：回车发送，上/下箭头调历史（最近 50 条）。
- 行尾选择：CR / LF / CRLF / 无，下拉，默认 CRLF（AT 标准）。
- 快捷按钮：每个 `{label, content, mode(ASCII/HEX), line_ending}`，点 `+` 增加，右键编辑/删除，配置持久化。
- 发送后输入框内容**保留**（便于微调连发）。

**日志视图**
- 字体：等宽（Consolas/JetBrains Mono）。
- 单击行：展开显示该行原始字节（HEX + ASCII），方便核对。MVP 可选，右键菜单留 v2。
- 选中文字 Ctrl+C 复制（MVP），右键菜单留 v2。

### 6.3 持久化范围

- 窗口大小/位置、所有连接栏参数、快捷按钮列表、ASCII/HEX 模式、行尾默认值，关闭时全量写。

## 7. 配置持久化与错误处理

### 7.1 配置持久化

存盘位置：`%APPDATA%\cmserial\settings.json`（即 `C:\Users\SR\AppData\Roaming\cmserial\settings.json`）。

```jsonc
{
  "version": 1,
  "window": { "width": 1100, "height": 720, "x": 100, "y": 80 },
  "serial_defaults": {
    "baud_rate": 115200,
    "data_bits": "Eight",
    "parity": "None",
    "stop_bits": "One",
    "flow_control": "None"
  },
  "last_port": "COM3",
  "ui": {
    "display_mode": "Ascii",      // "Ascii" | "Hex"
    "line_ending": "CRLF",        // "Cr" | "Lf" | "CrLf" | "None"
    "auto_scroll": true,
    "ring_buffer_capacity": 5000
  },
  "quick_commands": [
    { "label": "AT",     "content": "AT",          "mode": "Ascii", "line_ending": "CRLF" },
    { "label": "CSQ",    "content": "AT+CSQ",      "mode": "Ascii", "line_ending": "CRLF" },
    { "label": "Reset",  "content": "AT+CFUN=1,1", "mode": "Ascii", "line_ending": "CRLF" },
    { "label": "HEX5A",  "content": "5A A5 01 02", "mode": "Hex",   "line_ending": "None" }
  ],
  "error_keywords": ["ERROR", "FAIL", "+CME ERROR", "+CMS ERROR"]
}
```

**要点：**
- `version` 字段：schema 版本号，预留迁移。MVP 先写版本号、暂不做迁移逻辑。
- **加载策略**：启动时读；文件不存在/损坏 → 用默认配置 + 不报错（静默降级）。损坏时把损坏文件备份成 `settings.json.bad` 再用默认，避免反复损坏。
- **保存时机**：窗口关闭时写全量（MVP 简单可靠）。崩溃会丢"本次会话改的快捷按钮"，可接受；v2 再做即时保存。
- `last_port` 仅作下拉默认项，端口列表每次启动重新枚举（USB 热插拔后端口号会变）。

### 7.2 错误处理（分级，不崩溃优先）

原则：**能恢复的降级，不能恢复的明确报错但不停 app**。

| 场景 | 处理 | 用户感知 |
|------|------|---------|
| 打开失败（端口占用/不存在） | 状态栏红字 `打开失败: COM3 被占用`，保持未连接 | 状态灯红 + 文字，app 正常可用 |
| 读取中端口断开（USB 拔了） | 读线程检测错误 → 通知 UI → 状态切"已断开" + 红字 `连接已断开` | 状态灯红，历史还在可看 |
| 写失败 | 通知 UI 状态栏提示 `发送失败`，不阻断后续操作 | 一行红字，继续用 |
| 配置文件损坏 | 备份 + 用默认配置启动 | 无感知 |
| 存盘写入失败（磁盘满/权限） | 存盘线程通知 UI → 关闭存盘开关 + 红字 `存盘失败，已停止` | 明确提示，避免"以为在存其实没存" |
| HEX 输入解析失败（`5Z` 非法） | 不发送 + 输入框旁红字 `HEX 格式错误`，保留输入便于修改 | 局部提示，不打断 |

**关键模式：端口断开检测**。读线程的 `read` 区分两种错误：
- `io::ErrorKind::TimedOut` → 正常，无数据，继续 loop。
- 其他错误 → 视为断开，通知 UI，退出读线程。

这是"拔了 USB 不崩"的核心。

### 7.3 运行时日志

MVP **不做文件运行日志**（YAGNI）。开发期用 `eprintln!` 打关键事件（端口打开/断开、错误）到 stderr，`cargo run` 时在终端看。遇到疑难问题再加 `tracing`。

## 8. 测试策略

### 8.1 可独立单测的纯逻辑模块

- `line_assembler.rs`：喂模拟的分片字节流（`"AT+\r"` + `"CFUN=1,1\r\nOK\r\n"`），验证拼出三行。覆盖半行、跨 read、混合换行符（\r\n/\n/\r）。
- `ring_buffer.rs`：填满验证丢旧、cap 边界、并发读写的正确性（用 channel 模拟）。
- `codec.rs`：ASCII↔HEX 互转、行尾追加、HEX 解析非法输入报错。
- `hex_fmt.rs`：16 字节折行格式、不足 16 字节的尾行、偏移量正确。
- `time_fmt.rs`：时间戳格式正确、+8 偏移正确（对比已知 UTC 时刻）。
- `config/settings.rs`：序列化/反序列化往返、损坏文件降级、默认值。

### 8.2 集成测试

- `connection` 模块：不依赖 UI，用虚拟串口对（Windows 上用 com0com，或跨平台用 socat/伪终端）验证收发闭环。MVP 可手动验证，自动化留后续。

### 8.3 手动验证清单（MVP 完成时）

1. 打开 COM 口 → 发 `AT` → 收到 `OK`，时间戳与着色正确。
2. 高频 log（向虚拟串口灌 100KB/s）→ UI 20ms 节流不卡、滚动流畅。
3. 开存盘 → 灌数据 → 关闭 → 文件字节完整（对比发送量）。
4. 运行中拔 USB → 状态切"已断开"，app 不崩，历史可看。
5. 暂停 → 屏幕冻结，后台仍在收（恢复后能看到暂停期间的新行）。
6. ASCII↔HEX 切换瞬间生效、内容正确。
7. 关闭重开 → 串口参数、快捷按钮、窗口尺寸恢复。
8. 主动滚到中间 → 自动滚动关闭；滚回底部 → 自动重开。

## 9. 术语表

- **RX/TX**：接收/发送方向。
- **LogLine**：日志流中的一行，含时间戳、方向、原始字节、ASCII/HEX 表示、颜色。
- **LineAssembler**：跨 read 拼行的状态机。
- **ConnectionHandle**：UI 线程持有的、向后台下指令的句柄。
- **环形缓冲**：固定容量、满了丢最旧的数据结构，UI 显示用。
- **存盘线程**：按需启动、通过非阻塞 channel 接收字节副本写文件的独立线程。
- **虚拟化 ListView**：Slint ListView 只实例化可见行，保证大量行渲染开销恒定。
