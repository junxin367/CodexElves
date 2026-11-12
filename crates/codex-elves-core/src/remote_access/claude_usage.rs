//! Claude 订阅额度只经对应 WSL 内的原生 CLI 读取，不读取凭据或自行请求额度接口。
use super::{Claude, WslConfig, now_ms};
use anyhow::Context;
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const CACHE_TTL: Duration = Duration::from_secs(60);
const QUERY_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_CONTROL_FRAME: usize = 512 * 1024;

pub(super) struct CachedUsage {
    config: WslConfig,
    checked: Instant,
    response: Value,
}

impl Claude {
    pub async fn session_subscription_usage(
        &self,
        id: &str,
        allow_probe: bool,
    ) -> anyhow::Result<Value> {
        let conversation = self.conversation(id).await?;
        self.subscription_usage(conversation.config, allow_probe)
            .await
    }

    pub async fn subscription_usage(
        &self,
        config: WslConfig,
        allow_probe: bool,
    ) -> anyhow::Result<Value> {
        config.validate()?;
        let instance = config.instance_id();
        let lock = self.lock(&format!("subscription-usage:{instance}")).await;
        let _guard = lock.lock().await;
        if let Some(cached) = self.subscription_usage_cache.lock().await.get(&instance)
            && cached.config == config
            && cached.checked.elapsed() < CACHE_TTL
        {
            return Ok(cached.response.clone());
        }
        let empty = json!({"ok":true,"backend":"claude","instanceId":instance,"usage":null});
        // 只读设备可查看同一实例的新鲜缓存，但不能因此启动可能加载用户配置的 CLI。
        if !allow_probe {
            return Ok(empty);
        }
        let usage = match native_usage(&config).await {
            Ok(value) => normalize_usage(&value, now_ms()),
            // 旧 CLI、不适用的认证或读取失败均保持未知；不回退到凭据读取、HTTP 或第三方服务。
            Err(_) => Value::Null,
        };
        let mut response = empty;
        response["usage"] = usage;
        self.subscription_usage_cache.lock().await.insert(
            instance,
            CachedUsage {
                config,
                checked: Instant::now(),
                response: response.clone(),
            },
        );
        Ok(response)
    }
}

async fn native_usage(config: &WslConfig) -> anyhow::Result<Value> {
    let mut command = Claude::wsl(config)?;
    // 用户要求所有额度读取经过原 WSL 的 Claude：仅初始化与 get_usage，不发送 user 帧、不恢复任务。
    // 此专用元数据进程不持久化会话；只在本进程关闭 Hooks/MCP，原实例设置及正常执行能力保持原样。
    command.args([
        "--cd",
        "~",
        "--exec",
        &config.executable,
        "--print",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--strict-mcp-config",
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ]);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("无法启动 Claude 额度读取进程")?;
    let mut writer = child.stdin.take().context("Claude stdin 不可用")?;
    let mut reader = BufReader::new(child.stdout.take().context("Claude stdout 不可用")?);
    let outcome = tokio::time::timeout(QUERY_TIMEOUT, async {
        control(
            &mut reader,
            &mut writer,
            "quota-initialize",
            json!({"subtype":"initialize"}),
        )
        .await?;
        // 原生 get_usage 支持跳过行为分析；额度读取不得扫描近七天的全部会话正文。
        control(
            &mut reader,
            &mut writer,
            "quota-read",
            json!({"subtype":"get_usage","skip_behaviors":true}),
        )
        .await
    })
    .await;
    drop(writer);
    drop(reader);
    if tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .is_err()
    {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }
    outcome.context("Claude 额度读取超时")?
}

