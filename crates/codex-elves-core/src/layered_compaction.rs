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

/// 压缩指令固定前缀：限定本轮只写交接摘要。写死在代码中，不随用户提示词变化。
///
/// 不得以 [`COMPACTION_PROMPT_PREFIX`] 开头，否则准备后的请求会被再次识别为 Codex 压缩请求。
pub const COMPACTION_INSTRUCTION_PREFIX: &str = "\
[Handoff checkpoint]\n\
This turn has one job: write a handoff summary of the conversation above for the agent that \
will resume the work.\n\
- Do not continue the task. Do not answer open questions, run the next step, or start new work.\n\
- Do not call tools. Do not write tool calls or commands in any text form.\n\
- If the history ends with tool results that were not handled yet, record them in the summary. \
Do not act on them.";

/// 压缩指令固定后缀：输出格式约束，优先于用户提示词中的任何输出格式要求。
pub const COMPACTION_INSTRUCTION_SUFFIX: &str = "\
[Output format - overrides every output-format instruction above]\n\
- You may first organize your notes inside <analysis></analysis>.\n\
- Then write the complete handoff summary inside <summary></summary>.\n\
- Only the text inside <summary></summary> is kept. Everything outside it is discarded.";

/// D 校验首次失败后，同模型重试使用的系统提示。
pub const COMPACTION_RETRY_SYSTEM_PROMPT: &str = "\
You are a handoff summary writer. You do not execute tasks and you have no tools.\n\
Follow the handoff checkpoint instruction in the supplied conversation. Write the complete \
handoff summary inside <summary></summary>.\n\
Never write tool calls or commands to run.";

const COMPACTION_TOOL_FIELDS: [&str; 3] = ["tools", "tool_choice", "parallel_tool_calls"];
const SUMMARY_OPEN_TAG: &str = "<summary>";
const SUMMARY_CLOSE_TAG: &str = "</summary>";
const ANALYSIS_OPEN_TAG: &str = "<analysis>";
const ANALYSIS_CLOSE_TAG: &str = "</analysis>";

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

// Codex legacy compaction 将摘要包装成 user 消息；只匹配完整的固定包装，
// 不能在普通提问、日志引用或多模态内容的任意位置查找 v3 标记。
pub(crate) const LEGACY_COMPACTION_SUMMARY_PREFIX: &str = "\
Another language model started to solve this problem and produced a summary of its thinking process. \
You also have access to the state of the tools that were used by that language model. \
Use this to build on the work that has already been done and avoid duplicating work. \
Here is the summary produced by the other language model, use the information in this summary to assist with your own analysis:";

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

/// 代理接管的压缩请求类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionKind {
    /// Codex 本地压缩：`input` 末项是 CONTEXT CHECKPOINT COMPACTION 指令。
    Legacy,
    /// Codex Remote Compaction V2：`input` 含 `compaction_trigger`。
    RemoteV2,
}

impl CompactionKind {
    pub fn of_request(request_json: &Value) -> Option<Self> {
        if is_compaction_request(Some(request_json)) {
            Some(Self::Legacy)
        } else if is_remote_compaction_v2_request(Some(request_json)) {
            Some(Self::RemoteV2)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::RemoteV2 => "remote_v2",
        }
    }

    fn control_kind(self) -> LocalCompactionControlKind {
        match self {
            Self::Legacy => LocalCompactionControlKind::LegacyPrompt,
            Self::RemoteV2 => LocalCompactionControlKind::RemoteV2Trigger,
        }
    }
}

/// 代理压缩的上游请求构造方式，按会话模型家族选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionRoute {
    /// A：Claude 家族。system、tools、thinking 等全部参数与原请求逐字段一致，
    /// 只把压缩触发项替换为压缩指令，以命中上游提示词缓存。
    CacheReuse,
    /// B：其他模型。沿用 Codex 本地压缩方式：主模型、主系统提示，移除工具字段。
    CodexLocal,
}

impl CompactionRoute {
    pub fn for_model(model: &str) -> Self {
        match crate::model_capabilities::model_family(model) {
            crate::model_capabilities::ModelFamily::Claude => Self::CacheReuse,
            _ => Self::CodexLocal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CacheReuse => "cache_reuse",
            Self::CodexLocal => "codex_local",
        }
    }
}

/// 一次压缩最多两次尝试：首次按路由构造，D 校验失败后同模型重试一次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionAttempt {
    First,
    Retry,
}

impl CompactionAttempt {
    pub const ALL: [Self; 2] = [Self::First, Self::Retry];

    pub fn number(self) -> u8 {
        match self {
            Self::First => 1,
            Self::Retry => 2,
        }
    }
}

/// 压缩请求构造与封装所需的设置快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionOptions {
    /// 用户提示词；空字符串表示使用项目默认提示词。
    pub user_prompt: String,
    /// 是否补回最近一轮原始记录（v3 结构化载荷）。
    pub retain_recent_round: bool,
    pub retain_tokens: u32,
}

impl Default for CompactionOptions {
    fn default() -> Self {
        Self {
            user_prompt: String::new(),
            retain_recent_round: false,
            retain_tokens: DEFAULT_RETAIN_TOKENS,
        }
    }
}

impl CompactionOptions {
    /// 仅供总开关开启后的压缩执行器使用；调用方必须在关闭时绕过整个压缩增强链路。
    pub fn from_settings(settings: &crate::settings::BackendSettings) -> Self {
        let enabled = settings.layered_compaction_enabled;
        Self {
            user_prompt: settings.layered_compaction_prompt_override.clone(),
            retain_recent_round: enabled && settings.layered_compaction_retain_recent_round_enabled,
            retain_tokens: settings.layered_compaction_retain_tokens,
        }
    }
}

/// C：固定前缀 + 用户提示词（为空时用项目默认提示词）+ 固定后缀。
pub fn compaction_instruction(user_prompt: &str) -> String {
    format!(
        "{COMPACTION_INSTRUCTION_PREFIX}\n\n{}\n\n{COMPACTION_INSTRUCTION_SUFFIX}",
        effective_compaction_prompt(user_prompt)
    )
}

/// 按路由和尝试序号构造发往上游的压缩请求（Responses 格式，协议转换由调用方完成）。
pub fn prepare_compaction_attempt_request(
    request_json: &Value,
    kind: CompactionKind,
    route: CompactionRoute,
    options: &CompactionOptions,
    attempt: CompactionAttempt,
) -> Value {
    let first = match route {
        CompactionRoute::CacheReuse => prepare_cache_reuse_request(request_json, kind, options),
        CompactionRoute::CodexLocal => prepare_codex_local_request(request_json, kind, options),
    };
    match attempt {
        CompactionAttempt::First => first,
        CompactionAttempt::Retry => compaction_retry_request(&first),
    }
}

