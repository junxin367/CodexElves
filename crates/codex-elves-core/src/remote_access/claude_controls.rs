//! 同一原生 Claude 会话的目标与压缩控制；命令受理与原生完成分开报告。
use super::*;

pub(super) fn compaction_item(event: &Value) -> Option<Value> {
    let id = event["id"].as_str().filter(|id| !id.is_empty())?;
    let status = event["status"].as_str()?;
    if !matches!(
        status,
        "inProgress" | "unknown" | "completed" | "failed" | "interrupted"
    ) {
        return None;
    }
    Some(
        json!({"id":id,"type":"contextCompaction","backend":"claude","status":status,
        "completed":matches!(status,"completed"|"failed"|"interrupted"),
        "source":if matches!(event["source"].as_str(),Some("auto"|"automatic")) {"automatic"} else {"manual"},
        "nativeId":event["nativeId"],"phase":event["phase"],"outcome":event["outcome"],
        "startedAtMs":event["startedAtMs"],"completedAtMs":event["completedAtMs"],
        "preTokens":event["preTokens"],"postTokens":event["postTokens"],"durationMs":event["durationMs"],
        "remoteInlineEvent":true,"remoteCompactionLifecycle":true}),
    )
}

pub(super) fn project_compactions(
    events: &[Value],
    turns: &mut Vec<Value>,
    positions: &HashMap<String, (usize, String)>,
) {
    // 同一原生压缩从开始到结束复用条目 ID；等待受理不进入正文，中断/失败在重连后仍保留。
    let present: std::collections::HashSet<String> = turns
        .iter()
        .flat_map(|turn| turn["items"].as_array().into_iter().flatten())
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    let mut standalone = vec![];
    for event in events.iter().rev() {
        let Some(item) = compaction_item(event) else {
            continue;
        };
        if present.contains(item["id"].as_str().unwrap()) {
            continue;
        }
        if let Some((index, after)) = event["afterUuid"].as_str().and_then(|id| positions.get(id)) {
            let items = turns[*index]["items"].as_array_mut().unwrap();
            let position = items
                .iter()
                .position(|item| item["id"] == after.as_str())
                .map_or(items.len(), |position| position + 1);
            items.insert(position, item);
        } else if event["afterUuid"].is_null() {
            // 无可见消息时，原生已确认的活动单独成行，不伪造用户消息。
            standalone.push(
                json!({"id":format!("compact-{}",item["id"].as_str().unwrap()),
                "_nativeUserId":item["id"],"_nativeDone":true,"status":"completed",
                "turnStartedAtMs":event["startedAtMs"],"itemsComplete":true,"items":[item]}),
            );
        }
    }
    for turn in standalone.into_iter().rev() {
        let at = turn["turnStartedAtMs"].as_u64().unwrap_or(0);
        let position = turns
            .iter()
            .position(|existing| {
                existing["turnStartedAtMs"]
                    .as_u64()
                    .is_some_and(|time| time > at)
            })
            .unwrap_or(turns.len());
        turns.insert(position, turn);
    }
}

pub(super) fn goal_argument(payload: &Value) -> anyhow::Result<&str> {
    let text = payload["prompt"]
        .as_str()
        .or_else(|| payload["objective"].as_str())
        .unwrap_or("")
        .trim();
    // CLI 原生目标上限为 4000 个 UTF-16 单元；保留其控制词，避免目标被误当成清除命令。
    if text.is_empty()
        || text.encode_utf16().count() > 4000
        || ["clear", "stop", "off", "reset", "none", "cancel"]
            .contains(&text.to_ascii_lowercase().as_str())
        || payload["images"].as_array().is_some_and(|v| !v.is_empty())
        || payload["skills"].as_array().is_some_and(|v| !v.is_empty())
    {
        return Err(error(
            400,
            "Claude 原生目标需要 1–4000 个字符，暂不支持附件、Skill 路径或保留控制词",
        ));
    }
    Ok(text)
}

impl Claude {
    pub(super) async fn dispatch_native_command(
        &self,
        c: &Conversation,
        process: &Arc<Process>,
        name: &str,
        argument: &str,
        expected_goal: Option<&Value>,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        if !c.shared
            || !process.initialization["commands"]
                .as_array()
                .is_some_and(|commands| commands.iter().any(|command| command["name"] == name))
        {
            return Err(error(409, "当前 Claude CLI 未提供此原生命令"));
        }
        let result = process
            .control(
                json!({"subtype":"shared_native_command",
            "id":uuid::Uuid::new_v4().to_string(),"name":name,"argument":argument,"expectedGoal":expected_goal}),
                Some(authorization),
            )
            .await?;
        // 只确认命令已交给原生；网页等 goal_status / compact_boundary 才显示实际结果。
        let current = self.conversation(&c.id).await?;
        Ok(
            json!({"ok":true,"backend":"claude","threadId":c.id,"status":"created",
            "nativeCommand":result["nativeCommand"],"goal":current.native_control["goal"],
            "compaction":current.native_control["compaction"]}),
        )
    }