async fn control(
    reader: &mut (impl AsyncBufRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    id: &str,
    request: Value,
) -> anyhow::Result<Value> {
    let mut bytes =
        serde_json::to_vec(&json!({"type":"control_request","request_id":id,"request":request}))?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    for _ in 0..64 {
        let mut line = Vec::new();
        let size = tokio::io::AsyncReadExt::take(&mut *reader, (MAX_CONTROL_FRAME + 1) as u64)
            .read_until(b'\n', &mut line)
            .await?;
        anyhow::ensure!(
            size > 0 && size <= MAX_CONTROL_FRAME && line.ends_with(b"\n"),
            "Claude 额度控制响应不完整或过大"
        );
        let frame: Value = serde_json::from_slice(&line)?;
        if frame["type"] != "control_response" || frame["response"]["request_id"] != id {
            continue;
        }
        anyhow::ensure!(
            frame["response"]["subtype"] == "success",
            "Claude 未提供额度读取能力"
        );
        return Ok(frame["response"]["response"].clone());
    }
    anyhow::bail!("Claude 未返回对应额度控制响应")
}

fn normalize_usage(value: &Value, observed_at: u64) -> Value {
    // 原生 CLI 负责判断账号订阅；API Key、Bedrock、Vertex 或缺少 profile scope 时不显示额度。
    if value["rate_limits_available"] != true
        || !value["subscription_type"]
            .as_str()
            .is_some_and(|kind| !kind.trim().is_empty())
    {
        return Value::Null;
    }
    let mut result = json!({"source":"claude-native-usage","observedAtMs":observed_at});
    let mut count = 0;
    for (native, field) in [("five_hour", "fiveHour"), ("seven_day", "sevenDay")] {
        let window = &value["rate_limits"][native];
        let Some(percent) = window
            .get("used_percentage")
            .or_else(|| window.get("utilization"))
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
        else {
            continue;
        };
        // get_usage 返回 ISO 时间；兼容 statusline 的 Unix 秒数。只放行时间字符，网页再校验并处理过期。
        let reset = match &window["resets_at"] {
            Value::String(value)
                if (20..=40).contains(&value.len())
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || b"-+:TZ.".contains(&byte)) =>
            {
                json!(value)
            }
            Value::Number(value) if value.as_f64().is_some_and(|n| n.is_finite() && n > 0.0) => {
                json!(value)
            }
            _ => Value::Null,
        };
        result[field] = json!({"usedPercentage":percent,"resetsAt":reset});
        count += 1;
    }
    if count == 0 { Value::Null } else { result }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_account_plan_windows_are_exported_without_private_account_fields() {
        let raw = json!({"rate_limits_available":true,"subscription_type":"max",
            "account":{"token":"must-not-export"},"session":{"total_cost_usd":123},
            "rate_limits":{"five_hour":{"utilization":0,"resets_at":"2026-11-13T01:00:00Z"},
                "seven_day":{"utilization":52.4,"resets_at":1794618000},
                "extra_usage":{"used_credits":99}}});
        let result = normalize_usage(&raw, 1234);
        assert_eq!(result["fiveHour"]["usedPercentage"], 0.0);
        assert_eq!(result["sevenDay"]["usedPercentage"], 52.4);
        assert_eq!(result["observedAtMs"], 1234);
        assert!(!result.to_string().contains("must-not-export"));
        assert!(!result.to_string().contains("used_credits"));
        for changes in [
            json!({"rate_limits_available":false}),
            json!({"subscription_type":null}),
            json!({"subscription_type":""}),
            json!({"rate_limits":null}),
        ] {
            let mut invalid = raw.clone();
            for (key, value) in changes.as_object().unwrap() {
                invalid[key] = value.clone();
            }
            assert!(normalize_usage(&invalid, 1234).is_null());
        }
    }

    #[test]
    fn missing_and_invalid_windows_do_not_become_zero_percent() {
        let mut raw = json!({"rate_limits_available":true,"subscription_type":"pro",
            "rate_limits":{"five_hour":{"used_percentage":24,"resets_at":"secret-account-value"},
                "seven_day":{"utilization":101}}});
        let result = normalize_usage(&raw, 1234);
        assert_eq!(result["fiveHour"]["usedPercentage"], 24.0);
        assert!(result["fiveHour"]["resetsAt"].is_null());
        assert!(result.get("sevenDay").is_none());
        for invalid in [json!(-1), json!(true), json!("24"), Value::Null] {
            raw["rate_limits"]["five_hour"]["used_percentage"] = invalid;
            assert!(normalize_usage(&raw, 1234).is_null());
        }
    }

    #[tokio::test]
    async fn native_control_uses_matching_read_only_requests_and_rejects_errors() {
        let (client, server) = tokio::io::duplex(4096);
        let (read, mut writer) = tokio::io::split(client);
        let mut reader = BufReader::new(read);
        let task = tokio::spawn(async move {
            let (read, mut writer) = tokio::io::split(server);
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["type"], "control_request");
            assert_eq!(
                request["request"],
                json!({"subtype":"get_usage","skip_behaviors":true})
            );
            for frame in [
                json!({"type":"control_response","response":{"subtype":"success","request_id":"old","response":{"private":"discard"}}}),
                json!({"type":"control_response","response":{"subtype":"success","request_id":"usage","response":{"rate_limits_available":false}}}),
            ] {
                writer
                    .write_all(format!("{frame}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let result = control(
            &mut reader,
            &mut writer,
            "usage",
            json!({"subtype":"get_usage","skip_behaviors":true}),
        )
        .await
        .unwrap();
        assert_eq!(result, json!({"rate_limits_available":false}));
        task.await.unwrap();
        let mut reader = BufReader::new(
            &b"{\"type\":\"control_response\",\"response\":{\"request_id\":\"usage\",\"subtype\":\"error\",\"error\":\"private-error\"}}\n"[..],
        );
        let failure = control(&mut reader, &mut tokio::io::sink(), "usage", json!({}))
            .await
            .unwrap_err();
        assert!(!failure.to_string().contains("private-error"));
    }

    #[tokio::test]
    async fn cached_usage_stays_bound_to_instance_executable_and_freshness() {
        let directory = tempfile::tempdir().unwrap();
        let claude = Claude::new(directory.path()).unwrap();
        let config = WslConfig::default();
        let response = json!({"ok":true,"backend":"claude","instanceId":config.instance_id(),
            "usage":{"source":"claude-native-usage","observedAtMs":now_ms(),
                "fiveHour":{"usedPercentage":24,"resetsAt":null}}});
        claude.subscription_usage_cache.lock().await.insert(
            config.instance_id(),
            CachedUsage {
                config: config.clone(),
                checked: Instant::now(),
                response: response.clone(),
            },
        );
        assert_eq!(
            claude
                .subscription_usage(config.clone(), false)
                .await
                .unwrap(),
            response
        );
        for other in [
            WslConfig {
                user: "another-user".into(),
                ..config.clone()
            },
            WslConfig {
                distribution: "another-instance".into(),
                ..config.clone()
            },
            WslConfig {
                executable: "/other/claude".into(),
                ..config.clone()
            },
        ] {
            assert!(claude.subscription_usage(other, false).await.unwrap()["usage"].is_null());
        }
        claude
            .subscription_usage_cache
            .lock()
            .await
            .get_mut(&config.instance_id())
            .unwrap()
            .checked = Instant::now() - CACHE_TTL;
        assert!(claude.subscription_usage(config, false).await.unwrap()["usage"].is_null());
    }
}