/// A：只替换压缩触发项。历史中的合成压缩项保持原样，由协议转换按主请求相同规则展开，
/// 保证转换后的 system / tools / messages 前缀与主请求逐字节一致。
fn prepare_cache_reuse_request(
    request_json: &Value,
    kind: CompactionKind,
    options: &CompactionOptions,
) -> Value {
    let instruction = compaction_instruction_item(&compaction_instruction(&options.user_prompt));
    let mut request = request_json.clone();
    let Some(input) = request
        .as_object_mut()
        .and_then(|object| object.get_mut("input"))
    else {
        return request;
    };
    match input {
        Value::Array(items) => {
            let control = kind.control_kind();
            if !options.retain_recent_round {
                let controls = items
                    .iter()
                    .enumerate()
                    .filter(|(index, item)| {
                        is_local_compaction_control_item(items, *index, item, control)
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                for index in controls {
                    items[index] = instruction.clone();
                }
                return request;
            }
            let mut conversation = items
                .iter()
                .enumerate()
                .filter(|(index, item)| {
                    !is_local_compaction_control_item(items, *index, item, control)
                })
                .map(|(_, item)| item.clone())
                .collect::<Vec<_>>();
            if options.retain_recent_round {
                let cut = cache_reuse_retained_tail_len(request_json, &conversation, control);
                conversation.truncate(conversation.len() - cut);
            }
            conversation.push(instruction);
            *items = conversation;
        }
        Value::Object(_) if is_remote_compaction_v2_trigger(input) => *input = instruction,
        _ => {}
    }
    request
}

/// A 路径补回最近一轮时只截末尾：返回原始 input 末尾与封装时保留尾部完全一致的条数。
///
/// 保留尾部按封装侧的展开视图计算；若尾部延伸进上一次合成压缩的载荷内部，只截与原始
/// input 末尾逐项相同的部分，其余仍留给摘要模型，宁可重复也不丢上下文。
fn cache_reuse_retained_tail_len(
    request_json: &Value,
    conversation: &[Value],
    control: LocalCompactionControlKind,
) -> usize {
    let expanded = expand_synthetic_local_compaction_request(request_json);
    let Some(expanded_input) = expanded.get("input").and_then(Value::as_array) else {
        return 0;
    };
    let tail = split_local_compaction_input(expanded_input, control).retained_tail;
    conversation
        .iter()
        .rev()
        .zip(tail.iter().rev())
        .take_while(|(raw, retained)| raw == retained)
        .count()
}

/// B：展开合成压缩历史、按需摘除最近一轮，替换触发项为压缩指令并移除工具字段。
fn prepare_codex_local_request(
    request_json: &Value,
    kind: CompactionKind,
    options: &CompactionOptions,
) -> Value {
    let instruction = compaction_instruction_item(&compaction_instruction(&options.user_prompt));
    let mut request = expand_synthetic_local_compaction_request(request_json);
    let Some(object) = request.as_object_mut() else {
        return request_json.clone();
    };
    if let Some(input) = object.get_mut("input") {
        match input {
            Value::Array(items) => {
                let control = kind.control_kind();
                *items = if options.retain_recent_round {
                    split_local_compaction_input(items, control).summary_input
                } else {
                    items
                        .iter()
                        .enumerate()
                        .filter(|(index, item)| {
                            !is_local_compaction_control_item(items, *index, item, control)
                        })
                        .map(|(_, item)| item.clone())
                        .collect()
                };
                items.push(instruction);
            }
            Value::Object(_) if is_remote_compaction_v2_trigger(input) => *input = instruction,
            _ => {}
        }
    }
    for key in COMPACTION_TOOL_FIELDS {
        object.remove(key);
    }
    request
}

/// D 的重试请求：历史与压缩指令不变，system 换成交接摘要撰写者说明并移除工具字段。
pub fn compaction_retry_request(first_attempt_request: &Value) -> Value {
    let mut request = first_attempt_request.clone();
    let Some(object) = request.as_object_mut() else {
        return request;
    };
    object.insert(
        "instructions".to_string(),
        json!(COMPACTION_RETRY_SYSTEM_PROMPT),
    );
    if object.contains_key("system") {
        object.insert("system".to_string(), json!(COMPACTION_RETRY_SYSTEM_PROMPT));
    }
    if let Some(Value::Array(items)) = object.get_mut("input") {
        items.retain(|item| {
            !matches!(
                item.get("role").and_then(Value::as_str),
                Some("system" | "developer")
            )
        });
    }
    for key in COMPACTION_TOOL_FIELDS {
        object.remove(key);
    }
    request
}

/// 协议转换层的兜底：未经代理压缩执行器准备的 V2 请求按模型路由、默认提示词转换，
/// 保证 `compaction_trigger` 不会原样发给不支持它的上游。非 V2 请求原样返回。
pub fn prepare_remote_compaction_v2_bridge_request(request_json: &Value) -> Value {
    if !is_remote_compaction_v2_request(Some(request_json)) {
        return request_json.clone();
    }
    prepare_compaction_attempt_request(
        request_json,
        CompactionKind::RemoteV2,
        CompactionRoute::for_model(request_json["model"].as_str().unwrap_or_default()),
        &CompactionOptions::default(),
        CompactionAttempt::First,
    )
}

pub fn prepare_remote_compaction_v2_bridge_request_with_prompt(
    request_json: &Value,
    prompt_override: Option<&str>,
) -> Value {
    prepare_remote_compaction_v2_bridge_request_with_options(
        request_json,
        prompt_override,
        prompt_override.is_some(),
    )
}

pub fn prepare_remote_compaction_v2_bridge_request_with_options(
    request_json: &Value,
    prompt_override: Option<&str>,
    retain_recent_round: bool,
) -> Value {
    if !is_remote_compaction_v2_request(Some(request_json)) {
        return request_json.clone();
    }
    prepare_compaction_attempt_request(
        request_json,
        CompactionKind::RemoteV2,
        CompactionRoute::for_model(request_json["model"].as_str().unwrap_or_default()),
        &CompactionOptions {
            user_prompt: prompt_override.unwrap_or_default().to_string(),
            retain_recent_round,
            ..Default::default()
        },
        CompactionAttempt::First,
    )
}

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

fn extract_compaction_summary_text(response_object: &Value) -> Option<String> {
    validate_compaction_response(response_object).ok()
}

fn compaction_instruction_item(prompt: &str) -> Value {
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
        })
        .map(|(_, item)| item.clone())
        .collect::<Vec<_>>();
    let Some(last_user) = conversation.iter().rposition(is_real_user_message) else {
        return LocalCompactionSplit {
            summary_input: conversation,
            retained_tail: Vec::new(),
        };
    };
    let mut retained_start = conversation[..last_user]
        .iter()
        .rposition(|item| {
            is_visible_assistant_message(item) && !is_historical_compaction_summary_item(item)
        })
        .unwrap_or(last_user);
    // reasoning 位于回答正文之前；同一次 assistant 输出还可能含 commentary、
    // 多条 message 或工具调用。保留整个连续 assistant 段，不能从正文中间切开。
    while retained_start > 0
        && effective_history_side(&conversation[retained_start - 1])
            == Some(EffectiveHistorySide::Assistant)
        && !is_historical_compaction_summary_item(&conversation[retained_start - 1])
    {
        retained_start -= 1;
    }
    let summary_input = conversation[..retained_start]
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) != Some("reasoning"))
        .cloned()
        .collect();
    LocalCompactionSplit {
        summary_input,
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
    expand_synthetic_local_compaction_request_with_summary_role(request_json, "assistant")
}

