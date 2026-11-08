use std::collections::HashSet;

use anyhow::Context;
use serde_json::{Value, json};

use super::super::{ApiError, RemoteAuthorization};
use super::{Harness, error, required, runtime::UnsupportedMethod};
use crate::codex_service::{normalize_thread, normalize_thread_response};

// Codex 原生固定置顶分区的 ID；分区名称可修改，不能用显示文案“Pinned”识别。
const PINNED_SECTION_ID: &str = "01984de2-8f74-7c91-a3b2-5c5e937cf318";
const PAGE_LIMIT: usize = 100;

fn next_cursor(page: &Value, cursors: &mut HashSet<String>) -> anyhow::Result<Option<String>> {
    match &page["nextCursor"] {
        Value::Null => Ok(None),
        Value::String(cursor) if !cursor.is_empty() && cursor.len() <= 4096 => {
            anyhow::ensure!(cursors.insert(cursor.clone()), "原生置顶分页游标重复");
            Ok(Some(cursor.clone()))
        }
        _ => Err(error(503, "原生置顶分页游标无效")),
    }
}

impl Harness {
    pub(super) async fn native_pinned_section_available(&self) -> anyhow::Result<bool> {
        let generation = self.runtime.connection().await?.generation.clone();
        let mut capability = self.pin_capability.lock().await;
        if let Some((known_generation, available)) = &*capability
            && known_generation == &generation
        {
            return Ok(*available);
        }
        // 能力只在同一引擎进程内复用，包含明确“不支持”的结果；重连换代重新探测，
        // 避免预热后首次进入仍多发探测请求。权限或临时读取错误不进入能力缓存。
        let mut cursor = None;
        let mut cursors = HashSet::new();
        loop {
            let page = match self
                .runtime
                .read(
                    "threadSection/list",
                    json!({"cursor":cursor,"limit":PAGE_LIMIT}),
                )
                .await
            {
                Ok(page) => page,
                Err(error)
                    if cursor.is_none() && error.downcast_ref::<UnsupportedMethod>().is_some() =>
                {
                    *capability = Some((generation, false));
                    return Ok(false);
                }
                Err(error) => return Err(error),
            };
            let sections = page["data"].as_array().context("原生会话分区响应无效")?;
            if sections
                .iter()
                .any(|section| section["id"] == PINNED_SECTION_ID)
            {
                *capability = Some((generation, true));
                return Ok(true);
            }
            cursor = next_cursor(&page, &mut cursors)?;
            if cursor.is_none() {
                return Err(error(503, "执行引擎缺少原生置顶分区"));
            }
        }
    }

    pub(super) async fn native_pinned_threads(&self) -> anyhow::Result<Option<Vec<Value>>> {
        if !self.native_pinned_section_available().await? {
            return Ok(None);
        }
        // 用户要求服务模式显示桌面已有置顶：独立读取全部置顶分页，不能等待最近目录加载到旧会话。
        // 原生顺序由 section_position 决定；支持原生分区时不能并回私有旧置顶，否则取消后会复活。
        let mut rows = Vec::new();
        let mut ids = HashSet::new();
        let mut cursors = HashSet::new();
        let mut cursor = None;
        loop {
            let page = self
                .runtime
                .read(
                    "thread/list",
                    json!({
                        "cursor":cursor,"limit":PAGE_LIMIT,"sectionId":PINNED_SECTION_ID,
                        "sortKey":"section_position","archived":false,"useStateDbOnly":true
                    }),
                )
                .await?;
            for row in page["data"].as_array().context("原生置顶列表响应无效")? {
                let id = row["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .context("原生置顶会话缺少 ID")?;
                anyhow::ensure!(ids.insert(id.to_owned()), "原生置顶分页包含重复会话");
                anyhow::ensure!(
                    row["section"]["id"] == PINNED_SECTION_ID,
                    "原生置顶列表混入其它分区会话"
                );
                // 与目录相同，只展示可持久化的主会话，保持服务模式的侧边会话与子智能体边界。
                if normalize_thread(row).is_ok() {
                    rows.push(row.clone());
                }
            }
            cursor = next_cursor(&page, &mut cursors)?;
            if cursor.is_none() {
                return Ok(Some(rows));
            }
        }
    }

    pub(super) async fn pin_native_thread(
        &self,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Option<Value>> {
        if !self.native_pinned_section_available().await? {
            return Ok(None);
        }
        authorization.check().await?;
        let id = required(payload, "threadId")?;
        let pinned = payload["pinned"]
            .as_bool()
            .ok_or_else(|| error(400, "置顶状态无效"))?;
        let identity = self
            .runtime
            .request(
                "thread/read",
                json!({"threadId":id,"includeTurns":false}),
                None,
            )
            .await?;
        normalize_thread_response(&identity)?;
        let already_pinned = identity["thread"]["section"]["id"] == PINNED_SECTION_ID;
        if already_pinned != pinned {
            // 用户要求两种模式操作同一份置顶；只经 stdio 写原生分区，不恢复会话，也不调用 CDP。
            // 取消非置顶会话不执行移动，避免误清除它所在的自定义分区；重复置顶不改变原生顺序。
            self.runtime.request("thread/section/move", json!({
                "threadId":id,"sectionId":if pinned { json!(PINNED_SECTION_ID) } else { Value::Null },
                "beforeThreadId":null
            }), Some(authorization)).await?;
            self.runtime.invalidate_read("thread/list").await;
            let unconfirmed = || ApiError {
                code: 503,
                message: "原生置顶操作已派发，状态尚未确认，请刷新后核对".into(),
                outcome_unknown: true,
            };
            let current = self
                .runtime
                .request(
                    "thread/read",
                    json!({"threadId":id,"includeTurns":false}),
                    None,
                )
                .await
                .map_err(|_| unconfirmed())?;
            if normalize_thread_response(&current).is_err()
                || (current["thread"]["section"]["id"] == PINNED_SECTION_ID) != pinned
            {
                return Err(unconfirmed().into());
            }
        }
        self.projection
            .lock()
            .await
            .push(json!({"method":"remote/sidebarChanged"}));
        Ok(Some(json!({"ok":true})))
    }
}
