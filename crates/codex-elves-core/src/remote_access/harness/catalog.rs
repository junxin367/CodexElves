use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::{Duration, Instant};

#[cfg(test)]
use anyhow::Context;
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify};

use super::projection::Projection;
use super::runtime::Runtime;
use crate::codex_service::normalize_thread;

// 用户要求非 CDP 首屏及重复进入保持流畅：先给最近 100 条，完整目录后台补齐；
// 热目录最多每 3 秒补读首页，每 60 秒完整核对删除和归档，失败后等待 5 秒再试。
const PAGE_SIZE: usize = 100;
const HEAD_MAX_AGE: Duration = Duration::from_secs(3);
const FULL_MAX_AGE: Duration = Duration::from_secs(60);
const RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Default)]
struct State {
    generation: String,
    revision: u64,
    rows: BTreeMap<String, Value>,
    loaded: bool,
    complete: bool,
    next_cursor: Option<String>,
    refreshed_at: Option<Instant>,
    fully_refreshed_at: Option<Instant>,
    retry_after: Option<Instant>,
    dirty: bool,
}

impl State {
    fn accept(&self, generation: &str, revision: u64) -> bool {
        self.generation == generation && self.revision == revision
    }

    fn needs_refresh(&self) -> bool {
        self.retry_after.is_none_or(|until| Instant::now() >= until)
            && (!self.complete
                || self.dirty
                || self
                    .refreshed_at
                    .is_none_or(|at| at.elapsed() >= HEAD_MAX_AGE)
                || self
                    .fully_refreshed_at
                    .is_none_or(|at| at.elapsed() >= FULL_MAX_AGE))
    }
}

#[derive(Default)]
pub(super) struct Catalog {
    state: Mutex<State>,
    gate: Arc<Mutex<()>>,
    worker: SyncMutex<Option<tokio::task::JoinHandle<()>>>,
    progress: Notify,
}

impl Drop for Catalog {
    fn drop(&mut self) {
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            worker.abort();
        }
    }
}

impl Catalog {
    pub async fn snapshot(
        self: &Arc<Self>,
        runtime: &Arc<Runtime>,
        projection: &Arc<Mutex<Projection>>,
    ) -> anyhow::Result<(Vec<Value>, bool)> {
        let generation = runtime.connection().await?.generation.clone();
        {
            let mut state = self.state.lock().await;
            if state.generation != generation {
                *state = State {
                    generation: generation.clone(),
                    ..Default::default()
                };
            }
            if state.loaded {
                let result = (state.rows.values().cloned().collect(), state.complete);
                let refresh = state.needs_refresh();
                drop(state);
                if refresh {
                    self.refresh_in_background(runtime, projection);
                }
                return Ok(result);
            }
        }
        let gate = self.gate.lock().await;
        let revision = {
            let state = self.state.lock().await;
            if state.loaded && state.generation == generation {
                return Ok((state.rows.values().cloned().collect(), state.complete));
            }
            state.revision
        };
        let page = runtime
            .request(
                "thread/list",
                json!({"limit":PAGE_SIZE,"archived":false,"sortKey":"updated_at"}),
                None,
            )
            .await?;
        let mut state = self.state.lock().await;
        anyhow::ensure!(
            state.accept(&generation, revision),
            "会话目录已变更，请重新读取"
        );
        state.rows = rows(&page);
        state.next_cursor = next_cursor(&page);
        state.loaded = true;
        state.complete = state.next_cursor.is_none();
        state.refreshed_at = Some(Instant::now());
        if state.complete {
            state.fully_refreshed_at = state.refreshed_at;
        }
        let result = (state.rows.values().cloned().collect(), state.complete);
        drop(state);
        drop(gate);
        self.refresh_in_background(runtime, projection);
        Ok(result)
    }

