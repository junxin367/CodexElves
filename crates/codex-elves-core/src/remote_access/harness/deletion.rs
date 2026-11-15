//! 永久删除以原生回执为准；派发意图跨窗口、设备和服务重启保留，未知结果不重放。
use serde_json::{Value, json};

use super::store::{Data, Deletion};
use super::{ApiError, Harness, error};
use crate::remote_access::{Receipt, RemoteAuthorization};

impl Harness {
    pub(in crate::remote_access) async fn protected_delete_receipts(
        &self,
    ) -> std::collections::HashSet<String> {
        self.store
            .lock()
            .await
            .data
            .deletions
            .values()
            .filter(|deletion| deletion.state != "deleted" || deletion.cleanup_pending)
            .filter_map(|deletion| {
                deletion
                    .request_id
                    .as_ref()
                    .map(|request| format!("{}:{request}", deletion.device_id))
            })
            .collect()
    }

    pub(in crate::remote_access) async fn check_deletion_barrier(
        &self,
        id: &str,
    ) -> anyhow::Result<()> {
        if self.projection.lock().await.is_deleted(id) {
            return Err(error(410, "会话已永久删除，当前操作未执行。"));
        }
        if let Some(deletion) = self.store.lock().await.data.deletions.get(id) {
            return Err(error(
                409,
                if deletion.state == "deleted" {
                    "会话已永久删除，当前操作未执行。"
                } else {
                    "此会话有尚未确认的删除，请核对原删除回执；当前操作未执行，不会重复删除。"
                },
            ));
        }
        Ok(())
    }

