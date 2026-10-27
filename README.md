<p align="center">
  <img src="assets/neoserial_icon_512.png" width="112" alt="NeoSerial 图标">
</p>

<h1 align="center">NeoSerial</h1>

<p align="center">
  面向通信模组与嵌入式开发的 Windows 串口调试工具，内置 MCP 服务，人和 AI agent 共用同一个串口连接。
</p>

<p align="center">
  <a href="https://github.com/Alzeros/NeoSerial/releases/latest"><img src="https://img.shields.io/github/v/release/Alzeros/NeoSerial?color=0F6E56" alt="Release"></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0F6E56" alt="Platform">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-0F6E56" alt="License"></a>
</p>

<p align="center">
  简体中文 | <a href="README.en.md">English</a>
</p>

<picture>
  <img src="assets/screenshot.png" alt="NeoSerial 主界面">
</picture>

<sub>截图中的端口与日志为模拟数据。</sub>

## 特性

- **原生 overlapped I/O**：直接调用 Win32 API 收发，USB 驱动缓冲满时发送不会卡住界面，高波特率下也能稳定拼行。
- **一个进程，多个窗口**：每个窗口独立连一个串口，并发收发互不阻塞；可托盘常驻，关掉窗口连接仍在后台运行。
- **AI agent 接入**：内嵌 MCP（Streamable HTTP）服务，Claude Code、Codex、Cursor、VS Code Copilot 等客户端可以列端口、收发、跑脚本、查手册。agent 发的命令会显示在界面上，你手动发的命令 agent 也能读到。
- **快捷指令与脚本序列**：多页签命令表，支持逐行延时、循环运行、拖拽排序，可导入导出 JSON。
- **数据处理工具**：帧构造器（自动计算长度与 CRC/SUM/XOR 校验）、编解码（GBK/UTF-8/UTF-16、Base64、转义文本）、短信 PDU 解析与生成。
- **AT 指令联想**：可对接知识库，在输入框里联想手册中的指令，附带语法、参数表和示例；没有知识库时只用发送历史联想。
- **日常细节**：HEX/ASCII 显示、时间戳、错误关键词高亮、日志搜索、文件发送、日志存盘、CTS/DSR 指示、主题编辑器、自动更新。

## 下载安装

