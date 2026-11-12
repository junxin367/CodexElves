//! Claude 子代理只读投影：身份绑定主会话和 WSL，读取不启动、恢复或控制原生子代理。
use super::{Claude, error, now_ms, project_shared_history};
use anyhow::Context;
use serde_json::{Value, json};

// ID 最长 13（前缀）+36（主会话 UUID）+1（分隔符）+64（原生 ID）=114 字符，
// 保持在网页关联会话的 120 字符限制内；修改时须同步核对 app.js 的 claudeThreadId 和 Python valid_id。
const PREFIX: &str = "claude-agent-";

pub(super) fn valid_native_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub(super) fn parse_id(id: &str) -> Option<(String, &str)> {
    let value = id.strip_prefix(PREFIX)?;
    let (parent, agent) = value.split_at_checked(36)?;
    let agent = agent.strip_prefix('-')?;
    (uuid::Uuid::parse_str(parent).is_ok() && valid_native_id(agent))
        .then(|| (format!("claude-{parent}"), agent))
}

pub(super) fn id(parent: &str, agent: &str) -> String {
    format!(
        "{PREFIX}{}-{agent}",
        parent.strip_prefix("claude-").unwrap_or(parent)
    )
}

pub(super) fn decorate_turns(parent: &str, turns: &mut [Value]) {
    for turn in turns {
        for item in turn["items"].as_array_mut().into_iter().flatten() {
            if let Some(agent) = item["_claudeAgentId"]
                .as_str()
                .filter(|id| valid_native_id(id))
            {
                item["agentThreadId"] = json!(id(parent, agent));
            }
        }
    }
}

// 接收/完成证据均来自原生 Agent 回执或任务通知；async_launched 只代表已受理，不能误标为已完成。
pub(super) fn observe_result(turn: &mut Value, frame: &Value) {
    let result = &frame["nativeAgentResult"];
    let Some(items) = turn["items"].as_array_mut() else {
        return;
    };
    if let Some(agent) = result["agentId"].as_str().filter(|id| valid_native_id(id)) {
        for block in frame["message"]["content"].as_array().into_iter().flatten() {
            if block["type"] != "tool_result" {
                continue;
            }
            if let Some(item) = items.iter_mut().find(|item| {
                item["id"] == block["tool_use_id"]
                    && (item["type"] == "collabAgentToolCall"
                        || result["resumed"] == true
                            && item["type"] == "dynamicToolCall"
                            && item["tool"] == "SendMessage")
            }) {
                if result["resumed"] == true {
                    item["type"] = json!("collabAgentToolCall");
                    item["tool"] = json!("send_message");
                }
                item["_claudeAgentId"] = json!(agent);
                item["model"] = result["resolvedModel"].clone();
                let finished = result["status"] == "completed";
                item["status"] = json!(if finished { "completed" } else { "inProgress" });
                item["displayStatus"] = json!(if finished { "completed" } else { "active" });
            }
        }
    }
}

pub(super) fn observe_notification(turns: &mut [Value], frame: &Value) -> bool {
    let notification = &frame["nativeAgentNotification"];
    let Some(agent) = notification["agentId"]
        .as_str()
        .filter(|id| valid_native_id(id))
    else {
        return false;
    };
    let status = match notification["status"].as_str() {
        Some("completed") => "completed",
        Some("failed") => "failed",
        Some("stopped") => "interrupted",
        _ => return false,
    };
    for turn in turns {
        for item in turn["items"].as_array_mut().into_iter().flatten() {
            if item["_claudeAgentId"] == agent {
                item["status"] = json!(status);
                item["displayStatus"] = json!(status);
                item["completedAtMs"] = notification["completedAtMs"].clone();
            }
        }
    }
    true
}

impl Claude {
    async fn read_native_subagents(
        self: &std::sync::Arc<Self>,
        parent: &str,
        agent: Option<&str>,
        messages: bool,
    ) -> anyhow::Result<(super::Conversation, Value)> {
        let conversation = self.conversation(parent).await?;
        let process = self.native_reader(&conversation.config).await?;
        let result = process
            .control(
                json!({
                    "subtype":if agent.is_some() { "read_subagent" } else { "read_subagents" },
                    "native_id":conversation.native_id,"agent_id":agent,"include_messages":messages
                }),
                None,
            )
            .await?;
        Ok((conversation, result))
    }