    pub(super) async fn delete_thread(
        &self,
        id: &str,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        if self.projection.lock().await.is_deleted(id) {
            return Ok(self.confirm_thread_deleted(id).await);
        }
        let previous = self.store.lock().await.data.deletions.get(id).cloned();
        if let Some(previous) = previous {
            if previous.state == "deleted" {
                return Ok(self.confirm_thread_deleted(id).await);
            }
            // 清理文件失败后，独立的成功回执可能已落盘；重启后按原设备、请求和摘要恢复，不能再删一次。
            if let Some(controller) = self.controller.get().and_then(std::sync::Weak::upgrade)
                && let Some(request_id) = previous.request_id.as_ref()
            {
                let confirmed = controller
                    .auth
                    .lock()
                    .await
                    .receipts
                    .get(&format!("{}:{request_id}", previous.device_id))
                    .is_some_and(|receipt| receipt_confirms_deletion(receipt, id));
                if confirmed {
                    return Ok(self.confirm_thread_deleted(id).await);
                }
            }
            self.check_deletion_barrier(id).await?;
        }
        // 删除与发送/队列已共用 thread_lock；删除前只读校验，不 resume、不取消任务或抢占其他 writer。
        let identity = self
            .runtime
            .request(
                "thread/read",
                json!({"threadId":id,"includeTurns":false}),
                None,
            )
            .await
            .map_err(delete_preflight_failure)?;
        crate::codex_service::normalize_thread_response(&identity)
            .map_err(delete_preflight_failure)?;
        let status = &identity["thread"]["status"];
        if matches!(
            status.as_str().or_else(|| status["type"].as_str()),
            Some("active" | "inProgress" | "running" | "waiting" | "unknown")
        ) {
            return Err(error(
                409,
                "会话仍在执行或执行状态未确认，本次未删除。请先停止任务并确认结束。",
            ));
        }
        {
            let projection = self.projection.lock().await;
            if projection.active.contains_key(id)
                || projection.compactions.contains_key(id)
                || !projection.requests_for(id).is_empty()
            {
                return Err(error(
                    409,
                    "会话仍有执行、压缩或待处理交互，本次未删除。请先处理并确认结束。",
                ));
            }
            if projection
                .sides
                .threads
                .values()
                .any(|side| side.parent == id && !side.closed && !side.expired)
            {
                return Err(error(
                    409,
                    "会话仍有打开的侧边会话，本次未删除。请先关闭侧边会话。",
                ));
            }
        }
        {
            let mut store = self.store.lock().await;
            if let Some(deletion) = store.data.deletions.get(id) {
                if deletion.state == "deleted" {
                    return Ok(deletion_result(id, deletion.cleanup_pending));
                }
                return Err(error(
                    409,
                    "此会话有尚未确认的删除，本次未重复删除，请核对原回执。",
                ));
            }
            if store.data.queues.get(id).is_some_and(|queue| {
                queue.iter().any(|message| {
                    matches!(
                        message.state.as_str(),
                        "sending" | "unknown" | "direct-sending" | "direct-unknown"
                    )
                })
            }) || store.data.edits.values().any(|edit| {
                edit.thread_id == id && !matches!(edit.state.as_str(), "committed" | "canceled")
            }) {
                return Err(error(
                    409,
                    "会话仍有待核对的发送或编辑，本次未删除。请先核对原操作。",
                ));
            }
            // 必须先落盘再发出唯一一次原生删除；磁盘失败属于确定未删除，不能降级成无保护派发。
            store
                .commit(|data| {
                    data.deletions.insert(
                        id.to_owned(),
                        Deletion {
                            state: "pending".into(),
                            device_id: authorization.device.id.clone(),
                            request_id: authorization.request_id.clone(),
                            cleanup_pending: false,
                        },
                    );
                    Ok(())
                })
                .map_err(|_| {
                    error(
                        503,
                        "无法保存删除保护记录，本次未删除。请检查本机存储后重试。",
                    )
                })?;
        }
        let response = self
            .runtime
            .request("thread/delete", json!({"threadId":id}), Some(authorization))
            .await;
        let failure = match response {
            // 当前原生协议以空对象确认完成；未知结构不能被误认成完整删除成功。
            Ok(response) if response.as_object().is_some_and(serde_json::Map::is_empty) => {
                return Ok(self.confirm_thread_deleted(id).await);
            }
            Ok(_) => ApiError {
                code: 503,
                message: "原生删除回执格式无效".into(),
                outcome_unknown: true,
            }
            .into(),
            Err(failure) => delete_failure(id, failure),
        };
        // 原生删除通知可能早于 RPC 错误：根会话已删除的事实不回退，但不承诺全部后代也删除成功。
        if self.projection.lock().await.is_deleted(id) {
            let mut result = self.confirm_thread_deleted(id).await;
            result["status"] = json!("partial");
            result["message"] =
                json!("本会话已永久删除，子会话或列表清理结果尚未全部确认，请在原生客户端核对。");
            return Ok(result);
        }
        let rejected = failure
            .downcast_ref::<ApiError>()
            .is_some_and(|failure| !failure.outcome_unknown);
        let mut store = self.store.lock().await;
        if let Some(deletion) = store
            .data
            .deletions
            .get(id)
            .filter(|deletion| deletion.state == "deleted")
        {
            return Ok(deletion_result(id, deletion.cleanup_pending));
        }
        let saved = store.commit(|data| {
            if rejected {
                data.deletions.remove(id);
            } else if let Some(deletion) = data.deletions.get_mut(id) {
                deletion.state = "unknown".into();
            }
            Ok(())
        });
        if saved.is_err() {
            // 原意图已经落盘，更新失败仍保留保护；不能因内存放行使重启前后允许的操作不同。
            if let Some(deletion) = store.data.deletions.get_mut(id) {
                deletion.state = "unknown".into();
            }
            return Err(ApiError {
                code: 503,
                message: format!("删除状态保存失败，原回执保护已保留：{failure}"),
                outcome_unknown: true,
            }
            .into());
        }
        Err(failure)
    }

    pub(super) async fn confirm_thread_deleted(&self, id: &str) -> Value {
        // 先登记删除事实，阻止迟到状态恢复已删除会话；不持有 thread_lock，避免事件接收与 RPC 互相等待。
        self.projection.lock().await.forget_deleted_thread(id);
        self.loaded.lock().await.remove(id);
        self.queue_recovery_after.lock().await.remove(id);
        self.runtime.invalidate_read(id).await;
        self.runtime.invalidate_read("thread/list").await;
        self.runtime.invalidate_read("thread/loaded/list").await;
        let event = json!({"method":"thread/deleted","params":{"threadId":id}});
        self.catalog.observe(&event).await;
        let mut store = self.store.lock().await;
        let cleanup_pending = store
            .commit(|data| {
                data.forget_deleted_thread(id);
                Ok(())
            })
            .is_err();
        if cleanup_pending {
            // 原生已确认删除不能因本地文件失败降级为未知；内存仍清理并保留 tombstone，响应明确提示部分清理失败。
            store.data.forget_deleted_thread(id);
            store.data.deletions.get_mut(id).unwrap().cleanup_pending = true;
            let _ = crate::diagnostic_log::append_diagnostic_log(
                "remote_access.delete_cleanup_failed",
                json!({"threadId":id}),
            );
        }
        drop(store);
        self.projection.lock().await.push(event);
        deletion_result(id, cleanup_pending)
    }

