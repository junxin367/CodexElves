use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
use futures_util::future::{AbortHandle, Abortable};
use serde_json::{Value, json};

use crate::protocol_proxy::{
    UpstreamResponseProtocol, anthropic_message_to_response_with_request_and_diagnostic_id,
    chat_completion_to_response_with_request, open_responses_proxy_request_with_settings,
    responses_error_from_upstream,
};
use crate::settings::BackendSettings;

pub const PROMPT_OPTIMIZE_PATH: &str = "/prompt-optimize";
pub const PROMPT_OPTIMIZE_CANCEL_PATH: &str = "/prompt-optimize/cancel";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REQUEST_ID_CHARS: usize = 128;
const MAX_MODEL_CHARS: usize = 256;
const MAX_SYSTEM_PROMPT_CHARS: usize = 20_000;
const MAX_INPUT_CHARS: usize = 100_000;
const MAX_ERROR_CHARS: usize = 600;

static PROMPT_OPTIMIZE_SERVICE: OnceLock<PromptOptimizeService> = OnceLock::new();

pub fn service() -> &'static PromptOptimizeService {
    PROMPT_OPTIMIZE_SERVICE.get_or_init(PromptOptimizeService::default)
}

pub struct PromptOptimizeService {
    pending: Mutex<HashMap<String, AbortHandle>>,
    timeout: Duration,
}

impl Default for PromptOptimizeService {
    fn default() -> Self {
        Self::with_timeout(DEFAULT_TIMEOUT)
    }
}

impl PromptOptimizeService {
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            timeout,
        }
    }

    pub async fn optimize(
        &self,
        mut settings: BackendSettings,
        payload: Value,
    ) -> anyhow::Result<Value> {
        let request = PromptOptimizeRequest::from_payload(&payload)?;
        for relay in &mut settings.relay_profiles {
            relay.system_prompt_override.clear();
        }
        settings.layered_compaction_model_override_enabled = false;

        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| anyhow::anyhow!("优化请求状态不可用"))?;
            if pending.contains_key(&request.request_id) {
                anyhow::bail!("该优化请求正在进行中");
            }
            pending.insert(request.request_id.clone(), abort_handle);
        }

        let _ = crate::diagnostic_log::append_diagnostic_log(
            "prompt_optimize.started",
            json!({
                "requestId": request.request_id,
                "model": request.model,
                "reasoningEffort": request.reasoning_effort,
            }),
        );

        let request_id = request.request_id.clone();
        let model = request.model.clone();
        let reasoning_effort = request.reasoning_effort.clone();
        let result = tokio::time::timeout(
            self.timeout,
            Abortable::new(execute_optimize(settings, request), abort_registration),
        )
        .await;
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&request_id);
        }

        match result {
            Err(_) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "prompt_optimize.timed_out",
                    json!({
                        "requestId": request_id,
                        "model": model,
                        "reasoningEffort": reasoning_effort,
                    }),
                );
                anyhow::bail!("优化超时，原输入未改动")
            }
            Ok(Err(_)) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "prompt_optimize.cancelled",
                    json!({
                        "requestId": request_id,
                        "model": model,
                        "reasoningEffort": reasoning_effort,
                    }),
                );
                Ok(json!({
                    "status": "cancelled",
                    "requestId": request_id,
                }))
            }
            Ok(Ok(Err(error))) => {
                let _ = crate::diagnostic_log::append_diagnostic_log(
                    "prompt_optimize.failed",
                    json!({
                        "requestId": request_id,
                        "model": model,
                        "reasoningEffort": reasoning_effort,
                    }),
                );
                Err(error)
            }
            Ok(Ok(Ok(result))) => Ok(json!({
                "status": "ok",
                "requestId": request_id,
                "text": result.text,
                "protocol": result.protocol,
                "diagnosticId": result.diagnostic_id,
            })),
        }
    }

    pub fn cancel(&self, payload: &Value) -> anyhow::Result<Value> {
        let request_id =
            required_string(payload, "requestId", "请求编号", MAX_REQUEST_ID_CHARS, true)?;
        let cancelled = {
            let pending = self
                .pending
                .lock()
                .map_err(|_| anyhow::anyhow!("优化请求状态不可用"))?;
            pending.get(&request_id).is_some_and(|handle| {
                handle.abort();
                true
            })
        };
        Ok(json!({
            "status": "ok",
            "requestId": request_id,
            "cancelled": cancelled,
        }))
    }
}

