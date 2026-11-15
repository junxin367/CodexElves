//! 独立原生子进程的标准输入派发；只管理授权和写入回执，不连接 Codex App。
use crate::codex_service::{ServiceDispatchGuard, ServiceDispatchPermit};
use anyhow::ensure;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

#[derive(Debug)]
pub(crate) struct DispatchFailure {
    pub dispatched: bool,
    message: String,
}

impl std::fmt::Display for DispatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DispatchFailure {}

pub(crate) struct WriteFrame {
    bytes: Vec<u8>,
    completion: oneshot::Sender<Result<(), String>>,
}

pub(crate) fn spawn_writer<W: AsyncWrite + Unpin + Send + 'static>(
    mut writer: W,
    timeout: Duration,
) -> (mpsc::Sender<WriteFrame>, JoinHandle<()>) {
    let (sender, mut receiver) = mpsc::channel::<WriteFrame>(1);
    let task = tokio::spawn(async move {
        while let Some(frame) = receiver.recv().await {
            let result = tokio::time::timeout(timeout, writer.write_all(&frame.bytes))
                .await
                .map_err(|_| "写入 Codex 请求超时".to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let failed = result.is_err();
            let _ = frame.completion.send(result);
            if failed {
                break;
            }
        }
    });
    (sender, task)
}

struct TrustedPermit;
impl ServiceDispatchPermit for TrustedPermit {
    fn commit(
        self: Box<Self>,
        dispatch: Box<dyn FnOnce() -> anyhow::Result<()> + Send>,
    ) -> anyhow::Result<()> {
        dispatch()
    }
}

/// The permit encloses only the synchronous, bounded queue insertion.
pub(crate) async fn guarded_write(
    sender: &mpsc::Sender<WriteFrame>,
    bytes: Vec<u8>,
    guard: Option<&dyn ServiceDispatchGuard>,
    timeout: Duration,
) -> anyhow::Result<()> {
    let permit = match guard {
        Some(guard) => guard.prepare_dispatch().await.map_err(|error| {
            let message = format!("{error:#}");
            error.context(DispatchFailure {
                dispatched: false,
                message,
            })
        })?,
        None => Box::new(TrustedPermit) as Box<dyn ServiceDispatchPermit>,
    };
    let (completion, result) = oneshot::channel();
    let frame = WriteFrame { bytes, completion };
    let sender = sender.clone();
    let dispatched = Arc::new(AtomicBool::new(false));
    let queued = Arc::clone(&dispatched);
    let commit = permit.commit(Box::new(move || {
        sender
            .try_send(frame)
            .map_err(|_| anyhow::anyhow!("Codex 请求写入队列不可用"))?;
        queued.store(true, Ordering::Release);
        Ok(())
    }));
    if let Err(error) = commit {
        let message = format!("{error:#}");
        return Err(error.context(DispatchFailure {
            dispatched: dispatched.load(Ordering::Acquire),
            message,
        }));
    }
    ensure!(
        dispatched.load(Ordering::Acquire),
        "Codex 授权派发器未提交请求"
    );
    tokio::time::timeout(timeout, result)
        .await
        .map_err(|_| DispatchFailure {
            dispatched: true,
            message: "Codex 请求已派发，等待写入结果超时".to_string(),
        })?
        .map_err(|_| DispatchFailure {
            dispatched: true,
            message: "Codex 请求已派发，写入连接已关闭".to_string(),
        })?
        .map_err(|message| DispatchFailure {
            dispatched: true,
            message,
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    #[derive(Debug)]
    struct AuthorizationMarker;
    impl std::fmt::Display for AuthorizationMarker {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("authorization-marker")
        }
    }
    impl std::error::Error for AuthorizationMarker {}
    struct DeniedBeforePermit;
    #[async_trait]
    impl ServiceDispatchGuard for DeniedBeforePermit {
        async fn prepare_dispatch(&self) -> anyhow::Result<Box<dyn ServiceDispatchPermit>> {
            Err(AuthorizationMarker.into())
        }
    }

    #[tokio::test]
    async fn authorization_failure_keeps_its_original_error_type_for_http_mapping() {
        let (sender, _receiver) = mpsc::channel(1);
        let error = guarded_write(
            &sender,
            vec![1],
            Some(&DeniedBeforePermit),
            Duration::from_millis(20),
        )
        .await
        .unwrap_err();
        assert!(error.downcast_ref::<AuthorizationMarker>().is_some());
        let failure = error.downcast_ref::<DispatchFailure>().unwrap();
        assert!(!failure.dispatched);
    }
}
