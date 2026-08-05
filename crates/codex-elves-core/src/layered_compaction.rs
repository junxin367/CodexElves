//! 上下文压缩：处理传统 CONTEXT CHECKPOINT COMPACTION，以及不支持原生 Remote
//! Compaction V2 的模型/协议降级摘要；把较早历史摘要与最近原始 Responses items
//! 分开保存，避免纯摘要压缩导致角色、工具配对和短回复指代信息丢失。
//!
//! 机制（基于 Codex `core/src/compact.rs` 与 `compact_remote_v2.rs` 源码验证）：
//! - Codex 压缩走普通 `/responses` 请求，`input` 最后一项是固定的压缩指令 user 消息。
//! - 上游返回一条 assistant message 作为摘要。
//! - 本地压缩在请求前摘除“上一条可见 assistant 指代锚点 → 最后一条真实 user →
//!   当前尾部”，只把更早历史发给摘要模型。
//! - v3 载荷保存摘要和原始尾部；下一轮恢复时删除 Codex 自己保留的重复 user /
//!   developer / system，再按原顺序插回完整尾部。
//! - 尾部仍是 assistant 且目标 Claude 不支持 prefill 时，代理返回空 output 的 completed
//!   响应，结束自动续接并等待真实 user。
//!
//! 该转换只作用于 Responses 协议 SSE 文本（Chat/Anthropic 上游已在上层转换为 Responses SSE），
//! 因此与上游协议无关。

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Codex 压缩指令的固定前缀（取自 codex 二进制 `core/src/tasks/compact.rs`）。
pub const COMPACTION_PROMPT_PREFIX: &str = "You are performing a CONTEXT CHECKPOINT COMPACTION";

/// CodexElves 默认的 LLM 摘要压缩提示词。
///
/// 管理器以空字符串表示“使用项目默认提示词”，HTTP 与 WebSocket 两条路径都必须通过
/// [`effective_compaction_prompt`] 解析该语义。
pub const DEFAULT_COMPACTION_PROMPT: &str = include_str!("../assets/default-compaction-prompt.md");

/// 解析实际使用的摘要压缩提示词。
///
/// 非空自定义值优先；空值表示使用 CodexElves 默认提示词。
pub fn effective_compaction_prompt(prompt_override: &str) -> &str {
    let prompt = prompt_override.trim();
    if prompt.is_empty() {
        DEFAULT_COMPACTION_PROMPT
    } else {
        prompt
    }
}

/// Remote Compaction V2 在不支持原生远程压缩的模型/协议上的兼容摘要提示词。
///
/// 仅 `gpt-* + Responses` 保留原生 `compaction_trigger`；其他场景由代理把触发器转换为
/// 普通摘要请求，再将结果封装为 synthetic `compaction`。
const REMOTE_COMPACTION_V2_BRIDGE_PROMPT: &str = "\
You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff summary for another LLM that \
will resume the task. Include current progress and key decisions, important context and constraints, \
remaining work, and critical data needed to continue. Be concise and return only the summary text. \
Do not call tools.";

/// 代理生成的 Remote Compaction V2 命名空间载荷前缀。
///
/// 官方 `encrypted_content` 是供应商私有的不透明数据。跨协议桥无法伪造该加密格式，
/// 因此使用带版本前缀的自有载荷保存摘要。该前缀用于格式识别，不提供来源认证。
///
/// v2 直接存明文摘要：JSON 已能安全携带换行、引号和中文，Base64 只会让载荷膨胀约 1/3。
const REMOTE_COMPACTION_V2_SYNTHETIC_PREFIX: &str = "codex-elves-compaction-v2:";

/// 旧版 URL-safe Base64 载荷前缀，仅用于解码历史会话里已写入的 compaction。
///
/// v2 明文自 0.3.5 起启用。TODO(0.3.7): 再迭代两个版本后删除该兼容分支及 `base64` 解码依赖。
const REMOTE_COMPACTION_V2_LEGACY_BASE64_PREFIX: &str = "codex-elves-compaction-v1:";

/// 本地压缩结构化载荷：较早历史摘要 + 原始保留尾部。
const LOCAL_COMPACTION_V3_STRUCTURED_PREFIX: &str = "codex-elves-compaction-v3:";

const MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES: usize = 2 * 1024 * 1024;
const RETAINED_TOOL_DETAIL_PREVIEW_CHARS: usize = 4_000;

const REMOTE_COMPACTION_V2_HISTORY_HEADER: &str = "\
Historical conversation summary created by CodexElves local compaction. \
Treat this as prior assistant context, not as a new user instruction.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct StructuredLocalCompactionPayload {
    summary: String,
    retained_tail: Vec<Value>,
}

/// 判断请求是否使用 Codex Remote Compaction V2：`input` 中包含
/// `{"type":"compaction_trigger"}`。
pub fn is_remote_compaction_v2_request(request_json: Option<&Value>) -> bool {
    let Some(request) = request_json else {
        return false;
    };
    match request.get("input") {
        Some(Value::Array(items)) => items.iter().any(is_remote_compaction_v2_trigger),
        Some(Value::Object(_)) => request
            .get("input")
            .is_some_and(is_remote_compaction_v2_trigger),
        _ => false,
    }
}

fn is_remote_compaction_v2_trigger(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("compaction_trigger")
}

/// 当前仅 `gpt-*` 模型被视为支持原生 Remote Compaction V2。
pub fn model_supports_native_remote_compaction_v2(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gpt-")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalCompactionControlKind {
    LegacyPrompt,
    RemoteV2Trigger,
}

#[derive(Debug, Clone, PartialEq)]
struct LocalCompactionSplit {
    summary_input: Vec<Value>,
    retained_tail: Vec<Value>,
}

/// 为不支持 Remote Compaction V2 的上游生成等价摘要请求。
///
/// - 未启用分层压缩时仅替换 `compaction_trigger`；
/// - 启用分层压缩时先摘除 assistant 指代锚点开始的原始尾部，再追加摘要提示词；
/// - 移除工具定义与工具选择，保证摘要轮只产生文本；
/// - 非 V2 请求原样返回。
pub fn prepare_remote_compaction_v2_bridge_request(request_json: &Value) -> Value {
    prepare_remote_compaction_v2_bridge_request_with_prompt(request_json, None)
}

/// 使用可选的分层压缩自定义提示词生成 V2 降级摘要请求。
///
/// `prompt_override` 为空时使用传统压缩提示词；只有分层压缩开启时调用方才应传入
/// 用户配置的自定义提示词。
pub fn prepare_remote_compaction_v2_bridge_request_with_prompt(
    request_json: &Value,
    prompt_override: Option<&str>,
) -> Value {
    if !is_remote_compaction_v2_request(Some(request_json)) {
        return request_json.clone();
    }

    let prompt = prompt_override
        .map(effective_compaction_prompt)
        .unwrap_or(REMOTE_COMPACTION_V2_BRIDGE_PROMPT);
    let mut request = expand_synthetic_local_compaction_request(request_json);
    let Some(object) = request.as_object_mut() else {
        return request_json.clone();
    };
    if let Some(input) = object.get_mut("input") {
        match input {
            Value::Array(items) if prompt_override.is_some() => {
                let split = split_local_compaction_input(
                    items,
                    LocalCompactionControlKind::RemoteV2Trigger,
                );
                *items = split.summary_input;
                items.push(remote_compaction_v2_bridge_prompt_item(prompt));
            }
            Value::Array(items) => {
                for item in items {
                    if is_remote_compaction_v2_trigger(item) {
                        *item = remote_compaction_v2_bridge_prompt_item(prompt);
                    }
                }
            }
            Value::Object(_) if is_remote_compaction_v2_trigger(input) => {
                *input = remote_compaction_v2_bridge_prompt_item(prompt);
            }
            _ => {}
        }
    }
    for key in ["tools", "tool_choice", "parallel_tool_calls"] {
        object.remove(key);
    }
    request
}

fn remote_compaction_v2_bridge_prompt_item(prompt: &str) -> Value {
    json!({
        "type": "message",
        "role": "user",
        "content": [{
            "type": "input_text",
            "text": prompt
        }]
    })
}

fn split_local_compaction_input(
    input: &[Value],
    control_kind: LocalCompactionControlKind,
) -> LocalCompactionSplit {
    let conversation = input
        .iter()
        .enumerate()
        .filter(|(index, item)| {
            !is_local_compaction_control_item(input, *index, item, control_kind)
                && item.get("type").and_then(Value::as_str) != Some("reasoning")
        })
        .map(|(_, item)| item.clone())
        .collect::<Vec<_>>();
    let Some(last_user) = conversation.iter().rposition(is_real_user_message) else {
        return LocalCompactionSplit {
            summary_input: conversation,
            retained_tail: Vec::new(),
        };
    };
    let retained_start = conversation[..last_user]
        .iter()
        .rposition(is_visible_assistant_message)
        .unwrap_or(last_user);
    LocalCompactionSplit {
        summary_input: conversation[..retained_start].to_vec(),
        retained_tail: conversation[retained_start..].to_vec(),
    }
}

fn is_local_compaction_control_item(
    input: &[Value],
    index: usize,
    item: &Value,
    control_kind: LocalCompactionControlKind,
) -> bool {
    match control_kind {
        LocalCompactionControlKind::LegacyPrompt => {
            index + 1 == input.len()
                && item.get("role").and_then(Value::as_str) == Some("user")
                && item_text(item)
                    .trim_start()
                    .starts_with(COMPACTION_PROMPT_PREFIX)
        }
        LocalCompactionControlKind::RemoteV2Trigger => is_remote_compaction_v2_trigger(item),
    }
}

fn is_visible_assistant_message(item: &Value) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
        == "message"
        && item.get("role").and_then(Value::as_str) == Some("assistant")
        && !item_text(item).trim().is_empty()
}

/// 将本项目生成的合成 compaction item 恢复为可发送给普通模型的 assistant 摘要文本。
///
/// v3 的原始尾部由 [`expand_synthetic_local_compaction_request`] 单独恢复；本入口只返回摘要，
/// 同时继续兼容 v2 明文与 v1 Base64 历史载荷。
pub fn synthetic_remote_compaction_history_text(item: &Value) -> Option<String> {
    let payload = synthetic_local_compaction_payload(item)?;
    historical_compaction_summary_text(&payload.summary)
}

/// 展开本地合成压缩历史：
///
/// - v1/v2：恢复为一条 assistant 摘要；
/// - v3：恢复为 assistant 摘要 + 原始 retained_tail；
/// - Codex 已原样保留的 user/developer/system 会先去重，再由 retained_tail 放回正确位置。
///
/// 真实 OpenAI `encrypted_content` 没有本项目前缀，不会被误解码。
pub fn expand_synthetic_local_compaction_request(request_json: &Value) -> Value {
    let mut request = request_json.clone();
    let Some(input) = request
        .as_object_mut()
        .and_then(|object| object.get_mut("input"))
        .and_then(Value::as_array_mut)
    else {
        return request;
    };
    *input = expand_synthetic_local_compaction_items(input);
    request
}

/// 判断请求历史是否含 CodexElves 生成的本地合成压缩载荷。
pub fn contains_synthetic_local_compaction(request_json: &Value) -> bool {
    request_json
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|input| {
            input
                .iter()
                .any(|item| synthetic_local_compaction_payload(item).is_some())
        })
}

/// Claude 家族禁止 assistant prefill。若本地合成压缩恢复后的最后一个有效协议项仍属于
/// assistant 侧，则本次自动续接必须结束并等待真实 user/tool result。
pub fn local_compaction_requires_real_user(request_json: &Value, model: &str) -> bool {
    if !model.trim().to_ascii_lowercase().contains("claude")
        || !contains_synthetic_local_compaction(request_json)
    {
        return false;
    }
    let expanded = expand_synthetic_local_compaction_request(request_json);
    expanded
        .get("input")
        .and_then(Value::as_array)
        .and_then(|input| input.iter().rev().find_map(effective_history_side))
        == Some(EffectiveHistorySide::Assistant)
}

fn expand_synthetic_local_compaction_items(input: &[Value]) -> Vec<Value> {
    let mut expanded = Vec::with_capacity(input.len());
    for item in input {
        let Some(payload) = synthetic_local_compaction_payload(item) else {
            expanded.push(item.clone());
            continue;
        };
        remove_codex_retained_duplicates(&mut expanded, &payload.retained_tail);
        if let Some(summary) = historical_compaction_summary_item(&payload.summary) {
            expanded.push(summary);
        }
        expanded.extend(payload.retained_tail);
    }
    expanded
}

