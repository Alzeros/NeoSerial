#![cfg(target_os = "windows")]

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Devices::Communication::{
    BuildCommDCBAndTimeoutsW, EscapeCommFunction, GetCommModemStatus, SetCommMask, SetCommState,
    SetCommTimeouts, WaitCommEvent, COMMTIMEOUTS, DCB, EV_CTS, EV_DSR, MS_CTS_ON, MS_DSR_ON,
    SETDTR, SETRTS,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE, DUPLICATE_SAME_ACCESS, TRUE,
    WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile,
    FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::{CancelIo, CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, WaitForSingleObject,
};

// DCB 位域:windows-sys 把 winbase.h 里 fBinary..fAbortOnError 这组 1/2 bit 字段打包进
// `_bitfield`,按声明序从 bit0 起——bit0 fBinary、bit2 fOutxCtsFlow、bit4-5 fDtrControl、
// bit12-13 fRtsControl。DTR_CONTROL_*/RTS_CONTROL_* 取值:DISABLE=0、ENABLE=1、HANDSHAKE=2。
const DCB_F_BINARY: u32 = 1 << 0;
const DCB_F_OUTX_CTS_FLOW: u32 = 1 << 2;
const DCB_F_DTR_CONTROL_SHIFT: u32 = 4;
const DCB_F_RTS_CONTROL_SHIFT: u32 = 12;
const DCB_F_DTR_CONTROL_MASK: u32 = 0b11 << DCB_F_DTR_CONTROL_SHIFT;
const DCB_F_RTS_CONTROL_MASK: u32 = 0b11 << DCB_F_RTS_CONTROL_SHIFT;
const DTR_CONTROL_ENABLE: u32 = 1;
const RTS_CONTROL_ENABLE: u32 = 1;
const RTS_CONTROL_HANDSHAKE: u32 = 2;

pub struct WinPort {
    handle: HANDLE,
    /// 波特率,供 [`WinPort::write_timeout_ms`] 按数据长度推算写超时。
    baud_rate: u32,
}

/// 一次 overlapped I/O 的收尾结果,见 [`WinPort::finish_overlapped`]。
struct OverlappedOutcome {
    /// 已传输字节数。超时被取消时是取消前已完成的部分。
    transferred: usize,
    /// WaitForSingleObject 超时(已发 CancelIo 并等到取消完成)。
    timed_out: bool,
    /// GetOverlappedResult 报告的失败;取消后通常是 ERROR_OPERATION_ABORTED。
    error: Option<io::Error>,
}

// SAFETY: WinPort holds a raw Windows HANDLE to a COM port.
// The handle is only accessed from one thread at a time (reader or writer).
unsafe impl Send for WinPort {}

impl WinPort {
    pub fn open(
        port_name: &str,
        baud_rate: u32,
        data_bits: u8,
        parity: &str,
        stop_bits: u8,
        flow_control: &str,
    ) -> io::Result<Self> {
        let full_name = if port_name.starts_with('\\') {
            port_name.to_string()
        } else {
            format!("\\\\.\\{}", port_name)
        };
        let mut name: Vec<u16> = full_name.encode_utf16().collect();
        name.push(0);

        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                0x80000000 | 0x40000000, // GENERIC_READ | GENERIC_WRITE
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                0 as *mut core::ffi::c_void,
            )
        };

        if handle == INVALID_HANDLE_VALUE as HANDLE {
            return Err(io::Error::last_os_error());
        }

        let port = WinPort { handle, baud_rate };
        port.configure(baud_rate, data_bits, parity, stop_bits, flow_control)?;

        // DTR/RTS 拉高。configure 里 DCB 已设 *_CONTROL_ENABLE,SetCommState 即生效,
        // 这里再显式 escape 一次兜底。用命名常量而非裸数字:SETRTS=3、SETDTR=5、CLRDTR=6,
        // 极易混——写成 6 就是刚拉高的 DTR 被立刻清掉,RTS 从未拉高。
        // 硬件流控下 RTS 归驱动握手控制(RTS_CONTROL_HANDSHAKE),手动 SETRTS 会被拒绝,跳过。
        unsafe {
            EscapeCommFunction(handle, SETDTR);
            if !flow_control.eq_ignore_ascii_case("hardware") {
                EscapeCommFunction(handle, SETRTS);
            }
        }

        Ok(port)
    }

    fn configure(
        &self,
        baud_rate: u32,
        data_bits: u8,
        parity: &str,
        stop_bits: u8,
        flow_control: &str,
    ) -> io::Result<()> {
        let flow = flow_control.to_lowercase();
        let software_flow = flow == "software";
        let hardware_flow = flow == "hardware";

        // BuildCommDCBAndTimeoutsW 按 mode 字串填 DCB(超时另由下面的 SetCommTimeouts 下发)。
        // mode 字串的 xon= 只认 on|off(XON/XOFF 软件流控);硬件流控不能写在这里
        // (xon=hard 之类非法值会让 BuildCommDCB 直接失败,选硬件流控就连不上),
        // 而是下面直接设 DCB 位。
        let dcb_str = format!(
            "baud={} parity={} data={} stop={} to=off xon={}",
            baud_rate,
            match parity.to_lowercase().as_str() {
                "none" => "n",
                "odd" => "o",
                "even" => "e",
                _ => "n",
            },
            data_bits,
            stop_bits,
            if software_flow { "on" } else { "off" },
        );
        let mut dcb_str_w: Vec<u16> = dcb_str.encode_utf16().collect();
        dcb_str_w.push(0);

        let mut dcb: DCB = unsafe { std::mem::zeroed() };
        dcb.DCBlength = std::mem::size_of::<DCB>() as u32;

        // BuildCommDCBAndTimeouts 的 COMMTIMEOUTS 参数是 [out]:按字串里的 to= 填结构体
        // (to=off 全填 0),并不下发到句柄。这里只当占位接收,真正的超时值在下面
        // 用 SetCommTimeouts 显式下发。
        let mut scratch_timeouts: COMMTIMEOUTS = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            BuildCommDCBAndTimeoutsW(dcb_str_w.as_ptr(), &mut dcb, &mut scratch_timeouts)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        // BuildCommDCB 只改字串里出现的字段:dtr=/rts= 没给,零初始化的 DCB 里
        // fDtrControl/fRtsControl 就是 *_CONTROL_DISABLE,SetCommState 会把两根线拉低。
        // 很多 USB CDC 设备(STM32 VCP / Arduino 原生 USB / RP2040 stdio / AT&D2 模组)
        // 见 DTR 低就不吐数据或直接挂断——显式 ENABLE,与 serialport 路径
        // (write_data_terminal_ready/write_request_to_send 均 true)一致。
        // fBinary 同样显式置 1:Windows 不支持非二进制模式,SetCommState 要求它为 TRUE。
        // 硬件流控(RTS/CTS 握手):fOutxCtsFlow=1(对端 CTS 拉低即停发)+
        // fRtsControl=HANDSHAKE(接收缓冲快满时驱动自动拉低 RTS);否则 RTS 常高。
        let rts_control = if hardware_flow { RTS_CONTROL_HANDSHAKE } else { RTS_CONTROL_ENABLE };
        dcb._bitfield &= !(DCB_F_DTR_CONTROL_MASK | DCB_F_RTS_CONTROL_MASK | DCB_F_OUTX_CTS_FLOW);
        dcb._bitfield |= DCB_F_BINARY
            | (DTR_CONTROL_ENABLE << DCB_F_DTR_CONTROL_SHIFT)
            | (rts_control << DCB_F_RTS_CONTROL_SHIFT);
        if hardware_flow {
            dcb._bitfield |= DCB_F_OUTX_CTS_FLOW;
        }

        // SetCommState
        let ok = unsafe { SetCommState(self.handle, &dcb) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        // COMMTIMEOUTS:
        // 读:Interval/Multiplier=MAXDWORD + Constant=20 是文档规定的特例——缓冲里有字节立即
        //     返回,没有则最多等 20ms 等第一个字节。
        // 写:驱动级超时全关(0)。驱动写超时到点是"成功返回 + 部分字节数",不是错误——
        //     低波特率发大块(1KB@9600 ≈ 1.07s)必被截,截掉的部分静默丢失。
        //     写超时只由 write_overlapped 的 WaitForSingleObject 兜底,超时值按长度/波特率算
        //     (见 write_timeout_ms),真卡住时 CancelIo 取消。
        let timeouts = COMMTIMEOUTS {
            ReadIntervalTimeout: u32::MAX,
            ReadTotalTimeoutMultiplier: u32::MAX,
            ReadTotalTimeoutConstant: 20,
            WriteTotalTimeoutMultiplier: 0,
            WriteTotalTimeoutConstant: 0,
        };
        let ok = unsafe { SetCommTimeouts(self.handle, &timeouts) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    /// 写 `len` 字节的等待上限(ms):线速传完的时间 ×2 + 500ms 余量。
    /// 驱动级写超时已关(见 configure),这是唯一的写超时——按长度/波特率算,
    /// 低波特率发大块不会被误判超时截断;真卡住(设备不收、USB 缓冲满)时才触发取消。
    /// 每字节按 10 bit(起始+8 数据+停止)估,校验位/2 停止位的差额由 ×2 覆盖。
    pub fn write_timeout_ms(&self, len: usize) -> u32 {
        let line_ms = (len as u64) * 10 * 1000 / u64::from(self.baud_rate.max(1));
        (line_ms * 2 + 500).min(u64::from(u32::MAX)) as u32
    }

    /// Overlapped write：WriteFile 立即返回，等事件完成，超时 CancelIo。
    pub fn write_overlapped(&self, data: &[u8], timeout_ms: u32) -> io::Result<usize> {
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }

        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = event;

        let mut bytes_written: u32 = 0;
        let result = unsafe {
            WriteFile(
                self.handle,
                data.as_ptr(),
                data.len() as u32,
                &mut bytes_written,
                &mut overlapped,
            )
        };

        if result != 0 {
            // 立即完成
            unsafe { CloseHandle(event) };
            return Ok(bytes_written as usize);
        }

        // 检查 ERROR_IO_PENDING (997)
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(997) {
            unsafe { CloseHandle(event) };
            return Err(err);
        }

        let outcome = self.finish_overlapped(&overlapped, event, timeout_ms);
        match outcome {
            // 完成(含取消发出后恰好完成的)
            OverlappedOutcome { error: None, transferred, .. } => Ok(transferred),
            // 取消前已写出一部分:按部分写入返回,调用方补写余下;一字节没出去才算超时
            OverlappedOutcome { transferred, .. } if transferred > 0 => Ok(transferred),
            OverlappedOutcome { timed_out: true, .. } => {
                Err(io::Error::new(io::ErrorKind::TimedOut, "write 超时"))
            }
            OverlappedOutcome { error: Some(e), .. } => Err(e),
        }
    }

    /// overlapped I/O 收尾:等事件,超时(或等待出错)先 CancelIo,最后**必须**以
    /// GetOverlappedResult(bWait=TRUE) 等到 I/O 真正完成才返回、才关事件。
    ///
    /// CancelIo 只是发起取消——驱动(尤其是已经卡住的 USB 转串口,正是走到超时分支的场景)
    /// 完成取消可能远超几十 ms。I/O 完成前 `overlapped` 和数据缓冲区都还归内核所有,
    /// 提前返回让它们出作用域,驱动稍后完成时会往已被复用的栈帧里写 Internal/InternalHigh,
    /// 表现为随机崩溃或静默的内存损坏。这里宁可阻塞等取消完成。
    fn finish_overlapped(&self, overlapped: &OVERLAPPED, event: HANDLE, timeout_ms: u32) -> OverlappedOutcome {
        let wait = unsafe { WaitForSingleObject(event, timeout_ms) };
        if wait != WAIT_OBJECT_0 {
            // 只取消本线程在该句柄上的在途 I/O;reader/writer 各持独立句柄,互不影响
            unsafe { CancelIo(self.handle) };
        }
        let mut transferred: u32 = 0;
        let ok = unsafe { GetOverlappedResult(self.handle, overlapped, &mut transferred, TRUE) };
        // 取消后的失败通常是 ERROR_OPERATION_ABORTED(995),此时 transferred 是取消前已完成的字节数
        let error = if ok == 0 { Some(io::Error::last_os_error()) } else { None };
        unsafe { CloseHandle(event) };
        OverlappedOutcome {
            transferred: transferred as usize,
            timed_out: wait == WAIT_TIMEOUT,
            error,
        }
    }

    /// Overlapped read：立即返回已读数据，超时返回取消前读到的字节数（通常 0）。
    pub fn read_overlapped(&self, buf: &mut [u8], timeout_ms: u32) -> io::Result<usize> {
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }

        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = event;

        let mut bytes_read: u32 = 0;
        let result = unsafe {
            ReadFile(
                self.handle,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut bytes_read,
                &mut overlapped,
            )
        };

        if result != 0 {
            unsafe { CloseHandle(event) };
            return Ok(bytes_read as usize);
        }

        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(997) {
            unsafe { CloseHandle(event) };
            return Err(err);
        }

        let outcome = self.finish_overlapped(&overlapped, event, timeout_ms);
        match outcome {
            OverlappedOutcome { error: None, transferred, .. } => Ok(transferred),
            // 读超时是常态(设备静默),不报错:取消前读到多少算多少(通常 0)
            OverlappedOutcome { timed_out: true, transferred, .. } => Ok(transferred),
            // 非超时的完成失败(拔线等):上抛,reader 据此退出
            OverlappedOutcome { error: Some(e), .. } => Err(e),
        }
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        let proc = unsafe { GetCurrentProcess() };
        let mut cloned: HANDLE = std::ptr::null_mut();
        let ok = unsafe {
            DuplicateHandle(proc, self.handle, proc, &mut cloned, 0, 1, DUPLICATE_SAME_ACCESS)
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(WinPort { handle: cloned, baud_rate: self.baud_rate })
        }
    }

    /// 读 CTS/DSR 当前电平。GetCommModemStatus 是即时驱动查询,与其他句柄上在途的
    /// overlapped 读/写/事件等待互不干扰(驱动内部自串行化)。
    pub fn modem_status(&self) -> io::Result<(bool, bool)> {
        let mut status: u32 = 0;
        if unsafe { GetCommModemStatus(self.handle, &mut status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((status & MS_CTS_ON != 0, status & MS_DSR_ON != 0))
    }

    /// CTS/DSR 电平变化监视:事件驱动(SetCommMask + WaitCommEvent),不轮询。
    /// 驱动在电平变化当下把事件入队,本函数被唤醒后读 GetCommModemStatus 取当前值上抛。
    /// 通知延迟 ≈ 线程唤醒(亚毫秒~几毫秒);两次唤醒之间的快速抖动合并为一次
    /// (微秒级短脉冲抓不住,那是逻辑分析仪的活)。running 置 false 后 ≤100ms 退出。
    /// 返回 Err 仅限驱动层失败(拔线/句柄失效);正常停止返回 Ok。
    pub fn watch_modem_status(
        &self,
        running: &AtomicBool,
        mut on_change: impl FnMut(bool, bool),
    ) -> io::Result<()> {
        if unsafe { SetCommMask(self.handle, EV_CTS | EV_DSR) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // 初始状态先报一次:窗口建立/接管/刷新后不用等下一次变化就能显示
        let (cts, dsr) = self.modem_status()?;
        on_change(cts, dsr);
        loop {
            if !running.load(Ordering::SeqCst) {
                return Ok(());
            }
            // 每轮新建 event/overlapped:WaitCommEvent 完成后手动复位事件保持有信号,
            // 复用会让下一轮等待立刻假返回。每轮结束前保证在途 I/O 已收尾再 drop。
            let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if event.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            overlapped.hEvent = event;
            let mut mask: u32 = 0;

            let mut pending = false;
            let outcome = unsafe { WaitCommEvent(self.handle, &mut mask, &mut overlapped) };
            if outcome == 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(997) {
                    // ERROR_IO_PENDING:在途,等完成
                    pending = true;
                } else {
                    unsafe { CloseHandle(event) };
                    return Err(err);
                }
            }
            if pending {
                loop {
                    let wait = unsafe { WaitForSingleObject(event, 100) };
                    if wait == WAIT_OBJECT_0 {
                        break;
                    }
                    if wait == WAIT_FAILED {
                        unsafe { CloseHandle(event) };
                        return Err(io::Error::last_os_error());
                    }
                    // 100ms 只是停止检查周期,不影响检测精度(检测靠驱动事件入队)
                    if !running.load(Ordering::SeqCst) {
                        unsafe { CancelIoEx(self.handle, &overlapped) };
                        // 等取消真正完成才关 event(overlapped 归内核所有,同 finish_overlapped)
                        let mut got: u32 = 0;
                        unsafe { GetOverlappedResult(self.handle, &overlapped, &mut got, TRUE) };
                        unsafe { CloseHandle(event) };
                        return Ok(());
                    }
                }
                let mut got: u32 = 0;
                if unsafe { GetOverlappedResult(self.handle, &overlapped, &mut got, TRUE) } == 0 {
                    let err = io::Error::last_os_error();
                    unsafe { CloseHandle(event) };
                    return Err(err);
                }
            }
            unsafe { CloseHandle(event) };
            // 不区分哪个事件/边沿:直接读当前电平,连续抖动自然合并为最新值
            if mask & (EV_CTS | EV_DSR) != 0 {
                let (c, d) = self.modem_status()?;
                on_change(c, d);
            }
        }
    }
}

impl Drop for WinPort {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机测试用的端口:只认环境变量,不在仓库里写死任何 COM 号。未设则返回 None,测试自行跳过。
    /// 注意目标口必须空闲——串口是独占打开的,NeoSerial 本身连着它时这里会报"拒绝访问"
    /// (MCP 的复用只在 NeoSerial 进程内部成立),先在界面里断开再跑。
    /// 几个真机测试一起跑要加 --test-threads=1:并行会同时开同一个口,后到的被拒。
    fn test_port() -> Option<String> {
        match std::env::var("NEOSERIAL_TEST_PORT") {
            Ok(p) if !p.trim().is_empty() => Some(p),
            _ => {
                eprintln!("跳过:未设置 NEOSERIAL_TEST_PORT(如 NEOSERIAL_TEST_PORT=COM44)");
                None
            }
        }
    }

    /// 真机测试用的流控:NEOSERIAL_TEST_FLOW=none|software|hardware,默认 none。
    /// hardware 下 RTS 归驱动握手控制,可以看驱动到底把 RTS 置成什么。
    fn test_flow() -> String {
        std::env::var("NEOSERIAL_TEST_FLOW").unwrap_or_else(|_| "none".to_string())
    }

    /// 真机验证(默认跳过):监视线程能启动、上报初始态、按 running 干净退出。
    /// 观察窗口默认 1.5s,NEOSERIAL_TEST_WAIT_MS 可拉长(比如 10000),期间给对端
    /// 断电/复位,它的 RTS/DTR 会跟着变,这里就能看到 CTS/DSR 的变化与时间戳。
    ///   NEOSERIAL_TEST_PORT=COM44 cargo test --lib test_watch_modem_status_reports_initial -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_watch_modem_status_reports_initial() {
        let Some(name) = test_port() else { return };
        let wait_ms: u64 = std::env::var("NEOSERIAL_TEST_WAIT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1500);
        let port = WinPort::open(&name, 115200, 8, "none", 1, &test_flow())
            .unwrap_or_else(|e| panic!("打开 {}: {}", name, e));
        let running = std::sync::Arc::new(AtomicBool::new(true));
        let states: std::sync::Arc<std::sync::Mutex<Vec<(bool, bool)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = states.clone();
        let stop = running.clone();
        let watcher = std::thread::spawn(move || {
            port.watch_modem_status(&stop, move |c, d| {
                sink.lock().unwrap().push((c, d));
            })
        });
        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
        running.store(false, Ordering::SeqCst);
        let res = watcher.join().unwrap();
        assert!(res.is_ok(), "监视不应报错退出: {:?}", res.err());
        let got = states.lock().unwrap();
        assert!(!got.is_empty(), "应至少上报一次初始状态");
        println!("{}ms 内上报 {} 次, 首次 {:?}", wait_ms, got.len(), got[0]);
        for (i, s) in got.iter().enumerate().skip(1) {
            println!("  变化 #{}: {:?}", i, s);
        }
    }

    /// 真机验证(默认跳过):监视线程与 reader 的 overlapped 读并存时,从第三个句柄翻转
    /// RTS/DTR 六次。断言的只有"并存不互卡 + 停得干净";CTS/DSR 是否跟随取决于板子
    /// 有没有 RTS↔CTS / DTR↔DSR 回环跳线——有则打印每次变化的时间戳(可读出通知延迟),
    /// 没有就只有一条初始状态。
    ///   NEOSERIAL_TEST_PORT=COM44 cargo test --lib test_watch_modem_status_with_reader_and_rts_toggle -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_watch_modem_status_with_reader_and_rts_toggle() {
        use windows_sys::Win32::Devices::Communication::{CLRDTR, CLRRTS};
        let Some(name) = test_port() else { return };
        let port = WinPort::open(&name, 115200, 8, "none", 1, &test_flow())
            .unwrap_or_else(|e| panic!("打开 {}: {}", name, e));
        let ctl = port.try_clone().unwrap();
        let rd = port.try_clone().unwrap();
        let running = std::sync::Arc::new(AtomicBool::new(true));
        let t0 = std::time::Instant::now();
        let states: std::sync::Arc<std::sync::Mutex<Vec<(u128, bool, bool)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = states.clone();
        let stop = running.clone();
        let watcher = std::thread::spawn(move || {
            port.watch_modem_status(&stop, move |c, d| {
                sink.lock().unwrap().push((t0.elapsed().as_micros(), c, d));
            })
        });
        let stop_r = running.clone();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            let mut reads = 0u32;
            while stop_r.load(Ordering::SeqCst) {
                let _ = rd.read_overlapped(&mut buf, 20);
                reads += 1;
            }
            reads
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        for i in 0..6u32 {
            let (r, d) = if i % 2 == 0 { (CLRRTS, CLRDTR) } else { (SETRTS, SETDTR) };
            unsafe { EscapeCommFunction(ctl.handle, r); EscapeCommFunction(ctl.handle, d); }
            println!("t={}us 置 RTS/DTR {}", t0.elapsed().as_micros(), if i % 2 == 0 { "低" } else { "高" });
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        running.store(false, Ordering::SeqCst);
        let res = watcher.join().unwrap();
        let reads = reader.join().unwrap();
        println!("watcher 退出: {:?}; reader 期间读了 {} 次; 直接查询 = {:?}", res, reads, ctl.modem_status());
        let got = states.lock().unwrap();
        for (t, c, d) in got.iter() {
            println!("  t={}us cts={} dsr={}", t, c, d);
        }
        assert!(res.is_ok(), "监视不应报错退出: {:?}", res.err());
        // 20ms 读超时,1.2s 里若被监视线程卡住读次数会远低于此
        assert!(reads >= 20, "reader 与监视并存时读被卡住了: 只读了 {} 次", reads);
        assert!(!got.is_empty(), "应至少上报一次初始状态");
        if got.len() > 1 {
            println!("检测到 {} 次电平变化(板子有回环)", got.len() - 1);
        } else {
            println!("未见电平变化:板子无 RTS↔CTS/DTR↔DSR 回环,变化检测本次未覆盖");
        }
    }
}