/// thinking 历史不能把没有原始思考的合成摘要当成模型回答。
/// 摘要以带来源说明的历史上下文传入，原始 retained_tail 的角色和内容保持不变。
pub(crate) fn expand_synthetic_local_compaction_request_as_context(request_json: &Value) -> Value {
    expand_synthetic_local_compaction_request_with_summary_role(request_json, "user")
}

fn expand_synthetic_local_compaction_request_with_summary_role(
    request_json: &Value,
    summary_role: &str,
) -> Value {
    let mut request = request_json.clone();
    let Some(input) = request
        .as_object_mut()
        .and_then(|object| object.get_mut("input"))
        .and_then(Value::as_array_mut)
    else {
        return request;
    };
    *input = expand_synthetic_local_compaction_items(input, summary_role);
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

fn expand_synthetic_local_compaction_items(input: &[Value], summary_role: &str) -> Vec<Value> {
    let mut expanded = Vec::with_capacity(input.len());
    for item in input {
        let Some(payload) = synthetic_local_compaction_payload(item) else {
            expanded.push(item.clone());
            continue;
        };
        remove_codex_retained_duplicates(&mut expanded, &payload.retained_tail);
        if let Some(summary) = historical_compaction_summary_item(&payload.summary, summary_role) {
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
            let text = match item.get("content")? {
                Value::String(text) => text.as_str(),
                Value::Array(parts)
                    if parts.len() == 1
                        && matches!(
                            parts[0].get("type").and_then(Value::as_str),
                            Some("input_text" | "text")
                        ) =>
                {
                    parts[0].get("text")?.as_str()?
                }
                _ => return None,
            };
            let encoded = text.trim().strip_prefix(LEGACY_COMPACTION_SUMMARY_PREFIX)?;
            structured_local_compaction_payload(encoded)
        }
        _ => None,
    }
}

fn structured_local_compaction_payload(text: &str) -> Option<StructuredLocalCompactionPayload> {
    let payload = text
        .trim()
        .strip_prefix(LOCAL_COMPACTION_V3_STRUCTURED_PREFIX)?;
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

fn historical_compaction_summary_item(summary: &str, role: &str) -> Option<Value> {
    historical_compaction_summary_text(summary).map(|text| {
        json!({
            "type": "message",
            "role": role,
            "content": [{
                "type": if role == "user" { "input_text" } else { "output_text" },
                "text": text
            }]
        })
    })
}

fn is_historical_compaction_summary_item(item: &Value) -> bool {
    item.get("role").and_then(Value::as_str) == Some("assistant")
        && item_text(item).starts_with(REMOTE_COMPACTION_V2_HISTORY_HEADER)
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
    if let (Some(candidate_id), Some(retained_id)) = (candidate_id, retained_id) {
        return candidate_id == retained_id;
    }
    let candidate_turn = candidate
        .pointer("/internal_chat_message_metadata_passthrough/turn_id")
        .and_then(Value::as_str)
        .filter(|turn_id| !turn_id.is_empty());
    let retained_turn = retained
        .pointer("/internal_chat_message_metadata_passthrough/turn_id")
        .and_then(Value::as_str)
        .filter(|turn_id| !turn_id.is_empty());
    if let (Some(candidate_turn), Some(retained_turn)) = (candidate_turn, retained_turn)
        && candidate_turn != retained_turn
    {
        return false;
    }
    // 无可比较的消息 ID 时，只去重完整内容一致的副本；同一轮也可以有多条
    // 不同消息，正文相同的截图、音频等更不能按文本当成同一条。
    candidate.get("content").is_some_and(|content| {
        !retained_detail_is_empty(content) && Some(content) == retained.get("content")
    })
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

/// D 校验失败原因。只看响应结构与 `<summary>` 标签，不按任何模型特有文本判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionValidationFailure {
    /// 上游没有返回可解析的终止响应（流中断、格式错误、多个终止事件等）。
    NoTerminalResponse,
    /// 终止状态不是 completed。
    NotCompleted,
    /// output 含工具调用项。
    ToolCallOutput,
    /// assistant 文本中没有完整的 `<summary></summary>`。
    SummaryTagMissing,
    /// `<summary>` 内去空白后为空。
    SummaryEmpty,
}

impl CompactionValidationFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoTerminalResponse => "no_terminal_response",
            Self::NotCompleted => "not_completed",
            Self::ToolCallOutput => "tool_call_output",
            Self::SummaryTagMissing => "summary_tag_missing",
            Self::SummaryEmpty => "summary_empty",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::NoTerminalResponse => "the upstream returned no valid terminal response",
            Self::NotCompleted => "the upstream response did not complete",
            Self::ToolCallOutput => "the upstream response contains tool call output items",
            Self::SummaryTagMissing => "the upstream response has no <summary></summary> block",
            Self::SummaryEmpty => "the <summary> block is empty",
        }
    }
}

/// Responses output 中被视为工具调用的 item 类型。
fn is_tool_call_output_item(item: &Value) -> bool {
    let Some(kind) = item.get("type").and_then(Value::as_str) else {
        return false;
    };
    kind.ends_with("_call")
        || kind.ends_with("_call_output")
        || matches!(
            kind,
            "tool_use"
                | "server_tool_use"
                | "tool_call"
                | "tool_result"
                | "tool_search_output"
                | "mcp_list_tools"
        )
}

