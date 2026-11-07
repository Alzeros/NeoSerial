//! Durable storage for WebView diagnostics; separate from serial traffic logging.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{Emitter, Listener, Manager};

const FILE_LIMIT: u64 = 4 * 1024 * 1024;
const FILE_COUNT: usize = 8;
const BATCH_LIMIT: usize = 256 * 1024;
const NATIVE_QUEUE_LIMIT: usize = 256;
static EXPORT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static WRITE_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

#[derive(Deserialize, Serialize)]
pub struct FrontendRecord {
    ts: String,
    session: String,
    seq: u64,
    level: String,
    kind: String,
    data: Value,
}

struct DiagnosticStore {
    dir: PathBuf,
    limit: u64,
}

impl DiagnosticStore {
    fn path(&self, index: usize) -> PathBuf {
        self.dir.join(if index == 0 {
            "frontend.jsonl".to_string()
        } else {
            format!("frontend.{}.jsonl", index)
        })
    }

    fn append(&self, records: &[Value]) -> io::Result<()> {
        // Serialize/validate the entire batch before changing any files.
        let lines = records
            .iter()
            .map(serde_json::to_vec)
            .collect::<Result<Vec<_>, _>>()?;
        if lines.iter().any(|line| line.len() as u64 + 1 > self.limit) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "诊断记录过大"));
        }
        fs::create_dir_all(&self.dir)?;
        for line in lines {
            let size = match fs::metadata(self.path(0)) {
                Ok(metadata) => metadata.len(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                Err(error) => return Err(error),
            };
            // A killed process/disk error may leave a partial tail. Keep it as a
            // separate damaged record so the next launch's first event survives.
            let partial_tail = if size > 0 {
                let mut file = fs::File::open(self.path(0))?;
                file.seek(SeekFrom::End(-1))?;
                let mut last = [0];
                file.read_exact(&mut last)?;
                last[0] != b'\n'
            } else {
                false
            };
            let rotate = size + u64::from(partial_tail) + line.len() as u64 + 1 > self.limit;
            if rotate {
                for index in (1..FILE_COUNT).rev() {
                    let destination = self.path(index);
                    match fs::remove_file(&destination) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                    match fs::rename(self.path(index - 1), destination) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            // No user-space buffer: WebView termination cannot discard already received records.
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.path(0))?;
            if partial_tail && !rotate {
                file.write_all(b"\n")?;
            }
            file.write_all(&line)?;
            file.write_all(b"\n")?;
        }
        Ok(())
    }

    fn snapshot(&self) -> io::Result<Vec<Value>> {
        let mut records = Vec::new();
        for index in (0..FILE_COUNT).rev() {
            let path = self.path(index);
            match fs::metadata(&path) {
                Ok(metadata) if metadata.len() > self.limit => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "诊断日志超出大小上限",
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
            let bytes = fs::read(path)?;
            for line in bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                match serde_json::from_slice(line) {
                    Ok(record) => records.push(record),
                    Err(_) => records
                        .push(json!({ "kind": "storage.incomplete_record", "level": "warn" })),
                }
            }
        }
        Ok(records)
    }

    fn report(
        &self,
        path: &Path,
        flush: &FrontendExportResult,
        replay: Value,
    ) -> io::Result<Value> {
        if path.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "请选择导出文件",
            ));
        }
        reject_storage_destination(path, &self.dir)?;
        let mut report = json!({
            "schema": "neoserial.frontend-diagnostics.v1",
            "exported_at": chrono::Local::now().to_rfc3339(),
            "app_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "flush": flush,
            "coverage": {
                "input": "global events plus DOM replay",
                "ipc": "central Tauri transport including plugins; payload schemas only",
                "state": "exported shared state summaries and visible DOM mutations",
                "private_content": "input/text masked; serial log, canvas and iframes blocked",
                "retention": "whole replay checkpoints rotate; explicit gap records on overflow",
            },
            "notes": [
                "记录来自所有窗口；session/seq 关联页面，native_seq 关联原生观察顺序。",
                "日志自动轮换，未包含串口原文、IPC 参数和返回值。错误文本仍可能含业务信息，分享前请确认。",
                "心跳间隔变大也可能来自系统休眠或窗口调度；日志停止本身不能证明 WebView 崩溃。",
                "界面回放记录 DOM 与交互，不是原生屏幕视频，也不重放串口副作用；WebView2 原生故障通知另记。"
            ],
        });
        report["records"] = Value::Array(self.snapshot()?);
        report["replay"] = replay;
        Ok(report)
    }

    #[cfg(test)]
    fn export(&self, path: &Path, flush: &FrontendExportResult, replay: Value) -> io::Result<()> {
        write_report(path, &self.report(path, flush, replay)?)
    }
}

