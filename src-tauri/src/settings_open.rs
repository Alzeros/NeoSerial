use std::sync::Mutex;
use tokio::sync::oneshot;

#[derive(Debug, PartialEq)]
pub enum SettingsOpenOutcome {
    Ready,
    Cancelled,
}

type OpenResult = Result<SettingsOpenOutcome, String>;

pub async fn run_preparation(
    guard: tokio::sync::OwnedMutexGuard<()>,
    operation: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
    tokio::task::spawn_blocking(move || {
        operation()?;
        Ok(guard)
    })
    .await
    .map_err(|error| format!("准备设置窗口任务失败: {}", error))?
}

pub struct SettingsOpenReplies {
    pending: Mutex<Option<(u64, oneshot::Sender<OpenResult>)>>,
}

impl SettingsOpenReplies {
    pub const fn new() -> Self {
        Self {
            pending: Mutex::new(None),
        }
    }

    pub fn begin(&self, request_id: u64) -> Result<oneshot::Receiver<OpenResult>, String> {
        let (sender, receiver) = oneshot::channel();
        *self.pending.lock().map_err(|error| error.to_string())? = Some((request_id, sender));
        Ok(receiver)
    }

    pub fn complete(&self, request_id: u64, result: OpenResult) -> Result<bool, String> {
        let mut pending = self.pending.lock().map_err(|error| error.to_string())?;
        if pending
            .as_ref()
            .is_some_and(|(current, _)| *current == request_id)
        {
            if let Some((_, sender)) = pending.take() {
                let _ = sender.send(result);
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub fn clear(&self, request_id: u64) -> Result<(), String> {
        let mut pending = self.pending.lock().map_err(|error| error.to_string())?;
        if pending
            .as_ref()
            .is_some_and(|(current, _)| *current == request_id)
        {
            pending.take();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgement_is_correlated_and_consumed_once() {
        let replies = SettingsOpenReplies::new();
        let mut receiver = replies.begin(1).unwrap();
        assert!(!replies.complete(2, Ok(SettingsOpenOutcome::Ready)).unwrap());
        assert_eq!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );
        assert!(replies.complete(1, Ok(SettingsOpenOutcome::Ready)).unwrap());
        assert_eq!(receiver.try_recv(), Ok(Ok(SettingsOpenOutcome::Ready)));
        assert!(!replies.complete(1, Ok(SettingsOpenOutcome::Ready)).unwrap());
    }

    #[test]
    fn failed_load_is_reported_to_the_opener() {
        let replies = SettingsOpenReplies::new();
        let mut receiver = replies.begin(1).unwrap();
        replies
            .complete(1, Err("读取设置超时".to_string()))
            .unwrap();
        assert_eq!(receiver.try_recv(), Ok(Err("读取设置超时".to_string())));
    }

    #[test]
    fn late_cleanup_does_not_cancel_a_new_request() {
        let replies = SettingsOpenReplies::new();
        let _previous = replies.begin(1).unwrap();
        let mut current = replies.begin(2).unwrap();
        replies.clear(1).unwrap();
        assert!(!replies.complete(1, Ok(SettingsOpenOutcome::Ready)).unwrap());
        assert_eq!(current.try_recv(), Err(oneshot::error::TryRecvError::Empty));
        assert!(replies.complete(2, Ok(SettingsOpenOutcome::Ready)).unwrap());
        assert_eq!(current.try_recv(), Ok(Ok(SettingsOpenOutcome::Ready)));
    }

    #[tokio::test]
    async fn a_timed_out_request_does_not_block_the_next_open() {
        let replies = SettingsOpenReplies::new();
        let previous = replies.begin(1).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), previous)
                .await
                .is_err()
        );
        replies.clear(1).unwrap();
        let mut current = replies.begin(2).unwrap();
        assert!(!replies.complete(1, Ok(SettingsOpenOutcome::Ready)).unwrap());
        assert!(replies.complete(2, Ok(SettingsOpenOutcome::Ready)).unwrap());
        assert_eq!(current.try_recv(), Ok(Ok(SettingsOpenOutcome::Ready)));
    }

    #[test]
    fn cancellation_is_not_reported_as_readiness() {
        let replies = SettingsOpenReplies::new();
        let mut receiver = replies.begin(1).unwrap();
        replies
            .complete(1, Ok(SettingsOpenOutcome::Cancelled))
            .unwrap();
        assert_eq!(receiver.try_recv(), Ok(Ok(SettingsOpenOutcome::Cancelled)));
    }

    #[tokio::test]
    async fn blocked_preparation_respects_the_deadline_without_duplicate_preparation() {
        let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let guard = lock.clone().lock_owned().await;
        let (release, waiting) = std::sync::mpsc::channel();
        let preparation = run_preparation(guard, move || {
            waiting.recv().map_err(|error| error.to_string())?;
            Ok(())
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), preparation)
                .await
                .is_err()
        );
        assert!(lock.try_lock().is_err());
        release.send(()).unwrap();
        let next = tokio::time::timeout(std::time::Duration::from_secs(1), lock.lock_owned())
            .await
            .unwrap();
        assert!(run_preparation(next, || Ok(())).await.is_ok());
    }
}