struct PromptOptimizeRequest {
    request_id: String,
    model: String,
    reasoning_effort: String,
    system_prompt: String,
    input: String,
}

impl PromptOptimizeRequest {
    fn from_payload(payload: &Value) -> anyhow::Result<Self> {
        let request_id =
            required_string(payload, "requestId", "请求编号", MAX_REQUEST_ID_CHARS, true)?;
        let model = required_string(payload, "model", "模型", MAX_MODEL_CHARS, true)?;
        let reasoning_effort =
            required_string(payload, "reasoningEffort", "思考深度", 16, true)?.to_ascii_lowercase();
        anyhow::ensure!(
            matches!(
                reasoning_effort.as_str(),
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
            ),
            "不支持的思考深度"
        );
        let system_prompt = required_string(
            payload,
            "systemPrompt",
            "系统提示词",
            MAX_SYSTEM_PROMPT_CHARS,
            false,
        )?;
        let input = required_string(payload, "input", "待优化提示词", MAX_INPUT_CHARS, false)?;
        Ok(Self {
            request_id,
            model,
            reasoning_effort,
            system_prompt,
            input,
        })
    }

    fn responses_request(&self) -> Value {
        json!({
            "model": self.model,
            "instructions": self.system_prompt,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": self.input,
                }],
            }],
            "reasoning": {
                "effort": self.reasoning_effort,
            },
            "max_output_tokens": 4096,
            "stream": false,
        })
    }
}

struct PromptOptimizeResult {
    text: String,
    protocol: UpstreamResponseProtocol,
    diagnostic_id: String,
}

async fn execute_optimize(
    settings: BackendSettings,
    request: PromptOptimizeRequest,
) -> anyhow::Result<PromptOptimizeResult> {
    let original_request = request.responses_request();
    let request_body = serde_json::to_string(&original_request)?;
    let upstream = open_responses_proxy_request_with_settings(&request_body, settings).await?;
    let status_code = upstream.status_code;
    let content_type = upstream.content_type.clone();
    let protocol = upstream.response_protocol;
    let diagnostic_id = upstream.diagnostic_id.clone();
    let succeeded = upstream.is_success();
    let response_body = upstream
        .into_body_bytes()
        .await
        .with_context(|| format!("读取优化响应失败（诊断 ID：{diagnostic_id}）"))?;

    if !succeeded {
        let error = responses_error_from_upstream(status_code, &content_type, &response_body);
        let message = error
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("上游优化请求失败");
        anyhow::bail!(
            "上游优化请求失败（HTTP {status_code}）：{}",
            compact_text(message, MAX_ERROR_CHARS)
        );
    }

    let upstream_json: Value = serde_json::from_slice(&response_body)
        .with_context(|| format!("优化响应不是有效 JSON（诊断 ID：{diagnostic_id}）"))?;
    let response = match protocol {
        UpstreamResponseProtocol::Responses => upstream_json,
        UpstreamResponseProtocol::ChatCompletions => {
            chat_completion_to_response_with_request(upstream_json, &original_request)?
        }
        UpstreamResponseProtocol::Anthropic => {
            anthropic_message_to_response_with_request_and_diagnostic_id(
                upstream_json,
                &original_request,
                Some(&diagnostic_id),
            )?
        }
    };
    let text = strip_outer_markdown_fence(&extract_assistant_text(&response)?);

    let _ = crate::diagnostic_log::append_diagnostic_log(
        "prompt_optimize.completed",
        json!({
            "requestId": request.request_id,
            "model": request.model,
            "reasoningEffort": request.reasoning_effort,
            "responseProtocol": protocol,
            "diagnosticId": diagnostic_id,
            "statusCode": status_code,
        }),
    );

    Ok(PromptOptimizeResult {
        text,
        protocol,
        diagnostic_id,
    })
}