fn write_report(path: &Path, report: &Value) -> io::Result<()> {
    let mut output = BufWriter::new(fs::File::create(path)?);
    serde_json::to_writer(&mut output, report)?;
    output.flush()
}

fn reject_storage_destination(path: &Path, storage: &Path) -> io::Result<()> {
    // Resolve symlinks and reject even a new destination inside the recorder's
    // directory, including replay checkpoints and paths created by rotation.
    let resolved = path.canonicalize().or_else(|_| {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Ok::<_, io::Error>(
            parent
                .canonicalize()?
                .join(path.file_name().unwrap_or_default()),
        )
    })?;
    if storage
        .canonicalize()
        .is_ok_and(|dir| resolved.starts_with(dir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "不能导出到正在保留诊断记录的目录",
        ));
    }
    Ok(())
}

#[derive(Default, Serialize)]
pub struct FrontendExportResult {
    unresponsive_windows: BTreeSet<String>,
    failed_windows: BTreeSet<String>,
}

#[derive(Deserialize)]
struct FlushAck {
    window: String,
    ok: bool,
}

impl FrontendExportResult {
    fn acknowledge(&mut self, ack: FlushAck) {
        // Unknown or duplicate replies cannot complete another window's flush.
        if self.unresponsive_windows.remove(&ack.window) && !ack.ok {
            self.failed_windows.insert(ack.window);
        }
    }
}

async fn flush_windows(app: &tauri::AppHandle) -> FrontendExportResult {
    static NEXT_FLUSH: AtomicU64 = AtomicU64::new(0);
    let mut result = FrontendExportResult {
        unresponsive_windows: app
            .webview_windows()
            .into_keys()
            .filter(|label| super::runtime::expects_heartbeat(label))
            .collect(),
        ..Default::default()
    };
    let reply_event = format!(
        "frontend-diagnostics-flushed-{}-{}",
        std::process::id(),
        NEXT_FLUSH.fetch_add(1, Ordering::Relaxed)
    );
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let listener = app.listen(&reply_event, move |event| {
        if let Ok(ack) = serde_json::from_str::<FlushAck>(event.payload()) {
            let _ = sender.send(ack);
        }
    });
    if app
        .emit(
            "frontend-diagnostics-flush",
            json!({ "reply_event": reply_event }),
        )
        .is_ok()
    {
        // A stuck/crashed WebView must not prevent exporting its previously saved records.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(6), async {
            while !result.unresponsive_windows.is_empty() {
                match receiver.recv().await {
                    Some(ack) => result.acknowledge(ack),
                    None => break,
                }
            }
        })
        .await;
    }
    app.unlisten(listener);
    result
}

static STORE: LazyLock<Mutex<DiagnosticStore>> = LazyLock::new(|| {
    Mutex::new(DiagnosticStore {
        dir: crate::config::local_dir().join("diagnostics"),
        limit: FILE_LIMIT,
    })
});

fn envelope(window: &str, record: impl Serialize) -> Value {
    json!({
        "received_at": chrono::Local::now().to_rfc3339(),
        "pid": std::process::id(), "window": window,
        "app_version": env!("CARGO_PKG_VERSION"),
        "build_mode": if cfg!(debug_assertions) { "dev" } else { "release" },
        "record": record,
    })
}

