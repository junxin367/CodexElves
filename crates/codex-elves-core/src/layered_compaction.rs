//! 分层压缩：拦截 Codex 的上下文压缩（CONTEXT CHECKPOINT COMPACTION）响应，在 LLM 摘要
//! 之上追加「压缩前最近的原始对话 / 工具调用记录」，避免 Codex 纯摘要压缩导致的“断片”。
//!
//! 机制（基于真实抓包验证）：
//! - Codex 压缩走普通 `/responses` 请求，`input` 最后一项是固定的压缩指令 user 消息。
//! - 上游返回一条 assistant message 作为摘要，Codex 用它替换历史窗口。
//! - 本模块在代理层把「摘要 + 最近原始记录」重新组织成一条 assistant message 回注，
//!   使 Codex 压缩后仍保留最近的原始上下文。
//!
//! 该转换只作用于 Responses 协议 SSE 文本（Chat/Anthropic 上游已在上层转换为 Responses SSE），
//! 因此与上游协议无关。

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

/// Codex 压缩指令的固定前缀（取自 codex 二进制 `core/src/tasks/compact.rs`）。
pub const COMPACTION_PROMPT_PREFIX: &str = "You are performing a CONTEXT CHECKPOINT COMPACTION";

/// Codex 内置的完整默认压缩提示词原文（逐字节从本机 `codex.exe` 提取，未任意改动）。
/// 用于：1) 未配置自定义提示词时作为 UI 默认展示值；2) “重置提示词”时回退目标。
pub const DEFAULT_COMPACTION_PROMPT: &str = "You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary for another LLM that will resume the task.\r\n\r\nInclude:\r\n- Current progress and key decisions made\r\n- Important context, constraints, or user preferences\r\n- What remains to be done (clear next steps)\r\n- Any critical data, examples, or references needed to continue\r\n\r\nBe concise, structured, and focused on helping the next LLM seamlessly continue the work.\r\n";

/// 分层压缩结果。
#[derive(Debug, Clone)]
pub struct LayeredCompactionResult {
    /// 最终回注给 Codex 的 Responses SSE 文本。
    pub sse_text: String,
    /// 是否真正触发了改写。
    pub triggered: bool,
    /// 实际保留的原始记录条数。
    pub retained_items: u32,
    /// 实际保留的原始记录字符数（用于诊断）。
    pub retained_chars: u32,
}

impl LayeredCompactionResult {
    fn unchanged(sse_text: String) -> Self {
        Self {
            sse_text,
            triggered: false,
            retained_items: 0,
            retained_chars: 0,
        }
    }
}

/// 判断请求是否是 Codex 的上下文压缩请求：`input` 最后一项是 user 消息，
/// 且其文本以固定压缩指令前缀开头。
pub fn is_compaction_request(request_json: Option<&Value>) -> bool {
    let Some(request) = request_json else {
        return false;
    };
    let Some(input) = request.get("input").and_then(Value::as_array) else {
        return false;
    };
    let Some(last) = input.last() else {
        return false;
    };
    if last.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    item_text(last)
        .trim_start()
        .starts_with(COMPACTION_PROMPT_PREFIX)
}

/// 若请求是 Codex 压缩请求且配置了自定义压缩提示词，将 `input` 最后一项（压缩指令）的文本
/// 替换为自定义内容，保持其余结构（type/role/content 数组形态）不变。
///
/// - 非压缩请求、自定义提示词为空、或无法定位最后一项时原样返回（继续使用 Codex 默认提示词）。
pub fn apply_custom_compaction_prompt(request_json: &Value, custom_prompt: &str) -> Value {
    let custom_prompt = custom_prompt.trim();
    if custom_prompt.is_empty() || !is_compaction_request(Some(request_json)) {
        return request_json.clone();
    }
    let mut updated = request_json.clone();
    let Some(input) = updated
        .as_object_mut()
        .and_then(|object| object.get_mut("input"))
        .and_then(Value::as_array_mut)
    else {
        return request_json.clone();
    };
    let Some(last) = input.last_mut() else {
        return request_json.clone();
    };
    replace_message_text(last, custom_prompt);
    updated
}