    pub(in crate::remote_access) async fn related_threads(
        self: &std::sync::Arc<Self>,
        parent: &str,
    ) -> anyhow::Result<Value> {
        if parse_id(parent).is_some() {
            // 当前子代理详情沿用只读子视图；根列表统一收录主会话下的原生子代理，不开放侧边会话写入口。
            return Ok(
                json!({"ok":true,"threadId":parent,"subagents":[],"completedSubagents":[],
                "completedSubagentCount":0,"subagentsComplete":true,"sideThreads":[],"canCreateSideThread":false}),
            );
        }
        let (_, result) = self.read_native_subagents(parent, None, false).await?;
        let mut active = Vec::new();
        let mut completed = Vec::new();
        for raw in result["agents"]
            .as_array()
            .context("Claude 子代理列表格式无效")?
        {
            let agent = raw["nativeId"]
                .as_str()
                .filter(|id| valid_native_id(id))
                .context("Claude 原生子代理身份无效")?;
            let mut item = raw.clone();
            item["id"] = json!(id(parent, agent));
            item["parentThreadId"] = json!(parent);
            item["ephemeral"] = json!(false);
            if let Some(duration) = raw["totalDurationMs"].as_u64() {
                item["elapsedMs"] = json!(duration);
            } else if let Some(started) = raw["startedAtMs"].as_u64() {
                let end = raw["completedAtMs"].as_u64().unwrap_or_else(now_ms);
                item["elapsedMs"] = json!(end.saturating_sub(started));
            }
            if raw["outcome"].is_string() {
                completed.push(item);
            } else {
                active.push(item);
            }
        }
        Ok(json!({"ok":true,"threadId":parent,"subagents":active,
            "completedSubagentCount":completed.len(),"completedSubagents":completed,
            "subagentsComplete":true,"subagentsLoading":false,"sideThreads":[],"canCreateSideThread":false}))
    }

    pub(in crate::remote_access) async fn subagent_detail(
        self: &std::sync::Arc<Self>,
        child: &str,
    ) -> anyhow::Result<Value> {
        let (parent, agent) = parse_id(child).ok_or_else(|| error(400, "Claude 子代理身份无效"))?;
        let (conversation, result) = self
            .read_native_subagents(&parent, Some(agent), false)
            .await?;
        let agent = &result["agent"];
        anyhow::ensure!(
            agent["nativeId"].as_str().is_some_and(valid_native_id),
            "Claude 子代理详情格式无效"
        );
        let mut view = conversation.view();
        for (key, value) in json!({
            "id":child,"nativeId":agent["nativeId"],"nativeParentId":conversation.native_id,
            "parentThreadId":parent,"viewKind":"subagent","readOnly":true,"agentName":agent["name"],
            "title":agent["name"],"model":agent["model"],"reasoningEffort":agent["reasoningEffort"],
            "status":{"type":if agent["status"] == "active" { "active" } else { "idle" }},
            "createdAt":agent["startedAtMs"],"updatedAt":agent["updatedAtMs"],
            "outcome":agent["outcome"],"hasUnreadTurn":false,"pinned":false,"cliCommand":"",
            "goal":null,"compaction":{"state":"idle"},"nativeCommands":[],
            "tokenUsage":null,"editableMessage":null,"queuedMessages":[],"queuedEdits":[],
            "requests":[],"requestsKnown":true,"requestsEpoch":self.epoch,
            "requestsRevision":0,"capabilities":{"subagents":true,"sideThreads":false,
                "queue":false,"messageEdit":false,"goalComposer":false,"compact":false}
        })
        .as_object()
        .unwrap()
        {
            view[key] = value.clone();
        }
        Ok(json!({"ok":true,"thread":view,"nativeBridge":"claude"}))
    }

    pub(in crate::remote_access) async fn subagent_history(
        self: &std::sync::Arc<Self>,
        payload: &Value,
    ) -> anyhow::Result<Value> {
        let child = payload["threadId"]
            .as_str()
            .context("缺少 Claude 子代理 ID")?;
        let (parent, agent) = parse_id(child).ok_or_else(|| error(400, "Claude 子代理身份无效"))?;
        let (_, result) = self
            .read_native_subagents(&parent, Some(agent), true)
            .await?;
        let agent = &result["agent"];
        let mut frame = json!({"messages":agent["messages"],"clients":{},"subagentReadOnly":true,"native_status":{
            "tempo":if agent["status"]=="active" { "active" } else { "idle" },
            "state":agent["outcome"]},"pending":[],"requests":{}});
        if agent["status"] == "unknown" {
            frame["native_status"] = Value::Null;
        }
        let mut turns = project_shared_history(&frame, child);
        if let Some(turn) = turns.last_mut() {
            match agent["outcome"].as_str() {
                Some(outcome @ ("completed" | "failed" | "stopped")) => {
                    turn["status"] = json!(if outcome == "stopped" {
                        "interrupted"
                    } else {
                        outcome
                    });
                    turn["_nativeDone"] = json!(true);
                    if turn["turnCompletedAtMs"].is_null() {
                        turn["turnCompletedAtMs"] = agent["completedAtMs"].clone();
                    }
                }
                _ => {}
            }
        }
        decorate_turns(&parent, &mut turns);
        let end = match &payload["cursor"] {
            Value::Null => turns.len(),
            Value::String(value) => value
                .parse::<usize>()
                .ok()
                .filter(|end| *end <= turns.len())
                .ok_or_else(|| error(400, "Claude 子代理历史游标无效"))?,
            _ => return Err(error(400, "Claude 子代理历史游标无效")),
        };
        let start = end.saturating_sub(10);
        Ok(
            json!({"ok":true,"threadId":child,"data":turns[start..end].iter().rev().collect::<Vec<_>>(),
            "turnIds":turns.iter().map(|turn| &turn["id"]).collect::<Vec<_>>(),
            "nextCursor":if start==0 { Value::Null } else { json!(start.to_string()) },
            "transcriptEvents":[],"backend":"claude"}),
        )
    }