fn synthetic_local_compaction_payload(item: &Value) -> Option<StructuredLocalCompactionPayload> {
    match item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
    {
        "compaction" => {
            let encrypted_content = item.get("encrypted_content")?.as_str()?;
            if let Some(payload) = structured_local_compaction_payload(encrypted_content) {
                return Some(payload);
            }
            synthetic_remote_compaction_summary(encrypted_content).map(|summary| {
                StructuredLocalCompactionPayload {
                    summary,
                    retained_tail: Vec::new(),
                }
            })
        }
        "message" if item.get("role").and_then(Value::as_str) == Some("user") => {
            structured_local_compaction_payload(&item_text(item))
        }
        _ => None,
    }
}

fn structured_local_compaction_payload(text: &str) -> Option<StructuredLocalCompactionPayload> {
    let marker = text.find(LOCAL_COMPACTION_V3_STRUCTURED_PREFIX)?;
    let payload = text.get(marker + LOCAL_COMPACTION_V3_STRUCTURED_PREFIX.len()..)?;
    if payload.len() > MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES {
        return None;
    }
    serde_json::from_str(payload.trim()).ok()
}

fn historical_compaction_summary_text(summary: &str) -> Option<String> {
    let summary = summary.trim();
    if summary.is_empty() {
        return None;
    }
    Some(format!(
        "{REMOTE_COMPACTION_V2_HISTORY_HEADER}\n\n{summary}"
    ))
}

fn historical_compaction_summary_item(summary: &str) -> Option<Value> {
    historical_compaction_summary_text(summary).map(|text| {
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text
            }]
        })
    })
}

fn remove_codex_retained_duplicates(output: &mut Vec<Value>, retained_tail: &[Value]) {
    for retained in retained_tail
        .iter()
        .filter(|item| is_codex_retained_message(item))
    {
        let Some(index) = output
            .iter()
            .rposition(|candidate| retained_message_matches(candidate, retained))
        else {
            continue;
        };
        // Anthropic 需要首条有效消息为 user。若这是压缩前唯一可用的 user，则保留这一份
        // 原生副本；v3 尾部仍会追加原始 user，避免伪造传输消息。
        if is_real_user_message(retained)
            && !output
                .iter()
                .enumerate()
                .any(|(candidate_index, candidate)| {
                    candidate_index != index && is_real_user_message(candidate)
                })
        {
            continue;
        }
        output.remove(index);
    }
}

fn is_codex_retained_message(item: &Value) -> bool {
    if item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
        != "message"
    {
        return false;
    }
    matches!(
        item.get("role").and_then(Value::as_str),
        Some("user" | "developer" | "system" | "latest_reminder")
    )
}

fn is_real_user_message(item: &Value) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
        == "message"
        && item.get("role").and_then(Value::as_str) == Some("user")
        && !item_text(item)
            .trim_start()
            .starts_with(COMPACTION_PROMPT_PREFIX)
}