从 [Releases](https://github.com/Alzeros/NeoSerial/releases/latest) 下载：

| 文件 | 说明 |
|------|------|
| `NeoSerial_<版本>_x64-setup.exe` | NSIS 安装版，**推荐**。支持自动更新；缺少 WebView2 时会联网安装 |
| `NeoSerial_<版本>_x64-portable.exe` | 免安装单文件。需要系统已有 WebView2（Win11 自带） |

想让免安装版把配置也存在自己旁边（不写 `%APPDATA%`），在 exe 同目录放一个空文件 `neoserial.portable`，详见 [配置与数据](#配置与数据)。

NeoSerial 是**单实例**的：再次启动只会把已有窗口拉到前台，不会再起一个进程去抢 COM 口和 MCP 端口。

## 快速上手

1. 在顶部连接栏选端口（悬停可看设备名）、波特率等参数，点 **连接**。
2. 在底部输入框输入命令，回车或点 **发送**。打开 **回车换行** 会自动追加 CRLF；打开 **HEX发送** 则按十六进制发送。
3. 常用命令写进右侧 **快捷指令** 表，可以单行发送，也可以设置次数和间隔批量运行。
4. 标题栏的 **+** 可以再开一个窗口连另一个串口；齿轮按钮打开设置。
5. 想让 AI agent 操作串口，看 [AI agent 接入（MCP）](#ai-agent-接入mcp)。

## 功能

### 连接与收发

- 参数：端口、波特率（预设列表可在下拉里直接"添加…"）、数据位 5–8、校验 None/Odd/Even、停止位 1/2、流控 None/软件/硬件。
- 端口下拉带设备友好名（通过 SetupAPI 获取，非 USB 端口也有），列表会去重并统一 COM 大小写。未连接时每 2 秒检测一次热插拔。
- 端口已被别的窗口连着时，连接按钮下方会出现提示。
- 发送：ASCII / HEX、可选追加 CRLF、**发送 Ctrl-Z**（0x1A，用于短信文本模式结束输入）。
- **中文标点转半角**（设置 → 通用 → 发送处理）：发送前把 `？，：（）` 等转成半角，引号里的内容不动。只作用于输入框和快捷指令，MCP 发送的内容原样透传。
- 文件发送：按 1024 字节分块并显示进度。进度到 100% 表示驱动已收下全部字节，不是指数据只塞进了队列。
- 状态栏：连接状态、Tx/Rx 字节计数（可清零）；硬件流控时显示 CTS/DSR 电平指示灯。

### 日志显示

- ASCII / HEX 切换、时间戳（`HH:MM:SS.mmm`）、行号、方向标签（`Tx/Rx` 或 `发送/接收`）、暂停、清空。
- 文本解码可选 ASCII / UTF-8 / GBK（设置 → 通用 → 日志内容），只影响显示。
- 错误关键词高亮：默认 `ERROR`、`FAIL`、`+CME ERROR`、`+CMS ERROR`，可增删。只匹配接收行，不区分大小写，改了不用重连。
- 留存上限按行数（1000–10000，默认 5000）和原始字节数（1–16 MiB，默认 4 MiB）同时控制，超出时从最旧的行开始丢；单条超大记录会替换成占位提示。存盘文件不受影响。
- `Ctrl+F` 搜索日志（`F3` / `Shift+F3` 跳到下一个/上一个）。复制时按行和字段重建文本，软换行不会混进复制内容。
- 字体（英文等宽字体与中文字体分别设置）、字号、行高可调，实时预览。

### 日志存盘

- 新建覆盖或续写同一文件，可选是否记录发送内容。
- 不可打印字节转成 `\xHH` 写入；默认存到缓存目录下的 `logs\`。
- 存盘是全局的：所有连接的收发写进同一个文件；任一窗口开始或停止存盘，其他窗口会同步状态。

### 快捷指令与脚本序列

- 结构为"模块 → 页签 → 命令行"。每行可设置：启用、命令、Hex、回车、延时（ms）、注释；有注释时，注释显示在该行的发送按钮上。
- 单行发送（在命令框按回车也行）、表头一键全选/取消、**调整顺序** 模式下拖拽排序、右键菜单（重命名/删除页签、编辑注释、删除行）。
- 批量运行：设置次数和轮间隔，显示进度和待发送条数，可随时停止；端口断开时自动中止，并记录停在哪一行。
- 自动保存；也可以 **另存为** / **加载** JSON。编辑区的字号、行高、字体可调（设置 → 扩展 → 快捷指令）。

### 数据处理

右侧 **数据处理** 页签里有三个工具，可以在设置里单独隐藏：

- **帧构造器**：固定四段 `[帧头][长度][数据][校验]`。
  - 长度字段可选 无 / 1 字节 / 2 字节，支持大小端，覆盖范围可选数据 / 数据+校验 / 整帧。
  - 数据来源：手动输入、随机字符（可勾选字符类型并设最少数量）、随机字节、全 00、全 FF、递增、自定义循环。
  - 校验：CRC16（Modbus / CCITT-FALSE / XModem / ARC）、CRC32、SUM8、XOR8，可选是否把帧头、长度纳入计算。
  - 实时预览，可以直接发送、填入发送框或存为模板。单帧上限 64 KiB。
- **编解码**：文本 ⇄ Hex、Base64 编解码、转义文本 ⇄ Hex（`\r \n \t \0 \xHH` 等）。编码可选 UTF-8 / GBK / UTF-16LE / UTF-16BE，GBK 由原生层处理。
- **短信**：
  - 解析 PDU：支持 `+CMGR` / `+CMT` / `+CMGL` 的返回内容，可粘贴多段长短信合并解析。
  - 生成 PDU：编码可选 GSM 7-bit / UCS-2 / 自动，支持短信中心地址、有效期和长短信拆分（最多 255 段），同时给出 `AT+CMGS=<长度>`。

### 指令联想与指令查询

需要一个提供"手册 AT 指令索引"的知识库服务（HTTP 接口 `GET /api/v1/commands/documents`、`GET /api/v1/commands?document_id=&page=&page_size=`，鉴权头为 `X-API-Key`）。没配置时，联想只用发送历史。

- **配置**：设置 → 扩展 → 指令联想，填写服务地址和 API Key，点 **刷新指令库**。指令会缓存到本地，可以设为启动时自动刷新。
  - API Key 用 DPAPI 加密存放，不会写进 `settings.json`。
  - 刷新是增量的：手册的 `updated_at` 和条数都没变就沿用缓存。单本手册可以强制刷新。
  - 手册按知识库里的分组折叠显示，可以勾选哪些手册参与联想。
- **输入框联想**：默认去掉 `AT+` 前缀后输入满 2 个字符就弹出候选，门槛可调。匹配不区分大小写，输入 `csq` 也能命中 `AT+CSQ`，中文名也能搜。
  - 候选附带中文名、语法、参数表、示例、来源手册和页码。同名指令在多本手册里时合并成一条，可以切换查看各本的版本。
  - 发送历史按前缀匹配，排在手册候选前面。两者各自限量（默认手册 20 条、历史 10 条），互不挤占。
  - 键盘：`↑` 进入列表并选中最佳候选，继续 `↑` 向上翻，`↓` 往回；`Tab` 或高亮状态下回车只填入、不发送；`Esc` 收起。没有高亮时回车照常发送。输入框为空时按 `↑` 可以翻看发送历史。
- **指令查询页签**：在右侧面板独立模糊搜索手册指令，可以用键盘浏览，点示例行就填进输入框。

### 多窗口与后台运行

- 点标题栏的 **+** 开新窗口。所有窗口地位相同，没有"主窗口"：连接不属于某个窗口，窗口只是连接的视图。
- agent 建立的连接、或者窗口关掉后留下的连接，会在 **+** 上显示红点。悬停可以看到这些连接，点一下就开窗口接上，并回填历史日志。
- **托盘常驻**（设置 → 通用 → 后台与退出，默认关）：
  - **开启**：关窗口不断开连接，后台继续收发、存日志、跑序列；关掉所有窗口后应用留在托盘，MCP 照常可用。托盘菜单可以新建窗口、打开或切换到某个连接、退出。
  - **关闭**：关窗口会断开该窗口建立的连接（agent 建立的连接交还给 agent）；关掉最后一个窗口就退出。如果这时 agent 的连接还活着，会先弹窗确认。

### 外观

- 4 套预设主题（3 浅 1 深），另有自定义主题编辑器：分类编辑颜色、主窗口实时预览、悬停高亮对应区域，可导入导出 JSON。
- 无边框窗口与自定义标题栏（新窗口、置顶、折叠右侧面板、设置），自定义右键菜单与下拉框。

### 自动更新

- 默认开启自动检查（每天最多一次，可在设置 → 关于里关闭）。有新版本时标题栏版本号旁会出现提示，可以稍后提醒（7 天）或忽略该版本，也可以在设置 → 关于里手动检查。
- 更新源优先用自建镜像 `dl.neo.loc.cc`（国内可达），失败后改用 GitHub Releases。更新包经 minisign 签名校验，安装后自动重启。

### 快捷键

| 按键 | 作用 |
|------|------|
| `Enter` | 发送（联想列表有高亮时改为填入） |
| `↑` / `↓` / `Tab` / `Esc` | 联想列表导航 / 填入 / 收起；输入框为空时 `↑` 打开发送历史 |
| `Ctrl+F`，`F3` / `Shift+F3` | 搜索日志，跳到下一个/上一个匹配 |
| `Ctrl+A` | 只全选当前聚焦的区域（日志区、MCP 记录、指令详情） |

## AI agent 接入（MCP）

NeoSerial 启动时会在 `127.0.0.1` 起一个 MCP（Streamable HTTP）服务，默认端口 `34594`，被占用时向上顺延。串口始终由 NeoSerial 持有，agent 不直接打开端口，所以人和 agent 可以同时操作同一个连接。

### 添加到客户端

最简单的方式是用 **设置 → 扩展 → MCP 服务** 里的接入向导：选客户端和生效范围，复制命令或配置，再用"本机服务自检"确认服务正常。下面是用户级配置的写法（端口以设置页显示的为准）：

<details>
<summary>Claude Code</summary>

```bash
claude mcp add --scope user --transport http neoserial http://127.0.0.1:34594/mcp
```
</details>

<details>
<summary>Codex</summary>

```bash
codex mcp add neoserial --url http://127.0.0.1:34594/mcp
```

或者合并进 `~/.codex/config.toml`：

```toml
[mcp_servers.neoserial]
url = "http://127.0.0.1:34594/mcp"
```
</details>

<details>
<summary>Cursor（<code>~/.cursor/mcp.json</code>）</summary>

```json
{ "mcpServers": { "neoserial": { "url": "http://127.0.0.1:34594/mcp" } } }
```
</details>

<details>
<summary>VS Code Copilot（<code>.vscode/mcp.json</code> 或用户配置）</summary>

```json
{ "servers": { "neoserial": { "type": "http", "url": "http://127.0.0.1:34594/mcp" } } }
```
</details>

其他客户端：传输方式选 Streamable HTTP，名称填 `neoserial`，URL 填上面的地址。

配好后可以在客户端里发这句话来验证：

> 请调用 NeoSerial 的 list_ports，列出可用串口。只读取端口列表，不要连接串口，不要发送数据；如果工具不可用，请说明，不要猜测结果。

右侧 **MCP 日志** 页签会实时显示 agent 的每次工具调用（工具名、参数、成败、耗时）。

### 工具（19 个）

所有工具都返回 `{ ok: bool, ... }`，`ok=false` 时 `error` 字段给出中文原因。

| 工具 | 说明 |
|------|------|
| `list_ports` | 列出可用串口 |
| `connect` / `disconnect` | 连接 / 断开。端口已被 GUI 或之前的连接占用时直接复用，返回 `reused`、实际 `baud` 和 `owner`（gui / agent） |
| `get_status` | 连接状态、收发字节数、当前序号；硬件流控时附带 `cts` / `dsr` |
| `send` | 只发送，不读响应 |
| `send_and_read` | 发送后阻塞读取响应；`timeout_ms` 是静默间隙超时，只返回接收到的行 |
| `get_history_since` | 拉取某序号之后的收发历史（带方向、`ts` / `ts_ms` 时间戳），被截断时可以续读，不会丢行 |
| `reset_stats` | 清零收发字节统计 |
| `send_file` | 发送文件，二进制安全，用于固件烧录；阻塞到全部写完才返回 |
| `start_logging` / `stop_logging` | 开始 / 停止存盘 |
| `sequence_run` / `sequence_stop` / `get_sequence_status` | 脚本序列：批量命令 + 循环 + 延时，可以轮询进度 |
| `get_settings` / `save_settings` | 读写配置 |
| `kb_search` / `kb_get` / `kb_manuals` | 查本地缓存的手册 AT 指令：搜索、取完整记录（语法/参数/示例）、列手册。不请求知识库，不消耗配额 |

### agent 使用约定

- **发现实例**：读 `%APPDATA%\neoserial\mcp-registry.json`（路径固定，便携模式也写在这里），取 `heartbeat_at` 在 30 秒内的条目，用它的 `port`；`connections` 里列出了已连的 COM 和波特率。没有有效条目说明 NeoSerial 没在运行，可以启动它再等它登记；也可以从 34594 起逐个端口调用 `get_status` 探测。
- **波特率不一致会报错**：`connect` 已连着的端口时，如果请求的波特率和现有连接不同会直接报错，不会悄悄沿用旧值。要改波特率得先 `disconnect`，这会断开用户正在用的连接，应先征得用户同意。
- **重连**：`disconnect` 后立即 `connect` 同一端口，极少数情况下驱动还没释放，会提示"端口被占用"，等 0.5–1 秒重试即可。
- **枚举参数**（`ending`、`data_bits`、`parity`、`flow_control`）不区分大小写，但写错会直接报错，不会悄悄换成默认值。
- **先查手册再发指令**：用 `kb_search` / `kb_get` 查语法。`also_in_count > 0` 说明不同手册里有不同版本，照语法发送前应先看 `kb_get` 的 `also_in`。

> MCP 服务只监听本机回环地址，没有鉴权，本机任何进程都能调用。并强制校验 Origin：所有携带 `Origin` 头的请求（即浏览器环境，包括 DNS rebinding）一律返回 403，只有不带 Origin 的原生 MCP 客户端（Claude Code / Codex / Cursor / VS Code 等）可以接入——浏览器内 MCP 客户端不是受支持的入口。不需要时可以在设置里关闭（重启后生效）。
>
> 并发过载时：HTTP 入口满返回 `429`（带 `Retry-After: 1`）；工具执行层满返回 `isError=true` 的「服务忙」结果（该次调用未执行、无副作用，重试安全，两者判别路径不同）。`disconnect`、`sequence_stop` 与状态查询走专用配额，服务过载时仍可执行。

## 配置与数据

数据分两处存放：**配置目录**（默认 `%APPDATA%\neoserial`）放设置和用户内容，**缓存目录**（默认 `%LOCALAPPDATA%\neoserial`）放可重建或体积大的文件。缓存目录不随漫游配置同步。两个目录可以在设置 → 通用 → 数据目录里查看并直接打开。

| 文件 | 位置 | 内容 |
|------|------|------|
| `settings.json` | 配置目录 | 串口默认值、界面偏好、主题、错误关键词、预设波特率、MCP、指令联想、更新检查等 |
| `sequence.json` | 配置目录 | 快捷指令模块、数据处理工具的配置与模板 |
| `send-history.json` | 配置目录 | 输入框发送历史（默认留 200 条，可调 50–1000） |
| `kb-credential.dat` | 配置目录 | 知识库 API Key，用当前 Windows 用户的 DPAPI 加密 |
| `command-index.json` | 缓存目录 | 手册指令缓存，可以重新刷新得到 |
| `logs\` | 缓存目录 | 默认的日志存盘位置 |
| `mcp-registry.json` | **固定** `%APPDATA%\neoserial` | MCP 端口与已连端口，供 agent 发现实例 |

- **便携模式**：在 exe 同目录放一个空文件 `neoserial.portable`，配置和缓存都会存到 `<exe 目录>\data`，WebView2 数据也一样。也可以用环境变量 `NEOSERIAL_DATA_DIR` 指定目录，它的优先级更高。两种方式都只在启动时生效，已有数据不会自动搬过去。
- **registry 不随便携目录移动**：它是 agent 找实例的约定位置。条目带心跳，读取方会检查是否新鲜，即使被漫游同步到别的机器也不会误导 agent。
- 配置文件损坏时会改名为 `.bad` 备份，然后使用默认值；文件先写到临时文件再改名，断电不会留下写了一半的 JSON。
- 多个窗口、agent 和主题编辑器同时修改设置时，只提交改动的字段，互不覆盖。

## 开发

### 环境

- Windows 10/11 x64，已安装 WebView2
- [Rust](https://rustup.rs/) stable（MSVC 工具链）与 Tauri 2 的 [系统依赖](https://v2.tauri.app/start/prerequisites/)
- Node.js：构建前端用 20 及以上即可；`npm run test:unit` 依赖 Node 原生的 TypeScript 类型剥离，需要 **22.18 或更高**
- 浏览器回归测试需要本机装有 Chrome 或 Edge

### 构建

```bash
npm install
npm run tauri dev                       # 开发模式（Vite + Cargo，前端改动热重载）
npm run tauri build                     # 发布构建：NSIS 安装包 + 更新包签名
npm run tauri build -- --no-bundle      # 只编译 exe，不打包
```

`tauri.conf.json` 开了 updater 产物签名，打安装包时需要设置 `TAURI_SIGNING_PRIVATE_KEY`（以及 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`），否则打包这一步会报错。本地自用可以加 `--no-bundle`，产物在 `src-tauri/target/release/neoserial.exe`，可以直接运行。

代码里保留了非 Windows 平台的 `serialport` 兜底路径，但目前只发布和测试 Windows 版。

### 编译期注入默认知识库（可选）

构建时设置下面两个环境变量，它们会编译进二进制作为兜底值：内网用户装上就能用指令联想，不用自己填地址和 Key。用户在设置页填了值的话，以用户填的为准。

```bash
NEOSERIAL_KB_BASE_URL=http://kb.intranet:8200
NEOSERIAL_KB_API_KEY=kb_xxxxxxxx
```

- 注入值不会写进配置文件，也不会下发给界面：设置页只显示"已内置（留空即用）"，`get_settings` 和 devtools 里都拿不到。清空输入框就回到内置值。
- 不设置就等于没配置，联想只用发送历史。CI 从 GitHub Secrets 读取同名变量。本地可以写在 `src-tauri/.cargo/config.toml` 的 `[env]` 段（已 gitignore）：
  ```toml
  [env]
  NEOSERIAL_KB_BASE_URL = "http://kb.intranet:8200"
  NEOSERIAL_KB_API_KEY = "kb_xxxxxxxx"
  ```
  注意 TOML 里要加引号，GitHub Secret 输入框和 shell export 里**不要加引号**。万一带上了引号，`clean_injected` 也会把外层引号去掉。
- 改了变量要重新编译才生效（`build.rs` 声明了 `rerun-if-env-changed`）。
- **注入的 Key 会随二进制一起分发，用 `strings` 就能看到。** Secrets 只能保证 Key 不进 git 历史，不能保证安装包里没有 Key。所以这种方式只适合内网只读接口；要彻底解决，得让客户端不持有长期 Key。

### 测试

```bash
cd src-tauri && cargo test     # Rust 单元测试
npm run test:unit              # 前端纯函数与源码契约测试（node --test，Node ≥ 22.18）
npm run check                  # svelte-check 类型检查
npm run test:ui                # 界面回归：快捷指令表、发送区开关、窗口布局
npm run test:persistence       # 持久化回归：面板自动保存、设置并发保存
```

- `test:ui` 和 `test:persistence` 不需要 Rust 和串口设备：它们会启动临时 Vite 服务和独立的无头 Chrome/Edge，使用模拟 IPC，不读写真实配置。找不到浏览器时，用 `BROWSER_BINARY` 指定可执行文件路径。
- `npm run test:ui -- --screenshots <目录>` 会额外输出浅色、深色和最小窗口尺寸的截图（数据为模拟）。
- `tests/browser/` 下还有按功能拆分的回归脚本：
  - 用同一套无头环境、可以直接 `node tests/browser/<名称>.mjs` 运行的：`com-selection`、`mcp-setup`、`motion-regressions`、`settings-groups`、`settings-help`、`settings-reuse`、`settings-scrollbar`、`settings-shortcuts`、`titlebar-layout`、`update-reminder`。
  - 其余脚本（`codec-*`、`log-*`、`mcp-settings`、`port-tip`、`settings-close`、`theme-window`）需要先手动起 Vite 和远程调试浏览器，或者需要一个**一次性**的原生实例（单独的 `NEOSERIAL_DATA_DIR` 并开启 WebView2 远程调试）。运行方式见各脚本头部注释，不要对着自己日常用的配置跑。

### 发布

1. 同步修改版本号：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`，以及 `src/components/SettingsDialog.svelte`、`src/components/UpdaterCard.svelte` 里的版本兜底文案；`package-lock.json` 和 `Cargo.lock` 会随之更新，一并提交。
2. 更新 `.github/workflows/release.yml` 里的 `releaseBody`（本次更新说明）。
3. 推送 `v*` tag。CI 会构建并签名 NSIS 安装包、生成 `latest.json`、上传免安装版，然后把安装包和改写过下载地址的 `latest.json` 同步到镜像服务器。

需要配置的 Secrets：

| Secret | 用途 |
|--------|------|
| `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 更新包签名（必需） |
| `NEOSERIAL_KB_BASE_URL` / `NEOSERIAL_KB_API_KEY` | 编译期注入默认知识库（可选，不配就不注入） |
| `SERVER_SSH_KEY_B64` / `MIRROR_HOST` / `MIRROR_USER` / `MIRROR_PATH` | 同步到更新镜像（可选，这一步失败不影响发布） |

### 技术栈与架构

前端 Svelte 5 + TypeScript + Tailwind CSS + Vite；后端 Rust + Tauri 2 + windows-sys（overlapped I/O）+ crossbeam-channel + rmcp（MCP）。线程模型、数据通路、多窗口事件路由、MCP 内部实现和目录结构见 [ARCHITECTURE.md](ARCHITECTURE.md)。

## 已知限制

- 只提供 Windows x64 版本。
- 时间戳固定按 UTC+8 显示（`src-tauri/src/util/time_fmt.rs`）。
- 日志存盘是全局单文件，多个连接同时存盘时内容会交织在一起。
- MCP 服务没有鉴权（见上文）。

## 许可证

[MIT](LICENSE)