    pub async fn recover_delete_receipt(&self, receipt: &Receipt, value: Value) -> Value {
        // 只用同一会话、设备、原请求和删除摘要对应的原生确认恢复回执；会话读不到不能证明删除成功。
        if !matches!(receipt.state.as_str(), "pending" | "unknown" | "completed") {
            return value;
        }
        let store = self.store.lock().await;
        for (id, deletion) in &store.data.deletions {
            if deletion.device_id == receipt.device_id
                && deletion.request_id.as_deref() == Some(&receipt.request_id)
                && receipt.payload_hash
                    == super::super::hash(&json!(["deleteThread", {"threadId":id}]).to_string())
            {
                if receipt_confirms_deletion(receipt, id)
                    && (deletion.state != "deleted" || deletion.cleanup_pending)
                {
                    let id = id.clone();
                    drop(store);
                    let lock = self.thread_lock(&id).await;
                    let _serial = lock.lock().await;
                    let result = self.confirm_thread_deleted(&id).await;
                    return json!({"requestId":receipt.request_id,"state":"completed","result":result});
                }
                if deletion.state == "deleted" && receipt.state != "completed" {
                    return json!({"requestId":receipt.request_id,"state":"completed",
                        "result":deletion_result(id, deletion.cleanup_pending)});
                }
                break;
            }
        }
        value
    }
}

fn receipt_confirms_deletion(receipt: &Receipt, id: &str) -> bool {
    let value = super::super::receipt_value(receipt);
    let result = &value["result"];
    receipt.state == "completed"
        && receipt.payload_hash
            == super::super::hash(&json!(["deleteThread", {"threadId":id}]).to_string())
        && result["ok"] == true
        && result["deleted"] == true
        && result["threadId"] == id
        && result["outcomeUnknown"] != true
}

impl Data {
    fn forget_deleted_thread(&mut self, id: &str) {
        self.queues.remove(id);
        self.edits.retain(|_, edit| edit.thread_id != id);
        self.thread_pins.remove(id);
        self.settings.remove(id);
        self.thread_devices.remove(id);
        self.queue_gates.remove(id);
        let prefix = format!("{id}:");
        self.sent.retain(|key, _| !key.starts_with(&prefix));
        self.sent_hashes.retain(|key, _| !key.starts_with(&prefix));
        self.answered_questions
            .retain(|key, _| !key.starts_with(&prefix));
        let deletion = self.deletions.entry(id.to_owned()).or_insert(Deletion {
            state: "deleted".into(),
            device_id: String::new(),
            request_id: None,
            cleanup_pending: false,
        });
        deletion.state = "deleted".into();
        deletion.cleanup_pending = false;
    }
}

fn deletion_result(id: &str, cleanup_pending: bool) -> Value {
    let mut result = json!({"ok":true,"threadId":id,"deleted":true,
        "status":if cleanup_pending {"partial"} else {"local_deleted"}});
    if cleanup_pending {
        result["message"] =
            json!("会话已永久删除，部分本地状态清理未保存。请检查本机存储；无需再次删除。");
    }
    result
}

pub(super) fn delete_preflight_failure(failure: anyhow::Error) -> anyhow::Error {
    let code = failure
        .downcast_ref::<ApiError>()
        .map_or(503, |failure| failure.code);
    error(code, &format!("删除前校验失败，本次未删除：{failure}"))
}

fn delete_failure(id: &str, failure: anyhow::Error) -> anyhow::Error {
    if failure
        .downcast_ref::<super::runtime::UnsupportedMethod>()
        .is_some()
    {
        return error(
            409,
            "当前 Codex 引擎不支持永久删除，本次未删除。请更新 Codex 后重试。",
        );
    }
    if let Some(api) = failure.downcast_ref::<ApiError>() {
        if !api.outcome_unknown
            && api.code == 409
            && api.message == format!("Codex 操作失败：thread {id} already has an active writer")
        {
            // 用户要求占用提示简洁；仅映射目标会话已确认的写入锁拒绝，不扩展到后代删除或未知结果。
            return error(409, "该会话由其他 Codex 持有，当前无法删除。");
        }
        if matches!(api.code, 401 | 403) && !api.outcome_unknown {
            return failure;
        }
    }
    // 后代删除或引擎内部错误可能发生于部分写入之后，不能将所有 -32600 当作完整删除前拒绝。
    ApiError {
        code: 503,
        message: failure.to_string(),
        outcome_unknown: true,
    }
    .into()
}
