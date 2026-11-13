//! Claude 原生工具调用投影。
//!
//! 用户要求远程会话兼容 Claude 实例的会话格式，而不是把所有工具都显示成“动态工具”：
//! - 命令、文件修改、网页搜索和 MCP 工具复用网页已有的 Codex 条目类型及渲染；
//! - 任务清单工具（TaskCreate / TaskUpdate / TodoWrite）合成为轮次计划，不逐条显示卡片；
//! - 其余原生工具保留通用卡片并标记 `source: "claude"`，由网页 `claudeToolPresentation` 按工具名显示摘要。
//!
//! 结构化补丁、后台任务 ID、任务 ID 由连接端 `claude_shared.py` 的 `native_tool_result` 精简后转发，
//! 两侧字段需同步核对。

use serde_json::{Value, json};

/// 由轮次计划接管、不单独显示卡片的原生任务清单工具。
const PLAN_TOOLS: [&str; 3] = ["TaskCreate", "TaskUpdate", "TodoWrite"];

/// 原生 tool_use 投影为网页条目；任务清单工具返回 None，由 [`observe_plan_call`] 记入轮次计划。
pub(super) fn tool_item(block: &Value) -> Option<Value> {
    let id = &block["id"];
    let input = &block["input"];
    let name = block["name"].as_str().unwrap_or_default();
    let item = match name {
        "Agent" | "Task" => json!({"id":id,"type":"collabAgentToolCall","tool":"spawn_agent",
            "displayName":input["description"],"arguments":input,"status":"inProgress","contentItems":[]}),
        "Bash" => json!({"id":id,"type":"commandExecution","source":"claude","tool":name,
            "command":input["command"].as_str().unwrap_or_default(),"commandActions":[],
            "description":input["description"],"background":input["run_in_background"] == true,
            "status":"inProgress","aggregatedOutput":"","exitCode":null}),
        "Edit" | "MultiEdit" | "Write" => {
            // Write 在结果返回前无法区分新建或覆盖：先按新建记录全文，结果确认覆盖时改用原生结构化补丁。
            let (kind, diff) = if name == "Write" {
                (
                    json!({"type":"add"}),
                    input["content"].as_str().unwrap_or_default(),
                )
            } else {
                (json!({"type":"update","move_path":null}), "")
            };
            json!({"id":id,"type":"fileChange","source":"claude","tool":name,"status":"inProgress",
                "changes":[{"path":input["file_path"].as_str().unwrap_or_default(),"kind":kind,"diff":diff}]})
        }
        "WebSearch" => {
            let query = input["query"].as_str().unwrap_or_default();
            json!({"id":id,"type":"webSearch","source":"claude","tool":name,"query":query,
                "action":{"type":"search","query":query},"status":"inProgress","contentItems":[]})
        }
        // 计划模式提交的计划按 Markdown 计划正文展示；没有计划正文的版本保留通用卡片。
        "ExitPlanMode"
            if input["plan"]
                .as_str()
                .is_some_and(|plan| !plan.trim().is_empty()) =>
        {
            json!({"id":id,"type":"plan","source":"claude","tool":name,"text":input["plan"],
                "status":"inProgress"})
        }
        name if PLAN_TOOLS.contains(&name) => return None,
        name => match name
            .strip_prefix("mcp__")
            .and_then(|rest| rest.split_once("__"))
        {
            Some((server, tool)) if !server.is_empty() && !tool.is_empty() => {
                json!({"id":id,"type":"mcpToolCall","source":"claude","server":server,"tool":tool,
                    "arguments":input,"status":"inProgress","result":null})
            }
            _ => json!({"id":id,"type":"dynamicToolCall","source":"claude","tool":name,
                "arguments":input,"status":"inProgress","contentItems":[]}),
        },
    };
    Some(item)
}

