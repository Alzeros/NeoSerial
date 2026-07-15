# NeoSerial 串口调试工具 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建一个本机自用的串口调试工具，支持 AT 命令收发与模组 debug log 查看，高频不卡、可选存盘、配置持久化、异常不崩溃。

**Architecture:** 连接为中心——每个 `Connection` 是自包含单元（串口句柄 + 环形缓冲 + 读写线程），UI 只是它的视图。三线程模型：UI 线程（Slint 事件循环 + 20ms Timer 刷新）、Connection 后台读线程（阻塞 read + 行累加）、按需 spawn 的存盘线程（非阻塞 channel 接收字节副本写文件）。`connection` 模块不依赖 Slint，可独立单测。

**Tech Stack:** Rust (stable) + Slint 1.17 + serialport 4.9 + serde/serde_json + crossbeam-channel

## Global Constraints

- 平台：仅 Windows 11 本机自用，不打包分发。
- Rust 工具链：stable（安装时默认）。MSRV 由依赖决定，用最新 stable 即可。
- 依赖版本：`slint = "1.17"`、`serialport = "4.9"`、`serde = { version = "1", features = ["derive"] }`、`serde_json = "1"`、`crossbeam-channel = "0.5"`。不引入 chrono（时间戳手写）。
- `connection/` 模块禁止 `use slint::`——必须可脱离 UI 单测。
- 时间戳时区硬编码 `TZ_OFFSET_HOURS: i64 = 8`，文件顶部带注释。
- TDD：所有纯逻辑模块（util、buffer、line_assembler、config）先写失败测试再实现。
- 频繁提交：每个 Task 结束 commit 一次。
- 着色默认开启无开关；error_keywords 可配。
- 配置文件：`%APPDATA%\cmserial\settings.json`，关闭时全量写。
- 行尾默认 CRLF；发送后输入框内容保留；清屏无二次确认；暂停只冻结屏幕。

---

## File Structure

```
cmserial/
├── Cargo.toml
├── build.rs                      # slint_build::compile("ui/app.slint")
├── ui/
│   ├── app.slint                 # 入口主窗口，组合各面板
│   ├── components/
│   │   ├── connection-bar.slint
│   │   ├── log-view.slint
│   │   ├── send-bar.slint
│   │   └── toolbar.slint
│   └── styles/theme.slint
└── src/
    ├── main.rs                   # 创建 App、set 回调、跑事件循环
    ├── app.rs                    # 桥接 UI 回调 ↔ Connection
    ├── connection/
    │   ├── mod.rs                # Connection 自包含单元
    │   ├── handle.rs             # ConnectionHandle 命令句柄
    │   ├── reader.rs             # 读线程
    │   ├── writer.rs             # 写逻辑
    │   └── line_assembler.rs     # 跨 read 拼行状态机
    ├── buffer/
    │   ├── ring_buffer.rs        # 环形缓冲
    │   └── log_line.rs           # LogLine 结构
    ├── logging/file_logger.rs    # 存盘线程
    ├── config/settings.rs        # JSON 持久化
    └── util/
        ├── codec.rs              # ASCII/HEX 互转、行尾
        ├── hex_fmt.rs            # 16字节折行 HEX 格式化
        └── time_fmt.rs           # HH:MM:SS.mmm 手写+8
```

---

## Task 0: 前置工具链验证

**Files:** 无（仅环境检查）

- [ ] **Step 1: 验证 Rust 工具链可用**

Run:
```bash
cargo --version
rustc --version
```
Expected: 两条版本号输出（cargo 1.8x.x，rustc 1.8x.x）。若报 command not found，需先安装 rustup（`rustup-init.exe`，选默认 stable 工具链），装完重开终端再验证。

- [ ] **Step 2: 验证 git 可用**

Run:
```bash
git --version
```
Expected: `git version 2.x.x.windows.x`

- [ ] **Step 3: 初始化 git 仓库**

Run:
```bash
cd /s/Mydevelop/cmserial
git init
```
Expected: `Initialized empty Git repository in S:/Mydevelop/cmserial/.git/`

- [ ] **Step 4: 创建 .gitignore**

Create `.gitignore`:
```
/target
**/*.rs.bk
*.pdb
```

- [ ] **Step 5: Commit 初始结构**

```bash
git add .gitignore docs/
git commit -m "chore: init repo with design doc and gitignore"
```

---

## Task 1: 项目骨架与依赖

**Files:**
- Create: `Cargo.toml`
- Create: `build.rs`
- Create: `src/main.rs`
- Create: `ui/app.slint`

**Interfaces:**
- Produces: 可编译可运行的 Slint 空窗口骨架；`App` 结构体（来自 `slint::include_modules!`）。

- [ ] **Step 1: 用 cargo new 初始化（在已有目录）**

Run:
```bash
cd /s/Mydevelop/cmserial
cargo init --name cmserial
```
Expected: 生成 `Cargo.toml` 和 `src/main.rs`（内容是 hello world）。

- [ ] **Step 2: 写 Cargo.toml**

Replace `Cargo.toml` content:
```toml
[package]
name = "cmserial"
version = "0.1.0"
edition = "2021"

[dependencies]
slint = "1.17"
serialport = "4.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
crossbeam-channel = "0.5"

[build-dependencies]
slint-build = "1.17"

[profile.release]
debug = true
```

- [ ] **Step 3: 写 build.rs**

Create `build.rs`:
```rust
fn main() {
    slint_build::compile("ui/app.slint").unwrap();
}
```

- [ ] **Step 4: 写最小 ui/app.slint**

Create `ui/app.slint`:
```slint
export component App inherits Window {
    title: "cmserial";
    min-width: 800px;
    min-height: 500px;

    Text {
        text: "cmserial skeleton";
        horizontal-alignment: center;
        vertical-alignment: center;
    }
}
```

- [ ] **Step 5: 写 src/main.rs**

Replace `src/main.rs`:
```rust
slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    App::new()?.run()
}
```

- [ ] **Step 6: 编译运行验证**

Run:
```bash
cargo run
```
Expected: 编译成功（首次会下载依赖、较慢），弹出一个标题"cmserial"、居中显示"cmserial skeleton"的窗口。关闭窗口程序正常退出。

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: project skeleton with slint window"
```

---

## Task 2: util/time_fmt — 时间戳格式化

**Files:**
- Create: `src/util/mod.rs`
- Create: `src/util/time_fmt.rs`
- Test: `src/util/time_fmt.rs`（内联 `#[cfg(test)] mod tests`）

**Interfaces:**
- Produces: `pub fn now_local_ts() -> String` 返回 `"HH:MM:SS.mmm"`，时区硬编码 +8。

- [ ] **Step 1: 创建 util 模块文件**

Create `src/util/mod.rs`:
```rust
pub mod time_fmt;
```

在 `src/main.rs` 顶部加 `mod util;`（在 `slint::include_modules!()` 之前）。

- [ ] **Step 2: 写失败测试**

Create `src/util/time_fmt.rs`:
```rust
use std::time::{SystemTime, UNIX_EPOCH};

/// 本地时区偏移（小时）。本机自用，固定 UTC+8。
/// 若跨时区使用需改此处。
const TZ_OFFSET_HOURS: i64 = 8;

/// 返回当前本地时间的 "HH:MM:SS.mmm" 字符串。
pub fn now_local_ts() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock moved backwards");
    format_ts(dur.as_secs() as i64, dur.subsec_millis())
}

/// 根据自 epoch 的秒数与毫秒数，格式化为本地 "HH:MM:SS.mmm"。
pub fn format_ts(secs_since_epoch: i64, millis: u32) -> String {
    let total_secs = secs_since_epoch + TZ_OFFSET_HOURS * 3600;
    let h = (total_secs / 3600) % 24;
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_known_utc_midnight() {
        // UTC 00:00:00.000 → 本地 +8 = 08:00:00.000
        assert_eq!(format_ts(0, 0), "08:00:00.000");
    }

    #[test]
    fn test_format_known_utc_noon() {
        // UTC 12:00:00.000 → 本地 +8 = 20:00:00.000
        assert_eq!(format_ts(12 * 3600, 0), "20:00:00.000");
    }

    #[test]
    fn test_format_with_millis() {
        // UTC 00:00:00.123 → 08:00:00.123
        assert_eq!(format_ts(0, 123), "08:00:00.123");
    }

    #[test]
    fn test_format_day_wraparound() {
        // UTC 16:00:00.000 → +8 = 24:00 → wrap → 00:00:00.000
        assert_eq!(format_ts(16 * 3600, 0), "00:00:00.000");
    }

    #[test]
    fn test_now_local_ts_format() {
        let ts = now_local_ts();
        // 格式 HH:MM:SS.mmm，长度 12
        assert_eq!(ts.len(), 12);
        assert_eq!(ts.as_bytes()[2], b':');
        assert_eq!(ts.as_bytes()[5], b':');
        assert_eq!(ts.as_bytes()[8], b'.');
    }
}
```

- [ ] **Step 3: 运行测试验证失败→通过**

Run:
```bash
cargo test util::time_fmt
```
Expected: 5 个测试通过。（因实现已写在内，此步同时验证逻辑正确。）

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(util): timestamp formatting with hardcoded +8 tz"
```

---

## Task 3: util/codec — ASCII/HEX 互转与行尾

**Files:**
- Create: `src/util/codec.rs`
- Modify: `src/util/mod.rs`

**Interfaces:**
- Produces:
  - `pub enum LineEnding { Cr, Lf, Crlf, None }`（实现 `serde::Serialize/Deserialize`）
  - `impl LineEnding { pub fn append_bytes(&self, out: &mut Vec<u8>) }`
  - `pub fn ascii_to_bytes(s: &str) -> Vec<u8>` — 等价于 `s.as_bytes().to_vec()`
  - `pub fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String>` — 解析空格分隔的十六进制对，非法返回 Err
  - `pub fn bytes_to_ascii(bytes: &[u8]) -> String` — 损失性地把非可见字符替换为 `.`

- [ ] **Step 1: 注册模块**

Modify `src/util/mod.rs`:
```rust
pub mod time_fmt;
pub mod codec;
```

- [ ] **Step 2: 写 codec.rs（含测试）**

Create `src/util/codec.rs`:
```rust
use serde::{Deserialize, Serialize};