    fn refresh_in_background(
        self: &Arc<Self>,
        runtime: &Arc<Runtime>,
        projection: &Arc<Mutex<Projection>>,
    ) {
        let mut worker = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if worker.as_ref().is_some_and(|worker| !worker.is_finished()) {
            return;
        }
        let weak = Arc::downgrade(self);
        let runtime = runtime.clone();
        let projection = projection.clone();
        *worker = Some(super::execution::spawn_inherited(async move {
            let Some(catalog) = weak.upgrade() else {
                return;
            };
            // 等待同批首屏读取让出锁，避免并发预热时错过唯一一次后台补齐机会。
            let gate = catalog.gate.clone().lock_owned().await;
            let (generation, revision, full, mut cursor, mut collected) = {
                let state = catalog.state.lock().await;
                if !state.loaded || !state.needs_refresh() {
                    return;
                }
                let full = !state.complete
                    || state.dirty
                    || state
                        .fully_refreshed_at
                        .is_none_or(|at| at.elapsed() >= FULL_MAX_AGE);
                if !full
                    && state
                        .refreshed_at
                        .is_some_and(|at| at.elapsed() < HEAD_MAX_AGE)
                {
                    return;
                }
                let resume = !state.complete && !state.dirty && state.next_cursor.is_some();
                (
                    state.generation.clone(),
                    state.revision,
                    full,
                    if resume {
                        state.next_cursor.clone()
                    } else {
                        None
                    },
                    if resume {
                        state.rows.clone()
                    } else {
                        BTreeMap::new()
                    },
                )
            };
            // 后台任务不持有 Catalog 的强引用，退出服务能立即取消分页。
            drop(catalog);
            let mut cursors = HashSet::new();
            let result: anyhow::Result<()> = async {
                loop {
                    if let Some(cursor) = &cursor {
                        anyhow::ensure!(cursors.insert(cursor.clone()), "会话分页游标重复");
                    }
                    let page = runtime
                        .request(
                            "thread/list",
                            json!({"limit":PAGE_SIZE,"archived":false,"sortKey":"updated_at","cursor":cursor}),
                            None,
                        )
                        .await?;
                    let Some(catalog) = weak.upgrade() else {
                        return Ok(());
                    };
                    if !catalog.state.lock().await.accept(&generation, revision)
                        || runtime.connection().await?.generation != generation
                    {
                        return Ok(());
                    }
                    collected.extend(rows(&page));
                    cursor = next_cursor(&page);
                    if !full || cursor.is_none() {
                        let mut state = catalog.state.lock().await;
                        if !state.accept(&generation, revision) {
                            return Ok(());
                        }
                        let changed = if full {
                            state.rows != collected || !state.complete
                        } else {
                            collected.iter().any(|(id, row)| state.rows.get(id) != Some(row))
                        };
                        if full {
                            state.rows = collected;
                            state.complete = true;
                            state.next_cursor = None;
                            state.fully_refreshed_at = Some(Instant::now());
                            state.dirty = false;
                        } else {
                            state.rows.extend(collected);
                        }
                        state.refreshed_at = Some(Instant::now());
                        state.retry_after = None;
                        drop(state);
                        if changed {
                            projection.lock().await.push(json!({
                                "method":"remote/sidebarChanged","params":{"reason":"catalog"}
                            }));
                        }
                        break;
                    }
                }
                Ok(())
            }
            .await;
            if result.is_err()
                && let Some(catalog) = weak.upgrade()
            {
                let mut state = catalog.state.lock().await;
                if state.accept(&generation, revision) {
                    state.retry_after = Some(Instant::now() + RETRY_DELAY);
                }
            }
            if let Some(catalog) = weak.upgrade() {
                catalog.progress.notify_waiters();
            }
            drop(gate);
        }));
    }

