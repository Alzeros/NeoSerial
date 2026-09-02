#![cfg(target_os = "windows")]

use std::io;

use windows_sys::Win32::Devices::Communication::{
    BuildCommDCBAndTimeoutsW, EscapeCommFunction, SetCommState, SetCommTimeouts,
    COMMTIMEOUTS, DCB,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE, DUPLICATE_SAME_ACCESS,
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

pub struct WinPort {
    handle: HANDLE,
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

        let port = WinPort { handle };
        port.configure(baud_rate, data_bits, parity, stop_bits, flow_control)?;

        // DTR/RTS
        unsafe {
            EscapeCommFunction(handle, 5 /* SETDTR */);
            EscapeCommFunction(handle, 6 /* SETRTS */);
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
        // BuildCommDCBAndTimeoutsW 一次性设 DCB + COMMTIMEOUTS
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
            match flow_control.to_lowercase().as_str() {
                "software" => "on",
                "hardware" => "hard",
                _ => "off",
            }
        );
        let mut dcb_str_w: Vec<u16> = dcb_str.encode_utf16().collect();
        dcb_str_w.push(0);

        let mut dcb: DCB = unsafe { std::mem::zeroed() };
        dcb.DCBlength = std::mem::size_of::<DCB>() as u32;

        // COMMTIMEOUTS: 读立即返回(非阻塞)，写 200ms 超时(overlapped 下仅作 fallback)
        let timeouts = COMMTIMEOUTS {
            ReadIntervalTimeout: u32::MAX,
            ReadTotalTimeoutMultiplier: u32::MAX,
            ReadTotalTimeoutConstant: 20,
            WriteTotalTimeoutMultiplier: 0,
            WriteTotalTimeoutConstant: 200,
        };

        let ok = unsafe {
            BuildCommDCBAndTimeoutsW(
                dcb_str_w.as_ptr(),
                &mut dcb,
                &timeouts as *const COMMTIMEOUTS as *mut COMMTIMEOUTS,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        // SetCommState
        let ok = unsafe { SetCommState(self.handle, &dcb) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        // SetCommTimeouts（确保生效，EscapeCommFunction 可能重置）
        let timeouts = COMMTIMEOUTS {
            ReadIntervalTimeout: u32::MAX,
            ReadTotalTimeoutMultiplier: u32::MAX,
            ReadTotalTimeoutConstant: 20,
            WriteTotalTimeoutMultiplier: 0,
            WriteTotalTimeoutConstant: 200,
        };
        let ok = unsafe { SetCommTimeouts(self.handle, &timeouts) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
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

        // 等 overlapped I/O 完成
        let wait = unsafe { WaitForSingleObject(event, timeout_ms) };
        match wait {
            0 /* WAIT_OBJECT_0 */ => {
                let mut transferred: u32 = 0;
                let ok = unsafe { GetOverlappedResult(self.handle, &overlapped, &mut transferred, FALSE) };
                unsafe { CloseHandle(event) };
                if ok != 0 {
                    Ok(transferred as usize)
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            0x102 /* WAIT_TIMEOUT */ => {
                unsafe { CancelIo(self.handle) };
                unsafe { WaitForSingleObject(event, 100) };
                unsafe { CloseHandle(event) };
                Err(io::Error::new(io::ErrorKind::TimedOut, "write 超时"))
            }
            _ => {
                unsafe { CloseHandle(event) };
                Err(io::Error::last_os_error())
            }
        }
    }

    /// Overlapped read：立即返回已读数据，超时返回 0。
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

        let wait = unsafe { WaitForSingleObject(event, timeout_ms) };
        match wait {
            0 => {
                let mut transferred: u32 = 0;
                let ok = unsafe { GetOverlappedResult(self.handle, &overlapped, &mut transferred, FALSE) };
                unsafe { CloseHandle(event) };
                if ok != 0 {
                    Ok(transferred as usize)
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            0x102 => {
                unsafe { CancelIo(self.handle) };
                unsafe { WaitForSingleObject(event, 50) };
                let mut transferred: u32 = 0;
                let ok = unsafe { GetOverlappedResult(self.handle, &overlapped, &mut transferred, FALSE) };
                unsafe { CloseHandle(event) };
                if ok != 0 && transferred > 0 {
                    Ok(transferred as usize)
                } else {
                    Ok(0)
                }
            }
            _ => {
                unsafe { CloseHandle(event) };
                Err(io::Error::last_os_error())
            }
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
            Ok(WinPort { handle: cloned })
        }
    }
}

impl Drop for WinPort {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}
