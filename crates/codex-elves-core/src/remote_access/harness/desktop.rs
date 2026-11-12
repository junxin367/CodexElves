//! 服务模式的桌面只读观察；订阅不能恢复会话或取得执行权。
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::{Duration, Instant};

use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use tokio::sync::{Mutex, Semaphore, mpsc};
use tokio::task::JoinHandle;

use super::execution;
use super::projection::Projection;
use crate::desktop_owner_ipc::OwnerSnapshot;

// 网页每 15 秒续期；45 秒覆盖一次短暂断线。已确认执行中的桌面会话持续观察到结束。
const INTEREST_TTL: Duration = Duration::from_secs(45);
const CATALOG_INTERVAL: Duration = Duration::from_secs(3);
const RETRY_DELAY: Duration = Duration::from_secs(3);

struct Watch {
    touched: Arc<SyncMutex<Instant>>,
    task: JoinHandle<()>,
}

pub(super) struct DesktopObservers {
    watches: Mutex<HashMap<String, Watch>>,
    projection: Arc<Mutex<Projection>>,
    audit: Arc<execution::Audit>,
    // 最多 4 个发现/首份快照并发，避免列表请求挤占桌面连接；执行中的订阅不占发现名额。
    handshakes: Arc<Semaphore>,
    catalog_interest: SyncMutex<(Instant, Option<Instant>)>,
}

impl Drop for DesktopObservers {
    fn drop(&mut self) {
        for (_, watch) in self.watches.get_mut().drain() {
            watch.task.abort();
        }
    }
}

impl DesktopObservers {
    pub fn new(projection: Arc<Mutex<Projection>>, audit: Arc<execution::Audit>) -> Self {
        Self {
            watches: Mutex::new(HashMap::new()),
            projection,
            audit,
            handshakes: Arc::new(Semaphore::new(4)),
            catalog_interest: SyncMutex::new((Instant::now(), None)),
        }
    }

    pub async fn observe(&self, id: &str, baseline: Option<Value>) {
        self.observe_with(id, baseline, crate::desktop_owner_ipc::watch_snapshots)
            .await;
    }

    async fn observe_with<F, Fut>(&self, id: &str, baseline: Option<Value>, watch_source: F)
    where
        F: Fn(String, mpsc::Sender<OwnerSnapshot>) -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let now = Instant::now();
        self.renew_catalog_interest();
        let mut watches = self.watches.lock().await;
        watches.retain(|_, watch| !watch.task.is_finished());
        if let Some(watch) = watches.get(id) {
            *watch.touched.lock().unwrap() = now;
            return;
        }
        let touched = Arc::new(SyncMutex::new(now));
        let task = execution::spawn_service(
            self.audit.clone(),
            "desktop-events",
            observe_loop(
                id.to_owned(),
                baseline,
                touched.clone(),
                self.projection.clone(),
                self.handshakes.clone(),
                watch_source,
            ),
        );
        watches.insert(id.to_owned(), Watch { touched, task });
    }

    pub fn renew_catalog_interest(&self) {
        // 空会话列表也要发现桌面新会话，目录兴趣不能依赖至少已有一个可订阅的 ID。
        self.catalog_interest.lock().unwrap().1 = Some(Instant::now());
    }

    pub fn catalog_refresh_due(&self) -> bool {
        let mut interest = self.catalog_interest.lock().unwrap();
        if interest.1.is_none_or(|at| at.elapsed() >= INTEREST_TTL)
            || interest.0.elapsed() < CATALOG_INTERVAL
        {
            return false;
        }
        interest.0 = Instant::now();
        true
    }

    pub async fn forget(&self, id: &str) {
        if let Some(watch) = self.watches.lock().await.remove(id) {
            watch.task.abort();
            // 等待取消落定，防止已排队的旧桌面快照覆盖服务新取得的执行权。
            let _ = watch.task.await;
        }
        self.projection.lock().await.forget_desktop(id);
    }

    pub async fn reset(&self) {
        let watches = std::mem::take(&mut *self.watches.lock().await);
        for (id, watch) in watches {
            watch.task.abort();
            let _ = watch.task.await;
            self.projection.lock().await.forget_desktop(&id);
        }
        self.catalog_interest.lock().unwrap().1 = None;
    }
}

fn interested(touched: &SyncMutex<Instant>) -> bool {
    touched.lock().unwrap().elapsed() < INTEREST_TTL
}

