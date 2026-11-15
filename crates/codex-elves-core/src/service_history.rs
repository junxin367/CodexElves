//! 独立 app-server 历史的状态与时间转换；不读取桌面快照。
use serde_json::{Value, json};

fn lifecycle_status(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value["type"].as_str())
        .or_else(|| value["status"].as_str())
}

const UNIX_SECONDS_LIMIT: u64 = 100_000_000_000;

fn timestamp_ms(value: &Value) -> Option<u64> {
    if let Some(value) = value.as_u64().filter(|value| *value > 0) {
        return Some(if value < UNIX_SECONDS_LIMIT {
            value.saturating_mul(1000)
        } else {
            value
        });
    }
    value
        .as_f64()
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| {
            if value < UNIX_SECONDS_LIMIT as f64 {
                (value * 1000.0).round() as u64
            } else {
                value.round() as u64
            }
        })
}

fn milliseconds_value(value: &Value) -> Option<u64> {
    if let Some(value) = value.as_u64().filter(|value| *value > 0) {
        return Some(value);
    }
    value
        .as_f64()
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.round() as u64)
}

fn timestamp_by_id(value: &Value, id: &str) -> Option<u64> {
    value.get(id).and_then(milliseconds_value)
}

fn turn_started_at_ms(turn: &Value) -> Option<u64> {
    milliseconds_value(&turn["turnStartedAtMs"]).or_else(|| timestamp_ms(&turn["startedAt"]))
}

fn turn_completed_at_ms(turn: &Value) -> Option<u64> {
    milliseconds_value(&turn["turnCompletedAtMs"]).or_else(|| timestamp_ms(&turn["completedAt"]))
}

fn project_message_timestamps(turn: &mut Value) {
    let turn_started_at_ms = turn_started_at_ms(turn);
    let turn_completed_at_ms = turn_completed_at_ms(turn);
    let aeon_assistant_started_at_ms_by_id = turn["aeonAssistantMessageStartedAtMsById"].clone();
    let assistant_started_at_ms_by_id = turn["assistantMessageStartedAtMsById"].clone();
    let final_assistant_started_at_ms = milliseconds_value(&turn["finalAssistantStartedAtMs"]);
    let Some(items) = turn["items"].as_array_mut() else {
        return;
    };
    let first_user_id = items.iter().find_map(|item| {
        matches!(
            item["type"].as_str(),
            Some("userMessage" | "steeringUserMessage")
        )
        .then(|| item["id"].as_str())
        .flatten()
        .map(str::to_owned)
    });
    let assistant_ids: Vec<String> = items
        .iter()
        .filter(|item| item["type"] == "agentMessage")
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    let last_assistant_id = assistant_ids.last().cloned();

    // 用户要求恢复 Codex 服务模式消息时间；只使用原生消息/轮次时间，不能用页面渲染时刻补造时间。
    for item in items {
        let item_type = item["type"].as_str();
        let id = item["id"].as_str().unwrap_or_default();
        let existing = milliseconds_value(&item["sentAtMs"])
            .or_else(|| milliseconds_value(&item["messageMetadata"]["sentAtMs"]));
        let sent_at_ms = match item_type {
            Some("userMessage" | "steeringUserMessage") => existing
                .or_else(|| milliseconds_value(&item["startedAtMs"]))
                .or_else(|| {
                    (first_user_id.as_deref() == Some(id))
                        .then_some(turn_started_at_ms)
                        .flatten()
                }),
            Some("agentMessage") if item["delivery"] != "async" => existing
                .or_else(|| milliseconds_value(&item["startedAtMs"]))
                .or_else(|| timestamp_by_id(&aeon_assistant_started_at_ms_by_id, id))
                .or_else(|| timestamp_by_id(&assistant_started_at_ms_by_id, id))
                .or_else(|| {
                    (last_assistant_id.as_deref() == Some(id))
                        .then_some(final_assistant_started_at_ms)
                        .flatten()
                })
                // 部分服务版本只提供轮次结束时间；这是原生生命周期时间，仍优于隐藏消息时间。
                .or_else(|| {
                    (last_assistant_id.as_deref() == Some(id))
                        .then_some(turn_completed_at_ms)
                        .flatten()
                }),
            _ => None,
        };
        if let Some(sent_at_ms) = sent_at_ms {
            item["sentAtMs"] = json!(sent_at_ms);
        }
    }
}

