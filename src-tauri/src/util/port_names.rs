//! Windows SetupAPI 设备名查找:端口号 → 设备友好名(SPDRP_FRIENDLYNAME)。
//!
//! serialport 只对能解析出 VID/PID 的 USB 设备填名字(product 字段),
//! 主板自带串口(ACPI\PNP0501,设备管理器里叫 "Communications Port (COM1)")、
//! 蓝牙口(BTHENUM)、虚拟串口、PCI 串口卡等类型都会落到 Unknown,
//! 取不到名字——而设备管理器对每个设备都显示友好名。这里自己做一遍
//! Ports/Modem 设备类枚举补上:与 serialport 的枚举结果按端口号合并,
//! 端口集合仍以 serialport 为准(这里只提供名字)。

use std::collections::HashMap;

#[cfg(windows)]
use std::ptr;

#[cfg(windows)]
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiClassGuidsFromNameW, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
    SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW, SetupDiOpenDevRegKey, DICS_FLAG_GLOBAL,
    DIGCF_PRESENT, DIREG_DEV, HDEVINFO, SPDRP_FRIENDLYNAME, SP_DEVINFO_DATA,
};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{FALSE, INVALID_HANDLE_VALUE, MAX_PATH};
#[cfg(windows)]
use windows_sys::Win32::System::Registry::{RegCloseKey, RegQueryValueExW, KEY_READ, REG_SZ};
#[cfg(windows)]
use windows_sys::core::GUID;

/// 端口号 → 设备友好名。枚举失败或非 Windows 返回空表,调用方退回
/// serialport 自带的 USB 信息(product/manufacturer)。
pub fn friendly_names_by_port() -> HashMap<String, String> {
    #[cfg(windows)]
    {
        windows_friendly_names().unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        HashMap::new()
    }
}

#[cfg(windows)]
fn as_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
fn from_utf16_lossy_trimmed(utf16: &[u16]) -> String {
    String::from_utf16_lossy(utf16)
        .trim_end_matches(0 as char)
        .trim()
        .to_string()
}

/// 取设备安装类的 GUID(与 serialport 同样查 "Ports" + "Modem" 两类)。
#[cfg(windows)]
fn class_guids(class_name: &str) -> Vec<GUID> {
    let name = as_utf16(class_name);
    // 注意:不能用 size=0 探测——该调用直接返回 FALSE。先给 1 个元素的真实
    // 缓冲区;返回 FALSE 视为无 GUID,under-allocated 时按回填的 required 扩容重试
    // (与 serialport 的 SetupDiClassGuidsFromNameW 用法一致)。
    let mut guids = vec![GUID::from_u128(0)];
    let mut required = 0u32;
    if unsafe { SetupDiClassGuidsFromNameW(name.as_ptr(), guids.as_mut_ptr(), 1, &mut required) } == FALSE {
        return Vec::new();
    }
    if required as usize > guids.len() {
        guids = vec![GUID::from_u128(0); required as usize];
        if unsafe {
            SetupDiClassGuidsFromNameW(name.as_ptr(), guids.as_mut_ptr(), required, &mut required)
        } == FALSE
        {
            return Vec::new();
        }
    }
    guids.truncate(required as usize);
    guids
}

/// HDEVINFO 的 RAII 包装:枚举结束/ panic 时销毁设备信息集。
#[cfg(windows)]
struct DevInfoSet(HDEVINFO);

#[cfg(windows)]
impl Drop for DevInfoSet {
    fn drop(&mut self) {
        unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

/// 读设备的字符串型注册表属性(如 SPDRP_FRIENDLYNAME)。非 REG_SZ / 读失败返回 None。
#[cfg(windows)]
fn device_registry_property(
    hdi: HDEVINFO,
    data: &SP_DEVINFO_DATA,
    property: u32,
) -> Option<String> {
    let mut buf = [0u16; 512];
    let mut value_type = 0u32;
    let ok = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            hdi,
            data,
            property,
            &mut value_type,
            buf.as_mut_ptr() as *mut u8,
            (buf.len() * 2) as u32,
            ptr::null_mut(),
        )
    };
    if ok == FALSE || value_type != REG_SZ {
        return None;
    }
    // 未回填实际长度:缓冲区初始化为 0,按去尾零处理(与 serialport 同做法)
    Some(from_utf16_lossy_trimmed(&buf))
}