fn contains_tool_call_output(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            is_tool_call_output_item(value) || object.values().any(contains_tool_call_output)
        }
        Value::Array(items) => items.iter().any(contains_tool_call_output),
        _ => false,
    }
}

/// D：校验压缩响应并只取 `<summary>` 内文本。
///
/// 通过条件：completed；无工具调用输出项；assistant 文本中存在 `<summary></summary>` 且去空白后非空。
pub fn validate_compaction_response(
    response_object: &Value,
) -> Result<String, CompactionValidationFailure> {
    if response_object.get("status").and_then(Value::as_str) != Some("completed") {
        return Err(CompactionValidationFailure::NotCompleted);
    }
    let output = response_object
        .get("output")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if output.iter().any(contains_tool_call_output) {
        return Err(CompactionValidationFailure::ToolCallOutput);
    }
    let text = output
        .iter()
        .filter(|item| {
            item.get("type").and_then(Value::as_str) == Some("message")
                && item.get("role").and_then(Value::as_str) == Some("assistant")
        })
        .map(item_text)
        .collect::<Vec<_>>()
        .join("\n");
    let summary =
        extract_summary_block(&text).ok_or(CompactionValidationFailure::SummaryTagMissing)?;
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(CompactionValidationFailure::SummaryEmpty);
    }
    Ok(summary.to_string())
}

/// 取 `<analysis>` 块之外第一个完整 `<summary>` 块，闭合标签后的文本不进入载荷。
fn extract_summary_block(text: &str) -> Option<&str> {
    let mut cursor = 0;
    loop {
        let rest = &text[cursor..];
        let summary_at = rest.find(SUMMARY_OPEN_TAG)?;
        match rest.find(ANALYSIS_OPEN_TAG) {
            Some(analysis_at) if analysis_at < summary_at => {
                let after_analysis = cursor + analysis_at + ANALYSIS_OPEN_TAG.len();
                let close = text[after_analysis..].find(ANALYSIS_CLOSE_TAG)?;
                cursor = after_analysis + close + ANALYSIS_CLOSE_TAG.len();
            }
            _ => {
                let start = cursor + summary_at + SUMMARY_OPEN_TAG.len();
                let end = start + text[start..].find(SUMMARY_CLOSE_TAG)?;
                return Some(&text[start..end]);
            }
        }
    }
}

/// 从完整 Responses SSE 中取唯一的终止响应并做 D 校验。
pub fn validate_compaction_sse(
    sse_text: &str,
) -> (Option<Value>, Result<String, CompactionValidationFailure>) {
    let normalized = sse_text.replace("\r\n", "\n").replace('\r', "\n");
    match extract_single_remote_compaction_v2_terminal_response(&normalized) {
        Ok(response) => {
            let tool_event = normalized.split("\n\n").any(|block| {
                let data = block
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join("\n");
                serde_json::from_str::<Value>(&data)
                    .ok()
                    .is_some_and(|event| {
                        event.get("item").is_some_and(contains_tool_call_output)
                            || event["type"].as_str().is_some_and(|kind| {
                                kind.starts_with("response.function_call_arguments.")
                                    || kind.starts_with("response.custom_tool_call_input.")
                            })
                    })
            });
            let verdict = if tool_event {
                Err(CompactionValidationFailure::ToolCallOutput)
            } else {
                validate_compaction_response(&response)
            };
            (Some(response), verdict)
        }
        Err(_) => (None, Err(CompactionValidationFailure::NoTerminalResponse)),
    }
}

/// HTTP 与 WebSocket 使用相同的尝试日志；缺失 usage 保持 null，不伪造为零。
pub(crate) fn log_compaction_attempt(
    transport: &str,
    kind: CompactionKind,
    route: CompactionRoute,
    attempt: CompactionAttempt,
    model: &str,
    response: Option<&Value>,
    verdict: &Result<String, CompactionValidationFailure>,
    error_detail: Option<&str>,
) {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "protocol_proxy.compaction_attempt",
        json!({
            "transport": transport,
            "kind": kind.as_str(),
            "route": route.as_str(),
            "attempt": attempt.number(),
            "model": model,
            "inputTokens": response.and_then(|r| r.pointer("/usage/input_tokens")),
            "cachedTokens": response.and_then(|r| r.pointer("/usage/input_tokens_details/cached_tokens")),
            "outputTokens": response.and_then(|r| r.pointer("/usage/output_tokens")),
            "validation": if verdict.is_ok() { "passed" } else { "failed" },
            "failureReason": verdict.as_ref().err().map(|failure| failure.as_str()),
            "error": error_detail,
        }),
    );
}

/// 压缩执行结果写回 Codex 时使用的载荷形态。
#[derive(Debug, Clone)]
pub struct CompactionPayloadResult {
    /// 成功时为 completed 响应，失败时为 `status: failed` 且 output 为空的响应。
    pub response: Value,
    pub layered: LayeredCompactionStats,
}

/// 校验通过后按现有载荷格式封装：
///
/// - V2：唯一 `compaction` item，补回关闭时为 v2 明文，开启时为 v3 结构化载荷；
/// - legacy：唯一 assistant message，补回关闭时为摘要明文，开启时为 v3 结构化载荷。
pub fn build_compaction_success_response(
    original_request: &Value,
    kind: CompactionKind,
    response_object: &Value,
    options: &CompactionOptions,
) -> CompactionPayloadResult {
    // 格式校验属于代码责任；封装入口本身也不接受未经校验的上游响应。
    let summary = match validate_compaction_response(response_object) {
        Ok(summary) => summary,
        Err(failure) => {
            return CompactionPayloadResult {
                response: compaction_validation_failure_response(
                    original_request,
                    kind,
                    Some(response_object),
                    failure,
                ),
                layered: LayeredCompactionStats::default(),
            };
        }
    };
    let summary = summary.as_str();
    let summary_with_path = directories::BaseDirs::new()
        .and_then(|dirs| {
            verified_rollout_path(
                original_request,
                &dirs.home_dir().join(".codex").join("sessions"),
            )
        })
        .map(|path| format!("{summary}\n\n完整会话记录：{}", path.display()));
    let summary = summary_with_path.as_deref().unwrap_or(summary);
    let structured = if options.retain_recent_round {
        match build_structured_local_compaction(original_request, summary, options.retain_tokens) {
            Ok(build) => build,
            Err(error) => {
                return CompactionPayloadResult {
                    response: remote_compaction_v2_failure_response(
                        original_request,
                        Some(response_object),
                        error.code(),
                        &error.message(),
                    ),
                    layered: LayeredCompactionStats::default(),
                };
            }
        }
    } else {
        None
    };
    let (output_item, layered) = match (kind, structured) {
        (CompactionKind::RemoteV2, Some(build)) => (
            synthetic_structured_compaction_item(&build.encoded),
            build.stats,
        ),
        (CompactionKind::RemoteV2, None) => (
            synthetic_remote_compaction_item(summary),
            LayeredCompactionStats::default(),
        ),
        (CompactionKind::Legacy, Some(build)) => (
            compaction_summary_message_item(response_object, &build.encoded),
            build.stats,
        ),
        (CompactionKind::Legacy, None) => (
            compaction_summary_message_item(response_object, summary),
            LayeredCompactionStats::default(),
        ),
    };
    let mut response = response_object.clone();
    if let Some(object) = response.as_object_mut() {
        object.insert("status".to_string(), json!("completed"));
        object.insert("output".to_string(), json!([output_item]));
        object.remove("error");
        object.remove("incomplete_details");
    }
    CompactionPayloadResult { response, layered }
}