    pub(in crate::remote_access) async fn subagent_models(
        self: &std::sync::Arc<Self>,
        child: &str,
    ) -> anyhow::Result<Value> {
        // 子代理详情为只读；读取模型名复用详情元数据，不为查看记录启动 CLI、Hooks 或 MCP。
        let detail = self.subagent_detail(child).await?;
        let thread = &detail["thread"];
        let data: Vec<_> = thread["model"].as_str().filter(|model| !model.is_empty())
            .map(|model| json!({"id":model,"model":model,"displayName":model,"supportedReasoningEfforts":[]}))
            .into_iter().collect();
        Ok(
            json!({"ok":true,"backend":"claude","readOnly":true,"data":data,
            "defaultModel":thread["model"],"defaultReasoningEffort":thread["reasoningEffort"]}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_ids_bind_the_parent_and_reject_paths_or_unbounded_native_ids() {
        let parent = "claude-00000000-0000-4000-8000-000000000001";
        let child = id(parent, "agent_fixture");
        assert_eq!(parse_id(&child), Some((parent.into(), "agent_fixture")));
        assert!(Claude::owns_id(&child));
        assert_ne!(
            child,
            id(
                "claude-00000000-0000-4000-8000-000000000002",
                "agent_fixture"
            )
        );
        for invalid in ["../other", "", "path/agent", "agent%2fother"] {
            assert!(parse_id(&id(parent, invalid)).is_none());
        }
        assert!(parse_id(&id(parent, &"x".repeat(65))).is_none());
        assert!(parse_id("claude-agent-中文").is_none());
    }

    #[test]
    fn native_agent_acceptance_completion_and_user_quotes_do_not_create_fake_user_turns() {
        let parent = "claude-00000000-0000-4000-8000-000000000001";
        let mut frame = json!({"native_status":{"tempo":"active"},"clients":{},"messages":[
            {"type":"user","uuid":"u","message":{"content":"检查文件"},"timestampMs":1000},
            {"type":"assistant","uuid":"spawn","message":{"id":"m","content":[
                {"type":"tool_use","id":"tool-agent","name":"Agent","input":{"description":"读取文件"}}]}},
            {"type":"user","uuid":"accepted","nativeAgentResult":{"agentId":"native-agent",
                "status":"async_launched","resolvedModel":"test-gpt"},"message":{"content":[
                {"type":"tool_result","tool_use_id":"tool-agent","content":"已启动"}]}}
        ]});
        let mut turns = project_shared_history(&frame, "generation");
        decorate_turns(parent, &mut turns);
        assert_eq!(turns.len(), 1);
        let item = turns[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "tool-agent")
            .unwrap();
        assert_eq!(item["displayStatus"], "active");
        assert_eq!(item["agentThreadId"], id(parent, "native-agent"));
        frame["messages"].as_array_mut().unwrap().push(json!({"type":"user","uuid":"notice",
            "nativeAgentNotification":{"agentId":"native-agent","status":"completed","completedAtMs":2000},
            "message":{"content":"<task-notification>native control</task-notification>"}}));
        let done = project_shared_history(&frame, "generation");
        assert_eq!(done.len(), 1, "原生完成通知不能形成用户消息或新的轮次");
        let item = done[0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "tool-agent")
            .unwrap();
        assert_eq!(item["displayStatus"], "completed");
        frame["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"user","uuid":"quote",
            "message":{"content":"<task-notification>用户引用</task-notification>"}}));
        assert_eq!(
            project_shared_history(&frame, "generation").len(),
            2,
            "真实用户引用必须保留"
        );
    }

    #[test]
    fn subagent_native_end_turn_keeps_resumed_inputs_and_completed_times_separate() {
        let frame = json!({"subagentReadOnly":true,"native_status":{"tempo":"idle"},"clients":{},"messages":[
            {"type":"user","uuid":"task","timestampMs":1000,"message":{"content":"原任务"}},
            {"type":"assistant","uuid":"answer-1","timestampMs":1400,"message":{"id":"m1",
                "stop_reason":"end_turn","content":[{"type":"text","text":"原结果"}]}},
            {"type":"user","uuid":"followup","timestampMs":2000,"isMeta":false,
                "nativeSubagentInput":true,"message":{"content":"继续读取"}},
            {"type":"assistant","uuid":"answer-2","timestampMs":2300,"message":{"id":"m2",
                "stop_reason":"end_turn","content":[{"type":"text","text":"新结果"}]}}
        ]});
        let turns = project_shared_history(&frame, "subagent");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0]["turnCompletedAtMs"], 1400);
        assert_eq!(turns[1]["turnCompletedAtMs"], 2300);
        assert_eq!(turns[0]["status"], "completed");
        assert_eq!(turns[1]["status"], "completed");
    }
}