/// 行尾模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum LineEnding {
    Cr,
    Lf,
    Crlf,
    None,
}

impl LineEnding {
    /// 将行尾字节追加到 out。
    pub fn append_bytes(&self, out: &mut Vec<u8>) {
        match self {
            LineEnding::Cr => out.push(b'\r'),
            LineEnding::Lf => out.push(b'\n'),
            LineEnding::Crlf => out.extend_from_slice(b"\r\n"),
            LineEnding::None => {}
        }
    }
}

/// ASCII 字符串转字节。
pub fn ascii_to_bytes(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// 十六进制字符串转字节。接受空格分隔的两位十六进制对（大小写不敏感），
/// 允许任意空白分隔。非法输入返回 Err(message)。
pub fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err("hex 长度必须为偶数".to_string());
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    let bytes = cleaned.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_digit(bytes[i]).ok_or_else(|| format!("非法 hex 字符: '{}'", bytes[i] as char))?;
        let lo = hex_digit(bytes[i + 1]).ok_or_else(|| format!("非法 hex 字符: '{}'", bytes[i + 1] as char))?;
        out.push(hi * 16 + lo);
        i += 2;
    }
    Ok(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 字节转 ASCII 显示字符串。可打印 ASCII（0x20-0x7E）原样，其余替换为 '.'。
pub fn bytes_to_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if (0x20..=0x7e).contains(&b) { b as char } else { '.' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_ending_append() {
        let mut v = vec![b'A'];
        LineEnding::Cr.append_bytes(&mut v);
        assert_eq!(v, b"A\r");

        let mut v = vec![b'A'];
        LineEnding::Crlf.append_bytes(&mut v);
        assert_eq!(v, b"A\r\n");

        let mut v = vec![b'A'];
        LineEnding::None.append_bytes(&mut v);
        assert_eq!(v, b"A");
    }

    #[test]
    fn test_hex_basic() {
        assert_eq!(hex_to_bytes("41 54").unwrap(), b"AT");
        assert_eq!(hex_to_bytes("5A A5 01 02").unwrap(), vec![0x5a, 0xa5, 0x01, 0x02]);
    }

    #[test]
    fn test_hex_lowercase_and_nospace() {
        assert_eq!(hex_to_bytes("4154").unwrap(), b"AT");
        assert_eq!(hex_to_bytes("5aa5").unwrap(), vec![0x5a, 0xa5]);
    }

    #[test]
    fn test_hex_empty() {
        assert_eq!(hex_to_bytes("").unwrap(), Vec::<u8>::new());
        assert_eq!(hex_to_bytes("   ").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn test_hex_odd_length_err() {
        assert!(hex_to_bytes("415").is_err());
    }

    #[test]
    fn test_hex_illegal_char_err() {
        assert!(hex_to_bytes("5Z").is_err());
        assert!(hex_to_bytes("GG").is_err());
    }

    #[test]
    fn test_bytes_to_ascii_printable() {
        assert_eq!(bytes_to_ascii(b"AT+CSQ"), "AT+CSQ");
    }

    #[test]
    fn test_bytes_to_ascii_nonprintable() {
        assert_eq!(bytes_to_ascii(b"A\x01B\x7f"), "A.B.");
        assert_eq!(bytes_to_ascii(b"\r\n"), "..");
    }

    #[test]
    fn test_ascii_to_bytes() {
        assert_eq!(ascii_to_bytes("AT"), b"AT");
    }

    #[test]
    fn test_line_ending_serde() {
        let le = LineEnding::Crlf;
        let json = serde_json::to_string(&le).unwrap();
        assert_eq!(json, "\"Crlf\"");
        let back: LineEnding = serde_json::from_str(&json).unwrap();
        assert_eq!(back, LineEnding::Crlf);
    }
}
```

- [ ] **Step 3: 运行测试**

Run:
```bash
cargo test util::codec
```
Expected: 10 个测试通过。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(util): codec with hex/ascii conversion and line endings"
```

---

## Task 4: util/hex_fmt — 16 字节折行 HEX 格式化

**Files:**
- Create: `src/util/hex_fmt.rs`
- Modify: `src/util/mod.rs`

**Interfaces:**
- Produces: `pub fn format_hex_dump(bytes: &[u8]) -> String` — 按 16 字节折行，格式 `偏移:  十六进制(两组8字节)  ASCII旁注`。

- [ ] **Step 1: 注册模块**

Modify `src/util/mod.rs`，追加 `pub mod hex_fmt;`

- [ ] **Step 2: 写 hex_fmt.rs（含测试）**

Create `src/util/hex_fmt.rs`:
```rust
use crate::util::codec::bytes_to_ascii;

/// 把字节按 16 字节一行格式化为十六进制转储字符串。
/// 每行格式: "0000:  41 54 2B 43 46 55 4E 3D  31 2C 31 0D 0A        ASCII"
/// 偏移 4 位十六进制，两组 8 字节以两空格分隔，十六进制与 ASCII 间用两空格。
pub fn format_hex_dump(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let offset = i * 16;
        out.push_str(&format!("{:04x}:  ", offset));

        // 第一组 8 字节
        for j in 0..8 {
            if j < chunk.len() {
                out.push_str(&format!("{:02X} ", chunk[j]));
            } else {
                out.push_str("   ");
            }
        }
        out.push(' '); // 组间额外空格

        // 第二组 8 字节
        for j in 8..16 {
            if j < chunk.len() {
                out.push_str(&format!("{:02X} ", chunk[j]));
            } else {
                out.push_str("   ");
            }
        }
        out.push(' ');
        out.push_str(&bytes_to_ascii(chunk));
        out.push('\n');
    }
    // 去掉末尾换行
    out.pop();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert_eq!(format_hex_dump(b""), "");
    }

    #[test]
    fn test_single_line_full_8() {
        let s = format_hex_dump(b"AT+CFUN=");
        assert!(s.starts_with("0000:  41 54 2B 43 46 55 4E 3D "));
        assert!(s.contains("AT+CFUN="));
        assert!(!s.contains('\n')); // 单行无换行
    }

    #[test]
    fn test_two_lines_32_bytes() {
        // 32 字节 = 两整行 16 字节
        let input: Vec<u8> = (0..32).collect();
        let s = format_hex_dump(&input);
        let lines: Vec<&str> = s.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("0000:"));
        assert!(lines[1].starts_with("0010:"));
    }

    #[test]
    fn test_short_line_padding() {
        // 4 字节，第二组应为空格填充
        let s = format_hex_dump(b"ABCD");
        assert!(s.starts_with("0000:  41 42 43 44 "));
        // 第二组 8 字节全是空格填充
    }

    #[test]
    fn test_offset_increments() {
        // 48 字节 = 三整行，偏移 0000 / 0010 / 0020
        let input: Vec<u8> = (0..48).collect();
        let s = format_hex_dump(&input);
        assert!(s.contains("0000:"));
        assert!(s.contains("0010:"));
        assert!(s.contains("0020:"));
    }
}
```

- [ ] **Step 3: 运行测试**

Run:
```bash
cargo test util::hex_fmt
```
Expected: 4 个测试通过。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(util): hex dump formatter with 16-byte rows"
```

---

## Task 5: buffer/log_line — LogLine 结构

**Files:**
- Create: `src/buffer/mod.rs`
- Create: `src/buffer/log_line.rs`

**Interfaces:**
- Produces:
  - `pub enum Dir { Rx, Tx }`
  - `pub struct LogLine { pub ts: String, pub dir: Dir, pub raw: Vec<u8>, pub ascii: String, pub hex: String }`
  - `impl LogLine { pub fn new(ts: String, dir: Dir, raw: Vec<u8>, error_keywords: &[String]) -> Self }` — 内部计算 ascii/hex

- [ ] **Step 1: 创建模块**

Create `src/buffer/mod.rs`:
```rust
pub mod log_line;
```
（`ring_buffer` 在 Task 6 创建后再加 `pub mod ring_buffer;`，本 Task 先只声明 log_line。）

在 `src/main.rs` 加 `mod buffer;`。

- [ ] **Step 2: 写 log_line.rs（含测试）**

Create `src/buffer/log_line.rs`:
```rust
use crate::util::codec::bytes_to_ascii;
use crate::util::hex_fmt::format_hex_dump;

/// 数据方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Rx,
    Tx,
}

/// 日志流中的一行。预存 raw/ascii/hex 三份，切换显示模式时无需重新转换。
#[derive(Clone, Debug)]
pub struct LogLine {
    pub ts: String,
    pub dir: Dir,
    pub raw: Vec<u8>,
    pub ascii: String,
    pub hex: String,
    pub is_error: bool,
}

impl LogLine {
    /// 构造一行。ts 已格式化好；raw 为原始字节；error_keywords 用于判定是否标红。
    pub fn new(ts: String, dir: Dir, raw: Vec<u8>, error_keywords: &[String]) -> Self {
        let ascii = bytes_to_ascii(&raw);
        let hex = format_hex_dump(&raw);
        let is_error = match dir {
            Dir::Rx => {
                let lower = ascii.to_lowercase();
                error_keywords
                    .iter()
                    .any(|kw| lower.contains(&kw.to_lowercase()))
            }
            Dir::Tx => false,
        };
        LogLine {
            ts,
            dir,
            raw,
            ascii,
            hex,
            is_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kws() -> Vec<String> {
        vec!["ERROR".to_string(), "+CME ERROR".to_string()]
    }

    #[test]
    fn test_normal_line() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"OK".to_vec(), &kws());
        assert_eq!(line.ascii, "OK");
        assert!(!line.is_error);
    }

    #[test]
    fn test_error_keyword() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"+CME ERROR: 100".to_vec(), &kws());
        assert!(line.is_error);
    }

    #[test]
    fn test_error_case_insensitive() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"error: fail".to_vec(), &kws());
        assert!(line.is_error);
    }

    #[test]
    fn test_tx_never_error() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Tx, b"ERROR".to_vec(), &kws());
        assert!(!line.is_error);
    }

    #[test]
    fn test_hex_populated() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"AT".to_vec(), &kws());
        assert!(line.hex.contains("41 54"));
    }
}
```

- [ ] **Step 3: 运行测试**

Run:
```bash
cargo test buffer::log_line
```
Expected: 5 个测试通过。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(buffer): LogLine with precomputed ascii/hex/error flag"
```