pub(crate) fn normalize_history_turn(mut turn: Value) -> Value {
    let running = matches!(
        lifecycle_status(&turn["status"]),
        Some("inProgress" | "active" | "running")
    );
    let completed_ids = turn["completedItemIds"].as_object().cloned();
    for item in turn["items"].as_array_mut().into_iter().flatten() {
        let Some(_) = item.as_object() else { continue };
        let raw_status = lifecycle_status(&item["status"]).map(str::to_string);
        let terminal_status = raw_status.as_deref().filter(|status| {
            matches!(
                *status,
                "completed" | "failed" | "interrupted" | "cancelled" | "canceled"
            )
        });
        let completed_by_id = item["id"]
            .as_str()
            .and_then(|id| completed_ids.as_ref().and_then(|ids| ids.get(id)))
            == Some(&Value::Bool(true));
        let has_completed_time = item["completedAtMs"].as_u64().is_some();
        let has_error = item.get("error").is_some_and(|error| {
            !error.is_null() && error != false && error.as_str().is_none_or(|text| !text.is_empty())
        });
        let outcome_failed = item["success"] == false
            || item["result"]["isError"] == true
            || has_error
            || item["exitCode"].as_i64().is_some_and(|code| code != 0);
        let has_outcome = item["success"].is_boolean()
            || item["exitCode"].as_i64().is_some()
            || item["result"]["isError"].is_boolean()
            || has_error;
        let complete =
            terminal_status.is_some() || completed_by_id || has_completed_time || has_outcome;
        let known_pending = running
            && (item["startedAtMs"].as_u64().is_some()
                || completed_ids.is_some() && item["id"].as_str().is_some_and(|id| !id.is_empty()));
        // 原生完成标记才是执行结果依据；工具声明的时长、经过时间或轮次结束均不能证明成功。
        let status = if let Some(status) = terminal_status.filter(|status| *status != "completed") {
            status
        } else if complete {
            if outcome_failed {
                "failed"
            } else {
                "completed"
            }
        } else if let Some(status) = raw_status.as_deref().filter(|status| !status.is_empty()) {
            status
        } else if known_pending {
            "inProgress"
        } else {
            "unknown"
        };
        item["status"] = json!(status);
        item["remoteItemCompleted"] = json!(complete);
    }
    project_message_timestamps(&mut turn);
    turn
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_tool_status_uses_completion_dictionary_and_times_instead_of_requested_duration() {
        let sleep = json!({
            "id":"sleep-tool","type":"sleep","durationMs":15000,"startedAtMs":1793490014982u64
        });
        let turn = json!({
            "turnId":"turn","status":"inProgress","completedItemIds":{},"items":[sleep]
        });
        let active = normalize_history_turn(turn.clone());
        assert_eq!(active["items"][0]["status"], "inProgress");
        assert_eq!(active["items"][0]["remoteItemCompleted"], false);
        assert_eq!(active["items"][0]["durationMs"], 15000);
        assert!(turn["items"][0].get("status").is_none());
        assert!(active["items"][0].get("completedAtMs").is_none());

        let mut by_id = turn.clone();
        by_id["completedItemIds"]["sleep-tool"] = json!(true);
        let completed = normalize_history_turn(by_id);
        assert_eq!(completed["items"][0]["status"], "completed");
        assert_eq!(completed["items"][0]["remoteItemCompleted"], true);
        let mut by_time = turn;
        by_time["items"][0]["completedAtMs"] = json!(1793490029985u64);
        let completed = normalize_history_turn(by_time);
        assert_eq!(completed["items"][0]["status"], "completed");
        assert_eq!(completed["items"][0]["startedAtMs"], 1793490014982u64);
        assert_eq!(completed["items"][0]["completedAtMs"], 1793490029985u64);
    }

    #[test]
    fn native_message_timestamps_project_real_user_and_assistant_times() {
        let projected = normalize_history_turn(json!({
            "turnId":"turn","status":"completed",
            "turnStartedAtMs":1000,"turnCompletedAtMs":5000,
            "assistantMessageStartedAtMsById":{"answer":3200},
            "items":[
                {"id":"user","type":"userMessage","content":[]},
                {"id":"answer","type":"agentMessage","text":"完成"},
                {"id":"async","type":"agentMessage","delivery":"async","text":"后台通知"}
            ]
        }));
        assert_eq!(projected["items"][0]["sentAtMs"], 1000);
        assert_eq!(projected["items"][1]["sentAtMs"], 3200);
        assert!(projected["items"][2].get("sentAtMs").is_none());

        let preserved = normalize_history_turn(json!({
            "turnId":"turn","status":"completed",
            "turnStartedAtMs":1000,"turnCompletedAtMs":5000,
            "finalAssistantStartedAtMs":3300,
            "items":[
                {"id":"user","type":"userMessage","sentAtMs":1500,"content":[]},
                {"id":"answer","type":"agentMessage","startedAtMs":2800,"text":"完成"}
            ]
        }));
        assert_eq!(preserved["items"][0]["sentAtMs"], 1500);
        assert_eq!(preserved["items"][1]["sentAtMs"], 2800);

        let final_fallback = normalize_history_turn(json!({
            "turnId":"turn","status":"completed",
            "turnStartedAtMs":1000,"turnCompletedAtMs":5000,
            "finalAssistantStartedAtMs":3300,
            "items":[
                {"id":"user","type":"userMessage","content":[]},
                {"id":"answer","type":"agentMessage","text":"完成"}
            ]
        }));
        assert_eq!(final_fallback["items"][1]["sentAtMs"], 3300);
    }

    #[test]
    fn native_tool_status_preserves_failed_and_interrupted_outcomes_and_rejects_unknown_success() {
        for item in [
            json!({"id":"tool","type":"commandExecution","status":"completed","exitCode":7}),
            json!({"id":"tool","type":"dynamicToolCall","success":false}),
            json!({"id":"tool","type":"mcpToolCall","result":{"isError":true,"content":[]}}),
            json!({"id":"tool","type":"customTool","error":{"message":"actual failure"}}),
        ] {
            let projected = normalize_history_turn(json!({
                "status":"completed","completedItemIds":{"tool":true},"items":[item]
            }));
            assert_eq!(projected["items"][0]["status"], "failed");
            assert_eq!(projected["items"][0]["remoteItemCompleted"], true);
        }
        let interrupted = normalize_history_turn(json!({
            "status":"interrupted","completedItemIds":{"tool":true},"items":[{
                "id":"tool","type":"commandExecution","status":"interrupted","exitCode":130
            }]
        }));
        assert_eq!(interrupted["items"][0]["status"], "interrupted");
        assert_eq!(interrupted["items"][0]["exitCode"], 130);
        for status in ["completed", "failed", "interrupted"] {
            let unknown = normalize_history_turn(json!({
                "status":status,"completedItemIds":{},"items":[{
                    "id":"tool","type":"sleep","durationMs":1,"startedAtMs":1000
                }]
            }));
            assert_eq!(unknown["items"][0]["status"], "unknown");
            assert_eq!(unknown["items"][0]["remoteItemCompleted"], false);
        }
        let malformed_ids = normalize_history_turn(json!({
            "status":"inProgress","completedItemIds":["tool"],"items":[{"id":"tool","type":"sleep"}]
        }));
        assert_eq!(malformed_ids["items"][0]["status"], "unknown");
        assert_eq!(malformed_ids["items"][0]["remoteItemCompleted"], false);
        let explicit = normalize_history_turn(json!({
            "status":"inProgress","items":[{
                "id":"tool","type":"customTool","status":{"type":"inProgress"}
            }]
        }));
        assert_eq!(explicit["items"][0]["status"], "inProgress");
    }
}