/// 从设备硬件注册表键读 PortName(如 "COM3"),拿不到返回 None。
#[cfg(windows)]
fn device_port_name(hdi: HDEVINFO, data: &SP_DEVINFO_DATA) -> Option<String> {
    // SAFETY: HKEY 与 HANDLE 同为指针类型,设备键打开失败时返回 INVALID_HANDLE_VALUE
    let hkey = unsafe { SetupDiOpenDevRegKey(hdi, data, DICS_FLAG_GLOBAL, 0, DIREG_DEV, KEY_READ) };
    if hkey == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut buf = [0u16; MAX_PATH as usize];
    let mut value_type = 0u32;
    let mut byte_len = (buf.len() * 2) as u32;
    let value_name = as_utf16("PortName");
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            ptr::null(),
            &mut value_type,
            buf.as_mut_ptr() as *mut u8,
            &mut byte_len,
        )
    };
    unsafe { RegCloseKey(hkey) };
    if err != 0 || value_type != REG_SZ || byte_len % 2 != 0 {
        return None;
    }
    let name = from_utf16_lossy_trimmed(&buf[..(byte_len / 2) as usize]);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(windows)]
fn windows_friendly_names() -> Option<HashMap<String, String>> {
    let mut names = HashMap::new();
    for guid in class_guids("Ports").into_iter().chain(class_guids("Modem")) {
        // HDEVINFO 是 isize;失败返回 INVALID_HANDLE_VALUE(-1)。hwndparent 传空指针。
        let hdi = unsafe { SetupDiGetClassDevsW(&guid, ptr::null(), ptr::null_mut(), DIGCF_PRESENT) };
        if hdi == INVALID_HANDLE_VALUE as isize {
            continue;
        }
        let _guard = DevInfoSet(hdi);
        let mut idx = 0u32;
        loop {
            let mut data = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ClassGuid: GUID::from_u128(0),
                DevInst: 0,
                Reserved: 0,
            };
            if unsafe { SetupDiEnumDeviceInfo(hdi, idx, &mut data) } == FALSE {
                break;
            }
            idx += 1;
            let friendly = device_registry_property(hdi, &data, SPDRP_FRIENDLYNAME);
            let port = device_port_name(hdi, &data);
            // 两类信息缺一就不记:没有名字的设备下游可退回 serialport 的 USB 信息
            if let (Some(friendly), Some(port)) = (friendly, port) {
                if port.starts_with("COM") {
                    names.insert(port, friendly);
                }
            }
        }
    }
    Some(names)
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;

    #[test]
    fn test_from_utf16_lossy_trimmed() {
        assert_eq!(from_utf16_lossy_trimmed(&[]), "");
        assert_eq!(from_utf16_lossy_trimmed(&[0]), "");
        // 设备友好名常见形态:全字符串 + 尾随零
        let w: Vec<u16> = "Communications Port (COM1)".encode_utf16().chain([0, 0]).collect();
        assert_eq!(from_utf16_lossy_trimmed(&w), "Communications Port (COM1)");
    }

    /// 手工诊断用:`cargo test --lib -- --ignored dump`:
    /// 打印本机 SetupAPI 枚举出的全部 COM → 友好名,用于核对名字缺失时
    /// 是枚举没拿到还是合并逻辑的问题。
    #[test]
    #[ignore]
    fn dump_friendly_names() {
        let names = friendly_names_by_port();
        println!("SetupAPI friendly names: {} entries", names.len());
        let mut kv: Vec<_> = names.into_iter().collect();
        kv.sort();
        for (port, name) in kv {
            println!("  {port} => {name}");
        }
    }
}