    #[cfg(test)]
    pub async fn find(
        self: &Arc<Self>,
        runtime: &Arc<Runtime>,
        projection: &Arc<Mutex<Projection>>,
        matches: impl Fn(&Value) -> bool,
    ) -> anyhow::Result<Option<Value>> {
        // 验证后台分页时仅让指定会话等待完整目录，不阻塞其他读取。
        tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                let changed = self.progress.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                let (rows, complete) = self.snapshot(runtime, projection).await?;
                if let Some(row) = rows.into_iter().find(&matches) {
                    return Ok(Some(row));
                }
                if complete {
                    return Ok(None);
                }
                anyhow::ensure!(
                    self.state.lock().await.retry_after.is_none(),
                    "会话目录更新失败，请稍后重新读取"
                );
                // 版本变化退役旧任务时也需重新检查；等待不会触发重复引擎请求。
                let _ = tokio::time::timeout(Duration::from_secs(1), &mut changed).await;
            }
        })
        .await
        .context("读取会话目录超时")?
    }

    pub async fn observe(&self, event: &Value) {
        let method = event["method"].as_str().unwrap_or("");
        if !matches!(
            method,
            "thread/started"
                | "thread/name/updated"
                | "thread/deleted"
                | "thread/archived"
                | "thread/unarchived"
                | "thread/project/updated"
                | "thread/readState/updated"
        ) {
            return;
        }
        let mut state = self.state.lock().await;
        if let Some(generation) = event["_generation"].as_str()
            && generation != state.generation
        {
            return;
        }
        let params = &event["params"];
        let id = params["threadId"]
            .as_str()
            .or_else(|| params["thread"]["id"].as_str())
            .unwrap_or("");
        if id.is_empty() {
            return;
        }
        if method == "thread/started" && metadata(&params["thread"]).is_none() {
            return;
        }
        state.revision += 1;
        state.refreshed_at = None;
        state.retry_after = None;
        match method {
            "thread/deleted" | "thread/archived" => {
                state.rows.remove(id);
                state.dirty |= !state.complete;
            }
            "thread/started" => {
                if let Some(thread) = metadata(&params["thread"]) {
                    state.rows.insert(id.to_owned(), thread);
                }
                state.dirty |= !state.complete;
            }
            "thread/name/updated" => {
                if let Some(thread) = state.rows.get_mut(id) {
                    thread["title"] = params["name"].clone();
                }
            }
            "thread/project/updated" => {
                // 桌面移动或移出项目后，服务目录同步显式归属，包括表示无项目的 null。
                if let Some(thread) = state.rows.get_mut(id) {
                    thread["projectId"] = params["projectId"].clone();
                }
                state.dirty |= !state.complete;
            }
            "thread/readState/updated" => {
                if let Some(thread) = state.rows.get_mut(id) {
                    thread["readState"] = params["readState"].clone();
                    thread["hasUnreadTurn"] = json!(!params["readState"]["firstUnread"].is_null());
                }
            }
            _ => state.dirty = true,
        }
    }

    pub async fn reset(&self) {
        // 先退役目录版本，再取消分页，阻止已读完的旧首批请求重新启动后台任务。
        let mut state = self.state.lock().await;
        *state = State::default();
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            worker.abort();
        }
        drop(state);
        self.progress.notify_waiters();
    }
}

fn rows(page: &Value) -> BTreeMap<String, Value> {
    page["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let thread = metadata(row)?;
            Some((thread["id"].as_str()?.to_owned(), thread))
        })
        .collect()
}

fn metadata(row: &Value) -> Option<Value> {
    let mut thread = normalize_thread(row).ok()?;
    for key in ["projectId", "readState"] {
        if let Some(value) = row.get(key) {
            thread[key] = value.clone();
        }
    }
    Some(thread)
}