/// 将 message item 的文本内容整体替换为 `text`，兼容字符串 content 与
/// content 数组两种形态：数组形态只保留第一个文本块并替换其 `text`，其余块丢弃
/// （压缩指令本身只有单一文本块，不存在多块情况）。
fn replace_message_text(item: &mut Value, text: &str) {
    let Some(object) = item.as_object_mut() else {
        return;
    };
    match object.get("content") {
        Some(Value::Array(parts)) if !parts.is_empty() => {
            let kind = parts[0]
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("input_text")
                .to_string();
            object.insert(
                "content".to_string(),
                json!([{ "type": kind, "text": text }]),
            );
        }
        _ => {
            object.insert("content".to_string(), json!(text));
        }
    }
}

/// 在压缩响应 SSE 上应用分层压缩：把上游摘要与请求中最近的原始记录合并回注。
///
/// - `enabled` 为 false、非压缩请求、或无法解析终止响应/摘要时，原样返回。
/// - `retain_tokens` 是希望保留的最近原始记录预算（按 token 估算，约 4 字符/token）。
pub fn apply_layered_compaction_to_responses_sse(
    request_json: &Value,
    enabled: bool,
    retain_tokens: u32,
    sse_text: String,
) -> LayeredCompactionResult {
    if !enabled || !is_compaction_request(Some(request_json)) {
        return LayeredCompactionResult::unchanged(sse_text);
    }
    let Some(response_object) =
        crate::continue_thinking::extract_terminal_response_object(&sse_text)
    else {
        return LayeredCompactionResult::unchanged(sse_text);
    };
    // 只在终止状态为 completed 时改写；incomplete/failed 保持原样。
    if response_object.get("status").and_then(Value::as_str) != Some("completed") {
        return LayeredCompactionResult::unchanged(sse_text);
    }
    let Some(summary) = extract_message_text(&response_object) else {
        return LayeredCompactionResult::unchanged(sse_text);
    };
    if summary.trim().is_empty() {
        return LayeredCompactionResult::unchanged(sse_text);
    }

    let input = request_json
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let budget_chars = retain_budget_chars(retain_tokens);
    let TailSection {
        text: tail,
        items: retained_items,
        chars: retained_chars,
    } = build_tail_section(&input, budget_chars);

    if tail.is_empty() {
        return LayeredCompactionResult::unchanged(sse_text);
    }

    let combined = combine_summary_with_tail(&summary, &tail);
    let rebuilt = build_responses_sse_for_message(&response_object, &combined);

    LayeredCompactionResult {
        sse_text: rebuilt,
        triggered: true,
        retained_items,
        retained_chars,
    }
}

/// token 预算换算为字符预算（粗略 4 字符 ≈ 1 token），并做上下限约束。
fn retain_budget_chars(retain_tokens: u32) -> usize {
    let tokens = retain_tokens.clamp(MIN_RETAIN_TOKENS, MAX_RETAIN_TOKENS);
    tokens as usize * 4
}

/// 保留 token 预算的下限 / 上限 / 默认值。
pub const MIN_RETAIN_TOKENS: u32 = 10_000;
pub const MAX_RETAIN_TOKENS: u32 = 64_000;
pub const DEFAULT_RETAIN_TOKENS: u32 = 10_000;

/// 单条工具调用 / 工具输出的独立截断上限（与总预算解耦），
/// 防止一条巨大的命令输出（如 `cat`/`ls -R`）占满整个 tail 预算，
/// 挤掉更有价值的用户/助手消息。参考 pi agent 对 tool result 的独立
/// 截断策略（pi 固定 2000 字符）。
const MAX_TOOL_ITEM_CHARS: usize = 4_000;

/// tail 预算中优先分配给 message（用户/助手文字）的比例，剩余预算
/// 才给工具调用 / 工具输出。避免“最近全是工具调用”时完全挤掉
/// 最近的对话意图。
const MESSAGE_BUDGET_RATIO: f64 = 0.7;

struct TailSection {
    text: String,
    items: u32,
    chars: u32,
}

/// item 在 tail 中的语义分类，用于两阶段预算分配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemCategory {
    /// 用户/助手文字消息，携带对话意图，优先保留。
    Message,
    /// 工具调用 / 工具输出，需要原子配对且优先级低于 message。
    Tool,
}