/// 原生 tool_result 写回对应条目。`frame` 是整条原生 user 记录；连接端只在其中恰有一个工具结果时附带
/// `nativeToolResult`，并以 `toolUseId` 标明归属。
pub(super) fn apply_result(item: &mut Value, block: &Value, frame: &Value) {
    let native =
        Some(&frame["nativeToolResult"]).filter(|meta| meta["toolUseId"] == block["tool_use_id"]);
    let failed = block["is_error"] == true;
    let interrupted = native.is_some_and(|meta| meta["interrupted"] == true);
    let parts = result_parts(&block["content"]);
    item["status"] = json!(if interrupted {
        "interrupted"
    } else if failed {
        "failed"
    } else {
        "completed"
    });
    match item["type"].as_str() {
        Some("commandExecution") => {
            let text = parts_text(&parts);
            // 原生失败命令的结果以“Exit code N”开头；拆出退出码，正文只保留实际输出。
            let (code, output) = if failed {
                split_exit_code(&text)
            } else {
                (None, text.as_str())
            };
            item["aggregatedOutput"] = json!(output);
            let background = native.and_then(|meta| meta["backgroundTaskId"].as_str());
            item["exitCode"] = match code {
                Some(code) => json!(code),
                // 转入后台的命令只是启动成功，尚无退出码；后台结束由任务通知更新。
                None if !failed && !interrupted && background.is_none() => json!(0),
                None => Value::Null,
            };
            if let Some(task) = background {
                item["backgroundTaskId"] = json!(task);
                // 已转入后台：结束状态由任务通知写回（notification_turn），会话离线时改为未知（settle_background）。
                if item["backgroundStatus"].is_null() {
                    item["backgroundStatus"] = json!("running");
                }
            }
        }
        Some("fileChange") if failed || interrupted => {
            // 修改未落盘（拒绝、报错或中断）：改为通用失败卡片保留原生原文，不计入本轮已编辑文件。
            let path = item["changes"][0]["path"].clone();
            *item = json!({"id":item["id"],"type":"dynamicToolCall","source":"claude","tool":item["tool"],
                "arguments":{"file_path":path},"status":item["status"],"contentItems":parts});
        }
        Some("fileChange") => {
            let change = &mut item["changes"][0];
            match native.filter(|meta| meta["fileChange"].is_string()) {
                Some(meta) => {
                    if let Some(path) = meta["filePath"].as_str() {
                        change["path"] = json!(path);
                    }
                    if meta["fileChange"] == "create" && change["kind"]["type"] == "add" {
                        // 新建文件沿用 Write 输入全文作为新增内容，与 Codex 新增文件记录一致。
                    } else {
                        change["kind"] = json!({"type":"update","move_path":null});
                        change["diff"] = json!(unified_diff(&meta["structuredPatch"]));
                    }
                }
                // 缺少原生补丁元数据时无法确认是否新建，不把覆盖误报为新增，也不伪造差异。
                None => {
                    change["kind"] = json!({"type":"update","move_path":null});
                    change["diff"] = json!("");
                }
            }
        }
        Some("mcpToolCall") => item["result"] = json!({"content":parts,"isError":failed}),
        Some("plan") => {}
        _ => item["contentItems"] = json!(parts),
    }
}

/// 原生任务通知（后台命令结束、监控事件、后台子代理结束）开启的自动续跑轮次。
///
/// 用户要求这类轮次不显示成用户消息、不可编辑，以“自动继续”事件开头，随后的回复归入本轮而不是并入上一轮。
/// 同时把通知状态写回对应的后台命令卡片；子代理卡片状态仍由 `subagents::observe_notification` 维护。
pub(super) fn notification_turn(
    turns: &mut [Value],
    row: &Value,
    generation: &str,
) -> Option<Value> {
    let notification = &row["nativeTaskNotification"];
    let task = notification["taskId"].as_str()?;
    let uuid = row["uuid"].as_str()?;
    let tool = notification["toolUseId"].as_str();
    let summary = notification["summary"].as_str().unwrap_or_default();
    // 连接端可直接给出类别（如定时任务 schedule）；否则带 event 的是监控事件，其余按所属工具卡片判断。
    let mut kind = match notification["kind"].as_str() {
        Some(kind) => kind,
        None if notification["event"].is_string() => "monitor",
        None => "task",
    };
    for item in turns
        .iter_mut()
        .flat_map(|turn| turn["items"].as_array_mut().into_iter().flatten())
    {
        if !(tool.is_some_and(|tool| item["id"] == tool) || item["backgroundTaskId"] == task) {
            continue;
        }
        match item["type"].as_str() {
            Some("commandExecution") => {
                kind = "command";
                let Some(status) = notification["status"].as_str() else {
                    continue;
                };
                item["backgroundStatus"] = json!(status);
                // 原生摘要形如 “… completed (exit code 0)” / “… failed with exit code 1”。
                let code = summary.rsplit_once("exit code ").and_then(|(_, rest)| {
                    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                    digits.parse::<i64>().ok()
                });
                if let Some(code) = code {
                    item["exitCode"] = json!(code);
                }
                if status == "failed" || code.is_some_and(|code| code != 0) {
                    item["status"] = json!("failed");
                }
            }
            Some("collabAgentToolCall") => kind = "agent",
            // Monitor 结束通知带 tool-use-id（2.1.323 实测），对应其工具卡片。
            Some("dynamicToolCall") if item["tool"] == "Monitor" => kind = "monitor",
            _ => {}
        }
    }
    Some(
        json!({"id":uuid,"_nativeUserId":uuid,"_nativeEditable":false,"_claudeAutoTurn":true,
        "_process":generation,"status":"completed","itemsView":"full","itemsComplete":true,
        "turnStartedAtMs":row["timestampMs"],
        "items":[{"id":format!("{uuid}-notification"),"type":"claudeTaskNotification","source":"claude",
            "kind":kind,"taskId":task,"status":notification["status"],"summary":summary,
            "event":notification["event"],"completedAtMs":row["timestampMs"]}]}),
    )
}