    /// 等本次压缩前触发的设置切换（模型或思考深度）有结果；未生效则返回错误，调用方不再发送 /compact。
    async fn wait_for_setting_switch(
        self: &Arc<Self>,
        id: &str,
        name: &str,
        before: &str,
    ) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let current = self.conversation(id).await?;
            match setting_switch_state(before, &current.native_control["settings"]["changes"][name])
            {
                SettingSwitch::Done => return Ok(()),
                SettingSwitch::NeedsConfirmation => {
                    return Err(error(
                        409,
                        "原版 Claude 正在等待确认模型或思考深度切换，压缩尚未发出。请先在会话中确认，确认后再点击压缩。",
                    ));
                }
                SettingSwitch::Rejected => {
                    return Err(error(
                        409,
                        "模型或思考深度切换已被拒绝，压缩尚未发出。请重新选择后再压缩，或取消选择按当前设置压缩。",
                    ));
                }
                SettingSwitch::Waiting => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(error(
                    409,
                    "模型或思考深度切换尚未得到原版 Claude 确认，压缩尚未发出。请稍后重试。",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    pub(super) async fn native_command_action(
        self: &Arc<Self>,
        c: &mut Conversation,
        action: &str,
        payload: &Value,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        if let Some(mode) = payload["permissionMode"].as_str() {
            c.permission_mode = Some(mode.to_owned());
        }
        let (name, argument) = match action {
            "compact" => ("compact", ""),
            "setGoal" => ("goal", goal_argument(payload)?),
            "updateGoal" => {
                let goal = &c.native_control["goal"];
                if goal["objective"] != payload["expectedObjective"]
                    || goal["createdAt"] != payload["expectedCreatedAt"]
                    || goal["status"] != payload["expectedStatus"]
                    || !matches!(goal["status"].as_str(), Some("active" | "blocked"))
                {
                    return Err(error(409, "原生目标已变化，请重新读取后操作"));
                }
                match payload["operation"].as_str().unwrap_or("edit") {
                    "edit" => ("goal", goal_argument(payload)?),
                    "delete" => ("goal", "clear"),
                    _ => return Err(error(400, "Claude 原生目标暂不提供暂停或恢复操作")),
                }
            }
            _ => return Err(error(400, "不支持的 Claude 原生命令")),
        };
        let process = self
            .processes
            .lock()
            .await
            .get(&c.id)
            .filter(|p| p.channel.live.load(Ordering::Acquire))
            .cloned();
        let process = match process {
            Some(process) => process,
            None => {
                let process = self.spawn(c).await?;
                self.processes
                    .lock()
                    .await
                    .insert(c.id.clone(), process.clone());
                process
            }
        };
        // 用户要求压缩与发送消息、/goal 一样触发所选模型切换：网页菜单里的模型只是本地选择，
        // 不在此先应用，/compact 会沿用 CLI 当前模型而界面仍显示新选择。
        // 只有请求明确带了模型或思考深度才应用，普通压缩不依赖模型目录；应用失败（模型不在列表、
        // CLI 不接受等）直接返回错误，不再发 /compact，避免用旧模型静默压缩。
        let switch_names = compact_setting_names(action, payload);
        let switch_before = if switch_names.is_empty() {
            vec![]
        } else {
            let current = self.conversation(&c.id).await?;
            switch_names
                .iter()
                .map(|name| {
                    setting_change_id(&current.native_control["settings"]["changes"][*name])
                })
                .collect()
        };
        if action == "setGoal" || compact_requests_model_choice(action, payload) {
            if action == "setGoal" {
                apply_permissions(&process, c, payload, authorization).await?;
            }
            apply_model(&process, c, payload, authorization).await?;
            self.change(|saved| {
                let current = saved
                    .conversations
                    .get_mut(&c.id)
                    .context("Claude 会话不存在")?;
                current.model = c.model.clone();
                current.effort = c.effort.clone();
                current.permission_mode = c.permission_mode.clone();
                Ok(())
            })
            .await?;
        }
        // 实测 CLI 2.1.325：已有对话时 /model 会弹出“Switch model?”确认框，排在后面的 /compact 会一直排队；
        // 选 No 后 /compact 仍会按旧模型执行。所以必须等切换有结果：生效才压缩，待确认、被拒绝或超时都不发 /compact，
        // 让用户先处理确认框，避免静默按旧模型压缩。
        for (name, before) in switch_names.iter().zip(&switch_before) {
            self.wait_for_setting_switch(&c.id, name, before).await?;
        }
        let expected_goal = (action == "updateGoal").then_some(&c.native_control["goal"]);
        self.dispatch_native_command(c, &process, name, argument, expected_goal, authorization)
            .await
    }
}

/// 压缩请求是否明确带了模型或思考深度选择，带了才需要先于 /compact 应用；
/// 空字符串模型不算选择，思考深度的显式空字符串表示恢复默认，仍算选择（与发送消息一致）。
fn compact_requests_model_choice(action: &str, payload: &Value) -> bool {
    action == "compact"
        && (payload["model"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
            || payload["reasoningEffort"].is_string())
}

/// 压缩请求里需要等待结果的设置名：明确带了模型才等模型，明确带了思考深度（含显式空值）才等思考深度。
fn compact_setting_names(action: &str, payload: &Value) -> Vec<&'static str> {
    if action != "compact" {
        return vec![];
    }
    let mut names = vec![];
    if payload["model"]
        .as_str()
        .is_some_and(|value| !value.is_empty())
    {
        names.push("model");
    }
    if payload["reasoningEffort"].is_string() {
        names.push("effort");
    }
    names
}

fn setting_change_id(change: &Value) -> String {
    change["id"].as_str().unwrap_or_default().to_owned()
}

#[derive(Debug, PartialEq)]
enum SettingSwitch {
    /// 已生效，或这次请求没有产生新的切换（选择与当前设置相同）。
    Done,
    /// 命令已交给原生，尚未看到结果。
    Waiting,
    /// 原生弹出确认框，等用户选择。
    NeedsConfirmation,
    /// 用户拒绝了本次切换。
    Rejected,
}

/// 判断本次请求触发的设置切换进展。`before` 是请求前该设置最近一次变更的 ID：
/// 同一 ID 且已结束的记录是更早的结果，不能当作本次切换的结论；每次新切换都会生成新 ID。
fn setting_switch_state(before: &str, change: &Value) -> SettingSwitch {
    let fresh = change["id"].as_str().is_some_and(|id| id != before);
    match change["state"].as_str() {
        Some("pending") => SettingSwitch::Waiting,
        Some("confirmation" | "confirming") => SettingSwitch::NeedsConfirmation,
        Some("rejected") if fresh => SettingSwitch::Rejected,
        _ => SettingSwitch::Done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_applies_the_selected_model_only_when_the_request_chooses_one() {
        assert!(compact_requests_model_choice(
            "compact",
            &json!({"threadId":"t","model":"claude-haiku-5-5"})
        ));
        assert!(compact_requests_model_choice(
            "compact",
            &json!({"threadId":"t","reasoningEffort":"high"})
        ));
        assert!(compact_requests_model_choice(
            "compact",
            &json!({"threadId":"t","reasoningEffort":""})
        ));
        // 普通压缩不带选择：不依赖模型目录，也不发多余的 /model。
        assert!(!compact_requests_model_choice(
            "compact",
            &json!({"threadId":"t"})
        ));
        assert!(!compact_requests_model_choice(
            "compact",
            &json!({"threadId":"t","model":""})
        ));
        // 其他命令沿用各自既有的处理，不由这个判断接管。
        assert!(!compact_requests_model_choice(
            "setGoal",
            &json!({"threadId":"t","model":"claude-haiku-5-5"})
        ));
        assert_eq!(
            compact_setting_names(
                "compact",
                &json!({"model":"claude-haiku-5-5","reasoningEffort":"high"})
            ),
            ["model", "effort"]
        );
        assert_eq!(
            compact_setting_names("compact", &json!({"reasoningEffort":""})),
            ["effort"]
        );
        assert!(compact_setting_names("compact", &json!({"threadId":"t"})).is_empty());
        assert!(compact_setting_names("setGoal", &json!({"model":"m"})).is_empty());
    }

    #[test]
    fn compaction_waits_for_the_setting_switch_it_triggered() {
        let change = |id: &str, state: &str| json!({"id":id,"state":state});
        // 命令已交给原生但没有结果：继续等。
        assert_eq!(
            setting_switch_state("old", &change("new", "pending")),
            SettingSwitch::Waiting
        );
        // 原生弹出“Switch model?”确认框：不发 /compact，等用户处理。
        for state in ["confirmation", "confirming"] {
            assert_eq!(
                setting_switch_state("old", &change("new", state)),
                SettingSwitch::NeedsConfirmation
            );
        }
        // 本次切换生效 / 被拒绝。
        assert_eq!(
            setting_switch_state("old", &change("new", "applied")),
            SettingSwitch::Done
        );
        assert_eq!(
            setting_switch_state("old", &change("new", "rejected")),
            SettingSwitch::Rejected
        );
        // 更早的拒绝记录（同一 ID）不能阻断没有产生新切换的压缩。
        assert_eq!(
            setting_switch_state("old", &change("old", "rejected")),
            SettingSwitch::Done
        );
        assert_eq!(
            setting_switch_state("old", &change("old", "applied")),
            SettingSwitch::Done
        );
        // 选择与当前设置相同、没有任何切换记录。
        assert_eq!(setting_switch_state("", &Value::Null), SettingSwitch::Done);
        // 重复点击时同一条切换仍在等待确认，不会因为 ID 相同而被当成旧记录。
        assert_eq!(
            setting_switch_state("same", &change("same", "confirmation")),
            SettingSwitch::NeedsConfirmation
        );
    }
}
