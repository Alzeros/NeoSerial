//! 知识库 API Key 的存放。
//!
//! **key 不是 Settings 的字段**:Settings 会经 get_settings 发给前端(devtools 可见)、
//! 经 MCP 的 get_settings 工具发给 agent(那个 HTTP 端点没有鉴权)、还整份序列化进
//! settings.json。把 key 从那个类型里摘出来,这几条路就都不可能泄出去——安全性来自
//! 类型结构,而不是"记得在每个出口打码"。
//!
//! 落盘:config_dir()/kb-credential.dat,内容是 DPAPI(当前用户作用域)加密后的原始字节。
//! 同一 Windows 用户能解开,别的用户/别的机器(本地账号)解不开 → 按"没配"降级到内置值,
//! 并让设置页提示重填。凭据放 Roaming 与 DPAPI 主密钥(%APPDATA%\Microsoft\Protect)同侧,
//! 域用户漫游到另一台机器仍可解。
//!
//! 编译期注入的内置 key 只作运行时兜底,永不落盘。注意它随二进制分发、strings 可见,
//! 只适合内网只读接口这类场景;要根治得让客户端不持长期 key(短期 token 或匿名只读接口)。

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const CREDENTIAL_FILE: &str = "kb-credential.dat";

/// DPAPI 的可选熵:与本应用绑定,免得同一用户下别的程序拿这个 blob 直接解。
/// 值在二进制里可见,不构成安全边界,只是免费的一道门。protect/unprotect 必须一致。
const ENTROPY: &[u8] = b"neoserial.kb.credential.v1";

/// 用户凭据的状态。只把这个发给前端,绝不发 key 本身。
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyState {
    /// 用户填过,能正常解开
    Set,
    /// 没填过(此时若有内置 key 就用内置的)
    Unset,
    /// 文件在但解不开:换了 Windows 用户、换了机器,或文件损坏。降级用内置值,提示重填
    Undecryptable,
}

/// 内部读取结果
pub(crate) enum UserKey {
    None,
    Ok(String),
    Bad,
}

/// 编译期注入的内置 Key:构建时设 NEOSERIAL_KB_API_KEY 即编进二进制。
/// 不设 = 空 = 无内置。清洗规则与地址那份共用,见 settings::clean_injected。
pub fn builtin_api_key() -> String {
    super::settings::clean_injected(option_env!("NEOSERIAL_KB_API_KEY"))
}

pub fn has_builtin() -> bool {
    !builtin_api_key().is_empty()
}

fn credential_path() -> PathBuf {
    super::config_dir().join(CREDENTIAL_FILE)
}

pub fn state() -> KeyState {
    match read_user_key_at(&credential_path()) {
        UserKey::Ok(_) => KeyState::Set,
        UserKey::None => KeyState::Unset,
        UserKey::Bad => KeyState::Undecryptable,
    }
}

/// 实际请求知识库时用的 key:用户填的优先,否则内置,都没有则空(=未配置)。
pub fn effective_api_key() -> String {
    match read_user_key_at(&credential_path()) {
        UserKey::Ok(k) => k,
        _ => builtin_api_key(),
    }
}

/// 写入/清除用户凭据。空串(或只有空白)= 清除,回到内置值。
pub fn set_user_key(key: &str) -> Result<(), String> {
    set_user_key_at(&credential_path(), key)
}

/// 指定路径写入(迁移与测试用:不碰真实 config_dir)。
pub(crate) fn set_user_key_at(path: &Path, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("清除凭据失败: {}", e)),
        }
    } else {
        let blob = protect(key.as_bytes())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("建目录失败: {}", e))?;
        }
        // 与其他配置文件一致:先写 .tmp 再改名,中途崩溃不留半个 blob
        let tmp = path.with_extension("dat.tmp");
        fs::write(&tmp, &blob).map_err(|e| format!("写凭据失败: {}", e))?;
        fs::rename(&tmp, path).map_err(|e| format!("替换凭据失败: {}", e))
    }
}

pub(crate) fn read_user_key_at(path: &Path) -> UserKey {
    let blob = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return UserKey::None,
    };
    if blob.is_empty() {
        return UserKey::None;
    }
    match unprotect(&blob) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) if !s.trim().is_empty() => UserKey::Ok(s.trim().to_string()),
            // 解开了但不是有效文本/是空串:按坏文件处理,让用户重填
            _ => UserKey::Bad,
        },
        Err(_) => UserKey::Bad,
    }
}