pub(super) fn executing(snapshot: &Value) -> bool {
    let status = &snapshot["threadRuntimeStatus"];
    matches!(
        status.as_str().or_else(|| status["type"].as_str()),
        Some("active" | "inProgress" | "running" | "waiting")
    )
}

fn should_observe(
    touched: &SyncMutex<Instant>,
    snapshot: Option<&Value>,
    connected: bool,
    last_observed: Instant,
) -> bool {
    interested(touched)
        || snapshot.is_some_and(executing) && (connected || last_observed.elapsed() < INTEREST_TTL)
}

async fn observe_loop<F, Fut>(
    id: String,
    baseline: Option<Value>,
    touched: Arc<SyncMutex<Instant>>,
    projection: Arc<Mutex<Projection>>,
    handshakes: Arc<Semaphore>,
    watch_source: F,
) where
    F: Fn(String, mpsc::Sender<OwnerSnapshot>) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    let mut previous = None;
    let mut last_observed = Instant::now();
    if let Some(snapshot) = baseline {
        project_snapshot(&id, &mut previous, snapshot, &projection).await;
    }
    loop {
        if !should_observe(&touched, previous.as_ref(), false, last_observed) {
            break;
        }
        let Ok(permit) = handshakes.clone().acquire_owned().await else {
            break;
        };
        if !should_observe(&touched, previous.as_ref(), false, last_observed) {
            break;
        }
        let mut permit = Some(permit);
        let (sender, mut receiver) = mpsc::channel(8);
        let watch = watch_source(id.clone(), sender);
        tokio::pin!(watch);
        let mut expiry = tokio::time::interval(Duration::from_secs(5));
        let mut connected = false;
        loop {
            tokio::select! {
                // 优先处理已入队快照；watch 本次 poll 内仍可能发送最后一帧后立即结束，结束分支也要排空。
                biased;
                Some(observation) = receiver.recv() => {
                    drop(permit.take());
                    connected = true;
                    last_observed = Instant::now();
                    execution::transport("desktop-ipc", "thread-observation", false);
                    project_snapshot(&id, &mut previous, observation.snapshot, &projection).await;
                }
                _ = &mut watch => {
                    drop(permit.take());
                    while let Ok(observation) = receiver.try_recv() {
                        connected = true;
                        last_observed = Instant::now();
                        execution::transport("desktop-ipc", "thread-observation", false);
                        project_snapshot(&id, &mut previous, observation.snapshot, &projection).await;
                    }
                    break;
                }
                _ = expiry.tick() => {
                    if !should_observe(&touched, previous.as_ref(), connected, last_observed) {
                        return;
                    }
                }
            }
        }
        drop(permit);
        if connected || previous.is_some() {
            projection.lock().await.desktop_disconnected(&id);
        }
        // 同一原生会话保留上次快照用于重连差异合并；只读重订阅绝不调用 resume/start。
        if !should_observe(&touched, previous.as_ref(), false, last_observed) {
            break;
        }
        tokio::time::sleep(RETRY_DELAY).await;
    }
}

