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
        if action == "setGoal" {
            apply_permissions(&process, c, payload, authorization).await?;
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
        let expected_goal = (action == "updateGoal").then_some(&c.native_control["goal"]);
        self.dispatch_native_command(c, &process, name, argument, expected_goal, authorization)
            .await
    }
}
