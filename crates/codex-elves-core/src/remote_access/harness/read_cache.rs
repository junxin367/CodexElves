use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Mutex;

// 用户要求优化服务读取：只合并只读请求，写入及派发前校验始终直读。
// 64 项 × 每项最多 256 KiB，将额外响应缓存限制在约 16 MiB 内。
const MAX_ENTRIES: usize = 64;
const MAX_VALUE_BYTES: usize = 256 * 1024;

#[derive(Default)]
struct Entry {
    value: Option<(Instant, Value)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    #[tokio::test]
    async fn identical_reads_share_work_and_keep_request_scopes_separate() {
        let cache = ReadCache::default();
        let calls = AtomicUsize::new(0);
        let load = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(json!({"model":"same"}))
        };
        let (first, second) = tokio::join!(
            cache.read(
                "thread-a",
                "engine:read".into(),
                Duration::from_secs(1),
                load()
            ),
            cache.read(
                "thread-a",
                "engine:read".into(),
                Duration::from_secs(1),
                load()
            )
        );
        assert_eq!(first.unwrap(), second.unwrap());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cache
            .read(
                "thread-b",
                "engine:read".into(),
                Duration::from_secs(1),
                load(),
            )
            .await
            .unwrap();
        cache
            .read(
                "thread-a",
                "new-engine:read".into(),
                Duration::from_secs(1),
                load(),
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn invalidation_detaches_inflight_results_and_preserves_other_scopes() {
        let cache = Arc::new(ReadCache::default());
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let task = {
            let cache = cache.clone();
            let started = started.clone();
            let release = release.clone();
            tokio::spawn(async move {
                cache
                    .read("thread-a", "same".into(), Duration::from_secs(1), async {
                        started.notify_one();
                        release.notified().await;
                        Ok(json!("old"))
                    })
                    .await
            })
        };
        started.notified().await;
        cache
            .read("thread-b", "same".into(), Duration::from_secs(1), async {
                Ok(json!("other"))
            })
            .await
            .unwrap();
        cache.invalidate("thread-a").await;
        let fresh = cache
            .read("thread-a", "same".into(), Duration::from_secs(1), async {
                Ok(json!("new"))
            })
            .await
            .unwrap();
        release.notify_one();
        assert_eq!(task.await.unwrap().unwrap(), json!("old"));
        assert_eq!(fresh, json!("new"));
        assert_eq!(
            cache
                .read("thread-a", "same".into(), Duration::from_secs(1), async {
                    panic!("旧响应不能覆盖失效后的新结果")
                })
                .await
                .unwrap(),
            json!("new")
        );
        assert_eq!(
            cache
                .read("thread-b", "same".into(), Duration::from_secs(1), async {
                    panic!("其他会话的缓存不应被清除")
                })
                .await
                .unwrap(),
            json!("other")
        );
    }

    #[tokio::test]
    async fn errors_and_cancelled_reads_do_not_block_retries_or_exceed_capacity() {
        let cache = Arc::new(ReadCache::default());
        assert!(
            cache
                .read("a", "failed".into(), Duration::from_secs(1), async {
                    anyhow::bail!("test read failure")
                })
                .await
                .is_err()
        );
        assert_eq!(
            cache
                .read("a", "failed".into(), Duration::from_secs(1), async {
                    Ok(json!(1))
                })
                .await
                .unwrap(),
            json!(1)
        );
        let started = Arc::new(Notify::new());
        let task = {
            let cache = cache.clone();
            let started = started.clone();
            tokio::spawn(async move {
                cache
                    .read("a", "cancelled".into(), Duration::from_secs(1), async {
                        started.notify_one();
                        std::future::pending::<anyhow::Result<Value>>().await
                    })
                    .await
            })
        };
        started.notified().await;
        task.abort();
        let _ = task.await;
        assert_eq!(
            cache
                .read("a", "cancelled".into(), Duration::from_secs(1), async {
                    Ok(json!(2))
                })
                .await
                .unwrap(),
            json!(2)
        );
        for index in 0..MAX_ENTRIES + 2 {
            cache
                .read("a", format!("key-{index}"), Duration::from_secs(1), async {
                    Ok(json!(index))
                })
                .await
                .unwrap();
        }
        assert!(cache.entries.lock().await.len() <= MAX_ENTRIES);
        cache.clear().await;
        assert!(cache.entries.lock().await.is_empty());
    }
}

#[derive(Default)]
pub(super) struct ReadCache {
    entries: Mutex<HashMap<(String, String), Arc<Mutex<Entry>>>>,
}

impl ReadCache {
    pub async fn read(
        &self,
        scope: &str,
        key: String,
        max_age: Duration,
        load: impl Future<Output = anyhow::Result<Value>>,
    ) -> anyhow::Result<Value> {
        let entry = {
            let mut entries = self.entries.lock().await;
            let key = (scope.to_owned(), key);
            if let Some(entry) = entries.get(&key) {
                entry.clone()
            } else {
                if entries.len() >= MAX_ENTRIES {
                    // 正在读取的调用仍持有自己的 Arc，淘汰不会取消或混淆请求。
                    entries.retain(|_, entry| Arc::strong_count(entry) > 1);
                }
                if entries.len() >= MAX_ENTRIES {
                    drop(entries);
                    return load.await;
                }
                let entry = Arc::new(Mutex::new(Entry::default()));
                entries.insert(key, entry.clone());
                entry
            }
        };
        // 同键只读请求共用一次读取；调用取消会自动释放锁，不留下永久等待者。
        let mut cached = entry.lock().await;
        if let Some((time, value)) = &cached.value
            && time.elapsed() < max_age
        {
            return Ok(value.clone());
        }
        let value = load.await?;
        cached.value = serde_json::to_vec(&value)
            .ok()
            .filter(|bytes| bytes.len() <= MAX_VALUE_BYTES)
            .map(|_| (Instant::now(), value.clone()));
        Ok(value)
    }

    pub async fn invalidate(&self, scope: &str) {
        // 移除键而非复用旧条目，变更前已发出的迟到响应不能回填新缓存。
        self.entries
            .lock()
            .await
            .retain(|(owner, _), _| owner != scope);
    }

    pub async fn clear(&self) {
        self.entries.lock().await.clear();
    }
}