fn native_record(window: &str, kind: &'static str, data: Value) -> Value {
    static NEXT_NATIVE_EVENT: AtomicU64 = AtomicU64::new(0);
    // Timestamp/sequence at observation, before workers can reorder disk submissions.
    envelope(
        window,
        json!({ "kind": kind, "native_seq": NEXT_NATIVE_EVENT.fetch_add(1, Ordering::Relaxed), "data": data }),
    )
}

enum NativeJob {
    Record(Value),
    Flush(tokio::sync::oneshot::Sender<()>),
}
struct NativeWriter {
    sender: tokio::sync::mpsc::Sender<NativeJob>,
    dropped: Arc<AtomicU64>,
}
impl NativeWriter {
    fn enqueue(&self, record: Value) {
        if self.sender.try_send(NativeJob::Record(record)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}
static NATIVE_WRITER: LazyLock<Option<NativeWriter>> = LazyLock::new(|| {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(NATIVE_QUEUE_LIMIT);
    let dropped = Arc::new(AtomicU64::new(0));
    let worker_dropped = dropped.clone();
    // One bounded queue and one disk worker, even if the disk/share stops responding.
    match std::thread::Builder::new()
        .name("diagnostic-writer".into())
        .spawn(move || {
            while let Some(job) = receiver.blocking_recv() {
                let (record, reply) = match job {
                    NativeJob::Record(record) => (Some(record), None),
                    NativeJob::Flush(reply) => (None, Some(reply)),
                };
                let omitted = worker_dropped.swap(0, Ordering::Relaxed);
                let mut records = Vec::with_capacity(2);
                if omitted > 0 {
                    records.push(native_record(
                        "application",
                        "diagnostics.native_dropped",
                        json!({"count":omitted}),
                    ));
                }
                let attempted = u64::from(record.is_some());
                if let Some(record) = record {
                    records.push(record);
                }
                if !records.is_empty() {
                    let result = STORE
                        .lock()
                        .map_err(|error| io::Error::other(error.to_string()))
                        .and_then(|store| store.append(&records));
                    if let Err(error) = result {
                        worker_dropped.fetch_add(omitted + attempted, Ordering::Relaxed);
                        log::warn!("保存 WebView 生命周期记录失败: {}", error);
                    }
                }
                if let Some(reply) = reply {
                    let _ = reply.send(());
                }
            }
        }) {
        Ok(_) => Some(NativeWriter { sender, dropped }),
        Err(error) => {
            log::warn!("启动诊断记录线程失败: {}", error);
            None
        }
    }
});

pub fn native_event(window: String, kind: &'static str, data: Value) {
    if let Some(writer) = NATIVE_WRITER.as_ref() {
        writer.enqueue(native_record(&window, kind, data));
    }
}

async fn flush_native() -> bool {
    let Some(writer) = NATIVE_WRITER.as_ref() else {
        return false;
    };
    let (reply, done) = tokio::sync::oneshot::channel();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        writer
            .sender
            .send(NativeJob::Flush(reply))
            .await
            .map_err(|_| ())?;
        done.await.map_err(|_| ())
    })
    .await
    .is_ok_and(|result| result.is_ok())
}