// ============ DPAPI ============

#[cfg(windows)]
fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    let mut input = blob(plain);
    let mut entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        CryptProtectData(
            &mut input,
            std::ptr::null(),
            &mut entropy,
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut out,
        )
    };
    take_blob(ok, out, "加密")
}

#[cfg(windows)]
fn unprotect(blob_in: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let mut input = blob(blob_in);
    let mut entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            &mut entropy,
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut out,
        )
    };
    take_blob(ok, out, "解密")
}

#[cfg(windows)]
fn blob(data: &[u8]) -> windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB {
    windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        // Win32 结构体要 *mut,但 DPAPI 只读入参,不会改这块内存
        pbData: data.as_ptr() as *mut u8,
    }
}

/// 把 DPAPI 输出的 blob 拷成 Vec 并 LocalFree 掉(它是 LocalAlloc 出来的)。
#[cfg(windows)]
fn take_blob(
    ok: windows_sys::Win32::Foundation::BOOL,
    out: windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
    what: &str,
) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    if ok == 0 || out.pbData.is_null() {
        if !out.pbData.is_null() {
            unsafe { LocalFree(out.pbData as _) };
        }
        return Err(format!("DPAPI {}失败: {}", what, std::io::Error::last_os_error()));
    }
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe { LocalFree(out.pbData as _) };
    Ok(bytes)
}

#[cfg(not(windows))]
fn protect(_plain: &[u8]) -> Result<Vec<u8>, String> {
    Err("凭据加密仅支持 Windows(DPAPI)".into())
}

#[cfg(not(windows))]
fn unprotect(_blob_in: &[u8]) -> Result<Vec<u8>, String> {
    Err("凭据解密仅支持 Windows(DPAPI)".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("neoserial_secret_{}_{}.dat", std::process::id(), name))
    }

    #[test]
    #[cfg(windows)]
    fn test_protect_unprotect_roundtrip() {
        let blob = protect(b"kb_live_abcdef").unwrap();
        assert_ne!(blob.as_slice(), b"kb_live_abcdef", "落盘的不该是明文");
        assert_eq!(unprotect(&blob).unwrap(), b"kb_live_abcdef");
    }

    #[test]
    #[cfg(windows)]
    fn test_set_read_and_clear() {
        let path = tmp("roundtrip");
        let _ = fs::remove_file(&path);
        assert!(matches!(read_user_key_at(&path), UserKey::None), "文件不存在 = 没配");

        set_user_key_at(&path, "  kb_secret  ").unwrap();
        // 明文不能出现在文件里
        let raw = fs::read(&path).unwrap();
        assert!(
            !String::from_utf8_lossy(&raw).contains("kb_secret"),
            "凭据文件里不该有明文"
        );
        match read_user_key_at(&path) {
            UserKey::Ok(k) => assert_eq!(k, "kb_secret", "读回时去掉首尾空白"),
            _ => panic!("应能解出 key"),
        }

        // 空串 = 清除;文件已不在时再清一次也不报错
        set_user_key_at(&path, "   ").unwrap();
        assert!(!path.exists());
        set_user_key_at(&path, "").unwrap();
        let _ = fs::remove_file(&path);
    }

    /// 换了 Windows 用户/换机器/文件损坏:解不开要报 Bad(让 UI 提示重填),
    /// 不能当成"没配"静默忽略,也不能崩。
    #[test]
    #[cfg(windows)]
    fn test_corrupt_blob_reports_undecryptable() {
        let path = tmp("corrupt");
        set_user_key_at(&path, "kb_secret").unwrap();
        let mut raw = fs::read(&path).unwrap();
        let n = raw.len();
        raw[n / 2] ^= 0xFF;
        fs::write(&path, &raw).unwrap();
        assert!(matches!(read_user_key_at(&path), UserKey::Bad));
        let _ = fs::remove_file(&path);
    }

    /// 空文件按"没配"处理(不是坏文件):写盘中断留下 0 字节时不该逼用户重填
    #[test]
    fn test_empty_file_is_unset() {
        let path = tmp("empty");
        fs::write(&path, b"").unwrap();
        assert!(matches!(read_user_key_at(&path), UserKey::None));
        let _ = fs::remove_file(&path);
    }
}