async fn project_snapshot(
    id: &str,
    previous: &mut Option<Value>,
    snapshot: Value,
    projection: &Mutex<Projection>,
) {
    // 首次订阅只补当前执行轮次；历史完成轮次不能触发新的完成通知。
    let events = crate::codex_service::ServiceProjection::snapshot_changes(
        id,
        previous.as_ref().unwrap_or(&Value::Null),
        &snapshot,
    );
    let mut state = projection.lock().await;
    state.observe_desktop(id, &snapshot, &Value::Array(events));
    *previous = Some(snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    fn snapshot(status: &str, text: &str) -> Value {
        json!({"id":"desktop-main","source":"vscode","threadSource":"user","ephemeral":false,
            "forkedFromId":null,"cwd":"C:\\repo","requests":[],
            "threadRuntimeStatus":{"type":if status == "inProgress" {"active"} else {"idle"}},
            "turns":[{"turnId":"native-turn","status":status,"turnStartedAtMs":1000,
                "items":[{"id":"native-user","type":"userMessage","content":[{"type":"text","text":"原生输入"}]},
                    {"id":"native-answer","type":"agentMessage","text":text}]}]})
    }

    fn observation(snapshot: Value) -> OwnerSnapshot {
        OwnerSnapshot {
            owner: "desktop".into(),
            revision: 1,
            snapshot,
        }
    }

    async fn wait_for(projection: &Mutex<Projection>, ready: impl Fn(&Projection) -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if ready(&*projection.lock().await) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn desktop_edges_empty_sidebar_keeps_catalog_discovery_without_opening_threads() {
        let observers = DesktopObservers::new(
            Arc::new(Mutex::new(Projection::default())),
            Arc::new(execution::Audit::default()),
        );
        observers.catalog_interest.lock().unwrap().0 = Instant::now() - CATALOG_INTERVAL;
        assert!(!observers.catalog_refresh_due());
        observers.renew_catalog_interest();
        assert!(observers.catalog_refresh_due());
        assert!(
            !observers.catalog_refresh_due(),
            "目录更新仍受既有 3 秒节流约束"
        );
        assert!(
            observers.watches.lock().await.is_empty(),
            "空列表续期不得恢复或订阅任意会话"
        );
    }

    #[test]
    fn desktop_edges_large_snapshot_uses_shared_replay_gap_instead_of_losing_changes() {
        let before = snapshot("inProgress", "正在执行");
        let mut after = before.clone();
        after["turns"][0]["items"]
            .as_array_mut()
            .unwrap()
            .extend((0..2500).map(
            |index| json!({"id":format!("item-{index}"),"type":"agentMessage","text":"新增内容"}),
        ));
        let events = crate::codex_service::ServiceProjection::snapshot_changes(
            "desktop-main",
            &before,
            &after,
        );
        assert_eq!(
            events.len(),
            2500,
            "单次完整快照的差异不能先被订阅自身的 2,048 条缓存截断"
        );
        let mut changed = after.clone();
        changed["turns"][0]["items"][2501]["text"] = json!("末尾消息继续更新");
        let delta = crate::codex_service::ServiceProjection::snapshot_changes(
            "desktop-main",
            &after,
            &changed,
        );
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0]["params"]["item"]["id"], "item-2499");
        let mut projection = Projection::default();
        let cursor = projection.events(&json!({}));
        projection.observe_desktop("desktop-main", &after, &Value::Array(events));
        assert_eq!(
            projection.events(&cursor)["reset"],
            true,
            "统一回放通道溢出时应明确要求网页补读"
        );
    }

    #[tokio::test]
    async fn desktop_edges_keep_the_final_snapshot_when_source_returns_immediately() {
        let projection = Arc::new(Mutex::new(Projection::default()));
        let observers =
            DesktopObservers::new(projection.clone(), Arc::new(execution::Audit::default()));
        let cursor = projection.lock().await.events(&json!({}));
        let (finished, mut finish) = mpsc::unbounded_channel();
        observers
            .observe_with("desktop-main", None, move |_, sender| {
                let finished = finished.clone();
                async move {
                    sender
                        .send(observation(snapshot("inProgress", "开始")))
                        .await
                        .unwrap();
                    sender
                        .send(observation(snapshot("completed", "最后结果")))
                        .await
                        .unwrap();
                    finished.send(()).unwrap();
                    anyhow::bail!("收到最终快照后桌面连接立即关闭");
                }
            })
            .await;
        finish.recv().await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let events = projection.lock().await.events(&cursor);
        observers.reset().await;
        let completed = events["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["method"] == "turn/completed")
            .expect("连接结束不能丢弃已收到的最终快照");
        assert_eq!(completed["params"]["turn"]["items"][1]["text"], "最后结果");
        assert_eq!(completed["params"]["turn"]["id"], "native-turn");
    }

    #[tokio::test]
    async fn desktop_reconnect_preserves_native_identity_and_completes_once() {
        let projection = Arc::new(Mutex::new(Projection::default()));
        let observers =
            DesktopObservers::new(projection.clone(), Arc::new(execution::Audit::default()));
        let (attempts, mut connections) = mpsc::unbounded_channel();
        observers
            .observe_with("desktop-main", None, move |_, sender| {
                let (close, closed) = oneshot::channel();
                attempts.send((sender, close)).unwrap();
                async {
                    let _ = closed.await;
                    Ok(())
                }
            })
            .await;
        let (first, close) = connections.recv().await.unwrap();
        first
            .send(observation(snapshot("inProgress", "正在")))
            .await
            .unwrap();
        wait_for(&projection, |state| {
            state.active.contains_key("desktop-main")
        })
        .await;
        let cursor = {
            let state = projection.lock().await;
            json!({"epoch":state.epoch,"cursor":state.sequence})
        };
        close.send(()).unwrap();
        wait_for(&projection, |state| {
            state.threads["desktop-main"]["status"]["type"] == "unknown"
        })
        .await;
        let (second, _close) = tokio::time::timeout(Duration::from_secs(5), connections.recv())
            .await
            .unwrap()
            .unwrap();
        // 重连取得同一原生轮次的最终快照，重复快照不能制造第二次执行或完成。
        let completed = snapshot("completed", "最终结果");
        second.send(observation(completed.clone())).await.unwrap();
        second.send(observation(completed)).await.unwrap();
        wait_for(&projection, |state| {
            state.threads["desktop-main"]["status"]["type"] == "idle"
        })
        .await;
        let state = projection.lock().await;
        let events = state.events(&cursor);
        let completed: Vec<_> = events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["method"] == "turn/completed")
            .collect();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0]["params"]["turn"]["id"], "native-turn");
        assert_eq!(
            completed[0]["params"]["turn"]["items"][0]["id"],
            "native-user"
        );
        assert_eq!(
            completed[0]["params"]["turn"]["items"][1]["text"],
            "最终结果"
        );
        assert!(
            events["events"]
                .as_array()
                .unwrap()
                .iter()
                .all(|event| event["method"] != "turn/started")
        );
        assert!(!state.active.contains_key("desktop-main"));
        drop(state);
        observers.reset().await;
    }

    #[tokio::test]
    async fn desktop_observations_share_subscriptions_and_do_not_evict_running_threads() {
        let projection = Arc::new(Mutex::new(Projection::default()));
        let observers =
            DesktopObservers::new(projection.clone(), Arc::new(execution::Audit::default()));
        let (started, mut starts) = mpsc::unbounded_channel();
        // 超过原先 8 条仍保留所有观察，不得随意淘汰正在执行或当前可见的会话。
        for index in 0..10 {
            let id = format!("desktop-{index}");
            let started = started.clone();
            observers
                .observe_with(&id, None, move |id, sender| {
                    let started = started.clone();
                    async move {
                        let mut value = snapshot("inProgress", "执行中");
                        value["id"] = json!(&id);
                        sender.send(observation(value)).await.unwrap();
                        started.send(id).unwrap();
                        std::future::pending::<()>().await;
                        Ok(())
                    }
                })
                .await;
        }
        for _ in 0..10 {
            tokio::time::timeout(Duration::from_secs(2), starts.recv())
                .await
                .unwrap()
                .unwrap();
        }
        wait_for(&projection, |state| state.active.len() == 10).await;
        let duplicate_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let duplicate = duplicate_started.clone();
        observers
            .observe_with("desktop-0", None, move |_, _| {
                duplicate.store(true, std::sync::atomic::Ordering::SeqCst);
                std::future::ready(Ok(()))
            })
            .await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            !duplicate_started.load(std::sync::atomic::Ordering::SeqCst),
            "重复页面不得建立第二个订阅"
        );
        assert_eq!(observers.watches.lock().await.len(), 10);
        observers.forget("desktop-0").await;
        assert!(!projection.lock().await.active.contains_key("desktop-0"));
        assert_eq!(observers.watches.lock().await.len(), 9);
        observers.reset().await;
        assert!(observers.watches.lock().await.is_empty());
    }

    #[test]
    fn desktop_interest_retains_active_work_but_releases_abandoned_history() {
        assert!(
            executing(&json!({"threadRuntimeStatus":{"type":"active"},"turns":[]})),
            "原生运行状态独立于历史分页，当前轮次尚未加载不能被当作空闲"
        );
        let expired = Instant::now() - INTEREST_TTL - Duration::from_secs(1);
        let touched = SyncMutex::new(expired);
        assert!(!should_observe(
            &touched,
            Some(&snapshot("completed", "")),
            true,
            Instant::now()
        ));
        assert!(should_observe(
            &touched,
            Some(&snapshot("inProgress", "")),
            true,
            expired
        ));
        assert!(should_observe(
            &touched,
            Some(&snapshot("inProgress", "")),
            false,
            Instant::now()
        ));
        assert!(!should_observe(
            &touched,
            Some(&snapshot("inProgress", "")),
            false,
            expired
        ));
    }
}