/// E：只接受 UUID cache key、真实存在且首行 session_meta.id 匹配的唯一 rollout。
/// 不按时间猜路径，不使用 archived_sessions，也不读取会话正文。
fn verified_rollout_path(
    request: &Value,
    sessions: &std::path::Path,
) -> Option<std::path::PathBuf> {
    use std::io::{BufRead, BufReader, Read};
    let key = request.get("prompt_cache_key")?.as_str()?;
    uuid::Uuid::parse_str(key).ok()?;
    let root = sessions.canonicalize().ok()?;
    let suffix = format!("-{key}.jsonl");
    let mut directories = vec![(sessions.to_path_buf(), 0)];
    let mut matches = Vec::new();
    while let Some((directory, depth)) = directories.pop() {
        for entry in std::fs::read_dir(directory).ok()?.flatten() {
            let kind = entry.file_type().ok()?;
            if kind.is_dir() && depth < 3 {
                directories.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                let name = entry.file_name();
                let name = name.to_str()?;
                if name.starts_with("rollout-") && name.ends_with(&suffix) {
                    let path = entry.path();
                    if !path.canonicalize().ok()?.starts_with(&root) {
                        continue;
                    }
                    let mut line = String::new();
                    BufReader::new(std::fs::File::open(&path).ok()?.take(256 * 1024))
                        .read_line(&mut line)
                        .ok()?;
                    let meta: Value = serde_json::from_str(&line).ok()?;
                    if meta["type"] == "session_meta"
                        && meta.pointer("/payload/id")?.as_str()? == key
                    {
                        matches.push(path);
                    }
                }
            }
        }
    }
    (matches.len() == 1).then(|| matches.remove(0))
}

/// D 两次尝试均失败时返回给 Codex 的失败响应（output 为空，不含任何摘要或工具调用）。
pub fn compaction_validation_failure_response(
    original_request: &Value,
    kind: CompactionKind,
    response_object: Option<&Value>,
    failure: CompactionValidationFailure,
) -> Value {
    let (code_prefix, label) = match kind {
        CompactionKind::Legacy => ("layered_compaction", "Context compaction"),
        CompactionKind::RemoteV2 => ("remote_compaction", "Remote Compaction V2 bridge"),
    };
    remote_compaction_v2_failure_response(
        original_request,
        response_object,
        &format!("{code_prefix}_{}", failure.as_str()),
        &format!(
            "{label} failed after {} attempts: {}.",
            CompactionAttempt::ALL.len(),
            failure.message()
        ),
    )
}

/// 把压缩结果（成功或失败）转成完整 Responses SSE。
pub fn compaction_payload_sse(kind: CompactionKind, response: &Value) -> String {
    if response.get("status").and_then(Value::as_str) != Some("completed") {
        return build_responses_sse_for_remote_compaction_failure(response);
    }
    match kind {
        CompactionKind::RemoteV2 => build_responses_sse_for_compaction(response),
        CompactionKind::Legacy => {
            let text = extract_message_text(response).unwrap_or_default();
            build_responses_sse_for_message(response, &text)
        }
    }
}

fn compaction_summary_message_item(response_object: &Value, text: &str) -> Value {
    let response_id = response_object
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_compaction");
    let item_id = existing_message_item_id(response_object)
        .and_then(|id| crate::protocol_proxy::normalize_responses_message_item_id(&id))
        .unwrap_or_else(|| crate::protocol_proxy::response_message_item_id(response_id));
    json!({
        "id": item_id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": text, "annotations": [] }]
    })
}

/// 分层压缩结果。
#[derive(Debug, Clone)]
pub struct LayeredCompactionResult {
    /// 最终回注给 Codex 的 Responses SSE 文本。
    pub sse_text: String,
    /// 是否成功写入保留尾部；失败时为 false，但 sse_text 仍须透传失败结果。
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
    prepare_legacy_layered_compaction_request_with_options(request_json, prompt_override, true)
}

/// 关闭最近一轮补回时只替换压缩提示词，不从摘要请求里摘除原始尾部。
pub fn prepare_legacy_layered_compaction_request_with_options(
    request_json: &Value,
    prompt_override: &str,
    retain_recent_round: bool,
) -> Value {
    if !is_compaction_request(Some(request_json)) {
        return request_json.clone();
    }
    prepare_compaction_attempt_request(
        request_json,
        CompactionKind::Legacy,
        CompactionRoute::for_model(request_json["model"].as_str().unwrap_or_default()),
        &CompactionOptions {
            user_prompt: prompt_override.to_string(),
            retain_recent_round,
            ..Default::default()
        },
        CompactionAttempt::First,
    )
}