/// 待选片段：已文本化内容 + 分类 + 在 `input` 中的原始下标（用于恢复顺序）。
struct Candidate {
    index: usize,
    category: ItemCategory,
    text: String,
}

/// 从 `input` 末尾向前选取最近的原始记录，跳过最后一项（压缩指令本身）。
///
/// 算法（参考 pi agent 的 turn 边界切点 + 单条截断策略）：
/// 1. 先按 `call_id` 建立 `function_call` 与 `function_call_output` 的配对关系。
/// 2. 逐条文本化，工具类 item 单条独立截断到 `MAX_TOOL_ITEM_CHARS`，与总预算无关。
/// 3. 预算分为两段：`MESSAGE_BUDGET_RATIO` 给 message，剩余给 tool。某一段用不完时
///    剩余额度自动让给另一段，避免“最近全是工具调用”时因 message 预算用不完而浪费。
/// 4. 若选中了 `function_call_output`，其配对的 `function_call` 强制一起保留
///    （即使超出常规预算），避免产生没有调用的孤儿工具输出。扫描结束仍未配对的
///    孤儿 output 直接丢弃（缺失调用上下文，保留也无法理解）。
/// 5. 按原始顺序重新排列已选中项后拼接。
fn build_tail_section(input: &[Value], budget_chars: usize) -> TailSection {
    let end = input.len().saturating_sub(1);
    let items = &input[..end];

    let call_index = index_function_calls_by_call_id(items);

    // 一次性为所有可文本化的 item 建立 `下标 -> Candidate` 映射，工具类 item 在此处
    // 就完成单条独立截断（与总预算解耦，防止一条巨大命令输出占满整个 tail）。
    let candidates_by_index: BTreeMap<usize, Candidate> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let (category, text) = textualize_item(item)?;
            let text = if category == ItemCategory::Tool {
                truncate_chars(&text, MAX_TOOL_ITEM_CHARS)
            } else {
                text
            };
            Some((
                index,
                Candidate {
                    index,
                    category,
                    text,
                },
            ))
        })
        .collect();

    let message_budget = (budget_chars as f64 * MESSAGE_BUDGET_RATIO) as usize;
    let mut selected_indices: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

    // 第一趟：只从 message 类候选中逆序选取，直到用满 message_budget。
    // 与工具类完全隔离，保证「最近全是工具调用」时仍能穿透工具调用一路回溯到
    // 更早的真实用户/助手消息，而不是被最近的工具调用挡住。
    let mut message_used = 0usize;
    for candidate in candidates_by_index
        .values()
        .rev()
        .filter(|candidate| candidate.category == ItemCategory::Message)
    {
        let cost = candidate.text.chars().count();
        if !selected_indices.is_empty() && message_used + cost > message_budget {
            break;
        }
        message_used += cost;
        selected_indices.insert(candidate.index);
    }

    // 第二趟：message 未用完的预算划给工具类；再逆序选取工具调用/输出，
    // 命中 function_call_output 时原子配对其 function_call（即使超预算)。
    let tool_budget = budget_chars.saturating_sub(message_used);
    let mut tool_used = 0usize;
    for candidate in candidates_by_index
        .values()
        .rev()
        .filter(|candidate| candidate.category == ItemCategory::Tool)
    {
        if selected_indices.contains(&candidate.index) {
            continue;
        }
        let cost = candidate.text.chars().count();
        if !selected_indices.is_empty() && tool_used + cost > tool_budget {
            continue;
        }
        tool_used += cost;
        selected_indices.insert(candidate.index);

        // 原子配对：选中 function_call_output 时，强制一起保留对应的 function_call，
        // 即使超预算（避免孤儿工具输出——没有调用上下文的返回值对模型无意义）。
        if let Some(call_id) = function_call_output_call_id(&input[candidate.index]) {
            if let Some(&call_idx) = call_index.get(call_id) {
                if selected_indices.insert(call_idx) {
                    if let Some(call_candidate) = candidates_by_index.get(&call_idx) {
                        tool_used += call_candidate.text.chars().count();
                    }
                }
            }
        }
    }

    // 丢弃未配对的孤儿 function_call_output（对应的 function_call 不在选中范围内）。
    selected_indices.retain(|&index| {
        if let Some(call_id) = function_call_output_call_id(&input[index]) {
            call_index.contains_key(call_id) && candidates_by_index.contains_key(&index)
        } else {
            true
        }
    });

    let mut used = 0usize;
    let mut pieces: Vec<&str> = Vec::new();
    for index in &selected_indices {
        if let Some(candidate) = candidates_by_index.get(index) {
            used += candidate.text.chars().count();
            pieces.push(candidate.text.as_str());
        }
    }

    TailSection {
        text: pieces.join("\n\n"),
        items: pieces.len() as u32,
        chars: used as u32,
    }
}

