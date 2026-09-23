//! 串口窗口最小内容区尺寸。通过系统尺寸协商约束，避免 Resized 回调再次改尺寸。

pub const MIN_WIDTH: u32 = 1216;
pub const MIN_HEIGHT: u32 = 600;

#[cfg(any(windows, test))]
fn minimum_track_size(dpi: u32, extra_width: i32, extra_height: i32) -> (i32, i32) {
    let dpi = if dpi == 0 { 96 } else { dpi };
    let physical = |logical: u32, extra: i32| {
        let pixels = (u64::from(logical) * u64::from(dpi)).div_ceil(96);
        (pixels.min(i32::MAX as u64) as i32).saturating_add(extra.max(0))
    };
    (
        physical(MIN_WIDTH, extra_width),
        physical(MIN_HEIGHT, extra_height),
    )
}

/// SetWindowSubclass 必须在窗口所属线程调用。仅由串口主窗口/新串口窗口接入。
pub fn install(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd()?.0 as usize;
        window.run_on_main_thread(move || {
            // HWND 只在所属 UI 线程使用；不持有引用或分配 subclass 私有数据。
            if !unsafe { windows::attach(hwnd as _) } {
                log::warn!(
                    "无法安装窗口最小尺寸约束: {}",
                    std::io::Error::last_os_error()
                );
            }
        })?;
    }
    #[cfg(not(windows))]
    let _ = window;
    Ok(())
}

#[cfg(windows)]
mod windows {
    use super::minimum_track_size;
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                GetClientRect, GetWindowRect, MINMAXINFO, WM_GETMINMAXINFO, WM_NCDESTROY,
            },
        },
    };

    const SUBCLASS_ID: usize = 0x4E535A45;

    pub(super) unsafe fn attach(hwnd: HWND) -> bool {
        SetWindowSubclass(hwnd, Some(minimum_size_proc), SUBCLASS_ID, 0) != 0
    }

    unsafe extern "system" fn minimum_size_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        id: usize,
        _: usize,
    ) -> LRESULT {
        if message == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(minimum_size_proc), id);
            return DefSubclassProc(hwnd, message, wparam, lparam);
        }
        // 先让框架设置其余约束（最大化区域等），只补最小拖动尺寸。
        let result = DefSubclassProc(hwnd, message, wparam, lparam);
        if message == WM_GETMINMAXINFO && lparam != 0 {
            let mut outer: RECT = std::mem::zeroed();
            let mut client: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut outer) != 0 && GetClientRect(hwnd, &mut client) != 0 {
                let extra_width = (outer.right - outer.left) - (client.right - client.left);
                let extra_height = (outer.bottom - outer.top) - (client.bottom - client.top);
                let (width, height) =
                    minimum_track_size(GetDpiForWindow(hwnd), extra_width, extra_height);
                // Windows 保证本消息的 lParam 指向可写 MINMAXINFO，仅在回调期间访问。
                let limits = &mut *(lparam as *mut MINMAXINFO);
                limits.ptMinTrackSize.x = limits.ptMinTrackSize.x.max(width);
                limits.ptMinTrackSize.y = limits.ptMinTrackSize.y.max(height);
            }
        }
        result
    }

    #[cfg(test)]
    mod native_tests {
        use super::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SendMessageW, WM_SIZE, WS_OVERLAPPEDWINDOW,
        };

        #[test]
        fn hidden_window_constraints_do_not_resize_the_window() {
            // 不设置 WS_VISIBLE，不改变桌面或显示器 DPI。
            unsafe {
                let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    1300,
                    800,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
                assert!(!hwnd.is_null());
                struct HiddenWindow(HWND);
                impl Drop for HiddenWindow {
                    fn drop(&mut self) {
                        unsafe {
                            DestroyWindow(self.0);
                        }
                    }
                }
                let _window = HiddenWindow(hwnd);
                assert!(attach(hwnd));
                let mut before: RECT = std::mem::zeroed();
                let mut client: RECT = std::mem::zeroed();
                assert_ne!(GetWindowRect(hwnd, &mut before), 0);
                assert_ne!(GetClientRect(hwnd, &mut client), 0);
                let expected = minimum_track_size(
                    GetDpiForWindow(hwnd),
                    (before.right - before.left) - (client.right - client.left),
                    (before.bottom - before.top) - (client.bottom - client.top),
                );
                for _ in 0..100 {
                    let mut limits: MINMAXINFO = std::mem::zeroed();
                    SendMessageW(hwnd, WM_GETMINMAXINFO, 0, &mut limits as *mut _ as LPARAM);
                    assert_eq!((limits.ptMinTrackSize.x, limits.ptMinTrackSize.y), expected);
                    SendMessageW(hwnd, WM_SIZE, 0, 0);
                }
                let mut after: RECT = std::mem::zeroed();
                assert_ne!(GetWindowRect(hwnd, &mut after), 0);
                assert_eq!(
                    (before.left, before.top, before.right, before.bottom),
                    (after.left, after.top, after.right, after.bottom)
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_tracks_dpi_and_non_client_margins() {
        assert_eq!(minimum_track_size(96, 16, 8), (1232, 608));
        assert_eq!(minimum_track_size(120, 20, 10), (1540, 760));
        assert_eq!(minimum_track_size(144, 24, 12), (1848, 912));
        assert_eq!(minimum_track_size(192, 32, 16), (2464, 1216));
    }

    #[test]
    fn repeated_monitor_switches_do_not_accumulate_size() {
        for _ in 0..100 {
            assert_eq!(minimum_track_size(144, 24, 12), (1848, 912));
            assert_eq!(minimum_track_size(96, 16, 8), (1232, 608));
        }
    }

    #[test]
    fn handles_no_border_invalid_dpi_and_rounding() {
        assert_eq!(minimum_track_size(96, 0, 0), (1216, 600));
        assert_eq!(minimum_track_size(0, -16, -8), (1216, 600));
        assert_eq!(minimum_track_size(110, 0, 0), (1394, 688));
    }
}
