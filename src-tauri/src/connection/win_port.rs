#![cfg(target_os = "windows")]

use std::io;

use windows_sys::Win32::Devices::Communication::{
    BuildCommDCBAndTimeoutsW, EscapeCommFunction, SetCommState, SetCommTimeouts,
    COMMTIMEOUTS, DCB, SETDTR, SETRTS,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE, DUPLICATE_SAME_ACCESS, TRUE,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile,
    FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::{CancelIo, GetOverlappedResult, OVERLAPPED};
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

        // BuildCommDCBAndTimeoutsW 一次性设 DCB + COMMTIMEOUTS。
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
}

impl Drop for WinPort {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}