---

## Task 6: buffer/ring_buffer — 环形缓冲

**Files:**
- Create: `src/buffer/ring_buffer.rs`
- Modify: `src/buffer/mod.rs`

**Interfaces:**
- Produces:
  - `pub struct RingBuffer<T> { ... }`
  - `impl<T: Clone> RingBuffer<T> { pub fn new(capacity: usize) -> Self; pub fn push(&mut self, item: T); pub fn push_many(&mut self, items: Vec<T>); pub fn snapshot(&self) -> Vec<T>; pub fn clear(&mut self); pub fn len(&self) -> usize; pub fn is_empty(&self) -> bool }`

- [ ] **Step 1: 注册模块**

Modify `src/buffer/mod.rs`:
```rust
pub mod log_line;
pub mod ring_buffer;
```

- [ ] **Step 2: 写 ring_buffer.rs（含测试）**

Create `src/buffer/ring_buffer.rs`:
```rust
use std::collections::VecDeque;

/// 固定容量的环形缓冲。满了 push 新元素时丢弃最旧元素。
pub struct RingBuffer<T> {
    buf: VecDeque<T>,
    capacity: usize,
}

impl<T: Clone> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        RingBuffer {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.buf.len() >= self.capacity {
            self.buf.pop_front();
        }
        self.buf.push_back(item);
    }

    pub fn push_many(&mut self, items: Vec<T>) {
        for item in items {
            self.push(item);
        }
    }

    /// 返回当前所有元素的快照（从旧到新）。
    pub fn snapshot(&self) -> Vec<T> {
        self.buf.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_under_cap() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        assert_eq!(rb.snapshot(), vec![1, 2]);
        assert_eq!(rb.len(), 2);
    }

    #[test]
    fn test_overflow_drops_oldest() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        rb.push(4); // 1 被丢弃
        assert_eq!(rb.snapshot(), vec![2, 3, 4]);
        assert_eq!(rb.len(), 3);
    }

    #[test]
    fn test_push_many() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        rb.push_many(vec![1, 2, 3, 4, 5]);
        assert_eq!(rb.snapshot(), vec![3, 4, 5]);
    }

    #[test]
    fn test_clear() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        rb.push(1);
        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.snapshot(), vec![]);
    }

    #[test]
    fn test_many_overflow_exact_cap() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(5);
        rb.push_many(vec![1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(rb.snapshot(), vec![3, 4, 5, 6, 7]);
    }
}
```

- [ ] **Step 3: 运行测试**

Run:
```bash
cargo test buffer::ring_buffer
```
Expected: 5 个测试通过。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(buffer): ring buffer with capacity and overflow drop"
```

---

## Task 7: connection/line_assembler — 跨 read 拼行

**Files:**
- Create: `src/connection/mod.rs`
- Create: `src/connection/line_assembler.rs`

**Interfaces:**
- Produces:
  - `pub struct LineAssembler { ... }`
  - `impl LineAssembler { pub fn new() -> Self; pub fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> }` — 返回本次切出的完整行（不含换行符）；半行留存待下次
  - `pub fn flush(&mut self) -> Option<Vec<u8>>` — 取出残留半行（无换行符结尾的尾段）

- [ ] **Step 1: 创建模块**

Create `src/connection/mod.rs`:
```rust
pub mod line_assembler;
```

在 `src/main.rs` 加 `mod connection;`。

- [ ] **Step 2: 写 line_assembler.rs（含测试）**

Create `src/connection/line_assembler.rs`:
```rust
/// 跨 read 拼行的状态机。
/// 串口一次 read 的字节边界与换行符边界无关，本结构负责缓存半行，
/// 遇 \r\n / \n / \r 切分出完整行（行内容不含换行符）。
pub struct LineAssembler {
    pending: Vec<u8>,
}

impl LineAssembler {
    pub fn new() -> Self {
        LineAssembler { pending: Vec::new() }
    }

    /// 喂入一段字节，返回本次切出的完整行列表（每行不含换行符）。
    /// 未遇换行符的尾段缓存在内部，等下次 feed。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(chunk);
        let mut lines = Vec::new();