fn retained_message_matches(candidate: &Value, retained: &Value) -> bool {
    if candidate
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
        != "message"
        || candidate.get("role").and_then(Value::as_str)
            != retained.get("role").and_then(Value::as_str)
    {
        return false;
    }
    let candidate_id = candidate
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let retained_id = retained
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    if candidate_id.is_some() && candidate_id == retained_id {
        return true;
    }
    let candidate_turn = candidate
        .pointer("/internal_chat_message_metadata_passthrough/turn_id")
        .and_then(Value::as_str)
        .filter(|turn_id| !turn_id.is_empty());
    let retained_turn = retained
        .pointer("/internal_chat_message_metadata_passthrough/turn_id")
        .and_then(Value::as_str)
        .filter(|turn_id| !turn_id.is_empty());
    if candidate_turn.is_some() && candidate_turn == retained_turn {
        return true;
    }
    let candidate_text = item_text(candidate);
    !candidate_text.is_empty() && candidate_text == item_text(retained)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectiveHistorySide {
    User,
    Assistant,
}

fn effective_history_side(item: &Value) -> Option<EffectiveHistorySide> {
    match item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
    {
        "message" => match item.get("role").and_then(Value::as_str) {
            Some("assistant") => Some(EffectiveHistorySide::Assistant),
            Some("user" | "latest_reminder") => Some(EffectiveHistorySide::User),
            _ => None,
        },
        "function_call" | "custom_tool_call" | "local_shell_call" | "tool_call" | "reasoning" => {
            Some(EffectiveHistorySide::Assistant)
        }
        "function_call_output"
        | "custom_tool_call_output"
        | "local_shell_call_output"
        | "tool_call_output"
        | "tool_result"
        | "tool_search_output" => Some(EffectiveHistorySide::User),
        _ => None,
    }
}

/// 解出合成 compaction 的摘要正文：优先 v2 明文，其次回退 v1 Base64。
fn synthetic_remote_compaction_summary(encrypted_content: &str) -> Option<String> {
    if let Some(payload) = encrypted_content.strip_prefix(REMOTE_COMPACTION_V2_SYNTHETIC_PREFIX) {
        if payload.len() > MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES {
            return None;
        }
        return Some(payload.to_string());
    }
    // TODO(0.3.7): 兼容期结束后删除该 Base64 分支。
    let payload = encrypted_content.strip_prefix(REMOTE_COMPACTION_V2_LEGACY_BASE64_PREFIX)?;
    if payload.len() > MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES.saturating_mul(4) / 3 + 4 {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    if decoded.len() > MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES {
        return None;
    }
    String::from_utf8(decoded).ok()
}

/// 把非 Responses 上游生成的普通 Responses 响应改写为 Remote Compaction V2 响应。
///
/// 成功时 `output` 中只保留一个 `compaction` item，满足 Codex V2 collector 的约束。
pub fn rewrite_remote_compaction_v2_response(
    request_json: &Value,
    response_object: &Value,
) -> Option<Value> {
    rewrite_remote_compaction_v2_response_with_layered_compaction(
        request_json,
        response_object,
        false,
        DEFAULT_RETAIN_TOKENS,
    )
    .map(|result| result.response)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LayeredCompactionStats {
    pub triggered: bool,
    pub retained_items: u32,
    pub retained_chars: u32,
}

#[derive(Debug, Clone)]
pub struct RemoteCompactionV2ResponseResult {
    pub response: Value,
    pub layered: LayeredCompactionStats,
}

struct StructuredCompactionBuild {
    encoded: String,
    stats: LayeredCompactionStats,
}

enum StructuredCompactionError {
    PayloadTooLarge { bytes: usize },
    Serialize(String),
}

impl StructuredCompactionError {
    fn code(&self) -> &'static str {
        match self {
            Self::PayloadTooLarge { .. } => "local_compaction_payload_too_large",
            Self::Serialize(_) => "local_compaction_payload_serialize_failed",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::PayloadTooLarge { bytes } => format!(
                "Local compaction structured payload is {bytes} bytes, exceeding the \
                 {MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES}-byte limit."
            ),
            Self::Serialize(error) => {
                format!("Local compaction could not serialize the structured payload: {error}")
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RetainedDetailEncoding {
    ToolOutput,
    ToolSearchTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedDetailReplacementMode {
    Preview,
    MarkerOnly,
    MediaOnly,
}

#[derive(Debug, Clone, Copy)]
enum RetainedDetailPath {
    TopLevel(&'static str),
    Nested {
        parent: &'static str,
        child: &'static str,
    },
}

impl RetainedDetailPath {
    fn get<'a>(&self, item: &'a Value) -> Option<&'a Value> {
        match self {
            Self::TopLevel(field) => item.get(field),
            Self::Nested { parent, child } => item.get(parent)?.get(child),
        }
    }

    fn set(&self, item: &mut Value, replacement: Value) -> bool {
        match self {
            Self::TopLevel(field) => item
                .as_object_mut()
                .map(|object| object.insert((*field).to_string(), replacement))
                .is_some(),
            Self::Nested { parent, child } => item
                .as_object_mut()
                .and_then(|object| object.get_mut(*parent))
                .and_then(Value::as_object_mut)
                .map(|object| object.insert((*child).to_string(), replacement))
                .is_some(),
        }
    }
}

#[derive(Debug, Clone)]
struct RetainedDetailCandidate {
    item_index: usize,
    path: RetainedDetailPath,
    encoding: RetainedDetailEncoding,
    original: Value,
    estimated_tokens: u64,
}

/// 将保留尾部中的大块工具细节裁剪到目标预算附近。
///
/// 工具 item 本身、顺序、`id` / `call_id` / `name` 和调用结果配对全部保留：
/// - 文本结果保留头尾，裁掉中间并插入明确标记；
/// - 图片等结构化媒体结果替换为合法文本结果，避免留下损坏的 data URL；
/// - 所有调用参数原样保留；
/// - 用户 / assistant 原文不裁剪，因此 `retain_tokens` 是裁剪目标而不是失败阈值。
fn trim_retained_tail_details_to_target(retained_tail: &mut [Value], retain_tokens: u64) -> u64 {
    let mut estimated_tokens = estimate_json_array_tokens(retained_tail);
    if estimated_tokens <= retain_tokens {
        return estimated_tokens;
    }

    // 一旦尾部超过目标，先无条件清掉所有可裁剪工具细节中的媒体载荷。
    // 该阶段不能在达到 token 目标后提前结束，否则较小或靠后的 data URL 会原样残留。
    estimated_tokens = redact_retained_tail_media(retained_tail, estimated_tokens);
    if estimated_tokens <= retain_tokens {
        return estimated_tokens;
    }

    trim_retained_detail_phase(retained_tail, retain_tokens, estimated_tokens)
}

fn redact_retained_tail_media(retained_tail: &mut [Value], mut estimated_tokens: u64) -> u64 {
    for candidate in retained_detail_candidates(retained_tail) {
        if !retained_detail_contains_media(&candidate.original) {
            continue;
        }
        estimated_tokens = apply_retained_detail_candidate(
            retained_tail,
            &candidate,
            RetainedDetailReplacementMode::MediaOnly,
            estimated_tokens,
            true,
        );
    }
    estimated_tokens
}

fn trim_retained_detail_phase(
    retained_tail: &mut [Value],
    retain_tokens: u64,
    mut estimated_tokens: u64,
) -> u64 {
    let candidates = retained_detail_candidates(retained_tail);

    // 第一轮保留每个大字段的头尾预览；媒体字段直接降为文字标记。
    for candidate in &candidates {
        if estimated_tokens <= retain_tokens {
            return estimated_tokens;
        }
        estimated_tokens = apply_retained_detail_candidate(
            retained_tail,
            candidate,
            RetainedDetailReplacementMode::Preview,
            estimated_tokens,
            false,
        );
    }

    // 若多个工具字段的预览相加仍超预算，再从最大的字段开始缩成仅保留标记。
    for candidate in &candidates {
        if estimated_tokens <= retain_tokens {
            return estimated_tokens;
        }
        estimated_tokens = apply_retained_detail_candidate(
            retained_tail,
            candidate,
            RetainedDetailReplacementMode::MarkerOnly,
            estimated_tokens,
            false,
        );
    }

    estimated_tokens
}

fn retained_detail_candidates(retained_tail: &[Value]) -> Vec<RetainedDetailCandidate> {
    let mut candidates = Vec::new();
    for (item_index, item) in retained_tail.iter().enumerate() {
        let Some(kind) = item.get("type").and_then(Value::as_str) else {
            continue;
        };
        let mut details = Vec::new();
        match kind {
            "function_call_output"
            | "custom_tool_call_output"
            | "local_shell_call_output"
            | "tool_call_output" => details.push((
                RetainedDetailPath::TopLevel("output"),
                RetainedDetailEncoding::ToolOutput,
            )),
            "tool_result"
                if item.get("content").and_then(Value::as_object).is_some()
                    && item
                        .get("content")
                        .and_then(|content| content.get("content"))
                        .is_some() =>
            {
                details.push((
                    RetainedDetailPath::Nested {
                        parent: "content",
                        child: "content",
                    },
                    RetainedDetailEncoding::ToolOutput,
                ));
            }
            "tool_result" => details.push((
                RetainedDetailPath::TopLevel("content"),
                RetainedDetailEncoding::ToolOutput,
            )),
            "tool_search_output" => {
                if item.get("output").is_some() {
                    details.push((
                        RetainedDetailPath::TopLevel("output"),
                        RetainedDetailEncoding::ToolOutput,
                    ));
                }
                if item.get("tools").is_some() {
                    details.push((
                        RetainedDetailPath::TopLevel("tools"),
                        RetainedDetailEncoding::ToolSearchTools,
                    ));
                }
            }
            _ => {}
        }
        for (path, encoding) in details {
            let Some(original) = path.get(item).cloned() else {
                continue;
            };
            if retained_detail_is_empty(&original) {
                continue;
            }
            candidates.push(RetainedDetailCandidate {
                item_index,
                path,
                encoding,
                estimated_tokens: estimate_json_value_tokens(&original),
                original,
            });
        }
    }
    candidates.sort_by(|left, right| {
        right
            .estimated_tokens
            .cmp(&left.estimated_tokens)
            .then_with(|| left.item_index.cmp(&right.item_index))
    });
    candidates
}

fn retained_detail_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(object) => object.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

fn apply_retained_detail_candidate(
    retained_tail: &mut [Value],
    candidate: &RetainedDetailCandidate,
    mode: RetainedDetailReplacementMode,
    estimated_tokens: u64,
    allow_growth: bool,
) -> u64 {
    let Some(item) = retained_tail.get_mut(candidate.item_index) else {
        return estimated_tokens;
    };
    let Some(current) = candidate.path.get(item) else {
        return estimated_tokens;
    };
    let replacement = retained_detail_replacement(candidate, mode);
    let current_tokens = estimate_json_value_tokens(current);
    let replacement_tokens = estimate_json_value_tokens(&replacement);
    if replacement == *current || (!allow_growth && replacement_tokens >= current_tokens) {
        return estimated_tokens;
    }
    if !candidate.path.set(item, replacement) {
        return estimated_tokens;
    }
    estimated_tokens
        .saturating_sub(current_tokens)
        .saturating_add(replacement_tokens)
}

fn retained_detail_replacement(
    candidate: &RetainedDetailCandidate,
    mode: RetainedDetailReplacementMode,
) -> Value {
    let original_text = retained_detail_text(&candidate.original);
    let original_chars = original_text.chars().count();
    let original_tokens = candidate.estimated_tokens;
    let contains_media = retained_detail_contains_media(&candidate.original);
    let marker_only = mode == RetainedDetailReplacementMode::MarkerOnly;

    match candidate.encoding {
        RetainedDetailEncoding::ToolOutput => {
            let structured = !candidate.original.is_string();
            let label = if structured {
                "structured tool output"
            } else {
                "tool output"
            };
            if mode == RetainedDetailReplacementMode::MediaOnly && !contains_media {
                return candidate.original.clone();
            }
            if marker_only || contains_media || mode == RetainedDetailReplacementMode::MediaOnly {
                return Value::String(retained_detail_marker(
                    label,
                    original_tokens,
                    contains_media,
                ));
            }
            if original_chars <= RETAINED_TOOL_DETAIL_PREVIEW_CHARS {
                return candidate.original.clone();
            }
            Value::String(retained_detail_with_middle_removed(
                &original_text,
                RETAINED_TOOL_DETAIL_PREVIEW_CHARS,
                label,
                original_tokens,
                contains_media,
            ))
        }
        RetainedDetailEncoding::ToolSearchTools => {
            let mut replacement = candidate.original.clone();
            if contains_media {
                redact_tool_search_media_descriptions(&mut replacement);
            }
            if mode != RetainedDetailReplacementMode::MediaOnly {
                trim_tool_search_descriptions(&mut replacement, marker_only);
            }
            replacement
        }
    }
}

fn trim_tool_search_descriptions(value: &mut Value, marker_only: bool) {
    match value {
        Value::Array(items) => {
            for item in items {
                trim_tool_search_descriptions(item, marker_only);
            }
        }
        Value::Object(object) => {
            if let Some(Value::String(description)) = object.get_mut("description") {
                let original_tokens = estimate_text_tokens(description);
                let retained_chars = if marker_only {
                    0
                } else {
                    RETAINED_TOOL_DETAIL_PREVIEW_CHARS
                };
                *description = retained_detail_with_middle_removed(
                    description,
                    retained_chars,
                    "tool search description",
                    original_tokens,
                    false,
                );
            }
            // `parameters` / `input_schema` 是动态工具注册协议的一部分，必须原样保留。
            // 只沿嵌套工具列表继续寻找 namespace / function 自身的描述。
            if let Some(tools) = object.get_mut("tools") {
                trim_tool_search_descriptions(tools, marker_only);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn redact_tool_search_media_descriptions(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                redact_tool_search_media_descriptions(item);
            }
        }
        Value::Object(object) => {
            if let Some(Value::String(description)) = object.get_mut("description")
                && contains_media_data_url(description)
            {
                let original_tokens = estimate_text_tokens(description);
                *description =
                    retained_detail_marker("tool search description", original_tokens, true);
            }
            // 与普通描述裁剪一致，只遍历动态工具列表；schema 必须保持原样。
            if let Some(tools) = object.get_mut("tools") {
                redact_tool_search_media_descriptions(tools);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn retained_detail_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn retained_detail_contains_media(value: &Value) -> bool {
    match value {
        Value::String(text) => contains_media_data_url(text),
        Value::Array(items) => items.iter().any(retained_detail_contains_media),
        Value::Object(object) => {
            object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    matches!(
                        kind,
                        "input_image"
                            | "output_image"
                            | "input_audio"
                            | "output_audio"
                            | "input_video"
                            | "output_video"
                    )
                })
                || object.values().any(retained_detail_contains_media)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn contains_media_data_url(text: &str) -> bool {
    ["data:image/", "data:audio/", "data:video/"]
        .into_iter()
        .any(|needle| {
            text.as_bytes()
                .windows(needle.len())
                .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
        })
}

fn retained_detail_with_middle_removed(
    text: &str,
    retained_chars: usize,
    label: &str,
    original_tokens: u64,
    contains_media: bool,
) -> String {
    let original_chars = text.chars().count();
    let retained_chars = retained_chars.min(original_chars);
    if retained_chars == original_chars {
        return text.to_string();
    }
    let head_chars = retained_chars.div_ceil(2);
    let tail_chars = retained_chars / 2;
    let head = text.chars().take(head_chars).collect::<String>();
    let tail = text
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let marker = retained_detail_marker(label, original_tokens, contains_media);
    if retained_chars == 0 {
        marker
    } else {
        format!("{head}\n\n{marker}\n\n{tail}")
    }
}

fn retained_detail_marker(label: &str, original_tokens: u64, contains_media: bool) -> String {
    let kind = if contains_media {
        "media"
    } else {
        match label {
            "tool output" => "tool",
            "structured tool output" => "structured",
            "custom tool input" => "input",
            "tool search description" => "tool-desc",
            _ => "content",
        }
    };
    format!("<truncated:{kind};~{original_tokens}t>")
}

fn build_structured_local_compaction(
    request_json: &Value,
    summary: &str,
    retain_tokens: u32,
) -> Result<Option<StructuredCompactionBuild>, StructuredCompactionError> {
    let expanded = expand_synthetic_local_compaction_request(request_json);
    let input = expanded
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let control_kind = if is_remote_compaction_v2_request(Some(request_json)) {
        LocalCompactionControlKind::RemoteV2Trigger
    } else {
        LocalCompactionControlKind::LegacyPrompt
    };
    let split = split_local_compaction_input(&input, control_kind);
    if split.retained_tail.is_empty() {
        return Ok(None);
    }
    let retain_tokens = retain_tokens.clamp(MIN_RETAIN_TOKENS, MAX_RETAIN_TOKENS);
    let mut retained_tail = split.retained_tail;
    trim_retained_tail_details_to_target(&mut retained_tail, u64::from(retain_tokens));
    let retained_json = serde_json::to_string(&retained_tail)
        .map_err(|error| StructuredCompactionError::Serialize(error.to_string()))?;
    let retained_chars = u32::try_from(retained_json.chars().count()).unwrap_or(u32::MAX);
    let payload = StructuredLocalCompactionPayload {
        summary: summary.trim().to_string(),
        retained_tail,
    };
    let payload_json = serde_json::to_string(&payload)
        .map_err(|error| StructuredCompactionError::Serialize(error.to_string()))?;
    let encoded = format!("{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{payload_json}");
    if encoded.len() > MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES {
        return Err(StructuredCompactionError::PayloadTooLarge {
            bytes: encoded.len(),
        });
    }
    Ok(Some(StructuredCompactionBuild {
        encoded,
        stats: LayeredCompactionStats {
            triggered: true,
            retained_items: u32::try_from(payload.retained_tail.len()).unwrap_or(u32::MAX),
            retained_chars,
        },
    }))
}

/// 将普通摘要响应封装为 synthetic compaction。启用分层压缩时写入 v3 结构化尾部，
/// 未启用时继续写入 v2 纯文本摘要。
pub fn rewrite_remote_compaction_v2_response_with_layered_compaction(
    request_json: &Value,
    response_object: &Value,
    layered_enabled: bool,
    retain_tokens: u32,
) -> Option<RemoteCompactionV2ResponseResult> {
    if !is_remote_compaction_v2_request(Some(request_json)) {
        return None;
    }
    let status = response_object
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if status != "completed" {
        let (code, message) = match status {
            "incomplete" => (
                "remote_compaction_upstream_incomplete",
                "Remote Compaction V2 bridge received an incomplete upstream response.",
            ),
            "failed" => (
                "remote_compaction_upstream_failed",
                "Remote Compaction V2 bridge received a failed upstream response.",
            ),
            _ => (
                "remote_compaction_terminal_response_invalid",
                "Remote Compaction V2 bridge received no valid completed upstream response.",
            ),
        };
        return Some(RemoteCompactionV2ResponseResult {
            response: remote_compaction_v2_failure_response(
                request_json,
                Some(response_object),
                code,
                message,
            ),
            layered: LayeredCompactionStats::default(),
        });
    }
    let Some(summary) = extract_compaction_summary_text(response_object) else {
        return Some(RemoteCompactionV2ResponseResult {
            response: remote_compaction_v2_failure_response(
                request_json,
                Some(response_object),
                "remote_compaction_summary_missing",
                "Remote Compaction V2 bridge received no summary text from the upstream model.",
            ),
            layered: LayeredCompactionStats::default(),
        });
    };
    let (compaction_item, layered) = if layered_enabled {
        match build_structured_local_compaction(request_json, &summary, retain_tokens) {
            Ok(Some(build)) => (
                synthetic_structured_compaction_item(&build.encoded),
                build.stats,
            ),
            Ok(None) => (
                synthetic_remote_compaction_item(&summary),
                LayeredCompactionStats::default(),
            ),
            Err(error) => {
                return Some(RemoteCompactionV2ResponseResult {
                    response: remote_compaction_v2_failure_response(
                        request_json,
                        Some(response_object),
                        error.code(),
                        &error.message(),
                    ),
                    layered: LayeredCompactionStats::default(),
                });
            }
        }
    } else {
        (
            synthetic_remote_compaction_item(&summary),
            LayeredCompactionStats::default(),
        )
    };
    let mut response = response_object.clone();
    let object = response.as_object_mut()?;
    object.insert("status".to_string(), json!("completed"));
    object.insert("output".to_string(), json!([compaction_item]));
    Some(RemoteCompactionV2ResponseResult { response, layered })
}

/// SSE 版本的 Remote Compaction V2 响应改写。
pub fn rewrite_remote_compaction_v2_responses_sse(
    request_json: &Value,
    sse_text: String,
) -> Option<String> {
    rewrite_remote_compaction_v2_responses_sse_with_layered_compaction(
        request_json,
        false,
        DEFAULT_RETAIN_TOKENS,
        sse_text,
    )
    .map(|result| result.sse_text)
}

/// SSE 版本的 V2 synthetic compaction 封装，可选应用分层压缩 tail。
pub fn rewrite_remote_compaction_v2_responses_sse_with_layered_compaction(
    request_json: &Value,
    layered_enabled: bool,
    retain_tokens: u32,
    sse_text: String,
) -> Option<LayeredCompactionResult> {
    if !is_remote_compaction_v2_request(Some(request_json)) {
        return None;
    }
    let normalized_sse = sse_text.replace("\r\n", "\n").replace('\r', "\n");
    let rewritten = match extract_single_remote_compaction_v2_terminal_response(&normalized_sse) {
        Ok(response_object) => rewrite_remote_compaction_v2_response_with_layered_compaction(
            request_json,
            &response_object,
            layered_enabled,
            retain_tokens,
        )
        .expect("Remote Compaction V2 request must always produce a terminal bridge result"),
        Err(error) => RemoteCompactionV2ResponseResult {
            response: remote_compaction_v2_failure_response(
                request_json,
                None,
                error.code(),
                error.message(),
            ),
            layered: LayeredCompactionStats::default(),
        },
    };
    let rewritten_sse =
        if rewritten.response.get("status").and_then(Value::as_str) == Some("completed") {
            build_responses_sse_for_compaction(&rewritten.response)
        } else {
            build_responses_sse_for_remote_compaction_failure(&rewritten.response)
        };
    Some(LayeredCompactionResult {
        sse_text: rewritten_sse,
        triggered: rewritten.layered.triggered,
        retained_items: rewritten.layered.retained_items,
        retained_chars: rewritten.layered.retained_chars,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteCompactionV2SseTerminalError {
    MalformedEvent,
    UpstreamErrorEvent,
    MissingTerminal,
    MultipleTerminals,
    InvalidTerminal,
}

impl RemoteCompactionV2SseTerminalError {
    fn code(self) -> &'static str {
        match self {
            Self::MalformedEvent => "remote_compaction_sse_parse_failed",
            Self::UpstreamErrorEvent => "remote_compaction_upstream_failed",
            Self::MissingTerminal => "remote_compaction_terminal_response_missing",
            Self::MultipleTerminals => "remote_compaction_multiple_terminal_responses",
            Self::InvalidTerminal => "remote_compaction_terminal_response_invalid",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::MalformedEvent => {
                "Remote Compaction V2 bridge received a malformed upstream SSE event."
            }
            Self::UpstreamErrorEvent => {
                "Remote Compaction V2 bridge received an upstream error event."
            }
            Self::MissingTerminal => {
                "Remote Compaction V2 bridge received no terminal upstream response."
            }
            Self::MultipleTerminals => {
                "Remote Compaction V2 bridge received multiple terminal upstream responses."
            }
            Self::InvalidTerminal => {
                "Remote Compaction V2 bridge received an invalid terminal upstream response."
            }
        }
    }
}

fn extract_single_remote_compaction_v2_terminal_response(
    sse_text: &str,
) -> Result<Value, RemoteCompactionV2SseTerminalError> {
    let mut terminal_response = None;
    for block in sse_text.split("\n\n") {
        let data = block
            .lines()
            .filter_map(|line| line.trim().strip_prefix("data:"))
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let event = serde_json::from_str::<Value>(&data)
            .map_err(|_| RemoteCompactionV2SseTerminalError::MalformedEvent)?;
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type == "error" {
            return Err(RemoteCompactionV2SseTerminalError::UpstreamErrorEvent);
        }
        if !matches!(
            event_type,
            "response.completed" | "response.incomplete" | "response.failed"
        ) {
            continue;
        }
        if terminal_response.is_some() {
            return Err(RemoteCompactionV2SseTerminalError::MultipleTerminals);
        }
        let response = event
            .get("response")
            .filter(|response| response.is_object())
            .cloned()
            .ok_or(RemoteCompactionV2SseTerminalError::InvalidTerminal)?;
        let expected_status = match event_type {
            "response.completed" => "completed",
            "response.incomplete" => "incomplete",
            "response.failed" => "failed",
            _ => unreachable!("terminal event type was already matched"),
        };
        if response.get("status").and_then(Value::as_str) != Some(expected_status) {
            return Err(RemoteCompactionV2SseTerminalError::InvalidTerminal);
        }
        terminal_response = Some(response);
    }
    terminal_response.ok_or(RemoteCompactionV2SseTerminalError::MissingTerminal)
}

fn synthetic_remote_compaction_item(summary: &str) -> Value {
    let summary =
        truncate_utf8_to_byte_limit(summary.trim(), MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES);
    json!({
        "type": "compaction",
        "encrypted_content": format!("{REMOTE_COMPACTION_V2_SYNTHETIC_PREFIX}{summary}")
    })
}

fn synthetic_structured_compaction_item(encoded: &str) -> Value {
    json!({
        "type": "compaction",
        "encrypted_content": encoded
    })
}

fn truncate_utf8_to_byte_limit(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

/// 为 Remote Compaction V2 降级桥生成规范的失败响应。
///
/// 所有异常终结都必须丢弃普通 message/tool 输出，避免 Codex V2 collector
/// 再次遇到“0 个或多个 compaction item”的不确定状态。
pub fn remote_compaction_v2_failure_response(
    request_json: &Value,
    response_object: Option<&Value>,
    code: &str,
    message: &str,
) -> Value {
    let mut response = response_object
        .filter(|response| response.is_object())
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "id": "resp_compaction",
                "object": "response",
                "created_at": 0,
                "model": request_json
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                "usage": null
            })
        });
    let object = response
        .as_object_mut()
        .expect("remote compaction failure response must be an object");
    object.insert("status".to_string(), json!("failed"));
    object.insert("output".to_string(), json!([]));
    object.remove("incomplete_details");
    object.insert(
        "error".to_string(),
        json!({
            "code": code,
            "message": message
        }),
    );
    response
}

/// 为流式 / WebSocket 降级桥生成只包含 `response.failed` 的规范 SSE。
pub fn remote_compaction_v2_failure_sse(request_json: &Value, code: &str, message: &str) -> String {
    compaction_failure_sse(request_json, None, code, message)
}

/// 为传统分层压缩或 Remote Compaction V2 桥生成规范的失败 SSE。
///
/// `response_object` 存在时保留其响应 ID、模型和 usage；所有普通输出都会被清空。
pub fn compaction_failure_sse(
    request_json: &Value,
    response_object: Option<&Value>,
    code: &str,
    message: &str,
) -> String {
    let response =
        remote_compaction_v2_failure_response(request_json, response_object, code, message);
    build_responses_sse_for_remote_compaction_failure(&response)
}

/// assistant 尾部无法安全续接时返回一个不产生任何新会话内容的 completed 响应。
pub fn local_compaction_wait_for_user_response(request_json: &Value) -> Value {
    json!({
        "id": "resp_codex_elves_wait_user",
        "object": "response",
        "created_at": 0,
        "status": "completed",
        "model": request_json
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "output": [],
        "usage": {
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
            "output_tokens_details": {
                "reasoning_tokens": 0
            }
        }
    })
}

/// 流式 / WebSocket 版本的“等待真实 user”响应。
pub fn local_compaction_wait_for_user_sse(request_json: &Value) -> String {
    build_responses_sse_for_empty_completed(&local_compaction_wait_for_user_response(request_json))
}

fn extract_compaction_summary_text(response_object: &Value) -> Option<String> {
    if let Some(text) = extract_message_text(response_object) {
        if !text.trim().is_empty() {
            return Some(text);
        }
    }

    let output = response_object.get("output")?.as_array()?;
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            continue;
        }
        let mut text = String::new();
        if let Some(parts) = item.get("summary").and_then(Value::as_array) {
            for part in parts {
                if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                    text.push_str(part_text);
                }
            }
        }
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

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

/// 为传统上下文压缩准备只生成摘要文本的上游请求。
///
/// 请求身份仍由调用方保存的原始请求判断；转发副本会先摘除 assistant 指代锚点开始的
/// 原始尾部，再使用有效项目提示词，并移除工具字段，避免摘要阶段产生工具调用。
pub fn prepare_legacy_layered_compaction_request(
    request_json: &Value,
    prompt_override: &str,
) -> Value {
    if !is_compaction_request(Some(request_json)) {
        return request_json.clone();
    }
    let prompt = effective_compaction_prompt(prompt_override);
    let mut request = expand_synthetic_local_compaction_request(request_json);
    if let Some(object) = request.as_object_mut() {
        if let Some(input) = object.get_mut("input").and_then(Value::as_array_mut) {
            let mut prompt_item = input
                .last()
                .cloned()
                .unwrap_or_else(|| remote_compaction_v2_bridge_prompt_item(prompt));
            replace_message_text(&mut prompt_item, prompt);
            let split =
                split_local_compaction_input(input, LocalCompactionControlKind::LegacyPrompt);
            *input = split.summary_input;
            input.push(prompt_item);
        }
        for key in ["tools", "tool_choice", "parallel_tool_calls"] {
            object.remove(key);
        }
    }
    request
}

/// 判断请求是否属于任一种上下文压缩（传统压缩或 Remote Compaction V2）。
pub fn is_any_compaction_request(request_json: &Value) -> bool {
    is_compaction_request(Some(request_json)) || is_remote_compaction_v2_request(Some(request_json))
}

/// 把传统本地压缩请求改写为使用独立的压缩模型。
///
/// 压缩轮只需要一段纯文本摘要，因此除了替换 `model` 还要做两件事：
///
/// - 剔除历史里的 `reasoning` item。它们携带原模型的 `encrypted_content`，跨模型
///   （尤其跨供应商）传回上游必被拒；摘要也用不到这些推理痕迹。
/// - 清理主模型的推理档位，再按独立压缩模型设置摘要专用默认值：
///   DeepSeek/GLM 使用 `max`，其他模型使用 `xhigh`。
///
/// Remote Compaction V2、非压缩请求、模型名为空、或目标模型与当前模型相同时原样返回。
pub fn apply_compaction_model_override(request_json: &Value, model: &str) -> Value {
    let model = model.trim();
    if model.is_empty() || !is_compaction_request(Some(request_json)) {
        return request_json.clone();
    }
    apply_confirmed_compaction_model_override(request_json, model)
}

/// 为已经在改写前确认身份的压缩请求替换模型。
///
/// 自定义压缩提示词会移除 Codex 固定前缀，因此代理必须先冻结请求身份，再调用此入口。
pub(crate) fn apply_confirmed_compaction_model_override(
    request_json: &Value,
    model: &str,
) -> Value {
    let model = model.trim();
    if model.is_empty() {
        return request_json.clone();
    }
    if request_json.get("model").and_then(Value::as_str) == Some(model) {
        return request_json.clone();
    }
    let mut request = request_json.clone();
    let Some(object) = request.as_object_mut() else {
        return request_json.clone();
    };
    object.insert("model".to_string(), json!(model));
    for key in ["reasoning", "model_reasoning_effort", "reasoning_effort"] {
        object.remove(key);
    }
    object.insert(
        "reasoning".to_string(),
        json!({ "effort": default_compaction_reasoning_effort(model) }),
    );
    if let Some(Value::Array(items)) = object.get_mut("input") {
        items.retain(|item| item.get("type").and_then(Value::as_str) != Some("reasoning"));
    }
    request
}

fn default_compaction_reasoning_effort(model: &str) -> &'static str {
    let model = model.trim().to_ascii_lowercase();
    if model.contains("deepseek") || model.contains("glm") {
        "max"
    } else {
        "xhigh"
    }
}

pub const DEFAULT_COMPACTION_OUTPUT_RESERVE_TOKENS: u64 = 8_192;

/// 估算独立压缩模型实际收到的请求 token 数。
///
/// 上游 tokenizer 不可用时采用偏保守估算，并取以下两种结果中的较大值：
///
/// - 字符加权：ASCII 连续词每 4 字符约 1 token，其他非空白字符按 1 token；
/// - 序列化 JSON UTF-8 字节数除以 3，覆盖结构字段与中英文混合内容。
pub fn estimate_compaction_request_tokens(request_json: &Value) -> u64 {
    let weighted = estimate_json_value_tokens(request_json);
    let serialized = serde_json::to_vec(request_json)
        .map(|bytes| (bytes.len() as u64).div_ceil(3))
        .unwrap_or(0);
    weighted.max(serialized)
}

/// 为压缩摘要输出预留上下文空间。
pub fn compaction_output_reserve_tokens(request_json: &Value) -> u64 {
    ["max_output_tokens", "max_tokens", "max_completion_tokens"]
        .into_iter()
        .filter_map(|key| request_json.get(key).and_then(Value::as_u64))
        .max()
        .unwrap_or(0)
        .max(DEFAULT_COMPACTION_OUTPUT_RESERVE_TOKENS)
}

pub fn compaction_request_fits_context(request_json: &Value, context_window: u64) -> bool {
    let estimated_input = estimate_compaction_request_tokens(request_json);
    let output_reserve = compaction_output_reserve_tokens(request_json);
    estimated_input.saturating_add(output_reserve) <= context_window
}

fn estimate_json_value_tokens(value: &Value) -> u64 {
    match value {
        Value::Null => 1,
        Value::Bool(_) | Value::Number(_) => 1,
        Value::String(text) => estimate_text_tokens(text),
        Value::Array(items) => estimate_json_array_tokens(items),
        Value::Object(object) => object.iter().fold(2_u64, |total, (key, value)| {
            total
                .saturating_add(estimate_text_tokens(key))
                .saturating_add(estimate_json_value_tokens(value))
                .saturating_add(2)
        }),
    }
}

fn estimate_json_array_tokens(items: &[Value]) -> u64 {
    items.iter().fold(2_u64, |total, item| {
        total
            .saturating_add(estimate_json_value_tokens(item))
            .saturating_add(1)
    })
}

fn estimate_text_tokens(text: &str) -> u64 {
    let mut tokens = 0_u64;
    let mut ascii_word_len = 0_u64;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            ascii_word_len += 1;
            continue;
        }
        if ascii_word_len > 0 {
            tokens = tokens.saturating_add(ascii_word_len.div_ceil(4));
            ascii_word_len = 0;
        }
        if !ch.is_whitespace() {
            tokens = tokens.saturating_add(1);
        }
    }
    if ascii_word_len > 0 {
        tokens = tokens.saturating_add(ascii_word_len.div_ceil(4));
    }
    tokens
}

/// 把压缩响应 SSE 里的 `model` 回填为会话原本的模型。
///
/// 上游返回的是压缩模型名，直接透传会让 Codex 侧的记账和展示错乱。
pub fn restore_response_model_in_sse(
    sse_text: &str,
    original_model: &str,
    compaction_model: &str,
) -> String {
    let original_model = original_model.trim();
    let compaction_model = compaction_model.trim();
    if original_model.is_empty()
        || compaction_model.is_empty()
        || original_model == compaction_model
    {
        return sse_text.to_string();
    }
    let from = format!("\"model\":{}", json!(compaction_model));
    let to = format!("\"model\":{}", json!(original_model));
    sse_text.replace(&from, &to)
}

/// 响应对象版本的压缩模型回填。
pub fn restore_response_model(response: &mut Value, original_model: &str) {
    let original_model = original_model.trim();
    if original_model.is_empty() {
        return;
    }
    if let Some(object) = response.as_object_mut()
        && object.contains_key("model")
    {
        object.insert("model".to_string(), json!(original_model));
    }
}

/// 判断 completed Responses 对象是否包含传统压缩可用的 assistant 摘要文本。
pub fn has_completed_compaction_summary(response_object: &Value) -> bool {
    response_object.get("status").and_then(Value::as_str) == Some("completed")
        && extract_message_text(response_object).is_some_and(|summary| !summary.trim().is_empty())
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

/// 在传统压缩响应 SSE 上应用结构化本地压缩：把上游摘要与原始保留尾部编码为 v3 载荷。
///
/// - `enabled` 为 false、非压缩请求、或无法解析终止响应/摘要时，原样返回。
/// - 原始尾部超出配置目标时裁剪工具输出 / 动态工具描述，保留消息、item、调用参数和调用配对结构。
/// - 不可裁剪的用户 / assistant 原文和调用参数仍可软超目标；只有结构化载荷超过物理上限才失败。
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

    match build_structured_local_compaction(request_json, &summary, retain_tokens) {
        Ok(Some(build)) => LayeredCompactionResult {
            sse_text: build_responses_sse_for_message(&response_object, &build.encoded),
            triggered: true,
            retained_items: build.stats.retained_items,
            retained_chars: build.stats.retained_chars,
        },
        Ok(None) => LayeredCompactionResult::unchanged(sse_text),
        Err(error) => LayeredCompactionResult {
            sse_text: compaction_failure_sse(
                request_json,
                Some(&response_object),
                error.code(),
                &error.message(),
            ),
            triggered: false,
            retained_items: 0,
            retained_chars: 0,
        },
    }
}

/// 保留 token 预算的下限 / 上限 / 默认值。
pub const MIN_RETAIN_TOKENS: u32 = 20_000;
pub const MAX_RETAIN_TOKENS: u32 = 64_000;
pub const DEFAULT_RETAIN_TOKENS: u32 = 20_000;

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

/// 以终止响应对象为骨架，重建只含单个 `compaction` item 的完整 Responses SSE。
fn build_responses_sse_for_compaction(response_object: &Value) -> String {
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
    let compaction_item = response_object
        .get("output")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or_else(|| synthetic_remote_compaction_item(""));

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
            "item": compaction_item
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": compaction_item
        }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.completed",
        json!({ "type": "response.completed", "response": response_object }),
        &mut sequence,
    );
    output.push_str("data: [DONE]\n\n");
    output
}

fn build_responses_sse_for_empty_completed(response_object: &Value) -> String {
    let mut sequence = 0u64;
    let mut output = String::new();
    let mut created = response_object.clone();
    if let Some(object) = created.as_object_mut() {
        object.insert("status".to_string(), json!("in_progress"));
        object.insert("output".to_string(), json!([]));
    }
    push_event(
        &mut output,
        "response.created",
        json!({ "type": "response.created", "response": created.clone() }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.in_progress",
        json!({ "type": "response.in_progress", "response": created }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.completed",
        json!({ "type": "response.completed", "response": response_object }),
        &mut sequence,
    );
    output.push_str("data: [DONE]\n\n");
    output
}

fn build_responses_sse_for_remote_compaction_failure(response_object: &Value) -> String {
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
    let mut sequence = 0u64;
    let mut output = String::new();
    let base_response = |status: &str| {
        json!({
            "id": response_id,
            "object": "response",
            "created_at": created_at,
            "status": status,
            "model": model,
            "output": [],
            "usage": usage
        })
    };

    push_event(
        &mut output,
        "response.created",
        json!({ "type": "response.created", "response": base_response("in_progress") }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.in_progress",
        json!({ "type": "response.in_progress", "response": base_response("in_progress") }),
        &mut sequence,
    );
    push_event(
        &mut output,
        "response.failed",
        json!({ "type": "response.failed", "response": response_object }),
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

    fn assert_short_marker(marker: &str, kind: &str) {
        assert!(
            marker.starts_with(&format!("<truncated:{kind};~")),
            "marker must use the short English angle-bracket format: {marker}"
        );
        assert!(
            marker.ends_with("t>"),
            "marker must end with a token estimate: {marker}"
        );
        assert!(
            !marker.contains("characters") && !marker.contains("CodexElves"),
            "marker must not carry the old verbose character-count explanation: {marker}"
        );
    }

    fn short_marker_in(text: &str) -> &str {
        text.lines()
            .find(|line| line.starts_with("<truncated:"))
            .expect("trimmed text must contain a short marker")
    }

    fn remote_compaction_v2_request() -> Value {
        json!({
            "model": "claude-sonnet-5",
            "stream": true,
            "input": [
                user_message("implement the fix"),
                {
                    "type": "compaction_trigger"
                }
            ],
            "tools": [{
                "type": "function",
                "name": "exec_command",
                "parameters": { "type": "object" }
            }],
            "tool_choice": "auto",
            "parallel_tool_calls": true
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
    fn detects_remote_compaction_v2_trigger() {
        assert!(is_remote_compaction_v2_request(Some(
            &remote_compaction_v2_request()
        )));
        assert!(!is_remote_compaction_v2_request(Some(&json!({
            "input": [user_message("normal")]
        }))));
    }

    #[test]
    fn remote_compaction_v2_keeps_original_model() {
        let request = remote_compaction_v2_request();
        assert_eq!(
            apply_compaction_model_override(&request, "gpt-5.6"),
            request
        );
    }

    #[test]
    fn effective_prompt_uses_project_default_for_blank_override() {
        assert_eq!(effective_compaction_prompt(""), DEFAULT_COMPACTION_PROMPT);
        assert_eq!(
            effective_compaction_prompt(" \r\n\t"),
            DEFAULT_COMPACTION_PROMPT
        );
        assert_eq!(
            effective_compaction_prompt("  CUSTOM SUMMARY PROMPT  "),
            "CUSTOM SUMMARY PROMPT"
        );
    }

    #[test]
    fn project_default_prompt_is_scope_aware_evidence_handoff() {
        assert!(DEFAULT_COMPACTION_PROMPT.starts_with("You are a conversation compaction writer."));
        assert!(!DEFAULT_COMPACTION_PROMPT.starts_with('#'));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("## Evidence Scope and Supersession"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("## Mandatory Internal Evidence Ledger"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("Example: `git log -3`"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("## Mandatory Internal Verification"));
    }

    #[test]
    fn remote_compaction_v2_bridge_replaces_trigger_and_removes_tools() {
        let rewritten =
            prepare_remote_compaction_v2_bridge_request(&remote_compaction_v2_request());
        assert!(rewritten.get("tools").is_none());
        assert!(rewritten.get("tool_choice").is_none());
        assert!(rewritten.get("parallel_tool_calls").is_none());
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "user");
        assert!(item_text(&input[1]).starts_with(COMPACTION_PROMPT_PREFIX));
    }

    #[test]
    fn remote_compaction_v2_bridge_uses_layered_custom_prompt() {
        let rewritten = prepare_remote_compaction_v2_bridge_request_with_prompt(
            &remote_compaction_v2_request(),
            Some("CUSTOM LAYERED COMPACTION PROMPT"),
        );
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(
            item_text(input.last().unwrap()),
            "CUSTOM LAYERED COMPACTION PROMPT"
        );
    }

    #[test]
    fn remote_compaction_v2_bridge_uses_project_default_for_blank_override() {
        let rewritten = prepare_remote_compaction_v2_bridge_request_with_prompt(
            &remote_compaction_v2_request(),
            Some(""),
        );
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(item_text(input.last().unwrap()), DEFAULT_COMPACTION_PROMPT);
    }

    #[test]
    fn remote_compaction_v2_response_contains_exactly_one_compaction_item() {
        let source = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [
                {
                    "id": "msg_bridge",
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "SUMMARY", "annotations": [] }]
                },
                {
                    "type": "function_call",
                    "call_id": "call_unexpected",
                    "name": "exec_command",
                    "arguments": "{}"
                }
            ],
            "usage": { "input_tokens": 100, "output_tokens": 10, "total_tokens": 110 }
        });
        let rewritten =
            rewrite_remote_compaction_v2_response(&remote_compaction_v2_request(), &source)
                .expect("V2 response should be rewritten");
        let output = rewritten.get("output").and_then(Value::as_array).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "compaction");
        let restored = synthetic_remote_compaction_history_text(&output[0])
            .expect("synthetic compaction should be decodable");
        assert!(restored.contains("SUMMARY"));
    }

    #[test]
    fn synthetic_remote_compaction_is_limited_to_decodable_size() {
        let summary = format!("{}界", "x".repeat(MAX_REMOTE_COMPACTION_V2_SYNTHETIC_BYTES));
        let item = synthetic_remote_compaction_item(&summary);
        let restored = synthetic_remote_compaction_history_text(&item)
            .expect("size-limited synthetic compaction should remain decodable");

        assert!(restored.ends_with('x'));
        assert!(!restored.ends_with('界'));
    }

    #[test]
    fn synthetic_remote_compaction_writes_plain_text_and_reads_legacy_base64() {
        let item = synthetic_remote_compaction_item("中文摘要\n带换行与\"引号\"");
        let encrypted_content = item["encrypted_content"].as_str().unwrap();
        // 新写入一律为 v2 明文，不再携带 Base64 膨胀。
        assert!(encrypted_content.starts_with(REMOTE_COMPACTION_V2_SYNTHETIC_PREFIX));
        assert!(encrypted_content.contains("中文摘要\n带换行与\"引号\""));
        let restored = synthetic_remote_compaction_history_text(&item)
            .expect("v2 plain-text compaction should be readable");
        assert!(restored.contains("中文摘要"));
        assert!(restored.contains("带换行与\"引号\""));

        // TODO(0.3.7): 兼容期结束后连同该断言一并删除。
        let legacy = json!({
            "type": "compaction",
            "encrypted_content": format!(
                "{REMOTE_COMPACTION_V2_LEGACY_BASE64_PREFIX}{}",
                URL_SAFE_NO_PAD.encode("LEGACY SUMMARY".as_bytes())
            )
        });
        let restored_legacy = synthetic_remote_compaction_history_text(&legacy)
            .expect("v1 base64 compaction should stay readable");
        assert!(restored_legacy.contains("LEGACY SUMMARY"));
    }

    #[test]
    fn remote_compaction_v2_uses_layered_tail_when_enabled() {
        let request = json!({
            "model": "claude-sonnet-5",
            "input": [
                user_message("USER CONTEXT KEPT NATIVELY"),
                assistant_message("KEEP THIS ASSISTANT CONTEXT"),
                { "type": "compaction_trigger" }
            ]
        });
        let source = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [{
                "id": "msg_bridge",
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "SUMMARY", "annotations": [] }]
            }]
        });
        let rewritten = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &source,
            true,
            MIN_RETAIN_TOKENS,
        )
        .expect("V2 layered response should be rewritten");
        assert!(rewritten.layered.triggered);
        // 最近一轮 = 最后一条 user → 结尾：此例为 [user, assistant]。
        assert_eq!(rewritten.layered.retained_items, 2);
        let payload = synthetic_local_compaction_payload(&rewritten.response["output"][0]).unwrap();
        assert_eq!(payload.summary, "SUMMARY");
        assert_eq!(
            payload.retained_tail,
            vec![
                user_message("USER CONTEXT KEPT NATIVELY"),
                assistant_message("KEEP THIS ASSISTANT CONTEXT")
            ]
        );

        let plain = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &source,
            false,
            MIN_RETAIN_TOKENS,
        )
        .expect("V2 plain response should be rewritten");
        assert!(!plain.layered.triggered);
        let restored_plain =
            synthetic_remote_compaction_history_text(&plain.response["output"][0]).unwrap();
        assert!(restored_plain.contains("SUMMARY"));
        assert!(!restored_plain.contains("USER CONTEXT KEPT NATIVELY"));
        assert!(!restored_plain.contains("KEEP THIS ASSISTANT CONTEXT"));
    }

    #[test]
    fn remote_compaction_v2_sse_emits_only_one_done_output_item() {
        let rewritten = rewrite_remote_compaction_v2_responses_sse(
            &remote_compaction_v2_request(),
            summary_sse("SUMMARY"),
        )
        .expect("V2 SSE should be rewritten");
        let done_items = rewritten
            .split("\n\n")
            .filter(|event| event.starts_with("event: response.output_item.done"))
            .collect::<Vec<_>>();
        assert_eq!(done_items.len(), 1);
        assert!(done_items[0].contains("\"type\":\"compaction\""));
        assert!(!rewritten.contains("\"type\":\"message\",\"status\":\"completed\""));
        let terminal = crate::continue_thinking::extract_terminal_response_object(&rewritten)
            .expect("rewritten SSE has terminal response");
        assert_eq!(
            terminal
                .get("output")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(terminal["output"][0]["type"], "compaction");
    }

    #[test]
    fn remote_compaction_v2_without_summary_fails_closed() {
        let source = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [{
                "type": "function_call",
                "call_id": "call_only",
                "name": "exec_command",
                "arguments": "{}"
            }],
            "usage": { "input_tokens": 100, "output_tokens": 10, "total_tokens": 110 }
        });
        let rewritten =
            rewrite_remote_compaction_v2_response(&remote_compaction_v2_request(), &source)
                .expect("V2 completed response must never fall back to ordinary outputs");
        assert_eq!(rewritten["status"], "failed");
        assert_eq!(rewritten["output"], json!([]));
        assert_eq!(
            rewritten["error"]["code"],
            "remote_compaction_summary_missing"
        );
    }

    #[test]
    fn remote_compaction_v2_sse_without_summary_emits_failed_terminal_event() {
        let response = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [{
                "type": "function_call",
                "call_id": "call_only",
                "name": "exec_command",
                "arguments": "{}"
            }],
            "usage": { "input_tokens": 100, "output_tokens": 10, "total_tokens": 110 }
        });
        let source = format!(
            "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::to_string(&json!({
                "type": "response.completed",
                "response": response
            }))
            .unwrap()
        );
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("V2 SSE must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_summary_missing"));
        assert!(!rewritten.contains("event: response.output_item.done"));
        assert!(!rewritten.contains("event: response.completed"));
    }

    #[test]
    fn remote_compaction_v2_incomplete_response_fails_closed() {
        let source = json!({
            "id": "resp_bridge",
            "object": "response",
            "created_at": 123,
            "status": "incomplete",
            "model": "claude-sonnet-5",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "PARTIAL SUMMARY" }]
            }],
            "incomplete_details": { "reason": "max_output_tokens" }
        });
        let rewritten =
            rewrite_remote_compaction_v2_response(&remote_compaction_v2_request(), &source)
                .expect("V2 incomplete response must fail closed");
        assert_eq!(rewritten["status"], "failed");
        assert_eq!(rewritten["output"], json!([]));
        assert_eq!(
            rewritten["error"]["code"],
            "remote_compaction_upstream_incomplete"
        );
        assert!(rewritten.get("incomplete_details").is_none());
    }

    #[test]
    fn remote_compaction_v2_sse_without_terminal_event_fails_closed() {
        let source = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_bridge\",\"status\":\"in_progress\"}}\n\n".to_string();
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("V2 SSE without terminal response must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_terminal_response_missing"));
        assert!(!rewritten.contains("event: response.output_item.done"));
    }

    #[test]
    fn remote_compaction_v2_malformed_sse_event_fails_even_if_completed_follows() {
        let source = format!(
            "event: response.output_text.delta\ndata: {{malformed-json}}\n\n{}",
            summary_sse("SUMMARY MUST NOT BE USED")
        );
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("malformed V2 SSE must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_sse_parse_failed"));
        assert!(!rewritten.contains("event: response.output_item.done"));
        assert!(!rewritten.contains("SUMMARY MUST NOT BE USED"));
    }

    #[test]
    fn remote_compaction_v2_multiple_terminal_events_fail_closed() {
        let failed = serde_json::to_string(&json!({
            "type": "response.failed",
            "response": {
                "id": "resp_duplicate_terminal",
                "status": "failed",
                "output": [],
                "error": { "message": "first terminal failed" }
            }
        }))
        .unwrap();
        let source = format!(
            "event: response.failed\ndata: {failed}\n\n{}",
            summary_sse("LATE SUMMARY MUST NOT BE USED")
        );
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("multiple V2 terminal events must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_multiple_terminal_responses"));
        assert!(!rewritten.contains("event: response.completed"));
        assert!(!rewritten.contains("LATE SUMMARY MUST NOT BE USED"));
    }

    #[test]
    fn remote_compaction_v2_terminal_event_status_mismatch_fails_closed() {
        let source = serde_json::to_string(&json!({
            "type": "response.failed",
            "response": {
                "id": "resp_mismatched_terminal",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "MISMATCHED SUMMARY MUST NOT BE USED"
                    }]
                }]
            }
        }))
        .unwrap();
        let rewritten = rewrite_remote_compaction_v2_responses_sse(
            &remote_compaction_v2_request(),
            format!("event: response.failed\ndata: {source}\n\n"),
        )
        .expect("mismatched V2 terminal must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_terminal_response_invalid"));
        assert!(!rewritten.contains("event: response.completed"));
        assert!(!rewritten.contains("MISMATCHED SUMMARY MUST NOT BE USED"));
    }

    #[test]
    fn remote_compaction_v2_error_event_fails_even_if_completed_follows() {
        let source = format!(
            "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"message\":\"upstream failed\"}}}}\n\n{}",
            summary_sse("SUMMARY AFTER ERROR MUST NOT BE USED")
        );
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("V2 error event must fail closed");
        assert!(rewritten.contains("event: response.failed"));
        assert!(rewritten.contains("remote_compaction_upstream_failed"));
        assert!(!rewritten.contains("event: response.completed"));
        assert!(!rewritten.contains("SUMMARY AFTER ERROR MUST NOT BE USED"));
    }

    #[test]
    fn remote_compaction_v2_accepts_crlf_sse_event_boundaries() {
        let source = summary_sse("CRLF SUMMARY").replace('\n', "\r\n");
        let rewritten =
            rewrite_remote_compaction_v2_responses_sse(&remote_compaction_v2_request(), source)
                .expect("valid CRLF V2 SSE should be rewritten");
        assert!(rewritten.contains("event: response.completed"));
        assert!(rewritten.contains("\"type\":\"compaction\""));
        assert!(!rewritten.contains("remote_compaction_sse_parse_failed"));
    }

    #[test]
    fn remote_compaction_v2_tail_keeps_latest_item_after_non_trailing_trigger() {
        let request = json!({
            "model": "claude-sonnet-5",
            "input": [
                user_message("earlier context"),
                { "type": "compaction_trigger" },
                assistant_message("latest context after trigger")
            ]
        });
        let source = json!({
            "id": "resp_non_trailing_trigger",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "SUMMARY" }]
            }]
        });
        let rewritten = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &source,
            true,
            DEFAULT_RETAIN_TOKENS,
        )
        .expect("V2 request should be rewritten");
        let payload = synthetic_local_compaction_payload(&rewritten.response["output"][0]).unwrap();
        assert_eq!(
            payload.retained_tail,
            vec![
                user_message("earlier context"),
                assistant_message("latest context after trigger")
            ]
        );
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
    fn legacy_layered_request_uses_effective_prompt_and_removes_tools() {
        let request = json!({
            "input": [user_message("hi"), compaction_prompt_item()],
            "tools": [{ "type": "function", "name": "exec_command" }],
            "tool_choice": "auto",
            "parallel_tool_calls": true
        });
        let rewritten = prepare_legacy_layered_compaction_request(&request, "");
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();

        assert_eq!(item_text(input.last().unwrap()), DEFAULT_COMPACTION_PROMPT);
        assert!(rewritten.get("tools").is_none());
        assert!(rewritten.get("tool_choice").is_none());
        assert!(rewritten.get("parallel_tool_calls").is_none());
        assert_eq!(
            item_text(&request["input"][1]),
            item_text(&compaction_prompt_item())
        );
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

    fn legacy_tool_call_item(call_id: &str, name: &str, input: Value) -> Value {
        json!({
            "type": "tool_call",
            "tool_use": {
                "id": call_id,
                "name": name,
                "input": input
            }
        })
    }

    fn legacy_tool_result_item(call_id: &str, content: Value) -> Value {
        json!({
            "type": "tool_result",
            "content": {
                "tool_use_id": call_id,
                "content": content
            }
        })
    }

    #[test]
    fn legacy_summary_request_excludes_anchor_and_raw_tail() {
        let request = json!({
            "input": [
                user_message("更早历史"),
                assistant_message("推荐方案：执行方案 1"),
                user_message("按推荐处理"),
                function_call_item("call_1", "shell_command"),
                function_call_output_item("call_1", "ok"),
                compaction_prompt_item()
            ],
            "tools": [{ "type": "function", "name": "shell_command" }]
        });
        let prepared = prepare_legacy_layered_compaction_request(&request, "");
        let input = prepared["input"].as_array().unwrap();

        assert_eq!(input.len(), 2);
        assert_eq!(item_text(&input[0]), "更早历史");
        assert_eq!(item_text(&input[1]), DEFAULT_COMPACTION_PROMPT);
        assert!(!prepared.to_string().contains("推荐方案：执行方案 1"));
        assert!(!prepared.to_string().contains("按推荐处理"));
        assert!(!prepared.to_string().contains("call_1"));
        assert!(prepared.get("tools").is_none());
    }

    #[test]
    fn structured_payload_preserves_anchor_user_and_tool_items_exactly() {
        let anchor = assistant_message("推荐方案：执行方案 1");
        let mut user = user_message("按推荐处理");
        user["id"] = json!("msg-user");
        user["content"] = json!([
            { "type": "input_text", "text": "按推荐处理" },
            { "type": "input_image", "image_url": "data:image/png;base64,AAAA" }
        ]);
        let call = function_call_item("call_1", "shell_command");
        let output = function_call_output_item("call_1", "probe ready");
        let request = json!({
            "input": [
                user_message("更早历史"),
                anchor.clone(),
                user.clone(),
                call.clone(),
                output.clone(),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            summary_sse("较早历史摘要"),
        );
        assert!(result.triggered);
        assert_eq!(result.retained_items, 4);
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("structured legacy response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        assert_eq!(payload.summary, "较早历史摘要");
        assert_eq!(
            payload.retained_tail,
            vec![anchor, user, call, output],
            "roles, content blocks and call ids must remain byte-for-byte JSON equivalent"
        );
    }

    #[test]
    fn expansion_removes_codex_user_duplicate_and_restores_original_order() {
        let anchor = assistant_message("推荐方案：执行方案 1");
        let user = json!({
            "type": "message",
            "id": "msg-user",
            "role": "user",
            "content": [{ "type": "input_text", "text": "按推荐处理" }],
            "internal_chat_message_metadata_passthrough": { "turn_id": "turn-1" }
        });
        let call = function_call_item("call_1", "shell_command");
        let output = function_call_output_item("call_1", "probe ready");
        let payload = StructuredLocalCompactionPayload {
            summary: "较早历史摘要".to_string(),
            retained_tail: vec![anchor.clone(), user.clone(), call.clone(), output.clone()],
        };
        let item = synthetic_structured_compaction_item(&format!(
            "{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{}",
            serde_json::to_string(&payload).unwrap()
        ));
        let request = json!({
            "input": [
                user_message("更早保留的 user"),
                user.clone(),
                item
            ]
        });
        let expanded = expand_synthetic_local_compaction_request(&request);
        let input = expanded["input"].as_array().unwrap();

        assert_eq!(input.len(), 6);
        assert_eq!(item_text(&input[0]), "更早保留的 user");
        assert_eq!(input[1]["role"], "assistant");
        assert!(item_text(&input[1]).contains("较早历史摘要"));
        assert_eq!(&input[2..], &[anchor, user, call, output]);
        assert_eq!(
            input
                .iter()
                .filter(|item| item.get("id").and_then(Value::as_str) == Some("msg-user"))
                .count(),
            1
        );
    }

    #[test]
    fn oversized_text_tool_output_is_trimmed_without_breaking_pair() {
        let huge_output = format!("BEGIN\n{}\nEND", "界".repeat(30_000));
        let call = function_call_item("call_1", "shell_command");
        let request = json!({
            "input": [
                assistant_message("推荐方案"),
                user_message("按推荐处理"),
                call.clone(),
                function_call_output_item("call_1", &huge_output),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        assert!(!result.sse_text.contains("response.failed"));
        assert!(
            !result
                .sse_text
                .contains("local_compaction_retained_tail_too_large")
        );
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        assert_eq!(payload.summary, "SUMMARY");
        assert_eq!(payload.retained_tail.len(), 4);
        assert_eq!(payload.retained_tail[2], call);
        assert_eq!(payload.retained_tail[3]["type"], "function_call_output");
        assert_eq!(payload.retained_tail[3]["call_id"], "call_1");
        let trimmed = payload.retained_tail[3]["output"]
            .as_str()
            .expect("text output remains text");
        assert!(trimmed.starts_with("BEGIN"));
        assert!(trimmed.ends_with("END"));
        let marker = trimmed
            .lines()
            .find(|line| line.starts_with("<truncated:"))
            .expect("trimmed tool output must carry a short marker");
        assert_short_marker(marker, "tool");
        assert!(
            trimmed.chars().count() < huge_output.chars().count(),
            "tool output details must actually be reduced"
        );
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                <= u64::from(MIN_RETAIN_TOKENS)
        );
    }

    #[test]
    fn image_tool_output_from_failed_session_is_replaced_with_text_marker() {
        let image_url = format!("data:image/png;base64,{}", "A".repeat(136_760));
        let call = json!({
            "type": "function_call",
            "id": "fc_view_image",
            "call_id": "call_view_image",
            "name": "view_image",
            "arguments": "{\"path\":\"temp/picker-dark.png\"}",
            "internal_chat_message_metadata_passthrough": { "turn_id": "turn-session" }
        });
        let output = json!({
            "type": "function_call_output",
            "id": "fco_view_image",
            "call_id": "call_view_image",
            "output": [{
                "type": "input_image",
                "image_url": image_url,
                "detail": "high"
            }],
            "internal_chat_message_metadata_passthrough": { "turn_id": "turn-session" }
        });
        let request = json!({
            "input": [
                assistant_message("亲自验收视觉效果"),
                user_message("继续"),
                call.clone(),
                output,
                assistant_message("视觉验收通过"),
                compaction_prompt_item()
            ]
        });

        assert!(
            estimate_json_value_tokens(&Value::Array(
                request["input"].as_array().unwrap()[..5].to_vec()
            )) > u64::from(MIN_RETAIN_TOKENS),
            "fixture must reproduce an oversized retained tail"
        );

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        assert!(!result.sse_text.contains("response.failed"));
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        assert_eq!(payload.retained_tail.len(), 5);
        assert_eq!(payload.retained_tail[2], call);
        let trimmed_output = &payload.retained_tail[3];
        assert_eq!(trimmed_output["type"], "function_call_output");
        assert_eq!(trimmed_output["id"], "fco_view_image");
        assert_eq!(trimmed_output["call_id"], "call_view_image");
        assert_eq!(
            trimmed_output["internal_chat_message_metadata_passthrough"]["turn_id"],
            "turn-session"
        );
        let marker = trimmed_output["output"]
            .as_str()
            .expect("structured image output becomes a valid text result");
        assert_short_marker(marker, "media");
        assert!(
            !serde_json::to_string(&payload)
                .unwrap()
                .contains("data:image/png;base64")
        );
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                <= u64::from(MIN_RETAIN_TOKENS)
        );
    }

    #[test]
    fn oversized_tool_arguments_are_preserved_even_when_tail_exceeds_target() {
        let arguments = serde_json::to_string(&json!({
            "patch": format!("BEGIN_PATCH{}END_PATCH", "x".repeat(100_000))
        }))
        .unwrap();
        let call = json!({
            "type": "function_call",
            "id": "fc_patch",
            "call_id": "call_patch",
            "name": "apply_patch",
            "arguments": arguments
        });
        let output = function_call_output_item("call_patch", "Done!");
        let request = json!({
            "input": [
                assistant_message("开始修改"),
                user_message("继续"),
                call.clone(),
                output.clone(),
                compaction_prompt_item()
            ]
        });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        assert!(!result.sse_text.contains("response.failed"));
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        assert_eq!(
            payload.retained_tail[2], call,
            "function call arguments must remain exactly as supplied"
        );
        assert_eq!(payload.retained_tail[3], output);
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                > u64::from(MIN_RETAIN_TOKENS),
            "an oversized immutable argument may leave the soft target exceeded"
        );
    }

    #[test]
    fn oversized_legacy_tool_result_preserves_nested_tool_use_id() {
        let call = legacy_tool_call_item("call_legacy", "lookup", json!({ "query": "weather" }));
        let output = legacy_tool_result_item(
            "call_legacy",
            json!(format!("BEGIN\n{}\nEND", "界".repeat(30_000))),
        );
        let request = json!({
            "input": [
                assistant_message("开始查询"),
                user_message("继续"),
                call.clone(),
                output,
                compaction_prompt_item()
            ]
        });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        assert_eq!(payload.retained_tail[2], call);
        let trimmed_result = &payload.retained_tail[3];
        assert_eq!(trimmed_result["type"], "tool_result");
        assert_eq!(
            trimmed_result["content"]["tool_use_id"],
            json!("call_legacy")
        );
        let trimmed = trimmed_result["content"]["content"]
            .as_str()
            .expect("legacy result content remains nested text");
        assert!(trimmed.starts_with("BEGIN"));
        assert!(trimmed.ends_with("END"));
        let marker = trimmed
            .lines()
            .find(|line| line.starts_with("<truncated:"))
            .expect("legacy result must carry a short marker");
        assert_short_marker(marker, "tool");
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                <= u64::from(MIN_RETAIN_TOKENS)
        );
    }

    #[test]
    fn oversized_legacy_tool_call_input_is_preserved_even_when_tail_exceeds_target() {
        let call = legacy_tool_call_item(
            "call_legacy",
            "lookup",
            json!({ "query": format!("BEGIN{}END", "x".repeat(100_000)) }),
        );
        let output = legacy_tool_result_item("call_legacy", json!("found"));
        let request = json!({
            "input": [
                assistant_message("开始查询"),
                user_message("继续"),
                call.clone(),
                output.clone(),
                compaction_prompt_item()
            ]
        });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        assert_eq!(
            payload.retained_tail[2], call,
            "legacy tool input must remain exactly as supplied"
        );
        assert_eq!(payload.retained_tail[3], output);
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                > u64::from(MIN_RETAIN_TOKENS),
            "an oversized immutable legacy input may leave the soft target exceeded"
        );
    }

    #[test]
    fn oversized_tool_search_descriptions_are_trimmed_without_losing_tool_schema() {
        let parameters = json!({
            "type": "object",
            "properties": {
                "step": {
                    "type": "string",
                    "description": format!(
                        "PARAMETER_SCHEMA_MUST_REMAIN_UNCHANGED data:image/png;base64,{} END_SCHEMA",
                        "S".repeat(12_000)
                    )
                }
            },
            "required": ["step"],
            "additionalProperties": false
        });
        let call = json!({
            "type": "tool_search_call",
            "call_id": "call_search",
            "status": "completed",
            "execution": "client",
            "arguments": { "query": "consensus" }
        });
        let output = json!({
            "type": "tool_search_output",
            "call_id": "call_search",
            "status": "completed",
            "execution": "client",
            "tools": [{
                "type": "namespace",
                "name": "mcp__pal",
                "description": format!("BEGIN_NAMESPACE{}END_NAMESPACE", "界".repeat(12_000)),
                "tools": [{
                    "type": "function",
                    "name": "consensus",
                    "description": format!("BEGIN_TOOL{}END_TOOL", "界".repeat(12_000)),
                    "parameters": parameters.clone()
                }]
            }]
        });
        let request = json!({
            "input": [
                assistant_message("查找工具"),
                user_message("继续"),
                call.clone(),
                output,
                compaction_prompt_item()
            ]
        });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        assert_eq!(payload.retained_tail[2], call);
        let trimmed_output = &payload.retained_tail[3];
        assert_eq!(trimmed_output["call_id"], "call_search");
        assert_eq!(trimmed_output["tools"][0]["name"], "mcp__pal");
        assert_eq!(trimmed_output["tools"][0]["tools"][0]["name"], "consensus");
        assert_eq!(
            trimmed_output["tools"][0]["tools"][0]["parameters"], parameters,
            "tool parameter schema must remain byte-for-byte equivalent as JSON"
        );
        assert_short_marker(
            short_marker_in(trimmed_output["tools"][0]["description"].as_str().unwrap()),
            "tool-desc",
        );
        assert_short_marker(
            short_marker_in(
                trimmed_output["tools"][0]["tools"][0]["description"]
                    .as_str()
                    .unwrap(),
            ),
            "tool-desc",
        );
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                <= u64::from(MIN_RETAIN_TOKENS)
        );
    }

    #[test]
    fn tool_search_output_with_output_and_tools_redacts_media_description() {
        let call = json!({
            "type": "tool_search_call",
            "call_id": "call_search_media",
            "status": "completed",
            "execution": "client",
            "arguments": { "query": "vision" }
        });
        let request = json!({
            "input": [
                assistant_message("查找视觉工具"),
                user_message("继续"),
                call.clone(),
                {
                    "type": "tool_search_output",
                    "call_id": "call_search_media",
                    "status": "completed",
                    "execution": "client",
                    "output": "ok",
                    "tools": [{
                        "type": "namespace",
                        "name": "mcp__vision",
                        "description": "vision namespace",
                        "tools": [{
                            "type": "function",
                            "name": "inspect",
                            "description": format!(
                                "preview data:image/png;base64,{} done",
                                "I".repeat(100_000)
                            ),
                            "parameters": {
                                "type": "object",
                                "properties": {},
                                "additionalProperties": false
                            }
                        }]
                    }]
                },
                compaction_prompt_item()
            ]
        });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");
        let retained_json = serde_json::to_string(&payload.retained_tail).unwrap();
        let output = payload
            .retained_tail
            .iter()
            .find(|item| item.get("type").and_then(Value::as_str) == Some("tool_search_output"))
            .expect("tool search output remains");

        assert_eq!(payload.retained_tail[2], call);
        assert_eq!(output["call_id"], "call_search_media");
        assert_eq!(output["output"], "ok");
        assert_eq!(output["tools"][0]["name"], "mcp__vision");
        assert_eq!(output["tools"][0]["tools"][0]["name"], "inspect");
        assert_short_marker(
            output["tools"][0]["tools"][0]["description"]
                .as_str()
                .unwrap(),
            "media",
        );
        assert!(!contains_media_data_url(&retained_json));
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                <= u64::from(MIN_RETAIN_TOKENS)
        );
    }

    #[test]
    fn embedded_data_url_tool_output_is_replaced_instead_of_partially_truncated() {
        let output = format!(
            "screenshot: data:image/png;base64,{} :done",
            "A".repeat(100_000)
        );
        let request = json!({
            "input": [
                assistant_message("检查截图"),
                user_message("继续"),
                function_call_item("call_image", "view_image"),
                function_call_output_item("call_image", &output),
                compaction_prompt_item()
            ]
        });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");
        let trimmed = payload.retained_tail[3]["output"]
            .as_str()
            .expect("media output becomes text marker");

        assert_short_marker(trimmed, "media");
        assert!(!trimmed.contains("data:image/"));
        assert!(!trimmed.contains(&"A".repeat(1_000)));
    }

    #[test]
    fn media_output_is_redacted_but_call_arguments_stay_original() {
        let video_arguments = serde_json::to_string(&json!({
            "source": format!("prefix DATA:VIDEO/mp4;base64,{} suffix", "V".repeat(128))
        }))
        .unwrap();
        let request = json!({
            "input": [
                assistant_message("处理多个工具结果"),
                user_message("继续"),
                function_call_item("call_large", "shell_command"),
                function_call_output_item("call_large", &"L".repeat(100_000)),
                function_call_item("call_audio", "inspect_audio"),
                function_call_output_item(
                    "call_audio",
                    &format!("prefix data:audio/wav;base64,{} suffix", "A".repeat(128))
                ),
                {
                    "type": "function_call",
                    "call_id": "call_video",
                    "name": "inspect_video",
                    "arguments": video_arguments.clone()
                },
                function_call_output_item("call_video", "ok"),
                compaction_prompt_item()
            ]
        });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");
        let retained_json = serde_json::to_string(&payload.retained_tail).unwrap();

        let audio_output = payload
            .retained_tail
            .iter()
            .find(|item| {
                item.get("call_id").and_then(Value::as_str) == Some("call_audio")
                    && item.get("type").and_then(Value::as_str) == Some("function_call_output")
            })
            .expect("audio output remains paired");
        assert_short_marker(audio_output["output"].as_str().unwrap(), "media");
        let video_call = payload
            .retained_tail
            .iter()
            .find(|item| {
                item.get("call_id").and_then(Value::as_str) == Some("call_video")
                    && item.get("type").and_then(Value::as_str) == Some("function_call")
            })
            .expect("video call remains paired");
        assert_eq!(
            video_call["arguments"], video_arguments,
            "function call arguments must not be redacted even when they contain media"
        );
        assert!(
            contains_media_data_url(&retained_json),
            "the original media data URL remains only inside the untouched call arguments"
        );
    }

    #[test]
    fn sanitized_failed_session_shape_keeps_all_25_items_and_reaches_token_target() {
        let image_url = format!(
            "data:image/png;base64,{}{}",
            "A".repeat(136_760),
            "/".repeat(10_809)
        );
        let original_tail = vec![
            assistant_message("anchor"),
            function_call_item("call_wait", "wait_agent"),
            function_call_output_item("call_wait", "done"),
            user_message("notification"),
            assistant_message("inspect"),
            function_call_item("call_shell_1", "shell_command"),
            function_call_output_item("call_shell_1", "ok"),
            function_call_item("call_image", "view_image"),
            json!({
                "type": "function_call_output",
                "call_id": "call_image",
                "output": [{
                    "type": "input_image",
                    "image_url": image_url,
                    "detail": "high"
                }]
            }),
            assistant_message("visual result"),
            function_call_item("call_shell_2", "shell_command"),
            function_call_item("call_shell_3", "shell_command"),
            function_call_output_item("call_shell_2", "ok"),
            function_call_output_item("call_shell_3", "ok"),
            assistant_message("verify"),
            function_call_item("call_shell_4", "shell_command"),
            function_call_item("call_shell_5", "shell_command"),
            function_call_output_item("call_shell_4", "ok"),
            function_call_output_item("call_shell_5", "ok"),
            assistant_message("cleanup"),
            function_call_item("call_shell_6", "shell_command"),
            function_call_item("call_shell_7", "shell_command"),
            function_call_output_item("call_shell_6", "ok"),
            function_call_output_item("call_shell_7", "ok"),
            assistant_message("final"),
        ];
        assert_eq!(original_tail.len(), 25);
        assert_eq!(
            estimate_json_value_tokens(&Value::Array(original_tail.clone())),
            45_754
        );
        let mut input = original_tail.clone();
        input.push(json!({ "type": "compaction_trigger" }));
        let request = json!({
            "model": "claude-sonnet-5",
            "input": input
        });
        let source = json!({
            "id": "resp_sanitized_session",
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "SUMMARY" }]
            }]
        });

        let result = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &source,
            true,
            MIN_RETAIN_TOKENS,
        )
        .expect("trimmed remote compaction response");
        assert!(result.layered.triggered);
        assert_eq!(result.layered.retained_items, 25);
        let payload = synthetic_local_compaction_payload(&result.response["output"][0])
            .expect("v3 payload should decode");

        for (original, retained) in original_tail.iter().zip(&payload.retained_tail) {
            for field in ["type", "id", "call_id", "name", "role"] {
                assert_eq!(
                    retained.get(field),
                    original.get(field),
                    "identity field {field} must remain unchanged"
                );
            }
        }
        assert_short_marker(
            payload.retained_tail[8]["output"].as_str().unwrap(),
            "media",
        );
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                <= u64::from(MIN_RETAIN_TOKENS)
        );
    }

    #[test]
    fn multiple_large_tool_outputs_fall_back_to_marker_only_until_under_target() {
        let mut input = vec![assistant_message("执行批量检查"), user_message("继续")];
        for index in 0..8 {
            let call_id = format!("call_{index}");
            input.push(function_call_item(&call_id, "shell_command"));
            input.push(function_call_output_item(
                &call_id,
                &format!("BEGIN_{index}{}END_{index}", "界".repeat(4_000)),
            ));
        }
        input.push(compaction_prompt_item());
        let request = json!({ "input": input });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );
        assert!(result.triggered);
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("trimmed compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        let marker_only_outputs = payload
            .retained_tail
            .iter()
            .filter(|item| {
                item.get("type").and_then(Value::as_str) == Some("function_call_output")
                    && item
                        .get("output")
                        .and_then(Value::as_str)
                        .is_some_and(|output| output.starts_with("<truncated:tool;~"))
            })
            .count();
        assert!(
            marker_only_outputs > 0,
            "the second pass must collapse at least one preview to marker-only"
        );
        for index in 0..8 {
            let call_id = format!("call_{index}");
            assert_eq!(
                payload
                    .retained_tail
                    .iter()
                    .filter(|item| {
                        item.get("call_id").and_then(Value::as_str) == Some(call_id.as_str())
                    })
                    .count(),
                2,
                "each tool call/output pair must remain present"
            );
        }
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                <= u64::from(MIN_RETAIN_TOKENS)
        );
    }

    #[test]
    fn physical_payload_limit_remains_a_hard_failure() {
        let request = json!({
            "model": "claude-sonnet-5",
            "input": [
                assistant_message("不可裁剪的助手原文"),
                user_message(&"界".repeat(700_000)),
                { "type": "compaction_trigger" }
            ]
        });
        let source = json!({
            "id": "resp_payload_too_large",
            "status": "completed",
            "model": "claude-sonnet-5",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "SUMMARY" }]
            }]
        });

        let rewritten = rewrite_remote_compaction_v2_response_with_layered_compaction(
            &request,
            &source,
            true,
            MIN_RETAIN_TOKENS,
        )
        .expect("remote compaction response");
        assert_eq!(rewritten.response["status"], "failed");
        assert_eq!(
            rewritten.response["error"]["code"],
            "local_compaction_payload_too_large"
        );
        assert_eq!(rewritten.response["output"], json!([]));
    }

    #[test]
    fn oversized_non_tool_tail_uses_configured_limit_as_soft_target() {
        let anchor = assistant_message("不可裁剪的助手原文");
        let user = user_message(&"界".repeat(30_000));
        let request = json!({
            "input": [
                anchor.clone(),
                user.clone(),
                compaction_prompt_item()
            ]
        });
        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            MIN_RETAIN_TOKENS,
            summary_sse("SUMMARY"),
        );

        assert!(result.triggered);
        assert!(!result.sse_text.contains("response.failed"));
        assert!(
            !result
                .sse_text
                .contains("local_compaction_retained_tail_too_large")
        );
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("soft-limit compaction response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");
        assert_eq!(payload.retained_tail, vec![anchor, user]);
        assert!(
            estimate_json_value_tokens(&Value::Array(payload.retained_tail))
                > u64::from(MIN_RETAIN_TOKENS),
            "non-tool dialogue remains intact even when the configured target cannot be met"
        );
    }

    #[test]
    fn claude_prefill_pause_depends_on_restored_tail_side() {
        let assistant_payload = StructuredLocalCompactionPayload {
            summary: "summary".to_string(),
            retained_tail: vec![
                user_message("按推荐处理"),
                assistant_message("已经开始执行"),
            ],
        };
        let assistant_item = synthetic_structured_compaction_item(&format!(
            "{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{}",
            serde_json::to_string(&assistant_payload).unwrap()
        ));
        let assistant_request = json!({
            "model": "claude-sonnet-5",
            "input": [user_message("older"), assistant_item]
        });
        assert!(local_compaction_requires_real_user(
            &assistant_request,
            "claude-sonnet-5"
        ));
        assert!(!local_compaction_requires_real_user(
            &assistant_request,
            "gpt-5.6"
        ));
        let legacy_v1_request = json!({
            "model": "claude-sonnet-5",
            "input": [
                user_message("older"),
                {
                    "type": "compaction",
                    "encrypted_content": format!(
                        "{REMOTE_COMPACTION_V2_LEGACY_BASE64_PREFIX}c3VtbWFyeQ"
                    )
                }
            ]
        });
        assert!(local_compaction_requires_real_user(
            &legacy_v1_request,
            "claude-sonnet-5"
        ));

        let user_payload = StructuredLocalCompactionPayload {
            summary: "summary".to_string(),
            retained_tail: vec![assistant_message("推荐方案"), user_message("按推荐处理")],
        };
        let user_request = json!({
            "model": "claude-sonnet-5",
            "input": [
                user_message("older"),
                synthetic_structured_compaction_item(&format!(
                    "{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{}",
                    serde_json::to_string(&user_payload).unwrap()
                ))
            ]
        });
        assert!(!local_compaction_requires_real_user(
            &user_request,
            "claude-sonnet-5"
        ));

        let tool_payload = StructuredLocalCompactionPayload {
            summary: "summary".to_string(),
            retained_tail: vec![
                user_message("按推荐处理"),
                function_call_item("call_1", "shell_command"),
                function_call_output_item("call_1", "ok"),
            ],
        };
        let tool_request = json!({
            "model": "claude-sonnet-5",
            "input": [
                user_message("older"),
                synthetic_structured_compaction_item(&format!(
                    "{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{}",
                    serde_json::to_string(&tool_payload).unwrap()
                ))
            ]
        });
        assert!(!local_compaction_requires_real_user(
            &tool_request,
            "claude-sonnet-5"
        ));
    }

    #[test]
    fn wait_for_user_response_has_no_output_items() {
        let request = json!({ "model": "claude-sonnet-5" });
        let response = local_compaction_wait_for_user_response(&request);
        assert_eq!(response["status"], "completed");
        assert_eq!(response["output"], json!([]));

        let sse = local_compaction_wait_for_user_sse(&request);
        assert!(sse.contains("event: response.completed"));
        assert!(!sse.contains("response.output_item.added"));
        assert!(!sse.contains("response.output_item.done"));
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