fn next_cursor(page: &Value) -> Option<String> {
    page["nextCursor"]
        .as_str()
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn engine_fixture() -> (tempfile::TempDir, crate::codex_service::RunningService) {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("catalog-engine.cjs");
        std::fs::write(&script, r#"
const fs=require('node:fs'),readline=require('node:readline');
const output=value=>process.stdout.write(JSON.stringify(value)+'\n');
const itemReads=[];
readline.createInterface({input:process.stdin}).on('line',line=>{
 const q=JSON.parse(line);if(q.method==='initialized')return;
 if(q.method==='initialize'){output({id:q.id,result:{}});return;}
 fs.appendFileSync(__dirname+'/reads.jsonl',JSON.stringify(q)+'\n');
 const catalog=fs.existsSync(__dirname+'/catalog.json')?JSON.parse(fs.readFileSync(__dirname+'/catalog.json','utf8')):null;
 if(q.method==='project/list'){
   output({id:q.id,result:{data:catalog?.projects||[{id:'project-a',name:'测试项目',roots:[{path:__dirname}]}],nextCursor:null}});return;
 }
 if(q.method==='model/list'){
   if(fs.existsSync(__dirname+'/models-fail'))output({id:q.id,error:{code:-32601,message:'isolated model failure'}});
   else output({id:q.id,result:{data:[{id:'gpt-test',model:'gpt-test'}],nextCursor:null}});return;
 }
 if(q.method==='config/read'){output({id:q.id,result:{config:{model:'gpt-test'}}});return;}
 if(q.method==='thread/turns/list'){
   output({id:q.id,result:{data:['new','middle','old'].map(id=>({id,status:'completed'})),nextCursor:null}});
   return;
 }
 if(q.method==='thread/items/list'){
   itemReads.push(q);
   if(itemReads.length===3)for(const request of itemReads.reverse()){
     output({id:request.id,result:{data:[{id:request.params.turnId+'-reply'},{id:request.params.turnId+'-user'}],nextCursor:null}});
   }
   return;
 }
 if(q.method==='thread/read'){output({id:q.id,result:{thread:{id:q.params.threadId,cwd:__dirname,ephemeral:false,source:'appServer',threadSource:'user'}}});return;}
 if(q.method!=='thread/list'){output({id:q.id,error:{code:-32601,message:'unsupported'}});return;}
 const removed=fs.existsSync(__dirname+'/deleted');
 const data=(catalog?.threads||Array.from({length:250},(_,i)=>({
   id:'thread-'+i,cwd:__dirname,name:'会话 '+i,projectId:'project-a',updatedAt:250-i,createdAt:1,
   status:{type:'idle'},readState:{firstUnread:null,revision:'r1'}
 }))).filter(row=>!removed||row.id!=='thread-125');
 const start=Number(q.params.cursor||0),end=start+q.params.limit;
 const result={data:data.slice(start,end),nextCursor:end<data.length?String(end):null};
 const gate=start===0&&fs.existsSync(__dirname+'/hold-first')?'continue-first':start===100?'continue':null;
 if(gate&&!fs.existsSync(__dirname+'/'+gate)){
   const timer=setInterval(()=>{if(fs.existsSync(__dirname+'/'+gate)){clearInterval(timer);output({id:q.id,result});}},5);
 }else output({id:q.id,result});
});
"#).unwrap();
        let mut command = tokio::process::Command::new("node");
        command
            .arg(script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(crate::windows_create_hidden_process());
        let service =
            crate::codex_service::RunningService::for_test(command.spawn().unwrap()).unwrap();
        (directory, service)
    }

    async fn fixture() -> (
        tempfile::TempDir,
        Arc<Runtime>,
        Arc<Catalog>,
        Arc<Mutex<Projection>>,
        tokio::sync::mpsc::Receiver<Value>,
    ) {
        let (directory, service) = engine_fixture().await;
        let (events, receiver) = tokio::sync::mpsc::channel(512);
        let runtime = Arc::new(Runtime::new(
            events,
            Arc::new(super::super::execution::Audit::default()),
        ));
        runtime.install_test_process(service).await;
        (
            directory,
            runtime,
            Arc::new(Catalog::default()),
            Arc::new(Mutex::new(Projection::default())),
            receiver,
        )
    }

    async fn wait_until(mut condition: impl AsyncFnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition().await {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn first_page_and_warm_reads_do_not_wait_for_background_pagination() {
        let (directory, runtime, catalog, projection, _receiver) = fixture().await;
        let (first, complete) = catalog.snapshot(&runtime, &projection).await.unwrap();
        assert_eq!(first.len(), 100);
        assert!(!complete);
        assert!(first.iter().all(|row| row["projectId"] == "project-a"));
        let (warm, complete) = tokio::time::timeout(
            Duration::from_secs(1),
            catalog.snapshot(&runtime, &projection),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(warm, first);
        assert!(!complete);
        wait_until(async || {
            std::fs::read_to_string(directory.path().join("reads.jsonl"))
                .unwrap_or_default()
                .lines()
                .count()
                == 2
        })
        .await;
        std::fs::write(directory.path().join("continue"), "").unwrap();
        wait_until(async || catalog.state.lock().await.complete).await;
        let (all, complete) = catalog.snapshot(&runtime, &projection).await.unwrap();
        assert_eq!(all.len(), 250);
        assert!(complete);
        let requests = std::fs::read_to_string(directory.path().join("reads.jsonl")).unwrap();
        assert_eq!(requests.lines().count(), 3);
        assert!(projection.lock().await.sequence > 0);
    }

    #[tokio::test]
    async fn deleted_threads_are_not_restored_by_an_older_inflight_page() {
        let (directory, runtime, catalog, projection, _receiver) = fixture().await;
        catalog.snapshot(&runtime, &projection).await.unwrap();
        wait_until(async || {
            std::fs::read_to_string(directory.path().join("reads.jsonl"))
                .unwrap_or_default()
                .lines()
                .count()
                == 2
        })
        .await;
        catalog
            .observe(&json!({"method":"thread/deleted","params":{"threadId":"thread-125"}}))
            .await;
        std::fs::write(directory.path().join("deleted"), "").unwrap();
        std::fs::write(directory.path().join("continue"), "").unwrap();
        wait_until(async || {
            let (rows, complete) = catalog.snapshot(&runtime, &projection).await.unwrap();
            assert!(rows.iter().all(|row| row["id"] != "thread-125"));
            complete
        })
        .await;
        assert_eq!(catalog.state.lock().await.rows.len(), 249);
    }

    #[tokio::test]
    async fn old_engine_events_and_old_read_revisions_cannot_change_current_catalog() {
        let catalog = Catalog::default();
        {
            let mut state = catalog.state.lock().await;
            state.generation = "current".into();
            state.loaded = true;
            state.complete = true;
            state
                .rows
                .insert("a".into(), json!({"id":"a","cwd":"test","title":"原名"}));
        }
        catalog
            .observe(
                &json!({"method":"thread/deleted","_generation":"old","params":{"threadId":"a"}}),
            )
            .await;
        assert_eq!(catalog.state.lock().await.rows["a"]["title"], "原名");
        let old_revision = catalog.state.lock().await.revision;
        catalog.observe(&json!({"method":"thread/name/updated","_generation":"current","params":{"threadId":"a","name":"新名"}})).await;
        let state = catalog.state.lock().await;
        assert_eq!(state.rows["a"]["title"], "新名");
        assert!(!state.accept("current", old_revision));
        drop(state);
        catalog.reset().await;
        assert!(!catalog.state.lock().await.loaded);
    }

    #[tokio::test]
    async fn history_content_reads_are_concurrent_and_preserve_turn_and_item_order() {
        let (directory, runtime, _catalog, _projection, _receiver) = fixture().await;
        let mut service = super::super::Harness::new(directory.path()).unwrap();
        Arc::get_mut(&mut service).unwrap().runtime = runtime;
        let history = tokio::time::timeout(
            Duration::from_secs(5),
            service.history(&json!({"threadId":"isolated-performance-history"})),
        )
        .await
        .unwrap()
        .unwrap();
        let turns = history["data"].as_array().unwrap();
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["new", "middle", "old"]
        );
        for turn in turns {
            assert_eq!(
                turn["items"][0]["id"],
                format!("{}-user", turn["id"].as_str().unwrap())
            );
            assert_eq!(
                turn["items"][1]["id"],
                format!("{}-reply", turn["id"].as_str().unwrap())
            );
        }
    }

    #[tokio::test]
    async fn a_specific_older_workspace_can_wait_without_blocking_other_reads() {
        let (directory, runtime, catalog, projection, _receiver) = fixture().await;
        catalog.snapshot(&runtime, &projection).await.unwrap();
        let lookup = {
            let catalog = catalog.clone();
            let runtime = runtime.clone();
            let projection = projection.clone();
            tokio::spawn(async move {
                catalog
                    .find(&runtime, &projection, |row| row["id"] == "thread-125")
                    .await
            })
        };
        let (_, complete) = catalog.snapshot(&runtime, &projection).await.unwrap();
        assert!(!complete);
        std::fs::write(directory.path().join("continue"), "").unwrap();
        let row = tokio::time::timeout(Duration::from_secs(5), lookup)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(row["id"], "thread-125");
    }

    fn requests(directory: &std::path::Path) -> Vec<Value> {
        std::fs::read_to_string(directory.join("reads.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn harness_sidebar_preserves_saved_projects_and_explicit_projectless_membership() {
        use sha2::{Digest, Sha256};

        let (directory, process) = engine_fixture().await;
        let service = super::super::Harness::new(directory.path()).unwrap();
        service.install_test_process(process).await;
        let fixture = json!({
            "projects": [
                {"id":"project-z","name":"同名项目","roots":[{"path":"/repo/z"},{"path":"/shared"}],"position":0},
                {"id":"project-a","name":"同名项目","roots":[{"path":"/repo/a"},{"path":"/shared"}],"position":1},
                {"id":"workspace-saved","name":"空项目","roots":[{"path":"/repo/empty"}],"position":2}
            ],
            "threads": [
                {"id":"assigned","cwd":"/repo/z","projectId":"project-a","updatedAt":10},
                {"id":"projectless","cwd":"/repo/z","projectId":null,"updatedAt":20},
                {"id":"legacy","cwd":"/repo/z/","updatedAt":30},
                {"id":"ambiguous","cwd":"/shared","updatedAt":40},
                {"id":"deleted-project","cwd":"/repo/z","projectId":"removed","updatedAt":50},
                {"id":"loose","cwd":"/tmp/new-chat","updatedAt":60},
                {"id":"worktree","cwd":"/worktrees/feature","projectId":"project-z","updatedAt":70}
            ]
        });
        std::fs::write(
            directory.path().join("catalog.json"),
            serde_json::to_vec(&fixture).unwrap(),
        )
        .unwrap();
        let sidebar = service.sidebar().await.unwrap();
        assert_eq!(sidebar["complete"], true);
        let projects = sidebar["projects"].as_array().unwrap();
        assert_eq!(
            projects
                .iter()
                .map(|project| project["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["project-z", "project-a", "workspace-saved"],
            "选择器保留项目顺序，不按 ID 排序或补入会话工作目录"
        );
        assert!(projects.iter().all(|project| project["synthetic"] != true));
        let recent = sidebar["recent"].as_array().unwrap();
        assert_eq!(recent.len(), 7, "无项目会话不能因移除合成项目而消失");
        for id in ["projectless", "ambiguous", "deleted-project", "loose"] {
            assert_eq!(
                recent.iter().find(|thread| thread["id"] == id).unwrap()["projectId"],
                Value::Null,
                "{id} 应保留无项目归属"
            );
        }
        assert_eq!(
            projects[0]["threads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|thread| thread["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["legacy", "worktree"]
        );
        assert_eq!(projects[1]["threads"][0]["id"], "assigned");
        assert_eq!(projects[2]["threads"], json!([]));
        assert_eq!(
            service
                .workspace_context(&json!({"projectId":"workspace-saved"}))
                .await
                .unwrap()["cwd"],
            "/repo/empty",
            "已保存项目不根据 ID 前缀误判为合成项目"
        );
        let stale_id = format!("workspace-{:x}", Sha256::digest(b"/tmp/new-chat"));
        let reads_before = requests(directory.path()).len();
        assert!(
            service
                .workspace_context(&json!({"projectId":stale_id}))
                .await
                .is_err(),
            "旧合成项目不能继续解析为可选工作目录"
        );
        assert_eq!(requests(directory.path()).len(), reads_before);
        service
            .store
            .lock()
            .await
            .data
            .project_pins
            .insert("project-a".into());
        let pinned = service.sidebar().await.unwrap();
        assert_eq!(
            pinned["projects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|project| project["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["project-a", "project-z", "workspace-saved"]
        );
        service.release().await;
    }

    #[tokio::test]
    async fn project_membership_events_keep_explicit_null_and_reject_old_generations() {
        let catalog = Catalog::default();
        {
            let mut state = catalog.state.lock().await;
            state.generation = "current".into();
            state.loaded = true;
            state.complete = true;
            state.rows.insert(
                "thread-a".into(),
                json!({"id":"thread-a","cwd":"/repo/a","projectId":"project-a"}),
            );
        }
        for project_id in [Value::Null, json!("project-b")] {
            catalog
                .observe(&json!({
                    "method":"thread/project/updated","_generation":"current",
                    "params":{"threadId":"thread-a","projectId":project_id}
                }))
                .await;
            assert_eq!(
                catalog.state.lock().await.rows["thread-a"]["projectId"],
                project_id
            );
        }
        catalog
            .observe(&json!({
                "method":"thread/project/updated","_generation":"old",
                "params":{"threadId":"thread-a","projectId":"project-a"}
            }))
            .await;
        assert_eq!(
            catalog.state.lock().await.rows["thread-a"]["projectId"],
            "project-b"
        );
    }

    #[tokio::test]
    async fn migrating_desktop_projects_keep_cdp_ids_memberships_order_and_pin_overrides() {
        let (directory, process) = engine_fixture().await;
        let service = super::super::Harness::new(directory.path()).unwrap();
        service.install_test_process(process).await;
        std::fs::write(
            directory.path().join("catalog.json"),
            serde_json::to_vec(&json!({
                "projects":[
                    {"id":"server-z","name":"引擎项目","roots":[{"path":"/repo/z"}]},
                    {"id":"server-a","name":"另一项目","roots":[{"path":"/repo/a"}]}
                ],
                "threads":[
                    {"id":"pending-assigned","cwd":"/different","projectId":null},
                    {"id":"pending-projectless","cwd":"/repo/z","projectId":null},
                    {"id":"pending-inferred","cwd":"/repo/z","projectId":null},
                    {"id":"migrated-projectless","cwd":"/repo/z","projectId":null},
                    {"id":"canonical","cwd":"/repo/a","projectId":"server-a"}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let scope = format!("local:{}", directory.path().display());
        let mut desktop = json!({
            "local-projects":{"desktop-z":{"name":"桌面项目"},"desktop-a":{"name":"另一项目"}},
            "thread-project-assignments":{
                "pending-assigned":{"projectKind":"local","projectId":"desktop-z"},
                "migrated-projectless":{"projectKind":"local","projectId":"desktop-z"}
            },
            "projectless-thread-ids":["pending-projectless"],
            "project-order":["desktop-z","desktop-a"],
            "pinned-project-ids":["desktop-a"],
            "app-server-project-id-by-legacy-project-id-by-host":{
                "remote:other":{"desktop-z":"foreign-project"},
                (scope.clone()):{"desktop-z":"server-z","desktop-a":"server-a"}
            },
            "app-server-projects-migration-by-host":{
                (scope.clone()):{"projectsMigrated":true,"threadAssignmentsMigrated":false,
                    "threadAssignmentsReadMigrated":true,
                    "pendingThreadAssignmentIds":["pending-assigned","pending-projectless","pending-inferred"]}
            }
        });
        let state_path = directory.path().join(".codex-global-state.json");
        std::fs::write(&state_path, serde_json::to_vec(&desktop).unwrap()).unwrap();
        let sidebar = service.sidebar().await.unwrap();
        assert_eq!(sidebar["projects"][0]["id"], "desktop-a");
        assert_eq!(sidebar["projects"][0]["pinned"], true);
        assert_eq!(sidebar["projects"][1]["id"], "desktop-z");
        assert_eq!(sidebar["projects"][1]["name"], "桌面项目");
        let threads = sidebar["recent"].as_array().unwrap();
        for (id, expected) in [
            ("pending-assigned", json!("desktop-z")),
            ("pending-inferred", json!("desktop-z")),
            ("pending-projectless", Value::Null),
            ("migrated-projectless", Value::Null),
            ("canonical", json!("desktop-a")),
        ] {
            assert_eq!(
                threads.iter().find(|thread| thread["id"] == id).unwrap()["projectId"],
                expected
            );
        }
        let context = service
            .workspace_context(&json!({"projectId":"desktop-z"}))
            .await
            .unwrap();
        assert_eq!(context["cwd"], "/repo/z");
        assert_eq!(context["serverProjectId"], "server-z");
        service
            .store
            .lock()
            .await
            .data
            .project_pin_overrides
            .insert("desktop-a".into(), false);
        let unpinned = service.sidebar().await.unwrap();
        assert_eq!(unpinned["projects"][0]["id"], "desktop-z");
        assert_eq!(unpinned["projects"][1]["pinned"], false);
        desktop["app-server-projects-migration-by-host"][&scope]["threadAssignmentsMigrated"] =
            json!(true);
        std::fs::write(&state_path, serde_json::to_vec(&desktop).unwrap()).unwrap();
        let migrated = service.sidebar().await.unwrap();
        assert_eq!(
            migrated["recent"]
                .as_array()
                .unwrap()
                .iter()
                .find(|thread| thread["id"] == "pending-assigned")
                .unwrap()["projectId"],
            Value::Null
        );
        service.release().await;
    }

    #[tokio::test]
    async fn harness_prewarm_is_read_only_shared_and_does_not_enable_dispatch() {
        let (directory, process) = engine_fixture().await;
        let service = super::super::Harness::new(directory.path()).unwrap();
        service.install_test_process(process).await;
        let (first, second) = tokio::join!(service.prewarm(), service.prewarm());
        first.unwrap();
        second.unwrap();
        assert!(!service.enabled.load(std::sync::atomic::Ordering::Acquire));
        assert!(service.loaded.lock().await.is_empty());
        assert!(service.store.lock().await.data.queues.is_empty());
        assert!(!directory.path().join("remote-harness-state.json").exists());
        let calls = requests(directory.path());
        for method in ["model/list", "config/read", "project/list"] {
            assert_eq!(
                calls.iter().filter(|call| call["method"] == method).count(),
                1
            );
        }
        assert_eq!(calls.iter().filter(|call| call["method"] == "thread/list"
            && call["params"]["cursor"].is_null()).count(), 1);
        assert!(calls.iter().all(|call| matches!(
            call["method"].as_str(),
            Some(
                "thread/list"
                    | "threadSection/list"
                    | "project/list"
                    | "model/list"
                    | "config/read"
            )
        )));
        service.stop_preparation().await;
        assert!(!service.catalog.state.lock().await.loaded);
        assert!(
            service.prepare().await.is_err(),
            "仅预热的引擎应已释放，测试禁止启动真实引擎"
        );
    }

    struct TestController(Arc<crate::remote_access::RemoteAccessController>);

    impl Drop for TestController {
        fn drop(&mut self) {
            if let Ok(mut live) = self.0.live.try_lock() {
                if let Some(handle) = live.handle.take() {
                    handle.shutdown();
                }
                for task in [live.server.take(), live.pump.take()].into_iter().flatten() {
                    task.abort();
                }
            }
        }
    }

    struct NativeOnly;

    #[async_trait::async_trait]
    impl crate::remote_access::NativeRemoteBridge for NativeOnly {
        async fn call(&self, _action: &str, _payload: Value) -> anyhow::Result<Value> {
            Ok(json!({"ok":true,"origin":"cdp","epoch":"native","cursor":0,"events":[]}))
        }
    }

    async fn listening_controller(
        directory: &std::path::Path,
        process: crate::codex_service::RunningService,
    ) -> (TestController, u16) {
        let controller = crate::remote_access::RemoteAccessController::new(
            Arc::new(NativeOnly),
            directory.to_owned(),
        )
        .unwrap();
        controller.harness.install_test_process(process).await;
        let reservation = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let settings = crate::settings::RemoteAccessSettings {
            enabled: true,
            port,
            ..Default::default()
        };
        let (handle, server, pump) = tokio::time::timeout(
            Duration::from_secs(2),
            controller.start_server(settings.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        {
            let mut live = controller.live.lock().await;
            live.settings = Some(settings.clone());
            live.handle = Some(handle);
            live.server = Some(server);
            live.pump = Some(pump);
        }
        controller.publish_access_policy(&settings);
        controller
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        (TestController(controller), port)
    }

    #[tokio::test]
    async fn harness_startup_prewarms_before_any_remote_read_without_blocking_cdp() {
        let (directory, process) = engine_fixture().await;
        std::fs::write(directory.path().join("hold-first"), "").unwrap();
        let (guard, port) = listening_controller(directory.path(), process).await;
        let controller = &guard.0;
        wait_until(async || {
            requests(directory.path())
                .iter()
                .any(|call| call["method"] == "thread/list")
        })
        .await;
        assert!(
            controller
                .runtime_for("unconnected-device")
                .await
                .cdp_injection_enabled
        );
        assert!(
            !controller
                .harness
                .enabled
                .load(std::sync::atomic::Ordering::Acquire)
        );
        assert_eq!(
            controller.native.call("models", json!({})).await.unwrap()["origin"],
            "cdp"
        );
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "目录尚未就绪时网页仍须正常返回"
        );
        assert!(!controller.harness.catalog.state.lock().await.loaded);
        std::fs::write(directory.path().join("continue-first"), "").unwrap();
        std::fs::write(directory.path().join("continue"), "").unwrap();
        wait_until(async || controller.harness.catalog.state.lock().await.complete).await;
        let before = requests(directory.path()).len();
        let sidebar = controller
            .harness
            .call("sidebar", json!({}), None)
            .await
            .unwrap();
        assert_eq!(sidebar["complete"], true);
        assert_eq!(sidebar["recent"].as_array().unwrap().len(), 250);
        assert_eq!(
            requests(directory.path()).len(),
            before,
            "首次服务模式读取应直接复用预热目录"
        );
        assert!(
            !directory
                .path()
                .join("remote-access-device-runtimes.json")
                .exists()
        );
        controller
            .reconcile(crate::settings::RemoteAccessSettings::default())
            .await;
        assert!(!controller.running.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!controller.harness.catalog.state.lock().await.loaded);
    }

    #[tokio::test]
    async fn harness_prewarm_failure_and_cancellation_leave_cdp_and_listener_available() {
        for fail_models in [false, true] {
            let (directory, process) = engine_fixture().await;
            std::fs::write(directory.path().join("hold-first"), "").unwrap();
            if fail_models {
                std::fs::write(directory.path().join("models-fail"), "").unwrap();
            }
            let (guard, _port) = listening_controller(directory.path(), process).await;
            let controller = &guard.0;
            wait_until(async || {
                requests(directory.path())
                    .iter()
                    .any(|call| call["method"] == "model/list")
            })
            .await;
            if fail_models {
                std::fs::write(directory.path().join("continue-first"), "").unwrap();
                std::fs::write(directory.path().join("continue"), "").unwrap();
                wait_until(async || controller.harness.catalog.state.lock().await.complete).await;
            }
            assert_eq!(
                controller.native.call("models", json!({})).await.unwrap()["origin"],
                "cdp"
            );
            assert!(controller.live.lock().await.error.is_none());
            controller
                .reconcile(crate::settings::RemoteAccessSettings::default())
                .await;
            assert!(!controller.running.load(std::sync::atomic::Ordering::SeqCst));
            assert!(!controller.harness.catalog.state.lock().await.loaded);
            assert!(controller.harness.prepare().await.is_err());
        }
    }
}