/// 终端本地斜杠命令（claudeLocalCommand）与原生系统提示（claudeNotice：离开期间回顾、原生提示、
/// 阻止继续或出错的 Stop Hook）投影为会话事件行；识别规则见连接端 native_local_command / native_system_notice。
pub(super) fn event_item(row: &Value) -> Option<Value> {
    let uuid = row["uuid"].as_str()?;
    let command = &row["nativeLocalCommand"];
    if command.is_object() {
        return Some(
            json!({"id":uuid,"type":"claudeLocalCommand","source":"claude",
            "name":command["name"],"args":command["args"],"output":null,"completedAtMs":row["timestampMs"]}),
        );
    }
    let notice = &row["nativeSystemNotice"];
    notice.is_object().then(|| {
        let mut item = notice.clone();
        item["id"] = json!(uuid);
        item["type"] = json!("claudeNotice");
        item["source"] = json!("claude");
        item["completedAtMs"] = row["timestampMs"].clone();
        item
    })
}

/// 本地命令输出并入对应的命令事件；找不到命令事件（例如被回退到其他分支）时输出不单独显示。
pub(super) fn attach_local_output(turns: &mut [Value], row: &Value) -> bool {
    let output = &row["nativeLocalOutput"];
    let Some(command) = output["commandId"].as_str() else {
        return false;
    };
    if let Some(item) = turns
        .iter_mut()
        .rev()
        .flat_map(|turn| turn["items"].as_array_mut().into_iter().flatten())
        .find(|item| item["type"] == "claudeLocalCommand" && item["id"] == command)
    {
        item["output"] = output["text"].clone();
        item["outputStream"] = output["stream"].clone();
    }
    true
}

/// 原生任务通知并入会话：连续送达、尚无回复的多条通知由原生合并为同一轮回复（2.1.323 实测），
/// 并入当前自动续跑轮次；否则开启新轮次，任务清单从上一轮延续。返回事件所在轮次与条目 ID。
pub(super) fn push_notification(
    turns: &mut Vec<Value>,
    row: &Value,
    generation: &str,
) -> Option<(usize, String)> {
    let mut turn = notification_turn(turns, row, generation)?;
    let item = turn["items"][0].clone();
    let item_id = item["id"].as_str().unwrap_or_default().to_owned();
    if let Some(last) = turns.last_mut().filter(|last| {
        last["_claudeAutoTurn"] == true
            && last["_nativeDone"] != true
            && last["items"].as_array().is_some_and(|items| {
                items
                    .iter()
                    .all(|item| item["type"] == "claudeTaskNotification")
            })
    }) {
        last["items"].as_array_mut().unwrap().push(item);
    } else {
        if let Some(tasks) = turns.last().and_then(carried_tasks) {
            turn["_claudeTaskSeed"] = tasks;
        }
        turns.push(turn);
    }
    Some((turns.len() - 1, item_id))
}