/// 预先扫描 `function_call` 项，建立 `call_id -> input 下标` 索引，用于原子配对。
fn index_function_calls_by_call_id(items: &[Value]) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            continue;
        }
        if let Some(call_id) = function_call_id(item) {
            map.insert(call_id, index);
        }
    }
    map
}

/// `function_call` 的配对键：优先 `call_id`，否则回退到 `id`
/// （与代码库其他处理 `function_call` 的规则保持一致，见 protocol_proxy.rs）。
fn function_call_id(item: &Value) -> Option<&str> {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// 若 item 是 `function_call_output`，返回其 `call_id`。
fn function_call_output_call_id(item: &Value) -> Option<&str> {
    if item.get("type").and_then(Value::as_str) != Some("function_call_output") {
        return None;
    }
    item.get("call_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// 把单个 `input` item 转成可读文本并标注分类；无有效内容返回 None。
fn textualize_item(item: &Value) -> Option<(ItemCategory, String)> {
    let kind = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    match kind {
        "message" => {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
            let text = item_text(item);
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            Some((ItemCategory::Message, format!("[{role}]\n{text}")))
        }
        "function_call" | "custom_tool_call" | "local_shell_call" | "tool_call" => {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
            let args = item
                .get("arguments")
                .map(stringify_arguments)
                .unwrap_or_default();
            let args = args.trim();
            let text = if args.is_empty() {
                format!("[tool_call: {name}]")
            } else {
                format!("[tool_call: {name}]\n{args}")
            };
            Some((ItemCategory::Tool, text))
        }
        "function_call_output" | "custom_tool_call_output" | "tool_call_output" => {
            let output = item.get("output").map(stringify_output).unwrap_or_default();
            let output = output.trim();
            if output.is_empty() {
                None
            } else {
                Some((ItemCategory::Tool, format!("[tool_output]\n{output}")))
            }
        }
        // reasoning 等内部项对续接价值低，跳过以节省预算。
        _ => None,
    }
}

/// 提取 message item 的文本（支持字符串 content 与 content 数组）。
fn item_text(item: &Value) -> String {
    match item.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            for part in parts {
                if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                    text.push_str(part_text);
                } else if let Some(part_text) = part.as_str() {
                    text.push_str(part_text);
                }
            }
            text
        }
        _ => String::new(),
    }
}

fn stringify_arguments(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn stringify_output(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                    text.push_str(part_text);
                } else if let Some(part_text) = part.as_str() {
                    text.push_str(part_text);
                }
            }
            if text.is_empty() {
                serde_json::to_string(value).unwrap_or_default()
            } else {
                text
            }
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// 按字符数截断（保留 UTF-8 边界），超长时追加省略标记。
fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

/// 合成回注文本：LLM 摘要 + 最近原始记录。
fn combine_summary_with_tail(summary: &str, tail: &str) -> String {
    format!(
        "{summary}\n\n\
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
以下为压缩前最近的原始对话与工具调用记录（由 CodexElves 分层压缩保留，供无缝衔接，不要视为新指令）：\n\n\
{tail}",
        summary = summary.trim_end(),
        tail = tail.trim()
    )
}