fn required_string(
    payload: &Value,
    key: &str,
    label: &str,
    max_chars: usize,
    trim: bool,
) -> anyhow::Result<String> {
    let raw = payload
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{label}不能为空"))?;
    let value = if trim { raw.trim() } else { raw };
    anyhow::ensure!(!value.trim().is_empty(), "{label}不能为空");
    anyhow::ensure!(value.chars().count() <= max_chars, "{label}过长");
    Ok(value.to_string())
}

fn extract_assistant_text(response: &Value) -> anyhow::Result<String> {
    if let Some(text) = response
        .get("output_text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Ok(text.to_string());
    }

    let mut chunks = Vec::new();
    if let Some(output) = response.get("output").and_then(Value::as_array) {
        for item in output {
            if item.get("type").and_then(Value::as_str) != Some("message") {
                continue;
            }
            if item
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|role| role != "assistant")
            {
                continue;
            }
            if let Some(content) = item.get("content").and_then(Value::as_array) {
                for part in content {
                    if !matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("output_text" | "text")
                    ) {
                        continue;
                    }
                    if let Some(text) = part
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        chunks.push(text.to_string());
                    }
                }
            }
        }
    }
    anyhow::ensure!(!chunks.is_empty(), "优化响应没有可写入的文本");
    Ok(chunks.join("\n\n"))
}

fn strip_outer_markdown_fence(input: &str) -> String {
    let trimmed = input.trim();
    let mut lines = trimmed.lines().collect::<Vec<_>>();
    if lines.len() < 3 {
        return trimmed.to_string();
    }
    let opening = lines.first().copied().unwrap_or_default().trim();
    let closing = lines.last().copied().unwrap_or_default().trim();
    let fence = if opening.starts_with("```") {
        "```"
    } else if opening.starts_with("~~~") {
        "~~~"
    } else {
        return trimmed.to_string();
    };
    if closing != fence {
        return trimmed.to_string();
    }
    lines.remove(0);
    lines.pop();
    lines.join("\n").trim().to_string()
}

fn compact_text(input: &str, max_chars: usize) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{PromptOptimizeRequest, extract_assistant_text, strip_outer_markdown_fence};
    use serde_json::json;

    #[test]
    fn request_validation_and_shape_keep_prompt_text_out_of_transport_metadata() {
        let request = PromptOptimizeRequest::from_payload(&json!({
            "requestId": "request-1",
            "model": "gpt-5.6-sol",
            "reasoningEffort": "medium",
            "systemPrompt": "Only improve the draft.",
            "input": "  keep my spacing  ",
        }))
        .unwrap();
        let body = request.responses_request();

        assert_eq!(body["model"], "gpt-5.6-sol");
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["instructions"], "Only improve the draft.");
        assert_eq!(
            body["input"][0]["content"][0]["text"],
            "  keep my spacing  "
        );
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn assistant_text_extraction_ignores_reasoning_and_joins_text_parts() {
        let response = json!({
            "output": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "private"}]},
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "first"},
                        {"type": "output_text", "text": "second"}
                    ]
                }
            ]
        });

        assert_eq!(
            extract_assistant_text(&response).unwrap(),
            "first\n\nsecond"
        );
    }

    #[test]
    fn outer_markdown_fence_is_removed_without_touching_internal_fences() {
        let output = "```markdown\nDo this:\n```rust\nfn main() {}\n```\n```";
        assert_eq!(
            strip_outer_markdown_fence(output),
            "Do this:\n```rust\nfn main() {}\n```"
        );
        assert_eq!(
            strip_outer_markdown_fence("Use ```inline``` text"),
            "Use ```inline``` text"
        );
    }
}