/// 后台任务随原生会话进程结束：连接端确认会话已不在线时，仍显示“后台运行”的命令改为结果未知。
pub(super) fn settle_background(turns: &mut [Value], native_status: &Value) {
    if native_status["sessionAlive"] != false {
        return;
    }
    for item in turns
        .iter_mut()
        .flat_map(|turn| turn["items"].as_array_mut().into_iter().flatten())
    {
        if item["backgroundStatus"] == "running" {
            item["backgroundStatus"] = json!("unknown");
        }
    }
}

/// 网页通用工具结果只认 MCP 图片格式；原生 base64 图片与工具引用块在此转换，其余内容原样保留。
fn result_parts(content: &Value) -> Vec<Value> {
    match content {
        Value::Array(parts) => parts
            .iter()
            .map(|part| match part["type"].as_str() {
                Some("image") if part["source"]["type"] == "base64" => {
                    json!({"type":"image","data":part["source"]["data"],
                        "mimeType":part["source"]["media_type"]})
                }
                Some("tool_reference") => json!({"type":"text","text":format!("已加载工具：{}",
                    part["tool_name"].as_str().unwrap_or_default())}),
                _ => part.clone(),
            })
            .collect(),
        other => vec![json!({"type":"text","text":other.as_str().unwrap_or_default()})],
    }
}

fn parts_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_exit_code(text: &str) -> (Option<i64>, &str) {
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    match first
        .strip_prefix("Exit code ")
        .and_then(|code| code.trim().parse::<i64>().ok())
    {
        Some(code) => (Some(code), rest),
        None => (None, text),
    }
}