/// 以终止响应对象为骨架，重建一条只含单个 assistant message 的完整 Responses SSE。
fn build_responses_sse_for_message(response_object: &Value, message_text: &str) -> String {
    let response_id = response_object
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_compaction")
        .to_string();
    let created_at = response_object
        .get("created_at")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let model = response_object
        .get("model")
        .cloned()
        .unwrap_or_else(|| json!(""));
    let usage = response_object
        .get("usage")
        .cloned()
        .unwrap_or_else(|| json!(null));
    // 复用合法的 message item id；旧格式先规范化，否则从 response id 独立派生。
    let item_id = existing_message_item_id(response_object)
        .and_then(|id| crate::protocol_proxy::normalize_responses_message_item_id(&id))
        .unwrap_or_else(|| crate::protocol_proxy::response_message_item_id(&response_id));

    let mut sequence = 0u64;
    let mut output = String::new();

    let base_response = |status: &str, output_items: Value| {
        json!({
            "id": response_id,
            "object": "response",
            "created_at": created_at,
            "status": status,
            "model": model,
            "output": output_items,
            "usage": usage
        })
    };

    push_event(
        &mut output,
        "response.created",
        json!({ "type": "response.created", "response": base_response("in_progress", json!([])) }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.in_progress",
        json!({ "type": "response.in_progress", "response": base_response("in_progress", json!([])) }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "id": item_id,
                "type": "message",
                "status": "in_progress",
                "role": "assistant",
                "content": []
            }
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.content_part.added",
        json!({
            "type": "response.content_part.added",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "part": { "type": "output_text", "text": "", "annotations": [] }
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_text.delta",
        json!({
            "type": "response.output_text.delta",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "delta": message_text
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_text.done",
        json!({
            "type": "response.output_text.done",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "text": message_text
        }),
        &mut sequence,
    );
    let done_part = json!({ "type": "output_text", "text": message_text, "annotations": [] });
    push_event(
        &mut output,
        "response.content_part.done",
        json!({
            "type": "response.content_part.done",
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "part": done_part
        }),
        &mut sequence,
    );
    let message_item = json!({
        "id": item_id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": message_text, "annotations": [] }]
    });
    push_event(
        &mut output,
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": message_item
        }),
        &mut sequence,
    );
    // response.completed：以原终止响应为骨架，替换 output/status，保留 instructions/tools 等字段。
    let mut completed = response_object.clone();
    if let Some(object) = completed.as_object_mut() {
        object.insert("status".to_string(), json!("completed"));
        object.insert("output".to_string(), json!([message_item]));
    }
    push_event(
        &mut output,
        "response.completed",
        json!({ "type": "response.completed", "response": completed }),
        &mut sequence,
    );
    output.push_str("data: [DONE]\n\n");
    output
}

fn existing_message_item_id(response_object: &Value) -> Option<String> {
    let output = response_object.get("output")?.as_array()?;
    for item in output {
        if item.get("type").and_then(Value::as_str) == Some("message") {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// 从终止响应对象中提取 assistant message 的纯文本。
fn extract_message_text(response_object: &Value) -> Option<String> {
    let output = response_object.get("output")?.as_array()?;
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let mut text = String::new();
        if let Some(parts) = item.get("content").and_then(Value::as_array) {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("output_text") | Some("text") | None => {
                        if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                            text.push_str(part_text);
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(direct) = item.get("content").and_then(Value::as_str) {
            text.push_str(direct);
        }
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// 写入一个带 `sequence_number` 的 SSE 事件。
fn push_event(output: &mut String, event: &str, mut data: Value, sequence: &mut u64) {
    if let Some(object) = data.as_object_mut() {
        object
            .entry("sequence_number".to_string())
            .or_insert_with(|| json!(*sequence));
        *sequence += 1;
    }
    output.push_str("event: ");
    output.push_str(event);
    output.push_str("\ndata: ");
    output.push_str(&serde_json::to_string(&data).unwrap_or_default());
    output.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compaction_prompt_item() -> Value {
        json!({
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": format!("{COMPACTION_PROMPT_PREFIX}. Create a handoff summary.\n")
            }]
        })
    }

    fn user_message(text: &str) -> Value {
        json!({
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": text }]
        })
    }

    fn assistant_message(text: &str) -> Value {
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": text }]
        })
    }

    /// 上游返回的压缩摘要 SSE（单条 assistant message，completed）。
    fn summary_sse(summary: &str) -> String {
        let response = json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "gpt-5.6-sol",
            "output": [{
                "id": "resp_test_msg",
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": summary, "annotations": [] }]
            }],
            "usage": { "input_tokens": 10, "output_tokens": 5, "total_tokens": 15 }
        });
        format!(
            "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::to_string(&json!({
                "type": "response.completed",
                "response": response
            }))
            .unwrap()
        )
    }

    #[test]
    fn detects_compaction_request_by_trailing_instruction() {
        let request = json!({
            "input": [user_message("hi"), compaction_prompt_item()]
        });
        assert!(is_compaction_request(Some(&request)));
    }

    #[test]
    fn ignores_normal_request() {
        let request = json!({ "input": [user_message("just a normal question")] });
        assert!(!is_compaction_request(Some(&request)));
    }

    #[test]
    fn custom_prompt_replaces_last_input_item_text() {
        let request = json!({
            "input": [user_message("hi"), compaction_prompt_item()]
        });
        let rewritten = apply_custom_compaction_prompt(&request, "自定义压缩提示词");
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(input.len(), 2, "不应增减 item 数量");
        assert_eq!(item_text(&input[1]), "自定义压缩提示词");
        assert_eq!(input[1]["role"], "user");
        // 未受影响的其他 item 保持不变。
        assert_eq!(item_text(&input[0]), "hi");
    }

    #[test]
    fn empty_custom_prompt_keeps_default_codex_prompt() {
        let request = json!({
            "input": [user_message("hi"), compaction_prompt_item()]
        });
        let rewritten = apply_custom_compaction_prompt(&request, "   ");
        assert_eq!(
            rewritten, request,
            "空自定义提示词应原样返回（继续用 codex 默认提示词）"
        );
    }

    #[test]
    fn custom_prompt_ignored_for_non_compaction_request() {
        let request = json!({ "input": [user_message("just a normal question")] });
        let rewritten = apply_custom_compaction_prompt(&request, "自定义提示词");
        assert_eq!(rewritten, request, "非压缩请求不应被改写");
    }

    #[test]
    fn disabled_returns_unchanged() {
        let request = json!({
            "input": [user_message("hi"), assistant_message("ok"), compaction_prompt_item()]
        });
        let sse = summary_sse("SUMMARY");
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            false,
            DEFAULT_RETAIN_TOKENS,
            sse.clone(),
        );
        assert!(!result.triggered);
        assert_eq!(result.sse_text, sse);
    }

    #[test]
    fn injects_summary_and_recent_tail() {
        let request = json!({
            "input": [
                user_message("old question that should be dropped"),
                assistant_message("old answer"),
                json!({
                    "type": "function_call",
                    "name": "exec_command",
                    "arguments": "{\"cmd\":\"ls\"}"
                }),
                json!({
                    "type": "function_call_output",
                    "output": "file_a\nfile_b"
                }),
                assistant_message("most recent assistant note"),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            summary_sse("SUMMARY-BODY"),
        );
        assert!(result.triggered);
        assert!(result.retained_items >= 1);

        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("rebuilt sse has terminal response");
        let text = extract_message_text(&response).expect("rebuilt message text");
        assert!(text.contains("SUMMARY-BODY"), "keeps summary");
        assert!(
            text.contains("most recent assistant note"),
            "keeps recent tail"
        );
        assert!(text.contains("exec_command"), "keeps recent tool call");
        assert!(text.contains("file_b"), "keeps recent tool output");
        let output = response.get("output").and_then(Value::as_array).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["role"], "assistant");
        assert_eq!(output[0]["id"], "msg_test");
    }

    #[test]
    fn tail_respects_char_budget() {
        let big = "x".repeat(10_000);
        let request = json!({
            "input": [user_message(&big), assistant_message(&big), compaction_prompt_item()]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        assert!(result.retained_chars <= MIN_RETAIN_TOKENS * 4 + 64);
    }

    fn function_call_item(call_id: &str, name: &str) -> Value {
        json!({
            "type": "function_call",
            "call_id": call_id,
            "name": name,
            "arguments": "{}"
        })
    }

    fn function_call_output_item(call_id: &str, output: &str) -> Value {
        json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": output
        })
    }

    /// 问题1：会话总量远低于保留预算时，应将全部原始记录保留，不报错也不硬填充。
    #[test]
    fn session_smaller_than_budget_keeps_everything() {
        let request = json!({
            "input": [
                user_message("hi"),
                assistant_message("hello"),
                user_message("how are you"),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        // 三条真实记录全部保留，远未用完预算，不报错也不会强行凑齐。
        assert_eq!(result.retained_items, 3);
        assert!(result.retained_chars < DEFAULT_RETAIN_TOKENS * 4 / 10);
    }

    /// 问题2：单条超大工具输出不得挤掉其他记录（与总预算解耦）。
    #[test]
    fn oversized_tool_output_does_not_starve_other_items() {
        let huge_output = "y".repeat(50_000);
        let request = json!({
            "input": [
                user_message("earlier user intent that must survive"),
                assistant_message("earlier assistant note that must survive"),
                function_call_item("call_1", "exec_command"),
                function_call_output_item("call_1", &huge_output),
                assistant_message("most recent assistant note"),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        let response =
            crate::continue_thinking::extract_terminal_response_object(&result.sse_text).unwrap();
        let text = extract_message_text(&response).unwrap();
        // 巨大工具输出必须被单条截断，不能占满预算挤掉更早的消息。
        assert!(
            text.contains("earlier user intent that must survive"),
            "oversized tool output must not evict earlier message"
        );
        assert!(text.contains("most recent assistant note"));
        // 巨大输出本身仍在（被截断），但不应包含完整 50000 字符。
        assert!(text.contains("tool_output"));
        assert!(text.len() < huge_output.len());
    }

    /// 问题2：选中 function_call_output 时必须原子保留对应 function_call，不产生孤儿项。
    #[test]
    fn tool_call_and_output_are_kept_atomically() {
        let request = json!({
            "input": [
                user_message("start"),
                function_call_item("call_1", "exec_command"),
                function_call_output_item("call_1", "output one"),
                function_call_item("call_2", "exec_command"),
                function_call_output_item("call_2", "output two"),
                compaction_prompt_item()
            ]
        });
        // 极小预算，只够选中最后一条 output，验证它不会单独出现。
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        let response =
            crate::continue_thinking::extract_terminal_response_object(&result.sse_text).unwrap();
        let text = extract_message_text(&response).unwrap();
        if text.contains("output two") {
            assert!(
                text.contains("exec_command"),
                "tool_call_output must never appear without its tool_call"
            );
        }
    }

    /// 问题2（核心场景）：最近大量连续工具调用比 message 预算大得多时，
    /// 仍应能穿透这些工具调用回溯到更早的真实用户/助手消息，而不是被工具
    /// 调用完全挤满。
    #[test]
    fn message_survives_when_recent_history_is_all_tool_calls() {
        let mut input = vec![
            user_message("the actual task the user cares about"),
            assistant_message("acknowledged, starting work"),
        ];
        // 模拟最近 20 轮工具调用，每条输出都接近单条截断上限，总量远超过
        // 整个预算，模拟你提出的“最近 10000 token 都是工具调用”场景。
        for i in 0..20 {
            let call_id = format!("call_{i}");
            input.push(function_call_item(&call_id, "exec_command"));
            input.push(function_call_output_item(&call_id, &"z".repeat(3_000)));
        }
        input.push(compaction_prompt_item());
        let request = json!({ "input": input });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("S"),
        );
        assert!(result.triggered);
        let response =
            crate::continue_thinking::extract_terminal_response_object(&result.sse_text).unwrap();
        let text = extract_message_text(&response).unwrap();
        assert!(
            text.contains("the actual task the user cares about"),
            "message budget must reserve room for the earlier real message even when \
recent history is dominated by tool calls"
        );
    }

    #[test]
    fn non_completed_status_unchanged() {
        let request = json!({
            "input": [user_message("hi"), assistant_message("ok"), compaction_prompt_item()]
        });
        let sse = "event: response.incomplete\ndata: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"output\":[]}}\n\n".to_string();
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            sse.clone(),
        );
        assert!(!result.triggered);
        assert_eq!(result.sse_text, sse);
    }
}
