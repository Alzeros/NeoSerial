use super::export_log_snippet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const LIMIT_BYTES: usize = 32 * 1024 * 1024;

struct TestDirectory {
    root: PathBuf,
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().canonicalize().unwrap();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = root.join(format!(
            "neoserial-snippet-export-{}-{}-{}",
            std::process::id(),
            timestamp,
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self { root, path }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if self.path.is_absolute() && self.path.parent() == Some(self.root.as_path()) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn export(path: &Path, text: &str) -> Result<(), String> {
    tauri::async_runtime::block_on(export_log_snippet(
        path.to_str().unwrap().to_string(),
        text.to_string(),
    ))
}

#[test]
fn export_log_snippet_round_trips_utf8_without_json_wrapping() {
    let directory = TestDirectory::new();
    let path = directory.path.join("日志片段-🦀.txt");
    let text = "[接收] 中文 café 🦀\r\n[发送] \"AT\\测试\"\n\0原样保留";

    export(&path, text).unwrap();

    assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn export_log_snippet_overwrites_and_truncates_existing_file() {
    let directory = TestDirectory::new();
    let path = directory.path.join("selected.txt");
    fs::write(&path, "old contents that must be fully replaced").unwrap();

    export(&path, "新\n").unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "新\n");
}

#[test]
fn export_log_snippet_rejects_directory_destination_with_path() {
    let directory = TestDirectory::new();

    let error = export(&directory.path, "snippet").unwrap_err();

    assert!(error.contains(directory.path.to_str().unwrap()), "{error}");
    assert!(directory.path.is_dir());
}

#[test]
fn export_log_snippet_rejects_missing_parent_without_creating_it() {
    let directory = TestDirectory::new();
    let parent = directory.path.join("missing");
    let path = parent.join("snippet.txt");

    let error = export(&path, "snippet").unwrap_err();

    assert!(error.contains(path.to_str().unwrap()), "{error}");
    assert!(!parent.exists());
}

#[test]
fn export_log_snippet_rejects_file_as_parent() {
    let directory = TestDirectory::new();
    let parent = directory.path.join("not-a-directory");
    fs::write(&parent, "unchanged").unwrap();
    let path = parent.join("snippet.txt");

    let error = export(&path, "snippet").unwrap_err();

    assert!(error.contains(path.to_str().unwrap()), "{error}");
    assert_eq!(fs::read_to_string(&parent).unwrap(), "unchanged");
}

#[test]
fn export_log_snippet_rejects_empty_destination() {
    assert!(export(Path::new(""), "snippet").is_err());
}

#[test]
fn export_log_snippet_rejects_oversize_utf8_before_overwriting() {
    let directory = TestDirectory::new();
    let path = directory.path.join("selected.txt");
    fs::write(&path, "keep existing content").unwrap();
    let text = "🦀".repeat(LIMIT_BYTES / 4 + 1);

    let error = export(&path, &text).unwrap_err();

    assert!(error.contains("32 MiB"), "{error}");
    assert!(error.contains("UTF-8"), "{error}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep existing content");
}

#[test]
fn export_log_snippet_rejects_oversize_before_creating_file() {
    let directory = TestDirectory::new();
    let path = directory.path.join("new.txt");
    let text = "x".repeat(LIMIT_BYTES + 1);

    let error = export(&path, &text).unwrap_err();

    assert!(error.contains("32 MiB"), "{error}");
    assert!(!path.exists());
}

#[test]
fn export_log_snippet_accepts_exact_byte_limit() {
    let directory = TestDirectory::new();
    let path = directory.path.join("limit.txt");
    let text = "🦀".repeat(LIMIT_BYTES / 4);

    export(&path, &text).unwrap();

    assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
}

#[test]
fn export_log_snippet_allows_empty_text_and_truncates_file() {
    let directory = TestDirectory::new();
    let path = directory.path.join("empty.txt");
    fs::write(&path, "old content").unwrap();

    export(&path, "").unwrap();

    assert!(fs::read(&path).unwrap().is_empty());
}