#[tauri::command]
pub async fn append_frontend_diagnostics(
    webview_window: tauri::WebviewWindow,
    records: Vec<FrontendRecord>,
) -> Result<(), String> {
    if records.len() > 32
        || records.iter().any(|entry| {
            entry.ts.len() > 64
                || entry.session.len() > 100
                || entry.kind.len() > 100
                || !matches!(entry.level.as_str(), "info" | "warn" | "error")
        })
    {
        return Err("前端诊断批次无效".to_string());
    }
    let label = webview_window.label().to_string();
    let records: Vec<Value> = records
        .into_iter()
        .map(|entry| envelope(&label, entry))
        .collect();
    if serde_json::to_vec(&records)
        .map_err(|error| error.to_string())?
        .len()
        > BATCH_LIMIT
    {
        return Err("前端诊断批次过大".to_string());
    }
    // Wait asynchronously so slow storage cannot fill the shared blocking pool
    // with diagnostic workers all waiting for the same file lock.
    let permit = WRITE_GATE
        .acquire()
        .await
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        STORE
            .lock()
            .map_err(|error| error.to_string())?
            .append(&records)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn export_frontend_diagnostics(
    app_handle: tauri::AppHandle,
    path: String,
) -> Result<FrontendExportResult, String> {
    let _export = EXPORT_LOCK
        .try_lock()
        .map_err(|_| "已有诊断文件正在导出，请稍后重试".to_string())?;
    let flush = flush_windows(&app_handle).await;
    let native_flushed = flush_native().await;
    tauri::async_runtime::spawn_blocking(move || {
        let replay = super::replay::snapshot()
            .unwrap_or_else(|error| json!({"error": error.to_string(), "segments": []}));
        let mut report = STORE
            .lock()
            .map_err(|error| error.to_string())?
            .report(Path::new(&path), &flush, replay)
            .map_err(|error| format!("导出前端日志失败: {}", error))?;
        report["native_queue"] = json!({"flushed":native_flushed,"limit":NATIVE_QUEUE_LIMIT,
            "dropped_not_yet_persisted":NATIVE_WRITER.as_ref().map(|writer|writer.dropped.load(Ordering::Relaxed))});
        // A slow destination disk/share must not block ongoing diagnostic writes.
        write_report(Path::new(&path), &report)
            .map_err(|error| format!("导出前端日志失败: {}", error))?;
        Ok(flush)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Native fallback: a broken renderer must not be the only route to its own diagnostic export.
pub fn export_from_tray(app: &tauri::AppHandle) {
    use tauri_plugin_dialog::DialogExt;
    let handle = app.clone();
    app.dialog()
        .file()
        .add_filter("NeoSerial 诊断文件", &["json"])
        .set_file_name(format!(
            "NeoSerial-diagnostics-{}.json",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ))
        .save_file(move |file| {
            let Some(file) = file else { return };
            let Ok(path) = file.into_path() else { return };
            tauri::async_runtime::spawn(async move {
                let message = match export_frontend_diagnostics(
                    handle.clone(),
                    path.to_string_lossy().to_string(),
                )
                .await
                {
                    Ok(result)
                        if result.unresponsive_windows.is_empty()
                            && result.failed_windows.is_empty() =>
                    {
                        "诊断文件已导出。".to_string()
                    }
                    Ok(_) => {
                        "诊断文件已导出；未响应窗口此前保存的记录已包含，最新记录缺失情况已注明。"
                            .to_string()
                    }
                    Err(error) => error,
                };
                handle
                    .dialog()
                    .message(message)
                    .title("NeoSerial 诊断导出")
                    .show(|_| {});
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestStore(DiagnosticStore);
    impl TestStore {
        fn new(limit: u64) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "neoserial-frontend-{}-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            Self(DiagnosticStore { dir, limit })
        }
    }
    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0.dir);
        }
    }

    #[test]
    fn retains_recent_records_across_rotation_and_reopen() {
        let store = TestStore::new(200);
        for seq in 0..30 {
            store
                .0
                .append(&[json!({ "seq": seq, "data": "中文 🦀".repeat(3) })])
                .unwrap();
        }
        let reopened = DiagnosticStore {
            dir: store.0.dir.clone(),
            limit: 200,
        };
        let records = reopened.snapshot().unwrap();
        assert!(!records.is_empty() && records.len() < 30);
        assert_eq!(records.last().unwrap()["seq"], 29);
        for pair in records.windows(2) {
            assert_eq!(
                pair[0]["seq"].as_u64().unwrap() + 1,
                pair[1]["seq"].as_u64().unwrap()
            );
        }
        assert_eq!(fs::read_dir(&store.0.dir).unwrap().count(), FILE_COUNT);
        for index in 0..FILE_COUNT {
            assert!(fs::metadata(store.0.path(index)).unwrap().len() <= 200);
        }
    }

    #[test]
    fn exports_all_windows_and_survives_partial_record() {
        let store = TestStore::new(FILE_LIMIT);
        store
            .0
            .append(&[
                envelope("main", json!({ "kind": "window.error" })),
                envelope("settings", json!({ "kind": "ui.click" })),
            ])
            .unwrap();
        OpenOptions::new()
            .append(true)
            .open(store.0.path(0))
            .unwrap()
            .write_all(b"{partial")
            .unwrap();
        let path = store.0.dir.with_extension("report.json");
        store
            .0
            .export(
                &path,
                &FrontendExportResult::default(),
                json!({"segments": []}),
            )
            .unwrap();
        let report: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(report["schema"], "neoserial.frontend-diagnostics.v1");
        assert_eq!(report["records"][0]["window"], "main");
        assert_eq!(report["records"][1]["window"], "settings");
        assert_eq!(report["records"][2]["kind"], "storage.incomplete_record");
    }

    #[test]
    fn appending_after_a_partial_tail_preserves_new_launch_records() {
        let store = TestStore::new(200);
        store.0.append(&[json!({"previous_launch":true})]).unwrap();
        OpenOptions::new()
            .append(true)
            .open(store.0.path(0))
            .unwrap()
            .write_all(b"{partial")
            .unwrap();
        let reopened = DiagnosticStore {
            dir: store.0.dir.clone(),
            limit: 200,
        };
        reopened.append(&[json!({"next_launch":true})]).unwrap();
        let records = reopened.snapshot().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0]["previous_launch"], true);
        assert_eq!(records[1]["kind"], "storage.incomplete_record");
        assert_eq!(records[2]["next_launch"], true);
    }

    #[test]
    fn export_cannot_overwrite_replay_checkpoints_or_create_files_in_storage() {
        let store = TestStore::new(200);
        let replay = store.0.dir.join("replay");
        fs::create_dir_all(&replay).unwrap();
        let checkpoint = replay.join("checkpoint.jsonl");
        fs::write(&checkpoint, b"saved").unwrap();
        for destination in [&checkpoint, &replay.join("new.json"), &store.0.path(7)] {
            assert!(store
                .0
                .report(destination, &FrontendExportResult::default(), json!({}))
                .is_err());
        }
        assert_eq!(fs::read(checkpoint).unwrap(), b"saved");
    }

    #[test]
    fn native_disk_stalls_keep_a_bounded_queue_and_count_omitted_records() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
        let writer = NativeWriter {
            sender,
            dropped: Arc::new(AtomicU64::new(0)),
        };
        for seq in 0..10000 {
            writer.enqueue(json!({"seq":seq}));
        }
        assert_eq!(writer.dropped.load(Ordering::Relaxed), 9998);
        assert_eq!(receiver.len(), 2);
        receiver.try_recv().unwrap();
        writer.enqueue(json!({"after_recovery":true}));
        assert_eq!(receiver.len(), 2);
        assert_eq!(writer.dropped.load(Ordering::Relaxed), 9998);
    }

    #[test]
    fn rejects_oversize_and_export_overwrite_before_changing_log() {
        let store = TestStore::new(200);
        store.0.append(&[json!({ "keep": true })]).unwrap();
        let before = fs::read(store.0.path(0)).unwrap();
        assert!(store
            .0
            .append(&[json!({ "data": "x".repeat(300) })])
            .is_err());
        assert!(store
            .0
            .export(
                &store.0.path(0),
                &FrontendExportResult::default(),
                json!({"segments": []})
            )
            .is_err());
        assert_eq!(fs::read(store.0.path(0)).unwrap(), before);
    }

    #[test]
    fn flush_tracks_failed_and_unresponsive_windows_and_ignores_duplicate_replies() {
        let mut result = FrontendExportResult {
            unresponsive_windows: ["main", "settings", "theme-editor"]
                .into_iter()
                .map(String::from)
                .collect(),
            ..Default::default()
        };
        for (window, ok) in [
            ("other", true),
            ("main", true),
            ("main", false),
            ("settings", false),
            ("settings", true),
        ] {
            result.acknowledge(FlushAck {
                window: window.to_string(),
                ok,
            });
        }
        assert_eq!(
            result.unresponsive_windows,
            ["theme-editor".to_string()].into_iter().collect()
        );
        assert_eq!(
            result.failed_windows,
            ["settings".to_string()].into_iter().collect()
        );
    }
}