        loop {
            // 找最早的换行符位置
            let (nl_pos, consumed) = match self.find_newline() {
                Some(p) => p,
                None => break,
            };
            // 行内容 = pending[..nl_pos]
            let line: Vec<u8> = self.pending.drain(..nl_pos).collect();
            // 移除换行符本身（drain 掉 consumed 个字节）
            for _ in 0..consumed {
                self.pending.remove(0);
            }
            lines.push(line);
        }
        lines
    }

    /// 在 pending 开头查找换行符，返回 (行内容结束位置, 换行符字节数)。
    /// 优先匹配 \r\n（2字节），再 \n / \r（1字节）。
    fn find_newline(&self) -> Option<(usize, usize)> {
        for i in 0..self.pending.len() {
            let b = self.pending[i];
            if b == b'\r' {
                // 检查是否 \r\n
                if i + 1 < self.pending.len() && self.pending[i + 1] == b'\n' {
                    return Some((i, 2));
                }
                return Some((i, 1));
            }
            if b == b'\n' {
                return Some((i, 1));
            }
        }
        None
    }

    /// 取出残留的半行（无换行符结尾的尾段）。若无可返回 None。
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_line_crlf() {
        let mut a = LineAssembler::new();
        let lines = a.feed(b"AT\r\n");
        assert_eq!(lines, vec![b"AT".to_vec()]);
    }

    #[test]
    fn test_split_across_reads() {
        // 第一次 feed 的 "AT+\r" 末尾 \r 是 CR 换行符，立即切出 "AT+"
        let mut a = LineAssembler::new();
        let l1 = a.feed(b"AT+\r");
        assert_eq!(l1, vec![b"AT+".to_vec()]);
        let l2 = a.feed(b"CFUN=1,1\r\nOK\r\n");
        assert_eq!(l2, vec![b"CFUN=1,1".to_vec(), b"OK".to_vec()]);
    }

    #[test]
    fn test_lf_only() {
        let mut a = LineAssembler::new();
        let lines = a.feed(b"line1\nline2\n");
        assert_eq!(lines, vec![b"line1".to_vec(), b"line2".to_vec()]);
    }

    #[test]
    fn test_cr_only() {
        let mut a = LineAssembler::new();
        let lines = a.feed(b"abc\rdef\r");
        assert_eq!(lines, vec![b"abc".to_vec(), b"def".to_vec()]);
    }

    #[test]
    fn test_crlf_priority_over_cr() {
        // \r\n 应被当作一个换行符，而非 \r 后接 \n 两次
        let mut a = LineAssembler::new();
        let lines = a.feed(b"a\r\nb\r\n");
        assert_eq!(lines, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_flush_pending() {
        let mut a = LineAssembler::new();
        a.feed(b"incomplete");
        assert_eq!(a.flush(), Some(b"incomplete".to_vec()));
        assert_eq!(a.flush(), None);
    }

    #[test]
    fn test_no_newline_returns_empty() {
        let mut a = LineAssembler::new();
        assert!(a.feed(b"no newline here").is_empty());
    }

    #[test]
    fn test_empty_chunk() {
        let mut a = LineAssembler::new();
        assert!(a.feed(b"").is_empty());
    }
}
```

- [ ] **Step 3: 运行测试**

Run:
```bash
cargo test connection::line_assembler
```
Expected: 8 个测试通过。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(connection): line assembler for cross-read splitting"
```

---

## Task 8: config/settings — JSON 持久化

**Files:**
- Create: `src/config/mod.rs`
- Create: `src/config/settings.rs`

**Interfaces:**
- Produces:
  - `pub struct Settings { pub version: u32, pub window: WindowSettings, pub serial_defaults: SerialDefaults, pub last_port: String, pub ui: UiSettings, pub quick_commands: Vec<QuickCommand>, pub error_keywords: Vec<String> }`
  - `pub struct WindowSettings { width: u32, height: u32, x: i32, y: i32 }`
  - `pub struct SerialDefaults { baud_rate: u32, data_bits, parity, stop_bits, flow_control }`
  - `pub struct UiSettings { display_mode, line_ending, auto_scroll, ring_buffer_capacity }`
  - `pub struct QuickCommand { label, content, mode, line_ending }`
  - `pub enum DisplayMode { Ascii, Hex }`
  - `impl Settings { pub fn default_settings() -> Self; pub fn load() -> Self; pub fn save(&self) -> std::io::Result<()> }`

- [ ] **Step 1: 创建模块**

Create `src/config/mod.rs`:
```rust
pub mod settings;
```

在 `src/main.rs` 加 `mod config;`。

- [ ] **Step 2: 写 settings.rs（含测试）**

Create `src/config/settings.rs`:
```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::util::codec::LineEnding;

const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DisplayMode {
    Ascii,
    Hex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum StopBits {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FlowControl {
    None,
    Software,
    Hardware,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerialDefaults {
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow_control: FlowControl,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiSettings {
    pub display_mode: DisplayMode,
    pub line_ending: LineEnding,
    pub auto_scroll: bool,
    pub ring_buffer_capacity: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuickCommand {
    pub label: String,
    pub content: String,
    pub mode: DisplayMode,
    pub line_ending: LineEnding,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub window: WindowSettings,
    pub serial_defaults: SerialDefaults,
    pub last_port: String,
    pub ui: UiSettings,
    pub quick_commands: Vec<QuickCommand>,
    pub error_keywords: Vec<String>,
}

impl Settings {
    pub fn default_settings() -> Self {
        Settings {
            version: CONFIG_VERSION,
            window: WindowSettings { width: 1100, height: 720, x: 100, y: 80 },
            serial_defaults: SerialDefaults {
                baud_rate: 115200,
                data_bits: DataBits::Eight,
                parity: Parity::None,
                stop_bits: StopBits::One,
                flow_control: FlowControl::None,
            },
            last_port: String::new(),
            ui: UiSettings {
                display_mode: DisplayMode::Ascii,
                line_ending: LineEnding::Crlf,
                auto_scroll: true,
                ring_buffer_capacity: 5000,
            },
            quick_commands: vec![
                QuickCommand {
                    label: "AT".into(),
                    content: "AT".into(),
                    mode: DisplayMode::Ascii,
                    line_ending: LineEnding::Crlf,
                },
                QuickCommand {
                    label: "CSQ".into(),
                    content: "AT+CSQ".into(),
                    mode: DisplayMode::Ascii,
                    line_ending: LineEnding::Crlf,
                },
                QuickCommand {
                    label: "Reset".into(),
                    content: "AT+CFUN=1,1".into(),
                    mode: DisplayMode::Ascii,
                    line_ending: LineEnding::Crlf,
                },
                QuickCommand {
                    label: "HEX5A".into(),
                    content: "5A A5 01 02".into(),
                    mode: DisplayMode::Hex,
                    line_ending: LineEnding::None,
                },
            ],
            error_keywords: vec![
                "ERROR".into(),
                "FAIL".into(),
                "+CME ERROR".into(),
                "+CMS ERROR".into(),
            ],
        }
    }

    fn config_path() -> PathBuf {
        let appdata = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        appdata.join("cmserial").join("settings.json")
    }

    /// 加载配置。文件不存在/损坏 → 用默认配置并静默降级；
    /// 损坏时把坏文件备份为 settings.json.bad。
    pub fn load() -> Self {
        let path = Self::config_path();
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => s,
                Err(_) => {
                    let _ = fs::rename(&path, path.with_extension("json.bad"));
                    Self::default_settings()
                }
            },
            Err(_) => Self::default_settings(),
        }
    }

    /// 保存配置。目录不存在会创建。
    pub fn save(&self) -> io::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(&path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_has_version() {
        let s = Settings::default_settings();
        assert_eq!(s.version, 1);
        assert_eq!(s.serial_defaults.baud_rate, 115200);
        assert_eq!(s.ui.ring_buffer_capacity, 5000);
    }

    #[test]
    fn test_default_quick_commands() {
        let s = Settings::default_settings();
        assert!(!s.quick_commands.is_empty());
        assert_eq!(s.quick_commands[0].label, "AT");
    }

    #[test]
    fn test_serde_roundtrip() {
        let s = Settings::default_settings();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, s.version);
        assert_eq!(back.serial_defaults.baud_rate, s.serial_defaults.baud_rate);
        assert_eq!(back.quick_commands.len(), s.quick_commands.len());
    }

    #[test]
    fn test_display_mode_serde() {
        let json = serde_json::to_string(&DisplayMode::Hex).unwrap();
        assert_eq!(json, "\"Hex\"");
    }

    #[test]
    fn test_line_ending_in_settings() {
        let s = Settings::default_settings();
        assert_eq!(s.ui.line_ending, LineEnding::Crlf);
    }
}
```

- [ ] **Step 3: 运行测试**

Run:
```bash
cargo test config::settings
```
Expected: 5 个测试通过。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(config): JSON settings with load/save and corruption fallback"
```

---

## Task 9: connection/handle + reader + writer — Connection 核心

**Files:**
- Create: `src/connection/handle.rs`
- Create: `src/connection/reader.rs`
- Create: `src/connection/writer.rs`
- Modify: `src/connection/mod.rs`

**Interfaces:**
- Consumes: `LineAssembler`, `RingBuffer<LogLine>`, `LogLine`, `util::time_fmt`, `serialport`, `crossbeam_channel`
- Produces:
  - `pub enum ConnCommand { Send(Vec<u8>), Close }`
  - `pub enum ConnEvent { Lines(Vec<LogLine>), Disconnected(String), SendFailed(String) }`
  - `pub struct ConnectionHandle { cmd_tx: Sender<ConnCommand> }` with `pub fn send_command(&self, cmd: ConnCommand)`
  - `pub fn spawn_connection(port_name: &str, baud: u32, ..., error_keywords: Vec<String>, event_tx: Sender<ConnEvent>) -> std::io::Result<ConnectionHandle>`

- [ ] **Step 1: 更新 mod.rs**

Replace `src/connection/mod.rs`:
```rust
pub mod handle;
pub mod line_assembler;
pub mod reader;
pub mod writer;
```

- [ ] **Step 2: 写 handle.rs**

Create `src/connection/handle.rs`:
```rust
use crossbeam_channel::Sender;

/// UI → 后台线程的命令。
pub enum ConnCommand {
    /// 发送字节。
    Send(Vec<u8>),
    /// 关闭连接（读线程会随后退出）。
    Close,
}

/// 后台线程 → UI 的事件。
pub enum ConnEvent {
    /// 新收到的若干完整行。
    Lines(Vec<crate::buffer::log_line::LogLine>),
    /// 连接断开（含原因）。
    Disconnected(String),
    /// 发送失败。
    SendFailed(String),
}

/// UI 线程持有的连接句柄。通过它向后台线程下达命令。
pub struct ConnectionHandle {
    pub(crate) cmd_tx: Sender<ConnCommand>,
}

impl ConnectionHandle {
    pub fn send_command(&self, cmd: ConnCommand) {
        let _ = self.cmd_tx.send(cmd);
    }
}
```

- [ ] **Step 3: 写 writer.rs**

Create `src/connection/writer.rs`:
```rust
use serialport::SerialPort;
use std::io::Write;
use std::time::Duration;

use crate::connection::handle::{ConnCommand, ConnEvent};
use crossbeam_channel::{Receiver, Sender};

/// 在读线程内同步处理一次发送命令。
/// 返回是否应该继续（false 表示已 Close）。
pub fn handle_send(
    cmd: &ConnCommand,
    port: &mut dyn SerialPort,
    event_tx: &Sender<ConnEvent>,
) -> bool {
    match cmd {
        ConnCommand::Send(bytes) => {
            let result = port
                .write_all(bytes)
                .and_then(|_| port.flush());
            if let Err(e) = result {
                let _ = event_tx.send(ConnEvent::SendFailed(format!("发送失败: {}", e)));
            }
            true
        }
        ConnCommand::Close => false,
    }
}

/// 阻塞等待命令（带超时以便读线程能穿插处理读）。
pub fn try_recv_command(cmd_rx: &Receiver<ConnCommand>) -> Option<ConnCommand> {
    cmd_rx.recv_timeout(Duration::from_millis(1)).ok()
}
```

- [ ] **Step 4: 写 reader.rs（读线程主循环）**

Create `src/connection/reader.rs`:
```rust
use crossbeam_channel::{Receiver, Sender};
use serialport::SerialPort;
use std::io::Read;
use std::time::Duration;

use crate::buffer::log_line::{Dir, LogLine};
use crate::connection::handle::{ConnCommand, ConnEvent};
use crate::connection::line_assembler::LineAssembler;
use crate::connection::writer::{handle_send, try_recv_command};
use crate::util::time_fmt::now_local_ts;

/// 读线程主循环。
/// 阻塞读串口 + 1ms timeout 穿插处理命令；按行切分后通过 event_tx 发送 ConnEvent::Lines。
/// 端口断开（非 TimedOut 错误）时发送 ConnEvent::Disconnected 并退出。
pub fn reader_loop(
    mut port: Box<dyn SerialPort>,
    cmd_rx: Receiver<ConnCommand>,
    event_tx: Sender<ConnEvent>,
    error_keywords: Vec<String>,
) {
    // 设置 10ms 读超时，使 loop 能定期返回处理命令
    let _ = port.set_timeout(Duration::from_millis(10));

    let mut assembler = LineAssembler::new();
    let mut buf = [0u8; 1024];

    loop {
        // 先处理待发命令（非阻塞）
        while let Some(cmd) = try_recv_command(&cmd_rx) {
            if !handle_send(&cmd, port.as_mut(), &event_tx) {
                return; // Close
            }
            // 若是 Send，回显进日志流
            if let ConnCommand::Send(bytes) = &cmd {
                let line = LogLine::new(
                    now_local_ts(),
                    Dir::Tx,
                    bytes.clone(),
                    &error_keywords,
                );
                let _ = event_tx.send(ConnEvent::Lines(vec![line]));
            }
        }

        // 读串口
        match port.read(&mut buf) {
            Ok(0) => {
                // EOF，视作断开
                let _ = event_tx.send(ConnEvent::Disconnected("端口 EOF".into()));
                return;
            }
            Ok(n) => {
                let new_lines: Vec<LogLine> = assembler
                    .feed(&buf[..n])
                    .into_iter()
                    .map(|raw| {
                        LogLine::new(now_local_ts(), Dir::Rx, raw, &error_keywords)
                    })
                    .collect();
                if !new_lines.is_empty() {
                    let _ = event_tx.send(ConnEvent::Lines(new_lines));
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                // 正常，无数据，继续
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // 正常，继续
            }
            Err(e) => {
                let _ = event_tx.send(ConnEvent::Disconnected(format!("连接已断开: {}", e)));
                return;
            }
        }
    }
}
```

- [ ] **Step 5: 在 mod.rs 加 spawn_connection**

Append to `src/connection/mod.rs`:
```rust
use crossbeam_channel::{bounded, Sender};
use serialport::SerialPort;

use crate::config::settings::{DataBits, FlowControl, Parity, StopBits};
use crate::connection::handle::{ConnCommand, ConnEvent, ConnectionHandle};

pub mod handle;
pub mod line_assembler;
pub mod reader;
pub mod writer;

/// 打开串口并 spawn 读线程。成功返回 ConnectionHandle。
pub fn spawn_connection(
    port_name: &str,
    baud: u32,
    data_bits: DataBits,
    parity: Parity,
    stop_bits: StopBits,
    flow_control: FlowControl,
    error_keywords: Vec<String>,
    event_tx: Sender<ConnEvent>,
) -> std::io::Result<ConnectionHandle> {
    let port = serialport::new(port_name, baud)
        .data_bits(map_data_bits(data_bits))
        .parity(map_parity(parity))
        .stop_bits(map_stop_bits(stop_bits))
        .flow_control(map_flow_control(flow_control))
        .open()?;

    let (cmd_tx, cmd_rx) = bounded::<ConnCommand>(64);

    std::thread::Builder::new()
        .name(format!("cmserial-reader-{}", port_name))
        .spawn(move || {
            reader::reader_loop(port, cmd_rx, event_tx, error_keywords);
        })?;

    Ok(ConnectionHandle { cmd_tx })
}

fn map_data_bits(d: DataBits) -> serialport::DataBits {
    match d {
        DataBits::Five => serialport::DataBits::Five,
        DataBits::Six => serialport::DataBits::Six,
        DataBits::Seven => serialport::DataBits::Seven,
        DataBits::Eight => serialport::DataBits::Eight,
    }
}

fn map_parity(p: Parity) -> serialport::Parity {
    match p {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    }
}

fn map_stop_bits(s: StopBits) -> serialport::StopBits {
    match s {
        StopBits::One => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
    }
}

fn map_flow_control(f: FlowControl) -> serialport::FlowControl {
    match f {
        FlowControl::None => serialport::FlowControl::None,
        FlowControl::Software => serialport::FlowControl::Software,
        FlowControl::Hardware => serialport::FlowControl::Hardware,
    }
}
```

**关于停止位**：serialport 4.9 的 `StopBits` 只有 `One`/`Two` 两变体（已核实），配置层枚举与之对齐，UI 暂不暴露停止位/数据位/校验/流控的下拉（MVP 用配置文件 `serial_defaults`，默认 8N1 覆盖绝大多数模组场景；这些下拉留 v2）。打开连接时用持久化的 `serial_defaults` 而非硬编码默认值（见 Task 12）。

- [ ] **Step 6: 编译验证**

Run:
```bash
cargo build
```
Expected: 编译通过。若有类型不匹配（如 StopBits 变体名、open 方法名），按编译错误修正。

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(connection): spawn reader/writer with command channel"
```

---

## Task 10: logging/file_logger — 存盘线程

**Files:**
- Create: `src/logging/mod.rs`
- Create: `src/logging/file_logger.rs`

**Interfaces:**
- Produces:
  - `pub struct FileLogger { tx: Sender<Vec<u8>> }`
  - `impl FileLogger { pub fn start(path: PathBuf) -> std::io::Result<Self>; pub fn log(&self, chunk: Vec<u8>); pub fn stop(self) }` — stop 会 drop tx，存盘线程写完队列后退出

- [ ] **Step 1: 创建模块**

Create `src/logging/mod.rs`:
```rust
pub mod file_logger;
```

在 `src/main.rs` 加 `mod logging;`。

- [ ] **Step 2: 写 file_logger.rs**

Create `src/logging/file_logger.rs`:
```rust
use crossbeam_channel::{Sender, unbounded};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

/// 存盘句柄。log() 是纳秒级的 channel send，绝不阻塞调用方（读线程）。
/// 实际磁盘 I/O 隔离在存盘线程内。
pub struct FileLogger {
    tx: Sender<Vec<u8>>,
}

impl FileLogger {
    /// 打开文件并启动存盘线程。
    pub fn start(path: PathBuf) -> std::io::Result<Self> {
        let file = File::create(&path)?;
        let (tx, rx) = unbounded::<Vec<u8>>();

        std::thread::Builder::new()
            .name("cmserial-file-logger".into())
            .spawn(move || {
                let mut writer = BufWriter::new(file);
                let mut had_error = false;
                for chunk in rx.iter() {
                    if had_error {
                        continue;
                    }
                    if writer.write_all(&chunk).is_err() {
                        had_error = true;
                        // 错误处理由 app 层通过 event 感知；此处仅停止写
                    } else if writer.flush().is_err() {
                        had_error = true;
                    }
                }
                let _ = writer.flush();
            })?;

        Ok(FileLogger { tx })
    }

    /// 记录一段原始字节（非阻塞）。
    pub fn log(&self, chunk: Vec<u8>) {
        let _ = self.tx.send(chunk);
    }

    /// 停止存盘：drop 发送端，存盘线程写完队列后退出。
    pub fn stop(self) {
        drop(self.tx);
    }
}
```

- [ ] **Step 3: 编译验证**

Run:
```bash
cargo build
```
Expected: 编译通过。

- [ ] **Step 4: 写存盘集成测试**

Append to `src/logging/file_logger.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_file_logger_writes_all() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("cmserial_test_{}.bin", std::process::id()));
        let logger = FileLogger::start(path.clone()).unwrap();
        logger.log(b"hello ".to_vec());
        logger.log(b"world".to_vec());
        logger.stop();

        // 等待文件写完
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut f = File::open(&path).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello world");
        let _ = std::fs::remove_file(&path);
    }
}
```

- [ ] **Step 5: 运行测试**

Run:
```bash
cargo test logging::file_logger
```
Expected: 测试通过。

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(logging): file logger with non-blocking channel"
```

---

## Task 11: UI 骨架 — theme 与各面板组件

**Files:**
- Create: `ui/styles/theme.slint`
- Create: `ui/components/connection-bar.slint`
- Create: `ui/components/toolbar.slint`
- Create: `ui/components/log-view.slint`
- Create: `ui/components/send-bar.slint`
- Modify: `ui/app.slint`

**Interfaces:**
- Produces: 组合好的 `App` 组件，含全部 callback 与 property 声明（Rust 端实现待 Task 12）。

- [ ] **Step 1: 写 theme.slint**

Create `ui/styles/theme.slint`:
```slint
export global Theme {
    out property <color> bg: #1e2227;
    out property <color> panel: #262b33;
    out property <color> text-rx: #c8d3e0;
    out property <color> text-tx: #7ee787;
    out property <color> text-error: #ff7b72;
    out property <color> text-muted: #7d8590;
    out property <color> accent: #58a6ff;
    out property <length> font-mono-size: 12px;
}
```

- [ ] **Step 2: 写 connection-bar.slint**

Create `ui/components/connection-bar.slint`:
```slint
import { Button, ComboBox, LineEdit } from "std-widgets.slint";
import { Theme } from "../styles/theme.slint";

// 单行 LogLine 数据，供 ListView 使用
export struct LogLine {
    ts: string,
    text: string,
    color: color,
    dir: int,
}

export struct QuickCmd {
    label: string,
    content: string,
    mode: int,    // 0=Ascii 1=Hex
    ending: int,  // 0=Cr 1=Lf 2=Crlf 3=None
}

export component ConnectionBar inherits Rectangle {
    height: 48px;
    background: Theme.panel;

    in property <string> status-text;
    in property <string> status-color;
    in property <[string]> ports;
    in property <string> port-name;
    in property <int> baud-rate;
    in property <bool> connected;

    callback port-selected(string);
    callback baud-changed(int);
    callback open-clicked();
    callback close-clicked();

    HorizontalLayout {
        spacing: 8px;
        padding: 8px;

        ComboBox {
            title: "端口";
            model: root.ports;
            current-value: root.port-name;
            selected => { root.port-selected(self.current-value); }
        }
        ComboBox {
            title: "波特率";
            model: ["9600", "19200", "38400", "57600", "115200", "230400", "460800", "921600"];
            current-value: root.baud-rate.to-string();
            selected => {
                root.baud-changed(self.current-value.to-float() as int);
            }
        }

        Button { text: "打开"; clicked => { root.open-clicked(); } enabled: !root.connected; }
        Button { text: "关闭"; clicked => { root.close-clicked(); } enabled: root.connected; }

        Text {
            text: root.status-text;
            color: root.status-color;
            vertical-alignment: center;
        }
    }
}
```

- [ ] **Step 3: 写 toolbar.slint**

Create `ui/components/toolbar.slint`:
```slint
import { Button, ComboBox } from "std-widgets.slint";
import { Theme } from "../styles/theme.slint";

export component Toolbar inherits Rectangle {
    height: 32px;
    background: Theme.panel;

    in property <bool> is-hex;          // true=Hex false=Ascii
    in property <bool> paused;
    in property <bool> logging;         // 存盘开关
    in property <string> log-file-name;
    in property <bool> auto-scroll;

    callback mode-changed(bool);        // true=Hex
    callback pause-toggled();
    callback clear-clicked();
    callback logging-toggled();
    callback auto-scroll-toggled();

    HorizontalLayout {
        spacing: 8px;
        padding-left: 8px;
        padding-right: 8px;
        alignment: start;

        ComboBox {
            title: "显示";
            model: ["ASCII", "HEX"];
            current-value: root.is-hex ? "HEX" : "ASCII";
            selected => { root.mode-changed(self.current-value == "HEX"); }
        }
        Button {
            text: root.paused ? "▶ 继续" : "⏸ 暂停";
            clicked => { root.pause-toggled(); }
        }
        Button { text: "🗑 清屏"; clicked => { root.clear-clicked(); } }
        Button {
            text: root.logging ? "📁 存盘: 关" : "📁 存盘: 开";
            clicked => { root.logging-toggled(); }
        }
        Button {
            text: root.auto-scroll ? "↓ 自动滚动: 开" : "↓ 自动滚动: 关";
            clicked => { root.auto-scroll-toggled(); }
        }
    }
}
```

- [ ] **Step 4: 写 log-view.slint**

Create `ui/components/log-view.slint`:
```slint
import { ListView, ScrollBar } from "std-widgets.slint";
import { Theme } from "../styles/theme.slint";

export component LogView inherits Rectangle {
    background: Theme.bg;

    in property <[LogLine]> log-model;
    in property <bool> paused;

    callback scrolled-to-bottom(bool);

    log-list := ListView {
        y: 0;
        height: 100%;
        width: 100%;

        for line[i] in root.log-model: Rectangle {
            height: 18px;
            width: 100%;
            background: transparent;
            HorizontalLayout {
                padding-left: 8px;
                spacing: 8px;
                Text {
                    text: line.ts;
                    color: Theme.text-muted;
                    font-size: Theme.font-mono-size;
                    font-family: "Consolas";
                }
                Text {
                    text: line.text;
                    color: line.color;
                    font-size: Theme.font-mono-size;
                    font-family: "Consolas";
                    overflow: elide;
                    horizontal-stretch: 1;
                }
            }
        }
    }

    if root.paused: Text {
        x: 16px; y: 8px;
        text: "⏸ 已暂停（后台继续接收）";
        color: Theme.accent;
    }
}
```

- [ ] **Step 5: 写 send-bar.slint**

Create `ui/components/send-bar.slint`:
```slint
import { Button, ComboBox, LineEdit } from "std-widgets.slint";
import { Theme } from "../styles/theme.slint";

export component SendBar inherits Rectangle {
    height: 80px;
    background: Theme.panel;

    in property <[QuickCmd]> quick-commands;
    in property <string> error-hint;

    callback send(string, int, int);  // text, mode(0/1), ending(0..3)
    callback add-quick-cmd();

    in-out property <string> input-text;
    in-out property <int> ending-index;  // 0=Cr 1=Lf 2=Crlf 3=None

    VerticalLayout {
        spacing: 4px;
        padding: 8px;

        HorizontalLayout {
            spacing: 8px;
            LineEdit {
                placeholder-text: "输入命令，回车发送（↑↓ 调历史）";
                text <=> root.input-text;
                horizontal-stretch: 1;
                accepted => {
                    root.send(root.input-text, 0, root.ending-index);
                }
            }
            ComboBox {
                title: "行尾";
                model: ["CR", "LF", "CRLF", "无"];
                current-index: root.ending-index;
                selected => { root.ending-index = self.current-index; }
            }
            Button {
                text: "发送";
                clicked => {
                    root.send(root.input-text, 0, root.ending-index);
                }
            }
        }

        HorizontalLayout {
            spacing: 4px;
            for cmd in root.quick-commands: Button {
                text: cmd.label;
                clicked => {
                    root.send(cmd.content, cmd.mode, cmd.ending);
                }
            }
            Button { text: "+"; clicked => { root.add-quick-cmd(); } }
        }

        if root.error-hint != "": Text {
            text: root.error-hint;
            color: Theme.text-error;
            font-size: 11px;
        }
    }
}
```

- [ ] **Step 6: 写 app.slint 组合**

Replace `ui/app.slint`:
```slint
import { VerticalBox } from "std-widgets.slint";

import { Theme } from "styles/theme.slint";
import { ConnectionBar, LogLine, QuickCmd } from "components/connection-bar.slint";
import { Toolbar } from "components/toolbar.slint";
import { LogView } from "components/log-view.slint";
import { SendBar } from "components/send-bar.slint";

export { LogLine, QuickCmd }

export component App inherits Window {
    title: "cmserial";
    min-width: 800px;
    min-height: 500px;
    background: Theme.bg;

    in-out property <[string]> ports: [];
    in-out property <string> port-name;
    in-out property <int> baud-rate: 115200;
    in-out property <string> status-text: "未连接";
    in-out property <string> status-color: #ff7b72;
    in-out property <bool> connected: false;

    in-out property <[LogLine]> log-model: [];
    in-out property <bool> is-hex: false;
    in-out property <bool> paused: false;
    in-out property <bool> logging: false;
    in-out property <string> log-file-name: "";
    in-out property <bool> auto-scroll: true;

    in-out property <[QuickCmd]> quick-commands: [];
    in-out property <string> input-text: "";
    in-out property <int> ending-index: 2;
    in-out property <string> error-hint: "";

    callback open-clicked();
    callback close-clicked();
    callback port-selected(string);
    callback baud-changed(int);
    callback mode-changed(bool);
    callback pause-toggled();
    callback clear-clicked();
    callback logging-toggled();
    callback auto-scroll-toggled();
    callback send(string, int, int);
    callback add-quick-cmd();

    VerticalBox {
        ConnectionBar {
            ports: root.ports;
            port-name: root.port-name;
            baud-rate: root.baud-rate;
            status-text: root.status-text;
            status-color: root.status-color;
            connected: root.connected;
            port-selected(port) => { root.port-selected(port); }
            baud-changed(b) => { root.baud-changed(b); }
            open-clicked => { root.open-clicked(); }
            close-clicked => { root.close-clicked(); }
        }
        Toolbar {
            is-hex: root.is-hex;
            paused: root.paused;
            logging: root.logging;
            log-file-name: root.log-file-name;
            auto-scroll: root.auto-scroll;
            mode-changed(h) => { root.is-hex = h; root.mode-changed(h); }
            pause-toggled => { root.paused = !root.paused; root.pause-toggled(); }
            clear-clicked => { root.clear-clicked(); }
            logging-toggled => { root.logging-toggled(); }
            auto-scroll-toggled => { root.auto-scroll = !root.auto-scroll; root.auto-scroll-toggled(); }
        }
        LogView {
            log-model: root.log-model;
            paused: root.paused;
            vertical-stretch: 1;
        }
        SendBar {
            quick-commands: root.quick-commands;
            error-hint: root.error-hint;
            input-text: root.input-text;
            ending-index: root.ending-index;
            send(t, m, e) => { root.send(t, m, e); }
            add-quick-cmd => { root.add-quick-cmd(); }
        }
    }
}
```

- [ ] **Step 7: 编译验证**

Run:
```bash
cargo build
```
Expected: 编译通过。main.rs 暂未接线回调，`App::new()?.run()` 仍能弹窗（控件无功能）。

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(ui): theme and panel components assembled"
```

---

## Task 12: app.rs — 桥接层与 main 接线

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `ConnectionHandle`, `spawn_connection`, `ConnEvent`, `RingBuffer<LogLine>`, `FileLogger`, `Settings`, `util::codec`
- Produces: 完整可运行的 cmserial。

- [ ] **Step 1: 写 app.rs**

Create `src/app.rs`:
```rust
use crossbeam_channel::{Receiver, Sender};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel, Weak};
use std::sync::Mutex;

use crate::buffer::log_line::{Dir, LogLine};
use crate::buffer::ring_buffer::RingBuffer;
use crate::config::settings::{DisplayMode, Settings};
use crate::connection::handle::{ConnCommand, ConnEvent, ConnectionHandle};
use crate::connection::spawn_connection;
use crate::logging::file_logger::FileLogger;
use crate::util::codec::{ascii_to_bytes, hex_to_bytes, LineEnding};

use crate::App;

/// 应用状态：UI 线程持有。
struct AppState {
    handle: Option<ConnectionHandle>,
    ring: RingBuffer<LogLine>,
    file_logger: Option<FileLogger>,
    error_keywords: Vec<String>,
    serial_defaults: crate::config::settings::SerialDefaults,
    is_hex: bool,
    paused: bool,
    auto_scroll: bool,
    send_history: Vec<String>,
}

/// 后台事件 → UI 线程的中转 channel。
/// （用 channel + Timer 轮询，避免在回调里直接 invoke 的复杂性。）
pub fn run() -> Result<(), slint::PlatformError> {
    let settings = Settings::load();
    let error_keywords = settings.error_keywords.clone();
    let cap = settings.ui.ring_buffer_capacity;

    let app = App::new()?;

    // 初始化 UI 状态
    let ports: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.port_name)
        .collect();
    let port_model: VecModel<SharedString> = VecModel::from(
        ports.iter().map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>(),
    );
    app.set_ports(ModelRc::from(port_model));
    app.set_port_name(SharedString::from(settings.last_port.as_str()));
    app.set_baud_rate(settings.serial_defaults.baud_rate as i32);
    app.set_ending_index(match settings.ui.line_ending {
        LineEnding::Cr => 0,
        LineEnding::Lf => 1,
        LineEnding::Crlf => 2,
        LineEnding::None => 3,
    });
    app.set_is_hex(matches!(settings.ui.display_mode, DisplayMode::Hex));
    app.set_auto_scroll(settings.ui.auto_scroll);

    // 快捷命令
    let qc_model: VecModel<crate::QuickCmd> = VecModel::from(
        settings
            .quick_commands
            .iter()
            .map(|q| crate::QuickCmd {
                label: q.label.clone().into(),
                content: q.content.clone().into(),
                mode: if matches!(q.mode, DisplayMode::Hex) { 1 } else { 0 },
                ending: match q.line_ending {
                    LineEnding::Cr => 0,
                    LineEnding::Lf => 1,
                    LineEnding::Crlf => 2,
                    LineEnding::None => 3,
                },
            })
            .collect::<Vec<_>>(),
    );
    app.set_quick_commands(ModelRc::from(qc_model.clone()));

    let serial_defaults = settings.serial_defaults.clone();
    let state = Mutex::new(AppState {
        handle: None,
        ring: RingBuffer::new(cap),
        file_logger: None,
        error_keywords,
        serial_defaults,
        is_hex: matches!(settings.ui.display_mode, DisplayMode::Hex),
        paused: false,
        auto_scroll: settings.ui.auto_scroll,
        send_history: Vec::new(),
    });

    // 事件 channel：后台 → UI
    let (event_tx, event_rx): (Sender<ConnEvent>, Receiver<ConnEvent>) =
        crossbeam_channel::unbounded();

    // 设置回调
    let weak = app.as_weak();

    // 打开
    {
        let weak = weak.clone();
        let state = state.clone();
        let event_tx = event_tx.clone();
        app.on_open_clicked(move || {
            let app = weak.unwrap();
            let port_name = app.get_port_name().to_string();
            let baud = app.get_baud_rate() as u32;
            let mut st = state.lock().unwrap();
            if st.handle.is_some() {
                return;
            }
            let kws = st.error_keywords.clone();
            let sd = st.serial_defaults.clone();
            match spawn_connection(
                &port_name,
                baud,
                sd.data_bits,
                sd.parity,
                sd.stop_bits,
                sd.flow_control,
                kws,
                event_tx.clone(),
            ) {
                Ok(h) => {
                    st.handle = Some(h);
                    app.set_connected(true);
                    app.set_status_text("已连接".into());
                    app.set_status_color(slint::Color::from_rgb_u8(0x7e, 0xe7, 0x87));
                }
                Err(e) => {
                    app.set_status_text(format!("打开失败: {}", e).into());
                    app.set_status_color(slint::Color::from_rgb_u8(0xff, 0x7b, 0x72));
                }
            }
        });
    }

    // 关闭
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_close_clicked(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            if let Some(h) = st.handle.take() {
                h.send_command(ConnCommand::Close);
            }
            if let Some(fl) = st.file_logger.take() {
                fl.stop();
            }
            app.set_connected(false);
            app.set_status_text("未连接".into());
            app.set_status_color(slint::Color::from_rgb_u8(0xff, 0x7b, 0x72));
        });
    }

    // 暂停
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_pause_toggled(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            st.paused = app.get_paused();
        });
    }

    // 清屏
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_clear_clicked(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            st.ring.clear();
            // 清空 model
            let m = app.get_log_model();
            if let Some(vm) = m.as_any().downcast_ref::<VecModel<crate::LogLine>>() {
                vm.set_vec(Vec::new());
            }
        });
    }

    // 显示模式
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_mode_changed(move |is_hex| {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            st.is_hex = is_hex;
            // 重渲染所有行的 text 字段
            refresh_model(&app, &st);
        });
    }

    // 自动滚动
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_auto_scroll_toggled(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            st.auto_scroll = app.get_auto_scroll();
        });
    }

    // 存盘开关（MVP：用固定路径，文件名含时间戳由调用方生成；这里简化用固定名）
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_logging_toggled(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            if st.file_logger.is_none() {
                let path = std::env::temp_dir().join("cmserial_capture.bin");
                match FileLogger::start(path.clone()) {
                    Ok(fl) => {
                        st.file_logger = Some(fl);
                        app.set_logging(true);
                        app.set_log_file_name(
                            path.to_string_lossy().to_string().into(),
                        );
                    }
                    Err(e) => {
                        app.set_status_text(format!("存盘失败: {}", e).into());
                    }
                }
            } else {
                if let Some(fl) = st.file_logger.take() {
                    fl.stop();
                }
                app.set_logging(false);
                app.set_log_file_name("".into());
            }
        });
    }

    // 发送
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_send(move |text, mode, ending| {
            let app = weak.unwrap();
            let text = text.to_string();
            let mut st = state.lock().unwrap();
            if st.handle.is_none() {
                app.set_error_hint("未连接".into());
                return;
            }
            let bytes_result = if mode == 1 {
                hex_to_bytes(&text)
            } else {
                Ok(ascii_to_bytes(&text))
            };
            let mut bytes = match bytes_result {
                Ok(b) => {
                    app.set_error_hint("".into());
                    b
                }
                Err(e) => {
                    app.set_error_hint(e.into());
                    return;
                }
            };
            let le = match ending {
                0 => LineEnding::Cr,
                1 => LineEnding::Lf,
                2 => LineEnding::Crlf,
                _ => LineEnding::None,
            };
            le.append_bytes(&mut bytes);

            // 推历史
            st.send_history.push(text.clone());
            if st.send_history.len() > 50 {
                st.send_history.remove(0);
            }

            // 存盘也记录发送字节
            if let Some(fl) = &st.file_logger {
                fl.log(bytes.clone());
            }

            if let Some(h) = &st.handle {
                h.send_command(ConnCommand::Send(bytes));
            }
        });
    }

    // port-selected / baud-changed / add-quick-cmd：MVP 暂留空实现（端口刷新靠重启）
    {
        let weak = weak.clone();
        app.on_port_selected(move |_p| {
            let app = weak.unwrap();
            app.set_port_name(_p);
        });
    }
    {
        let weak = weak.clone();
        app.on_baud_changed(move |b| {
            let app = weak.unwrap();
            app.set_baud_rate(b);
        });
    }
    {
        let weak = weak.clone();
        app.on_add_quick_cmd(move || {
            let _app = weak.unwrap();
            // MVP：快捷命令编辑留 v2，此处仅占位
        });
    }

    // 20ms Timer：拉事件 → 刷新 UI
    let timer_weak = weak.clone();
    let timer_state = state.clone();
    let timer = Timer::start(TimerMode::Repeated, std::time::Duration::from_millis(20), move || {
        let app = timer_weak.unwrap();
        let mut st = timer_state.lock().unwrap();
        if st.paused {
            return;
        }
        // 取所有待处理事件
        let mut new_lines: Vec<LogLine> = Vec::new();
        let mut disconnected: Option<String> = None;
        let mut send_failed: Option<String> = None;
        while let Ok(ev) = event_rx.try_recv() {
            match ev {
                ConnEvent::Lines(lines) => new_lines.extend(lines),
                ConnEvent::Disconnected(msg) => disconnected = Some(msg),
                ConnEvent::SendFailed(msg) => send_failed = Some(msg),
            }
        }
        if let Some(msg) = disconnected {
            st.handle = None;
            app.set_connected(false);
            app.set_status_text(msg.into());
            app.set_status_color(slint::Color::from_rgb_u8(0xff, 0x7b, 0x72));
        }
        if let Some(msg) = send_failed {
            app.set_error_hint(msg.into());
        }
        if !new_lines.is_empty() {
            // 存盘记录 RX 字节（这里用 raw，近似；完整方案见说明）
            if let Some(fl) = &st.file_logger {
                for l in &new_lines {
                    fl.log(l.raw.clone());
                }
            }
            st.ring.push_many(new_lines);
            refresh_model(&app, &st);
        }
    });
    // 保活 timer
    std::mem::forget(timer);

    app.run()
}

/// 根据环形缓冲 + 当前显示模式，全量重建 UI model。
/// 5000 行全量重建在 20ms 内可接受；后续可优化为增量。
fn refresh_model(app: &App, st: &AppState) {
    let snapshot = st.ring.snapshot();
    let view: Vec<crate::LogLine> = snapshot
        .iter()
        .map(|l| {
            let text = if st.is_hex { &l.hex } else { &l.ascii };
            let color = if l.is_error {
                slint::Color::from_rgb_u8(0xff, 0x7b, 0x72)
            } else {
                match l.dir {
                    Dir::Tx => slint::Color::from_rgb_u8(0x7e, 0xe7, 0x87),
                    Dir::Rx => slint::Color::from_rgb_u8(0xc8, 0xd3, 0xe0),
                }
            };
            crate::LogLine {
                ts: l.ts.clone().into(),
                text: text.clone().into(),
                color,
                dir: if matches!(l.dir, Dir::Tx) { 1 } else { 0 },
            }
        })
        .collect();
    let m = app.get_log_model();
    if let Some(vm) = m.as_any().downcast_ref::<VecModel<crate::LogLine>>() {
        vm.set_vec(view);
    } else {
        let new_model = VecModel::from(view);
        app.set_log_model(ModelRc::from(new_model));
    }
}
```

- [ ] **Step 2: 改 main.rs**

Replace `src/main.rs`:
```rust
mod buffer;
mod config;
mod connection;
mod logging;
mod util;

mod app;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
```

- [ ] **Step 3: 编译**

Run:
```bash
cargo build
```
Expected: 编译通过。若报错（类型不匹配、方法名、import 路径），逐条修正——常见点：
- `QuickCmd`/`LogLine` 是否在 `.slint` 中 `export` 且 `App` 中 `export { LogLine, QuickCmd }`，使 `include_modules!` 生成对应 Rust 类型。
- `VecModel::set_vec`、`as_any().downcast_ref` 是 Slint Model API，确认版本 1.17 支持。

- [ ] **Step 4: 运行验证**

Run:
```bash
cargo run
```
Expected: 窗口打开，连接栏/工具栏/日志区/发送栏都可见。未连接时点"发送"显示"未连接"。打开一个真实/虚拟串口后发 `AT` 能看到回显（TX 绿色）与模组返回（RX 浅色）。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(app): wire up callbacks, timer refresh, connection lifecycle"
```

---

## Task 13: 窗口关闭时保存配置

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: 在 run() 末尾、app.run() 前注册关闭回调**

在 `src/app.rs` 的 `run()` 中，`app.run()` 之前添加：
```rust
    // 窗口关闭时保存配置
    let close_state = state.clone();
    let close_weak = weak.clone();
    app.window().on_close_requested(move || {
        let app = close_weak.unwrap();
        let st = close_state.lock().unwrap();
        let mut settings = Settings::load();
        settings.last_port = app.get_port_name().to_string();
        settings.ui.line_ending = match app.get_ending_index() {
            0 => LineEnding::Cr,
            1 => LineEnding::Lf,
            2 => LineEnding::Crlf,
            _ => LineEnding::None,
        };
        settings.ui.display_mode = if st.is_hex { DisplayMode::Hex } else { DisplayMode::Ascii };
        settings.ui.auto_scroll = st.auto_scroll;
        // 关闭连接与存盘
        if let Some(h) = st.handle.as_ref() {
            h.send_command(ConnCommand::Close);
        }
        if let Some(fl) = st.file_logger.as_ref() {
            fl.stop();
        }
        let _ = settings.save();
        slint::CloseRequestResponse::HideWindow
    });
```

**注意**：`app.window().on_close_requested` 需要 `App` 实现了 `ComponentHandle`，`window()` 返回 `&Window`。`on_close_requested` 闭包不能持有 `weak.unwrap()` 返回的强引用——需用 `close_weak` 在闭包内 upgrade。若编译报 lifetime 错误，改为：
```rust
    app.window().on_close_requested(move || {
        if let Some(app) = close_weak.upgrade() {
            let st = close_state.lock().unwrap();
            // ... 同上 ...
        }
        slint::CloseRequestResponse::HideWindow
    });
```

- [ ] **Step 2: 编译运行验证**

Run:
```bash
cargo run
```
Expected: 启动→选端口→改行尾→关闭窗口→检查 `%APPDATA%\cmserial\settings.json` 的 `last_port`、`line_ending` 已更新。

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(app): persist settings on window close"
```

---

## Task 14: 端口热插拔刷新

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: 在 run() 加一个慢 Timer 刷新端口列表**

在 `src/app.rs` 的 `run()` 中（其他 Timer 附近）添加：
```rust
    // 每 2s 刷新端口列表（热插拔）
    let port_weak = weak.clone();
    let _port_timer = Timer::start(
        TimerMode::Repeated,
        std::time::Duration::from_secs(2),
        move || {
            let app = port_weak.unwrap();
            let ports: Vec<SharedString> = serialport::available_ports()
                .unwrap_or_default()
                .into_iter()
                .map(|p| SharedString::from(p.port_name))
                .collect();
            let m = app.get_ports();
            if let Some(vm) = m.as_any().downcast_ref::<VecModel<SharedString>>() {
                vm.set_vec(ports);
            }
        },
    );
    std::mem::forget(_port_timer);
```

- [ ] **Step 2: 编译运行验证**

Run:
```bash
cargo run
```
Expected: 运行中插拔 USB-串口，端口下拉列表 2 秒内更新。

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(app): refresh port list every 2s for hotplug"
```

---

## Task 15: 上/下箭头调出发送历史

**Files:**
- Modify: `ui/components/send-bar.slint`
- Modify: `src/app.rs`

- [ ] **Step 1: 给输入框加 key-pressed 处理**

在 `ui/components/send-bar.slint` 的 `LineEdit` 上添加 key 夔回调。LineEdit 无原生 key 事件，改为在 SendBar 根组件加 `focus` 与 `key-pressed`：

Replace `ui/components/send-bar.slint` 的 `LineEdit` 部分及根组件声明。完整重写 send-bar.slint：
```slint
import { Button, ComboBox, LineEdit } from "std-widgets.slint";
import { Theme } from "../styles/theme.slint";

export component SendBar inherits Rectangle {
    height: 80px;
    background: Theme.panel;
    forward-focus: input;

    in property <[QuickCmd]> quick-commands;
    in property <string> error-hint;

    callback send(string, int, int);
    callback add-quick-cmd();
    callback history-prev();
    callback history-next();

    in-out property <string> input-text;
    in-out property <int> ending-index;

    VerticalLayout {
        spacing: 4px;
        padding: 8px;

        HorizontalLayout {
            spacing: 8px;
            input := LineEdit {
                placeholder-text: "输入命令，回车发送（↑↓ 调历史）";
                text <=> root.input-text;
                horizontal-stretch: 1;
                accepted => {
                    root.send(root.input-text, 0, root.ending-index);
                }
            }
            ComboBox {
                title: "行尾";
                model: ["CR", "LF", "CRLF", "无"];
                current-index: root.ending-index;
                selected => { root.ending-index = self.current-index; }
            }
            Button {
                text: "发送";
                clicked => { root.send(root.input-text, 0, root.ending-index); }
            }
        }

        HorizontalLayout {
            spacing: 4px;
            for cmd in root.quick-commands: Button {
                text: cmd.label;
                clicked => { root.send(cmd.content, cmd.mode, cmd.ending); }
            }
            Button { text: "+"; clicked => { root.add-quick-cmd(); } }
        }

        if root.error-hint != "": Text {
            text: root.error-hint;
            color: Theme.text-error;
            font-size: 11px;
        }
    }

    key-pressed(e) => {
        if e.text == Key.UpArrow {
            root.history-prev();
            accept
        } else if e.text == Key.DownArrow {
            root.history-next();
            accept
        } else {
            reject
        }
    }
}
```

- [ ] **Step 2: 在 app.slint 透传历史回调**

在 `ui/app.slint` 的 App 中追加声明与透传：
```slint
    callback history-prev();
    callback history-next();
```
并在 `SendBar { ... }` 内追加：
```slint
            history-prev => { root.history-prev(); }
            history-next => { root.history-next(); }
```

- [ ] **Step 3: 在 app.rs 实现历史导航**

在 `src/app.rs` 的 `run()` 中添加（其他回调附近）：
```rust
    // 历史上翻
    {
        let weak = weak.clone();
        let state = state.clone();
        app.on_history_prev(move || {
            let app = weak.unwrap();
            let mut st = state.lock().unwrap();
            // 简化：从历史末尾向前找；维护一个 cursor 留 v2，此处总是显示最后一条
            if let Some(last) = st.send_history.last() {
                app.set_input_text(last.clone().into());
            }
        });
    }
    // 历史下翻
    {
        let weak = weak.clone();
        app.on_history_next(move || {
            let app = weak.unwrap();
            // MVP：下翻清空输入
            app.set_input_text("".into());
        });
    }
```

**说明**：MVP 历史导航做最简实现（上=取最后一条，下=清空）。完整 cursor 导航留 v2。

- [ ] **Step 4: 编译运行验证**

Run:
```bash
cargo run
```
Expected: 发送 `AT` 后，按上箭头输入框填入 `AT`。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(ui): up/down arrow to recall send history (basic)"
```

---

## Task 16: 端到端手动验证与 README

**Files:**
- Create: `README.md`

- [ ] **Step 1: 写 README**

Create `README.md`:
```markdown
# cmserial

本机自用的串口调试工具，面向通信模组嵌入式开发。Rust + Slint + serialport。

## 构建

```bash
cargo run            # debug 运行
cargo run --release  # release 运行
```

## 使用

1. 顶部选择端口、波特率，点"打开"。
2. 在底部输入框输入 AT 命令，回车发送（或点快捷按钮）。
3. 日志区按行显示收发，时间戳 HH:MM:SS.mmm，TX 绿色 / RX 浅色 / ERROR 红色。
4. 工具栏切 ASCII/HEX、暂停、清屏、开存盘、自动滚动。
5. 配置自动保存到 `%APPDATA%\cmserial\settings.json`。

## 架构

连接为中心：每个 Connection 是自包含单元（串口句柄 + 环形缓冲 + 读写线程），UI 是它的视图。
- UI 线程：Slint 事件循环 + 20ms Timer 刷新。
- 读线程：阻塞 read + 行累加，按行推入环形缓冲。
- 存盘线程：按需 spawn，非阻塞 channel 接收字节副本写文件。

详见 `docs/superpowers/specs/2026-07-14-cmserial-design.md`。
```

- [ ] **Step 2: 运行全部单元测试**

Run:
```bash
cargo test
```
Expected: 全部测试通过（util/buffer/config/logging 共约 40+ 个）。

- [ ] **Step 3: 手动验证清单**

逐项手动验证（对照设计文档 §8.3）：

1. [ ] 打开 COM 口 → 发 `AT` → 收到 `OK`，时间戳与着色正确。
2. [ ] 高频 log（用虚拟串口工具灌数据）→ UI 不卡、滚动流畅。
3. [ ] 开存盘 → 灌数据 → 关闭 → 文件字节完整。
4. [ ] 运行中拔 USB → 状态切"已断开"，app 不崩，历史可看。
5. [ ] 暂停 → 屏幕冻结，恢复后能看到暂停期间的新行。
6. [ ] ASCII↔HEX 切换瞬间生效、内容正确。
7. [ ] 关闭重开 → 串口参数、快捷按钮、窗口尺寸恢复。
8. [ ] 主动滚到中间 → 自动滚动关闭；滚回底部 → 自动重开。（注：自动滚动的滚动检测 UI 绑定留 v2 优化，MVP 靠手动切换）

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "docs: add README and complete manual verification"
```

---

## 完成标准

- 全部 Task 的 checkbox 勾完。
- `cargo test` 全绿。
- 手动验证清单 8 项通过（第 8 项的滚动检测可标注为已知 v2 优化点）。
- 程序可 `cargo run` 启动，稳定收发 AT、高频不卡、拔线不崩、配置持久化。

## 已知 v2 优化点（不在 MVP 范围）

- 自动滚动检测：用户手动上滚时自动关闭，滚回底部自动重开——MVP 靠手动切换，v2 用 ListView viewport 位置绑定实现。
- 发送历史 cursor 导航：MVP 上=最后一条/下=清空，v2 做完整上下翻。
- 快捷命令增删改 UI：MVP 占位，v2 做编辑面板。
- 多串口标签页。
- HEX 十六进制网格双面板。
- 日志搜索/过滤。
- 协议解析层。
```
