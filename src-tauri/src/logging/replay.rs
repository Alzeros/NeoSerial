//! Independently replayable checkpoints. Retention removes whole segments, never arbitrary deltas.
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

const SEGMENT_LIMIT: u64 = 8 * 1024 * 1024;
const DISK_LIMIT: u64 = 64 * 1024 * 1024;
static WRITE_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
struct ReplayStore {
    dir: PathBuf,
    segment_limit: u64,
    disk_limit: u64,
}

impl ReplayStore {
    fn files(&self) -> io::Result<Vec<(std::time::SystemTime, PathBuf, u64)>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let metadata = entry.metadata()?;
            if metadata.is_file() {
                files.push((metadata.modified()?, entry.path(), metadata.len()));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        Ok(files)
    }

    fn append(
        &self,
        window: &str,
        session: &str,
        segment: u64,
        events: &[Value],
    ) -> io::Result<()> {
        if session.is_empty()
            || session.len() > 80
            || !session
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid replay session",
            ));
        }
        let path = self.dir.join(format!(
            "{}-{}-{}.jsonl",
            std::process::id(),
            session,
            segment
        ));
        let size = match fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        if size > 0 {
            let mut existing = fs::File::open(&path)?;
            existing.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            existing.read_exact(&mut last)?;
            if last[0] != b'\n' {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "replay_partial_segment",
                ));
            }
        }
        let mut bytes = Vec::new();
        if size == 0 {
            serde_json::to_writer(
                &mut bytes,
                &json!({ "session": session, "segment": segment, "window": window, "pid": std::process::id() }),
            )?;
            bytes.push(b'\n');
        }
        for event in events {
            serde_json::to_writer(&mut bytes, event)?;
            bytes.push(b'\n');
        }
        if size + bytes.len() as u64 > self.segment_limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "replay_segment_limit",
            ));
        }
        fs::create_dir_all(&self.dir)?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?
            .write_all(&bytes)?;
        let files = self.files()?;
        let mut total: u64 = files.iter().map(|file| file.2).sum();
        let mut count = files.len();
        for (_, candidate, length) in files {
            if total <= self.disk_limit && count <= 512 {
                break;
            }
            if candidate != path {
                fs::remove_file(candidate)?;
                total -= length;
                count -= 1;
            }
        }
        Ok(())
    }

    fn snapshot(&self) -> io::Result<Value> {
        let mut segments = Vec::new();
        let mut incomplete = 0;
        for (_, path, length) in self.files()? {
            if length > self.segment_limit {
                incomplete += 1;
                continue;
            }
            let bytes = fs::read(path)?;
            let mut lines = bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty());
            let Some(header) = lines
                .next()
                .and_then(|line| serde_json::from_slice::<Value>(line).ok())
            else {
                incomplete += 1;
                continue;
            };
            let mut events = Vec::new();
            for line in lines {
                match serde_json::from_slice::<Value>(line) {
                    Ok(event) => events.push(event),
                    Err(_) => {
                        incomplete += 1;
                        break;
                    }
                }
            }
            // A replay cannot start without both a viewport and a full DOM checkpoint.
            if !events.iter().any(|event| event["type"] == 4)
                || !events.iter().any(|event| event["type"] == 2)
            {
                incomplete += 1;
                continue;
            }
            segments.push(json!({ "session": header["session"], "segment": header["segment"], "window": header["window"], "pid": header["pid"], "events": events }));
        }
        Ok(
            json!({ "format": "rrweb-2.2.1", "disk_limit_bytes": self.disk_limit, "incomplete_segments": incomplete, "segments": segments }),
        )
    }
}

static STORE: LazyLock<Mutex<ReplayStore>> = LazyLock::new(|| {
    Mutex::new(ReplayStore {
        dir: crate::config::local_dir()
            .join("diagnostics")
            .join("replay"),
        segment_limit: SEGMENT_LIMIT,
        disk_limit: DISK_LIMIT,
    })
});

pub fn snapshot() -> io::Result<Value> {
    STORE
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?
        .snapshot()
}

#[tauri::command]
pub async fn append_frontend_replay(
    webview_window: tauri::WebviewWindow,
    session: String,
    segment: u64,
    events: Vec<Value>,
) -> Result<(), String> {
    if events.len() > 4000
        || serde_json::to_vec(&events)
            .map_err(|error| error.to_string())?
            .len()
            > 8 * 1024 * 1024
    {
        return Err("replay batch limit".into());
    }
    let window = webview_window.label().to_string();
    let permit = WRITE_GATE
        .acquire()
        .await
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        STORE
            .lock()
            .map_err(|error| error.to_string())?
            .append(&window, &session, segment, &events)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> ReplayStore {
        ReplayStore {
            dir: std::env::temp_dir().join(format!(
                "neoserial-replay-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            )),
            segment_limit: 1000,
            disk_limit: 1400,
        }
    }
    fn checkpoint(index: u64) -> Vec<Value> {
        vec![
            json!({"type":4,"timestamp":index,"data":{"width":800,"height":600}}),
            json!({"type":2,"timestamp":index+1,"data":{"node":{"id":1}}}),
        ]
    }
    #[test]
    fn rotates_complete_checkpoints_and_reopens_without_losing_replay_headers() {
        let store = store();
        for index in 0..20 {
            store
                .append(
                    "new-future-window",
                    "test-session",
                    index,
                    &checkpoint(index),
                )
                .unwrap();
        }
        let snapshot = store.snapshot().unwrap();
        let segments = snapshot["segments"].as_array().unwrap();
        assert!(!segments.is_empty() && segments.len() < 20);
        assert_eq!(segments.last().unwrap()["segment"], 19);
        assert!(segments
            .iter()
            .all(|segment| segment["window"] == "new-future-window"
                && segment["events"][0]["type"] == 4
                && segment["events"][1]["type"] == 2));
        assert!(
            store
                .files()
                .unwrap()
                .iter()
                .map(|entry| entry.2)
                .sum::<u64>()
                <= 1400
        );
        fs::remove_dir_all(store.dir).unwrap();
    }
    #[test]
    fn rejects_path_traversal_and_oversize_without_poisoning_existing_segment() {
        let store = store();
        assert!(store
            .append("main", "../escape", 1, &checkpoint(1))
            .is_err());
        store.append("main", "safe", 1, &checkpoint(1)).unwrap();
        assert!(store
            .append("main", "safe", 1, &[json!({"data":"x".repeat(2000)})])
            .is_err());
        assert_eq!(
            store.snapshot().unwrap()["segments"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        store
            .append("main", "partial", 1, &[json!({"type":4})])
            .unwrap();
        assert_eq!(store.snapshot().unwrap()["incomplete_segments"], 1);
        fs::remove_dir_all(store.dir).unwrap();
    }

    #[test]
    fn a_partial_disk_write_requires_a_new_checkpoint_instead_of_appending_unreadable_deltas() {
        let store = store();
        store
            .append("future-window", "safe", 1, &checkpoint(1))
            .unwrap();
        let path = store.files().unwrap()[0].1.clone();
        OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(b"{partial")
            .unwrap();
        assert!(store
            .append("future-window", "safe", 1, &[json!({"type":3})])
            .unwrap_err()
            .to_string()
            .contains("replay_partial_segment"));
        store
            .append("future-window", "safe", 2, &checkpoint(20))
            .unwrap();
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot["incomplete_segments"], 1);
        assert_eq!(
            snapshot["segments"].as_array().unwrap().last().unwrap()["segment"],
            2
        );
        fs::remove_dir_all(store.dir).unwrap();
    }
}