/// 判断请求是否属于任一种上下文压缩（传统压缩或 Remote Compaction V2）。
pub fn is_any_compaction_request(request_json: &Value) -> bool {
    is_compaction_request(Some(request_json)) || is_remote_compaction_v2_request(Some(request_json))
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

/// 判断 completed Responses 对象是否包含传统压缩可用的 assistant 摘要文本。
pub fn has_completed_compaction_summary(response_object: &Value) -> bool {
    validate_compaction_response(response_object).is_ok()
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
/// - `enabled` 为 false、非压缩请求、或无法解析终止响应时，原样返回。
/// - completed 响应缺少有效摘要时返回失败，避免丢失未发往摘要模型的保留尾部。
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
    let Some(summary) = extract_compaction_summary_text(&response_object) else {
        return LayeredCompactionResult::unchanged(compaction_failure_sse(
            request_json,
            Some(&response_object),
            "layered_compaction_summary_missing",
            "Layered compaction received no summary text from the upstream model.",
        ));
    };

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

    fn tagged_response(text: &str) -> Value {
        json!({"status":"completed","output":[{
            "type":"message","role":"assistant","content":[{"type":"output_text","text":text}]
        }]})
    }

    #[test]
    fn compaction_contract_rollout_requires_existing_file_and_matching_session_metadata() {
        let root = tempfile::tempdir().unwrap();
        let key = "01a0ea47-0de4-72e2-be5b-dfd63296920c";
        let request = json!({"prompt_cache_key":key});
        assert!(verified_rollout_path(&request, root.path()).is_none());
        let day = root.path().join("2026/09/29");
        std::fs::create_dir_all(&day).unwrap();
        let file = day.join(format!("rollout-2026-09-29T07-08-40-{key}.jsonl"));
        std::fs::write(
            &file,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"different\"}}\n",
        )
        .unwrap();
        assert!(verified_rollout_path(&request, root.path()).is_none());
        std::fs::write(
            &file,
            format!("{}\n", json!({"type":"session_meta","payload":{"id":key}})),
        )
        .unwrap();
        assert_eq!(verified_rollout_path(&request, root.path()), Some(file));
        assert!(
            verified_rollout_path(&json!({"prompt_cache_key":"../invalid"}), root.path()).is_none()
        );
    }

    #[test]
    fn compaction_contract_routes_and_preserves_cache_prefix() {
        assert!(model_supports_native_remote_compaction_v2("gpt-5.4"));
        assert_eq!(
            CompactionRoute::for_model("claude-opus-5-5"),
            CompactionRoute::CacheReuse
        );
        assert_eq!(
            CompactionRoute::for_model("deepseek-v4.1-flash"),
            CompactionRoute::CodexLocal
        );
        let request = json!({
            "model":"claude-opus-5-5","instructions":"main system","system":"main",
            "tools":[{"type":"function","name":"read","parameters":{"type":"object"}}],
            "tool_choice":"auto","parallel_tool_calls":true,
            "thinking":{"type":"adaptive"},"output_config":{"effort":"max"},
            "reasoning":{"effort":"max"},"prompt_cache_key":"unchanged",
            "input":[user_message("earlier"),{"type":"reasoning","summary":[{"text":"reasoning"}]},
                assistant_message("done"),user_message("next"),{"type":"compaction_trigger"}]
        });
        let prepared = prepare_compaction_attempt_request(
            &request,
            CompactionKind::RemoteV2,
            CompactionRoute::CacheReuse,
            &CompactionOptions::default(),
            CompactionAttempt::First,
        );
        for (key, value) in request.as_object().unwrap() {
            if key != "input" {
                assert_eq!(&prepared[key], value, "{key}");
            }
        }
        assert_eq!(
            &prepared["input"].as_array().unwrap()[..4],
            &request["input"].as_array().unwrap()[..4]
        );
        let with_tail = prepare_compaction_attempt_request(
            &request,
            CompactionKind::RemoteV2,
            CompactionRoute::CacheReuse,
            &CompactionOptions {
                retain_recent_round: true,
                ..Default::default()
            },
            CompactionAttempt::First,
        );
        let kept = with_tail["input"].as_array().unwrap().len() - 1;
        assert_eq!(
            &with_tail["input"].as_array().unwrap()[..kept],
            &request["input"].as_array().unwrap()[..kept]
        );
        let local = prepare_compaction_attempt_request(
            &request,
            CompactionKind::RemoteV2,
            CompactionRoute::CodexLocal,
            &CompactionOptions::default(),
            CompactionAttempt::First,
        );
        for field in COMPACTION_TOOL_FIELDS {
            assert!(local.get(field).is_none());
        }
        assert_eq!(local["model"], request["model"]);
        assert_eq!(local["instructions"], request["instructions"]);
        let retry = compaction_retry_request(&prepared);
        assert_eq!(retry["input"], prepared["input"]);
        assert_eq!(retry["model"], prepared["model"]);
        assert_eq!(retry["instructions"], COMPACTION_RETRY_SYSTEM_PROMPT);
        for field in COMPACTION_TOOL_FIELDS {
            assert!(retry.get(field).is_none());
        }
    }

    #[test]
    fn compaction_contract_instruction_order_and_strict_validation() {
        assert_eq!(
            compaction_instruction("custom"),
            format!("{COMPACTION_INSTRUCTION_PREFIX}\n\ncustom\n\n{COMPACTION_INSTRUCTION_SUFFIX}")
        );
        assert_eq!(
            compaction_instruction(""),
            format!(
                "{COMPACTION_INSTRUCTION_PREFIX}\n\n{DEFAULT_COMPACTION_PROMPT}\n\n{COMPACTION_INSTRUCTION_SUFFIX}"
            )
        );
        // 已确认事故形态：completed 的任务继续执行文本，不能被当作摘要。
        for text in [
            "我先把相关文件找出来。",
            "<analysis>notes</analysis>",
            "   ",
        ] {
            assert_eq!(
                validate_compaction_response(&tagged_response(text)),
                Err(CompactionValidationFailure::SummaryTagMissing)
            );
        }
        assert_eq!(
            validate_compaction_response(&tagged_response("<summary> \n </summary>")),
            Err(CompactionValidationFailure::SummaryEmpty)
        );
        assert_eq!(
            validate_compaction_response(&tagged_response(
                "<analysis>example <summary>discard</summary></analysis><summary> kept </summary>outside"
            )),
            Ok("kept".to_string())
        );
        let tool_sse = format!(
            "data: {}\n\ndata: {}\n\n",
            json!({"type":"response.output_item.added","item":{"type":"function_call"}}),
            json!({"type":"response.completed","response":tagged_response("<summary>done</summary>")})
        );
        assert_eq!(
            validate_compaction_sse(&tool_sse).1,
            Err(CompactionValidationFailure::ToolCallOutput)
        );
        let mut response = tagged_response("<summary>done</summary>");
        for kind in [
            "function_call",
            "custom_tool_call",
            "mcp_call",
            "web_search_call",
            "tool_use",
        ] {
            response["output"].as_array_mut().unwrap().truncate(1);
            response["output"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":kind}));
            assert_eq!(
                validate_compaction_response(&response),
                Err(CompactionValidationFailure::ToolCallOutput)
            );
        }
        response["output"].as_array_mut().unwrap().truncate(1);
        response["status"] = json!("incomplete");
        assert_eq!(
            validate_compaction_response(&response),
            Err(CompactionValidationFailure::NotCompleted)
        );
        for kind in [CompactionKind::Legacy, CompactionKind::RemoteV2] {
            let failed = compaction_validation_failure_response(
                &json!({}),
                kind,
                Some(&response),
                CompactionValidationFailure::SummaryTagMissing,
            );
            assert_eq!(failed["status"], "failed");
            assert_eq!(failed["output"], json!([]));
            let sse = compaction_payload_sse(kind, &failed);
            assert!(sse.contains("event: response.failed"));
            assert!(!sse.contains("event: response.completed"));
        }
    }

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

    #[test]
    fn legacy_completed_without_summary_fails_instead_of_losing_the_retained_tail() {
        let request = json!({"input":[
            user_message("earlier"), assistant_message("anchor"),
            user_message("current task"), compaction_prompt_item()
        ]});
        for content in [
            json!([]),
            json!([{
                "type":"message","role":"assistant",
                "content":[{"type":"output_text","text":" \n "}]
            }]),
        ] {
            let source = format!(
                "event: response.completed\ndata: {}\n\n",
                json!({
                    "type":"response.completed",
                    "response":{"id":"resp-empty","status":"completed","output":content}
                })
            );
            let result = apply_layered_compaction_to_responses_sse(
                &request,
                true,
                DEFAULT_RETAIN_TOKENS,
                source,
            );
            assert!(!result.triggered);
            assert!(
                result
                    .sse_text
                    .contains("layered_compaction_summary_missing")
            );
            assert!(!result.sse_text.contains("event: response.completed"));
        }
    }

    #[test]
    fn retained_duplicates_require_consistent_identity_and_complete_content() {
        let mut original = user_message("check this screenshot");
        original["id"] = json!("msg-a");
        original["content"].as_array_mut().unwrap().push(json!({
            "type":"input_image","image_url":"https://example.invalid/a.png"
        }));
        let mut other = original.clone();
        other["id"] = json!("msg-b");
        assert!(!retained_message_matches(&original, &other));
        original.as_object_mut().unwrap().remove("id");
        other.as_object_mut().unwrap().remove("id");
        other["content"][1]["image_url"] = json!("https://example.invalid/b.png");
        original["internal_chat_message_metadata_passthrough"] = json!({"turn_id":"turn-1"});
        other["internal_chat_message_metadata_passthrough"] = json!({"turn_id":"turn-1"});
        assert!(!retained_message_matches(&original, &other));
        assert!(retained_message_matches(&original, &original));
        other = original.clone();
        other["internal_chat_message_metadata_passthrough"]["turn_id"] = json!("turn-2");
        assert!(!retained_message_matches(&original, &other));
    }

    #[test]
    fn quoted_compaction_payload_remains_a_user_message() {
        let encoded = format!(
            "{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{}",
            json!({
                "summary":"quoted summary", "retained_tail":[user_message("quoted user")]
            })
        );
        for text in [
            encoded.clone(),
            format!("Analyze this log: {encoded}"),
            format!("```json\n{encoded}\n```"),
        ] {
            let request = json!({"input":[user_message(&text)]});
            assert!(!contains_synthetic_local_compaction(&request));
            assert_eq!(expand_synthetic_local_compaction_request(&request), request);
        }
        assert!(structured_local_compaction_payload(&format!("log: {encoded}")).is_none());
    }

    #[test]
    fn compaction_decoder_accepts_protocol_and_legacy_wrapper_but_not_mixed_content() {
        let retained = user_message("current task");
        let encoded = format!(
            "{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{}",
            json!({
                "summary":"earlier summary", "retained_tail":[retained.clone()]
            })
        );
        let wrapped = format!("{LEGACY_COMPACTION_SUMMARY_PREFIX}\n\n{encoded}");
        for item in [
            synthetic_structured_compaction_item(&encoded),
            user_message(&wrapped),
            json!({"role":"user","content":wrapped}),
        ] {
            let request = json!({"input":[item]});
            assert!(contains_synthetic_local_compaction(&request));
            let expanded = expand_synthetic_local_compaction_request(&request);
            assert_eq!(
                expanded["input"].as_array().unwrap().last(),
                Some(&retained)
            );
            assert!(
                !expanded
                    .to_string()
                    .contains(LOCAL_COMPACTION_V3_STRUCTURED_PREFIX)
            );
        }
        let mut mixed = user_message(&wrapped);
        mixed["content"].as_array_mut().unwrap().push(json!({
            "type":"input_image","image_url":"https://example.invalid/screenshot.png"
        }));
        for item in [mixed, user_message(&format!("Please inspect:\n{wrapped}"))] {
            let request = json!({"input":[item]});
            assert!(!contains_synthetic_local_compaction(&request));
            assert_eq!(expand_synthetic_local_compaction_request(&request), request);
        }
    }

    #[test]
    fn expansion_does_not_remove_another_user_image_with_the_same_caption() {
        let mut first = user_message("check screenshot");
        first["id"] = json!("msg-a");
        first["content"].as_array_mut().unwrap().push(json!({
            "type":"input_image","image_url":"https://example.invalid/a.png"
        }));
        let mut retained = first.clone();
        retained["id"] = json!("msg-b");
        retained["content"][1]["image_url"] = json!("https://example.invalid/b.png");
        let encoded = format!(
            "{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{}",
            json!({
                "summary":"earlier summary","retained_tail":[retained]
            })
        );
        let request = json!({"input":[
            user_message("other question"),first.clone(),synthetic_structured_compaction_item(&encoded)
        ]});
        let expanded = expand_synthetic_local_compaction_request(&request);
        assert!(expanded["input"].as_array().unwrap().contains(&first));
        assert!(
            expanded
                .to_string()
                .contains("https://example.invalid/b.png")
        );
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
                "content": [{ "type": "output_text", "text": format!("<summary>{summary}</summary>"), "annotations": [] }]
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
    fn project_default_prompt_preserves_continuation_execution_point() {
        assert!(DEFAULT_COMPACTION_PROMPT.starts_with(
            "You are writing a continuation checkpoint for an agent task that is still in progress."
        ));
        assert!(!DEFAULT_COMPACTION_PROMPT.starts_with('#'));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("the exact point where work stopped"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("## Execution point"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("## Do not redo"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("BLOCKING"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("DEFAULTED"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("DEFERRED"));
        assert!(DEFAULT_COMPACTION_PROMPT.contains("## Silent final check (do not output)"));
    }

    #[test]
    fn remote_compaction_v2_bridge_replaces_trigger_and_preserves_claude_tools() {
        let rewritten =
            prepare_remote_compaction_v2_bridge_request(&remote_compaction_v2_request());
        assert_eq!(rewritten["tools"], remote_compaction_v2_request()["tools"]);
        assert_eq!(
            rewritten["tool_choice"],
            remote_compaction_v2_request()["tool_choice"]
        );
        assert_eq!(
            rewritten["parallel_tool_calls"],
            remote_compaction_v2_request()["parallel_tool_calls"]
        );
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "user");
        assert!(item_text(&input[1]).starts_with(COMPACTION_INSTRUCTION_PREFIX));
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
            compaction_instruction("CUSTOM LAYERED COMPACTION PROMPT")
        );
    }

    #[test]
    fn remote_compaction_v2_bridge_uses_project_default_for_blank_override() {
        let rewritten = prepare_remote_compaction_v2_bridge_request_with_prompt(
            &remote_compaction_v2_request(),
            Some(""),
        );
        let input = rewritten.get("input").and_then(Value::as_array).unwrap();
        assert_eq!(item_text(input.last().unwrap()), compaction_instruction(""));
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
                    "content": [{ "type": "output_text", "text": "<summary>SUMMARY</summary>", "annotations": [] }]
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
        assert_eq!(rewritten["status"], "failed");
        assert_eq!(rewritten["output"], json!([]));
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
                "content": [{ "type": "output_text", "text": "<summary>SUMMARY</summary>", "annotations": [] }]
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
                "content": [{ "type": "output_text", "text": "<summary>SUMMARY</summary>" }]
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

        assert_eq!(item_text(input.last().unwrap()), compaction_instruction(""));
        assert!(rewritten.get("tools").is_none());
        assert!(rewritten.get("tool_choice").is_none());
        assert!(rewritten.get("parallel_tool_calls").is_none());
        assert_eq!(
            item_text(&request["input"][1]),
            item_text(&compaction_prompt_item())
        );
    }

    #[test]
    fn legacy_prompt_only_keeps_recent_original_items_in_summary_request() {
        let request = json!({
            "input": [
                user_message("earlier request"),
                assistant_message("recent answer"),
                user_message("recent request"),
                compaction_prompt_item()
            ]
        });
        let rewritten = prepare_legacy_layered_compaction_request_with_options(
            &request,
            "CUSTOM PROMPT",
            false,
        );
        let input = rewritten["input"].as_array().unwrap();
        assert_eq!(&input[..3], &request["input"].as_array().unwrap()[..3]);
        assert_eq!(
            item_text(input.last().unwrap()),
            compaction_instruction("CUSTOM PROMPT")
        );
    }

    #[test]
    fn bridged_remote_prompt_only_keeps_recent_original_items() {
        let request = json!({
            "input": [
                user_message("earlier request"),
                assistant_message("recent answer"),
                user_message("recent request"),
                { "type": "compaction_trigger" }
            ]
        });
        let rewritten = prepare_remote_compaction_v2_bridge_request_with_options(
            &request,
            Some("CUSTOM PROMPT"),
            false,
        );
        let input = rewritten["input"].as_array().unwrap();
        assert_eq!(&input[..3], &request["input"].as_array().unwrap()[..3]);
        assert_eq!(
            item_text(input.last().unwrap()),
            compaction_instruction("CUSTOM PROMPT")
        );
    }

    #[test]
    fn prompt_only_compaction_expands_previous_structured_history_once() {
        let retained_user = user_message("previous recent request");
        let retained_answer = assistant_message("previous recent answer");
        let encoded = format!(
            "{LOCAL_COMPACTION_V3_STRUCTURED_PREFIX}{}",
            json!({
                "summary": "previous summary",
                "retained_tail": [retained_user.clone(), retained_answer.clone()]
            })
        );
        let request = json!({
            "input": [
                user_message("older request"),
                retained_user.clone(),
                synthetic_structured_compaction_item(&encoded),
                compaction_prompt_item()
            ]
        });
        let prepared =
            prepare_legacy_layered_compaction_request_with_options(&request, "NEW PROMPT", false);
        let input = prepared["input"].as_array().unwrap();

        assert_eq!(
            input.iter().filter(|item| *item == &retained_user).count(),
            1
        );
        assert!(input.contains(&retained_answer));
        assert!(prepared.to_string().contains("previous summary"));
        assert!(
            !prepared
                .to_string()
                .contains(LOCAL_COMPACTION_V3_STRUCTURED_PREFIX)
        );
        assert_eq!(
            item_text(input.last().unwrap()),
            compaction_instruction("NEW PROMPT")
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
        assert_eq!(item_text(&input[1]), compaction_instruction(""));
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
    fn structured_payload_retains_reasoning_with_the_assistant_tail() {
        let reasoning = json!({
            "type": "reasoning",
            "id": "rs_1",
            "summary": [{ "type": "summary_text", "text": "保留思考" }],
            "encrypted_content": "sig_1"
        });
        let commentary = assistant_message("正在整理结果");
        let anchor = assistant_message("上一轮回答");
        let user = user_message("继续处理");
        let request = json!({
            "input": [
                user_message("更早历史"),
                reasoning.clone(),
                commentary.clone(),
                anchor.clone(),
                user.clone(),
                compaction_prompt_item()
            ]
        });

        let result = apply_layered_compaction_to_responses_sse(
            &request,
            true,
            DEFAULT_RETAIN_TOKENS,
            summary_sse("较早历史摘要"),
        );
        let response = crate::continue_thinking::extract_terminal_response_object(&result.sse_text)
            .expect("structured response");
        let encoded = extract_message_text(&response).expect("structured payload text");
        let payload =
            structured_local_compaction_payload(&encoded).expect("v3 payload should decode");

        assert!(payload.retained_tail.contains(&reasoning));
        assert_eq!(
            payload.retained_tail,
            vec![reasoning, commentary, anchor, user],
            "reasoning before the retained assistant answer must not be cut off"
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
                "content": [{ "type": "output_text", "text": "<summary>SUMMARY</summary>" }]
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
                "content": [{ "type": "output_text", "text": "<summary>SUMMARY</summary>" }]
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