/// 原生结构化补丁转为统一差异文本，供网页“已编辑文件”统计和差异查看复用。
fn unified_diff(patch: &Value) -> String {
    let mut lines = Vec::new();
    for hunk in patch.as_array().into_iter().flatten() {
        let number = |key: &str| hunk[key].as_u64().unwrap_or_default();
        lines.push(format!(
            "@@ -{},{} +{},{} @@",
            number("oldStart"),
            number("oldLines"),
            number("newStart"),
            number("newLines")
        ));
        lines.extend(
            hunk["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned),
        );
    }
    lines.join("\n")
}

/// 记录任务清单工具调用并重算轮次计划；同一调用重复投影时按 ID 合并。
pub(super) fn observe_plan_call(turn: &mut Value, block: &Value) {
    if !turn["_claudePlanCalls"].is_array() {
        turn["_claudePlanCalls"] = json!([]);
    }
    let calls = turn["_claudePlanCalls"].as_array_mut().unwrap();
    match calls.iter_mut().find(|call| call["id"] == block["id"]) {
        Some(call) => call["input"] = block["input"].clone(),
        None => calls.push(
            json!({"id":block["id"],"name":block["name"],"input":block["input"],
            "state":"pending"}),
        ),
    }
    recompute_plan(turn);
}

/// 任务清单工具结果：成功时记下原生任务 ID；失败时返回通用失败卡片，保留原生报错原文。
pub(super) fn observe_plan_result(turn: &mut Value, block: &Value, frame: &Value) -> Option<Value> {
    let index = turn["_claudePlanCalls"]
        .as_array()?
        .iter()
        .position(|call| call["id"] == block["tool_use_id"])?;
    let native =
        Some(&frame["nativeToolResult"]).filter(|meta| meta["toolUseId"] == block["tool_use_id"]);
    let failed = block["is_error"] == true || native.is_some_and(|meta| meta["success"] == false);
    let call = &mut turn["_claudePlanCalls"][index];
    call["state"] = json!(if failed { "failed" } else { "done" });
    if let Some(task) = native.and_then(|meta| meta["createdTaskId"].as_str()) {
        call["taskId"] = json!(task);
    }
    let item = failed.then(|| {
        json!({"id":call["id"],"type":"dynamicToolCall","source":"claude","tool":call["name"],
            "arguments":call["input"],"status":"failed","contentItems":result_parts(&block["content"])})
    });
    recompute_plan(turn);
    item
}

/// 原生任务清单属于整个会话：新轮次从上一轮的最终清单开始，只在调用过清单工具的轮次显示计划。
pub(super) fn carried_tasks(previous: &Value) -> Option<Value> {
    [&previous["_claudeTasks"], &previous["_claudeTaskSeed"]]
        .into_iter()
        .find(|tasks| tasks.as_array().is_some_and(|tasks| !tasks.is_empty()))
        .cloned()
}

fn recompute_plan(turn: &mut Value) {
    let mut tasks = turn["_claudeTaskSeed"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for call in turn["_claudePlanCalls"].as_array().into_iter().flatten() {
        if call["state"] == "failed" {
            continue;
        }
        let input = &call["input"];
        match call["name"].as_str() {
            // TodoWrite 每次提交完整清单，直接替换。
            Some("TodoWrite") => {
                if let Some(todos) = input["todos"].as_array() {
                    tasks = todos
                        .iter()
                        .enumerate()
                        .filter_map(|(index, todo)| {
                            Some(json!({"id":format!("todo-{index}"),"subject":subject(&todo["content"])?,
                                "status":step_status(&todo["status"]).unwrap_or("pending")}))
                        })
                        .collect();
                }
            }
            // 原生任务 ID 只取 TaskCreate 结果；结果返回前先显示为待办，不推测编号。
            Some("TaskCreate") => {
                if let Some(text) = subject(&input["subject"]) {
                    tasks.push(json!({"id":call["taskId"],"subject":text,"status":"pending"}));
                }
            }
            Some("TaskUpdate") => {
                let Some(id) = task_id(&input["taskId"]) else {
                    continue;
                };
                let Some(index) = tasks.iter().position(|task| task["id"] == id.as_str()) else {
                    continue;
                };
                if input["status"] == "deleted" {
                    tasks.remove(index);
                    continue;
                }
                if let Some(status) = step_status(&input["status"]) {
                    tasks[index]["status"] = json!(status);
                }
                if let Some(text) = subject(&input["subject"]) {
                    tasks[index]["subject"] = json!(text);
                }
            }
            _ => {}
        }
    }
    turn["plan"] = json!(
        tasks
            .iter()
            .map(|task| json!({"step":task["subject"],"status":task["status"]}))
            .collect::<Vec<_>>()
    );
    turn["_claudeTasks"] = json!(tasks);
}

fn subject(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn task_id(value: &Value) -> Option<String> {
    match value {
        Value::String(id) if !id.trim().is_empty() => Some(id.trim().to_owned()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

/// 原生状态映射为网页计划步骤状态（Codex 的 pending / inProgress / completed）。
fn step_status(value: &Value) -> Option<&'static str> {
    match value.as_str()? {
        "pending" => Some("pending"),
        "in_progress" => Some("inProgress"),
        "completed" => Some("completed"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(tool: &str, content: Value, error: bool) -> Value {
        json!({"type":"tool_result","tool_use_id":tool,"content":content,"is_error":error})
    }

    #[test]
    fn bash_uses_command_item_and_splits_native_exit_code() {
        let mut item = tool_item(&json!({"type":"tool_use","id":"t1","name":"Bash",
            "input":{"command":"ls -la","description":"列目录"}}))
        .unwrap();
        assert_eq!(item["type"], "commandExecution");
        assert_eq!(item["command"], "ls -la");
        apply_result(
            &mut item,
            &result("t1", json!("Exit code 2\nls: 无法访问"), true),
            &json!({}),
        );
        assert_eq!(item["status"], "failed");
        assert_eq!(item["exitCode"], 2);
        assert_eq!(item["aggregatedOutput"], "ls: 无法访问");

        let mut ok = tool_item(&json!({"type":"tool_use","id":"t2","name":"Bash",
            "input":{"command":"echo hi"}}))
        .unwrap();
        apply_result(&mut ok, &result("t2", json!("hi"), false), &json!({}));
        assert_eq!(
            (ok["exitCode"].clone(), ok["aggregatedOutput"].clone()),
            (json!(0), json!("hi"))
        );
    }

    #[test]
    fn background_bash_has_no_exit_code_until_the_task_finishes() {
        let mut item = tool_item(&json!({"type":"tool_use","id":"t1","name":"Bash",
            "input":{"command":"sleep 30","run_in_background":true}}))
        .unwrap();
        assert_eq!(item["background"], true);
        let frame = json!({"nativeToolResult":{"toolUseId":"t1","backgroundTaskId":"b1"}});
        apply_result(
            &mut item,
            &result(
                "t1",
                json!("Command running in background with ID: b1."),
                false,
            ),
            &frame,
        );
        assert_eq!(item["status"], "completed");
        assert!(item["exitCode"].is_null());
        assert_eq!(item["backgroundTaskId"], "b1");
    }

    #[test]
    fn edit_uses_native_structured_patch_and_failed_edit_keeps_native_text() {
        let mut item = tool_item(&json!({"type":"tool_use","id":"e1","name":"Edit",
            "input":{"file_path":"/w/a.txt","old_string":"a","new_string":"b"}}))
        .unwrap();
        assert_eq!(item["type"], "fileChange");
        let frame = json!({"nativeToolResult":{"toolUseId":"e1","filePath":"/w/a.txt","fileChange":"update",
            "structuredPatch":[{"oldStart":3,"oldLines":2,"newStart":3,"newLines":2,
                "lines":[" 上文","-a","+b"]}]}});
        apply_result(&mut item, &result("e1", json!("已更新"), false), &frame);
        assert_eq!(item["status"], "completed");
        assert_eq!(item["changes"][0]["kind"]["type"], "update");
        assert_eq!(item["changes"][0]["diff"], "@@ -3,2 +3,2 @@\n 上文\n-a\n+b");

        let mut rejected = tool_item(&json!({"type":"tool_use","id":"e2","name":"Edit",
            "input":{"file_path":"/w/a.txt"}}))
        .unwrap();
        apply_result(
            &mut rejected,
            &result("e2", json!("The user doesn't want to proceed"), true),
            &json!({}),
        );
        assert_eq!(rejected["type"], "dynamicToolCall");
        assert_eq!(rejected["status"], "failed");
        assert_eq!(rejected["arguments"]["file_path"], "/w/a.txt");
        assert_eq!(
            rejected["contentItems"][0]["text"],
            "The user doesn't want to proceed"
        );
    }

    #[test]
    fn write_is_add_only_when_native_result_confirms_creation() {
        let block = json!({"type":"tool_use","id":"w1","name":"Write",
            "input":{"file_path":"/w/new.txt","content":"一\n二\n"}});
        let mut created = tool_item(&block).unwrap();
        let frame = json!({"nativeToolResult":{"toolUseId":"w1","filePath":"/w/new.txt",
            "fileChange":"create","structuredPatch":[]}});
        apply_result(&mut created, &result("w1", json!("ok"), false), &frame);
        assert_eq!(created["changes"][0]["kind"]["type"], "add");
        assert_eq!(created["changes"][0]["diff"], "一\n二\n");

        let mut overwritten = tool_item(&block).unwrap();
        let frame = json!({"nativeToolResult":{"toolUseId":"w1","filePath":"/w/new.txt",
            "fileChange":"update","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,
                "newLines":2,"lines":["-旧","+一","+二"]}]}});
        apply_result(&mut overwritten, &result("w1", json!("ok"), false), &frame);
        assert_eq!(overwritten["changes"][0]["kind"]["type"], "update");
        assert_eq!(
            overwritten["changes"][0]["diff"],
            "@@ -1,1 +1,2 @@\n-旧\n+一\n+二"
        );

        // 元数据属于其他调用或缺失时，不把可能的覆盖误报为新增。
        let mut unknown = tool_item(&block).unwrap();
        let other = json!({"nativeToolResult":{"toolUseId":"other","fileChange":"create"}});
        apply_result(&mut unknown, &result("w1", json!("ok"), false), &other);
        assert_eq!(unknown["changes"][0]["kind"]["type"], "update");
        assert_eq!(unknown["changes"][0]["diff"], "");
    }

    #[test]
    fn search_mcp_and_builtin_tools_keep_native_results_displayable() {
        let search = tool_item(&json!({"type":"tool_use","id":"s1","name":"WebSearch",
            "input":{"query":"rust serde"}}))
        .unwrap();
        assert_eq!(
            (search["type"].clone(), search["action"]["query"].clone()),
            (json!("webSearch"), json!("rust serde"))
        );

        let mut mcp = tool_item(
            &json!({"type":"tool_use","id":"m1","name":"mcp__chrome-devtools__take_screenshot",
            "input":{}}),
        )
        .unwrap();
        assert_eq!(
            (
                mcp["type"].clone(),
                mcp["server"].clone(),
                mcp["tool"].clone()
            ),
            (
                json!("mcpToolCall"),
                json!("chrome-devtools"),
                json!("take_screenshot")
            )
        );
        apply_result(
            &mut mcp,
            &result(
                "m1",
                json!([{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AQID"}}]),
                true,
            ),
            &json!({}),
        );
        assert_eq!(mcp["result"]["isError"], true);
        assert_eq!(
            mcp["result"]["content"][0],
            json!({"type":"image","data":"AQID","mimeType":"image/png"})
        );

        let mut read = tool_item(&json!({"type":"tool_use","id":"r1","name":"ToolSearch",
            "input":{"query":"select:WebSearch"}}))
        .unwrap();
        assert_eq!(
            (read["type"].clone(), read["source"].clone()),
            (json!("dynamicToolCall"), json!("claude"))
        );
        apply_result(
            &mut read,
            &result(
                "r1",
                json!([{"type":"tool_reference","tool_name":"WebSearch"}]),
                false,
            ),
            &json!({}),
        );
        assert_eq!(read["contentItems"][0]["text"], "已加载工具：WebSearch");
    }

    #[test]
    fn task_tools_build_one_session_plan_across_turns() {
        let mut first = json!({"items":[]});
        for (id, subject) in [("c1", "读代码"), ("c2", "写测试")] {
            assert!(tool_item(&json!({"type":"tool_use","id":id,"name":"TaskCreate"})).is_none());
            observe_plan_call(
                &mut first,
                &json!({"id":id,"name":"TaskCreate","input":{"subject":subject}}),
            );
        }
        assert_eq!(
            first["plan"][1],
            json!({"step":"写测试","status":"pending"})
        );
        for (call, task) in [("c1", "1"), ("c2", "2")] {
            let frame = json!({"nativeToolResult":{"toolUseId":call,"createdTaskId":task}});
            assert!(
                observe_plan_result(&mut first, &result(call, json!("ok"), false), &frame)
                    .is_none()
            );
        }
        observe_plan_call(
            &mut first,
            &json!({"id":"u1","name":"TaskUpdate","input":{"taskId":"1","status":"in_progress"}}),
        );
        assert_eq!(first["plan"][0]["status"], "inProgress");

        let mut second = json!({"items":[],"_claudeTaskSeed":carried_tasks(&first)});
        observe_plan_call(
            &mut second,
            &json!({"id":"u2","name":"TaskUpdate","input":{"taskId":1,"status":"completed"}}),
        );
        observe_plan_call(
            &mut second,
            &json!({"id":"u3","name":"TaskUpdate","input":{"taskId":"2","status":"deleted"}}),
        );
        assert_eq!(
            second["plan"],
            json!([{"step":"读代码","status":"completed"}])
        );
        // 失败的更新不改变清单，并以通用失败卡片保留原生报错。
        observe_plan_call(
            &mut second,
            &json!({"id":"u4","name":"TaskUpdate","input":{"taskId":"1","status":"pending"}}),
        );
        let failed = observe_plan_result(
            &mut second,
            &result("u4", json!("Task not found"), true),
            &json!({}),
        )
        .unwrap();
        assert_eq!(
            (failed["status"].clone(), failed["tool"].clone()),
            (json!("failed"), json!("TaskUpdate"))
        );
        assert_eq!(second["plan"][0]["status"], "completed");
        // 第一轮的计划保持当时的快照。
        assert_eq!(first["plan"][0]["status"], "inProgress");
    }

    #[test]
    fn task_notification_starts_auto_turn_and_settles_background_command() {
        let mut item = tool_item(&json!({"type":"tool_use","id":"b1","name":"Bash",
            "input":{"command":"make","run_in_background":true}}))
        .unwrap();
        apply_result(
            &mut item,
            &result("b1", json!("started"), false),
            &json!({"nativeToolResult":{"toolUseId":"b1","backgroundTaskId":"bg1"}}),
        );
        assert_eq!(item["backgroundStatus"], "running");
        let mut turns = vec![json!({"id":"u1","items":[item]})];
        let row = json!({"uuid":"n1","timestampMs":50,"nativeTaskNotification":{"taskId":"bg1",
            "toolUseId":"b1","status":"failed","summary":"Background command \"make\" failed with exit code 2"}});
        let turn = notification_turn(&mut turns, &row, "g").unwrap();
        assert_eq!(
            (
                turn["id"].clone(),
                turn["_nativeEditable"].clone(),
                turn["items"][0]["type"].clone(),
                turn["items"][0]["kind"].clone()
            ),
            (
                json!("n1"),
                json!(false),
                json!("claudeTaskNotification"),
                json!("command")
            )
        );
        let command = &turns[0]["items"][0];
        assert_eq!(
            (
                command["backgroundStatus"].clone(),
                command["exitCode"].clone(),
                command["status"].clone()
            ),
            (json!("failed"), json!(2), json!("failed"))
        );
        // 监控事件没有状态，只开启自动续跑轮次。
        let monitor = notification_turn(
            &mut turns,
            &json!({"uuid":"n2","nativeTaskNotification":{"taskId":"m1",
                "summary":"Monitor event: \"等待\"","event":"第一行"}}),
            "g",
        )
        .unwrap();
        assert_eq!(monitor["items"][0]["kind"], "monitor");
        assert!(notification_turn(&mut turns, &json!({"uuid":"n3"}), "g").is_none());

        let mut running = vec![
            json!({"items":[{"type":"commandExecution","backgroundStatus":"running"},
            {"type":"commandExecution","backgroundStatus":"completed"}]}),
        ];
        for status in [json!({"sessionAlive":true}), json!({})] {
            settle_background(&mut running, &status);
            assert_eq!(running[0]["items"][0]["backgroundStatus"], "running");
        }
        settle_background(&mut running, &json!({"sessionAlive":false}));
        assert_eq!(running[0]["items"][0]["backgroundStatus"], "unknown");
        assert_eq!(running[0]["items"][1]["backgroundStatus"], "completed");
    }

    #[test]
    fn consecutive_notifications_share_one_auto_turn_until_claude_replies() {
        let monitor = json!({"id":"m1","type":"dynamicToolCall","source":"claude","tool":"Monitor",
            "status":"completed"});
        let mut turns = vec![json!({"id":"u1","_nativeDone":true,"items":[monitor]})];
        let notification = |uuid: &str, task: &str, extra: Value| {
            let mut value = json!({"taskId":task,"summary":"摘要"});
            for (key, field) in extra.as_object().unwrap() {
                value[key] = field.clone();
            }
            json!({"uuid":uuid,"nativeTaskNotification":value})
        };
        // 2.1.323 实测：监控事件、监控结束、后台命令完成三条通知连续写入，原生只回复一轮。
        let rows = [
            notification("n1", "m-task", json!({"event":"tick-1"})),
            notification(
                "n2",
                "m-task",
                json!({"toolUseId":"m1","status":"completed"}),
            ),
            notification("n3", "b-task", json!({"status":"completed"})),
        ];
        let positions: Vec<_> = rows
            .iter()
            .map(|row| push_notification(&mut turns, row, "g").unwrap())
            .collect();
        assert_eq!(
            positions,
            [
                (1, "n1-notification".into()),
                (1, "n2-notification".into()),
                (1, "n3-notification".into())
            ]
        );
        let kinds: Vec<_> = turns[1]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["kind"].clone())
            .collect();
        assert_eq!(kinds, [json!("monitor"), json!("monitor"), json!("task")]);
        // Claude 回复后再来的通知开启新的一轮。
        turns[1]["items"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"r","type":"agentMessage","text":"都完成了"}));
        assert_eq!(
            push_notification(
                &mut turns,
                &notification("n4", "m-task", json!({"event":"tick-2"})),
                "g"
            ),
            Some((2, "n4-notification".into()))
        );
    }

    #[test]
    fn todo_write_replaces_the_whole_plan() {
        let mut turn = json!({"items":[],"_claudeTaskSeed":[{"id":"1","subject":"旧任务","status":"pending"}]});
        observe_plan_call(
            &mut turn,
            &json!({"id":"t1","name":"TodoWrite","input":{"todos":[
            {"content":"甲","status":"completed","activeForm":"做甲"},
            {"content":"乙","status":"in_progress","activeForm":"做乙"},
            {"content":"  ","status":"pending"}]}}),
        );
        assert_eq!(
            turn["plan"],
            json!([{"step":"甲","status":"completed"},{"step":"乙","status":"inProgress"}])
        );
    }
}
