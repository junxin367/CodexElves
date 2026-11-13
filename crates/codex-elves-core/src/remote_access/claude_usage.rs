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
const PRICE_SOURCE: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const PRICE_TTL_MS: u64 = 24 * 60 * 60 * 1000;
const PRICE_RETRY: Duration = Duration::from_secs(5 * 60);
const MAX_PRICE_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct CachedUsage {
    config: WslConfig,
    checked: Instant,
    response: Value,
}

pub(super) struct CachedLocalUsage {
    config: WslConfig,
    checked: Instant,
    windows: Value,
    metrics: Value,
}

#[derive(Default)]
pub(super) struct PriceCache {
    loaded: bool,
    fetched_at: u64,
    attempted: Option<Instant>,
    models: Value,
}

impl Claude {
    pub async fn session_subscription_usage(
        &self,
        id: &str,
        allow_probe: bool,
        include_local: bool,
    ) -> anyhow::Result<Value> {
        let conversation = self.conversation(id).await?;
        self.subscription_usage(conversation.config, allow_probe, include_local)
            .await
    }

    pub async fn subscription_usage(
        &self,
        config: WslConfig,
        allow_probe: bool,
        include_local: bool,
    ) -> anyhow::Result<Value> {
        config.validate()?;
        let mut response = self.account_usage(&config, allow_probe).await?;
        if response["usage"].is_object() {
            // 账号百分比经原生 CLI；金额、Token、预测仅覆盖对应 WSL 的本地数值账本。
            // 用户要求弹窗缓存优先：普通轮询也附带已有新鲜详情，只有显式展开才允许扫描日志或更新价格。
            let metrics = if include_local {
                self.local_usage(&config, &response["usage"], allow_probe)
                    .await
            } else {
                self.cached_local_usage(&config, &response["usage"])
                    .await
                    .unwrap_or(Value::Null)
            };
            for key in ["fiveHour", "sevenDay"] {
                if response["usage"][key].is_object() && metrics[key].is_object() {
                    response["usage"][key]["metrics"] = metrics[key].clone();
                }
            }
        }
        Ok(response)
    }

    async fn account_usage(&self, config: &WslConfig, allow_probe: bool) -> anyhow::Result<Value> {
        let instance = config.instance_id();
        let lock = self.lock(&format!("subscription-usage:{instance}")).await;
        let _guard = lock.lock().await;
        if let Some(cached) = self.subscription_usage_cache.lock().await.get(&instance)
            && cached.config == *config
            && cached.checked.elapsed() < CACHE_TTL
        {
            return Ok(cached.response.clone());
        }
        let empty = json!({"ok":true,"backend":"claude","instanceId":instance,"usage":null});
        // 只读设备可查看同一实例的新鲜缓存，但不能因此启动可能加载用户配置的 CLI。
        if !allow_probe {
            return Ok(empty);
        }
        let usage = match native_usage(config).await {
            Ok(value) => normalize_usage(&value, now_ms()),
            // 旧 CLI、不适用的认证或读取失败均保持未知；不回退到凭据读取、HTTP 或第三方服务。
            Err(_) => Value::Null,
        };
        let mut response = empty;
        response["usage"] = usage;
        self.subscription_usage_cache.lock().await.insert(
            instance,
            CachedUsage {
                config: config.clone(),
                checked: Instant::now(),
                response: response.clone(),
            },
        );
        Ok(response)
    }

    async fn local_usage(&self, config: &WslConfig, usage: &Value, allow_probe: bool) -> Value {
        let instance = config.instance_id();
        let windows = local_windows(usage);
        if windows.as_array().is_none_or(Vec::is_empty) {
            return Value::Null;
        }
        let lock = self.lock(&format!("subscription-local:{instance}")).await;
        let _guard = lock.lock().await;
        if let Some(cached) = self.cached_local_usage(config, usage).await {
            return cached;
        }
        if !allow_probe {
            return Value::Null;
        }
        let prices = self.local_usage_prices().await;
        let metrics = local_metrics(config, windows.clone(), prices)
            .await
            .unwrap_or(Value::Null);
        // 失败只让详情显示未知；不清空原生额度，不阻断会话或反复启动扫描。
        self.local_usage_cache.lock().await.insert(
            instance,
            CachedLocalUsage {
                config: config.clone(),
                checked: Instant::now(),
                windows,
                metrics: metrics.clone(),
            },
        );
        metrics
    }

    async fn cached_local_usage(&self, config: &WslConfig, usage: &Value) -> Option<Value> {
        if let Some(cached) = self
            .local_usage_cache
            .lock()
            .await
            .get(&config.instance_id())
            && cached.config == *config
            && cached.windows == local_windows(usage)
            && cached.checked.elapsed() < CACHE_TTL
        {
            return Some(cached.metrics.clone());
        }
        None
    }

    async fn local_usage_prices(&self) -> Value {
        let mut cache = self.usage_prices.lock().await;
        let path = self.directory.join("claude-public-model-prices.json");
        let now = now_ms();
        if !cache.loaded {
            cache.loaded = true;
            if let Ok(bytes) = std::fs::read(&path)
                && bytes.len() <= MAX_PRICE_BYTES
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                && value["source"] == PRICE_SOURCE
                && let Some(fetched) = value["fetchedAtMs"]
                    .as_u64()
                    .filter(|at| *at <= now + 30_000)
                && let Some(models) = normalize_prices(&value["models"])
            {
                cache.fetched_at = fetched;
                cache.models = models;
            }
        }
        // 用户允许第三方价格表：只下载公开规则，绝不上传日志、Token、消息、账号或凭据。
        // 每 24 小时刷新，失败最多每 5 分钟重试并沿用旧表；无价格时仍可统计 Token。
        if (cache.models.is_null() || now.saturating_sub(cache.fetched_at) >= PRICE_TTL_MS)
            && cache.attempted.is_none_or(|at| at.elapsed() >= PRICE_RETRY)
        {
            cache.attempted = Some(Instant::now());
            if let Ok(models) = download_prices().await {
                cache.fetched_at = now_ms();
                cache.models = models;
                let stored = json!({"source":PRICE_SOURCE,"fetchedAtMs":cache.fetched_at,"models":cache.models});
                if let Ok(bytes) = serde_json::to_vec(&stored) {
                    let _ = crate::settings::atomic_write(&path, &bytes);
                }
            }
        }
        cache.models.clone()
    }
}

fn local_windows(usage: &Value) -> Value {
    json!(
        [("fiveHour", 5 * 60 * 60 * 1000u64), ("sevenDay", 7 * 24 * 60 * 60 * 1000u64)]
            .into_iter()
            .filter(|(key, _)| !usage[*key]["resetsAt"].is_null())
            .map(|(key, duration)| json!({"key":key,"durationMs":duration,"resetsAt":usage[key]["resetsAt"]}))
            .collect::<Vec<_>>()
    )
}

fn normalize_prices(value: &Value) -> Option<Value> {
    let mut models = serde_json::Map::new();
    for (name, row) in value.as_object()? {
        if name.len() > 256
            || (!row["input_cost_per_token"].is_number()
                && !row["output_cost_per_token"].is_number())
        {
            continue;
        }
        let mut prices = serde_json::Map::new();
        for (key, value) in row.as_object()? {
            if key.len() <= 160
                && (key.starts_with("input_cost_per_token")
                    || key.starts_with("output_cost_per_token")
                    || key.starts_with("cache_read_input_token_cost")
                    || key.starts_with("cache_creation_input_token_cost"))
                && value.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0)
            {
                prices.insert(key.clone(), value.clone());
            } else if key == "provider_specific_entry"
                && let Some(extra) = value.as_object()
            {
                let extra: serde_json::Map<String, Value> = extra
                    .iter()
                    .filter(|(key, value)| {
                        key.len() <= 64 && value.as_f64().is_some_and(|n| n.is_finite() && n > 0.0)
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                prices.insert(key.clone(), Value::Object(extra));
            }
        }
        if !prices.is_empty() {
            models.insert(name.clone(), Value::Object(prices));
        }
    }
    (!models.is_empty()).then_some(Value::Object(models))
}

async fn download_prices() -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(2))
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(6))
        .build()?;
    let mut response = client.get(PRICE_SOURCE).send().await?.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= MAX_PRICE_BYTES,
            "公开模型价格表过大"
        );
        bytes.extend_from_slice(&chunk);
    }
    normalize_prices(&serde_json::from_slice::<Value>(&bytes)?)
        .context("公开模型价格表不含可用价格")
}

async fn local_metrics(config: &WslConfig, windows: Value, prices: Value) -> anyhow::Result<Value> {
    let mut command = Claude::wsl(config)?;
    // 独立只读 Python 采集器在目标 WSL 运行；不执行 Claude、不操作原生日志或配置。
    // 扫描预算 12 秒，外层 16 秒覆盖 WSL 启动；连同原生查询与价格下载不超过网页 60 秒读取上限。
    let mut child = command
        .args([
            "--cd",
            "~",
            "--exec",
            "python3",
            "-I",
            "-c",
            include_str!("claude_usage_stats.py"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut writer = child.stdin.take().context("本地额度采集输入不可用")?;
    let mut reader = child.stdout.take().context("本地额度采集输出不可用")?;
    let value = tokio::time::timeout(Duration::from_secs(16), async {
        let request = json!({"nowMs":now_ms(),"windows":windows,"prices":prices});
        writer.write_all(&serde_json::to_vec(&request)?).await?;
        writer.shutdown().await?;
        drop(writer);
        let mut bytes = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(
            &mut tokio::io::AsyncReadExt::take(&mut reader, 32 * 1024 + 1),
            &mut bytes,
        )
        .await?;
        anyhow::ensure!(bytes.len() <= 32 * 1024, "本地额度采集响应过大");
        anyhow::ensure!(child.wait().await?.success(), "本地额度采集未完成");
        let value: Value = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(value.is_object(), "本地额度采集响应无效");
        Ok::<Value, anyhow::Error>(value)
    })
    .await??;
    Ok(value)
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
        .kill_on_drop(true)
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
                .subscription_usage(config.clone(), false, false)
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
            assert!(
                claude
                    .subscription_usage(other, false, false)
                    .await
                    .unwrap()["usage"]
                    .is_null()
            );
        }
        claude
            .subscription_usage_cache
            .lock()
            .await
            .get_mut(&config.instance_id())
            .unwrap()
            .checked = Instant::now() - CACHE_TTL;
        assert!(
            claude
                .subscription_usage(config, false, false)
                .await
                .unwrap()["usage"]
                .is_null()
        );
    }

    #[test]
    fn public_prices_only_export_numeric_pricing_rules() {
        let raw = json!({"sample-model":{"input_cost_per_token":0.000001,"output_cost_per_token":0.000005,
            "input_cost_per_token_above_100k_tokens_priority":0.000004,
            "cache_creation_input_token_cost_above_1hr":0.000002,
            "output_cost_per_token_flex":-1,"secret":"discard",
            "provider_specific_entry":{"fast":2,"us":1.1,"unexpected":"discard"}},
            "metadata":{"description":"discard"}});
        let prices = normalize_prices(&raw).unwrap();
        assert_eq!(prices["sample-model"]["provider_specific_entry"]["fast"], 2);
        assert_eq!(
            prices["sample-model"]["input_cost_per_token_above_100k_tokens_priority"],
            0.000004
        );
        assert!(
            prices["sample-model"]
                .get("output_cost_per_token_flex")
                .is_none()
        );
        assert!(!prices.to_string().contains("discard"));
        assert!(normalize_prices(&json!({})).is_none());
    }

    #[tokio::test]
    async fn public_prices_use_disk_cache_and_keep_previous_rules_during_retry_backoff() {
        let directory = tempfile::tempdir().unwrap();
        let models = json!({"sample-model":{"input_cost_per_token":0.000001,"output_cost_per_token":0.000005}});
        crate::settings::atomic_write(
            &directory.path().join("claude-public-model-prices.json"),
            &serde_json::to_vec(
                &json!({"source":PRICE_SOURCE,"fetchedAtMs":now_ms(),"models":models}),
            )
            .unwrap(),
        )
        .unwrap();
        let claude = Claude::new(directory.path()).unwrap();
        assert_eq!(claude.local_usage_prices().await, models);
        {
            let mut cache = claude.usage_prices.lock().await;
            cache.fetched_at = 1;
            cache.attempted = Some(Instant::now());
        }
        assert_eq!(
            claude.local_usage_prices().await,
            models,
            "失败重试期间沿用旧表，不能删除可用价格"
        );
    }

    #[tokio::test]
    async fn local_details_reuse_cache_without_scanning_and_stay_bound_to_instance_and_window() {
        let directory = tempfile::tempdir().unwrap();
        let claude = Claude::new(directory.path()).unwrap();
        let config = WslConfig::default();
        let response = json!({"ok":true,"backend":"claude","instanceId":config.instance_id(),
            "usage":{"source":"claude-native-usage","observedAtMs":now_ms(),
                "fiveHour":{"usedPercentage":24,"resetsAt":now_ms()+9000000}}});
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
                .subscription_usage(config.clone(), false, false)
                .await
                .unwrap(),
            response,
            "没有详情缓存时普通读取不能启动扫描"
        );
        let metrics = json!({"fiveHour":{"source":"claude-wsl-local-journal","usedTokens":110,"usedCostUsd":null}});
        claude.local_usage_cache.lock().await.insert(
            config.instance_id(),
            CachedLocalUsage {
                config: config.clone(),
                checked: Instant::now(),
                windows: local_windows(&response["usage"]),
                metrics: metrics.clone(),
            },
        );
        let cached = claude
            .subscription_usage(config.clone(), false, false)
            .await
            .unwrap();
        assert_eq!(cached["usage"]["fiveHour"]["metrics"], metrics["fiveHour"]);
        let detailed = claude
            .subscription_usage(config.clone(), false, true)
            .await
            .unwrap();
        assert_eq!(
            detailed["usage"]["fiveHour"]["metrics"],
            metrics["fiveHour"]
        );
        assert_eq!(cached, detailed, "普通读取可直接复用展开时的同一份详情");
        let mut changed = response["usage"].clone();
        changed["fiveHour"]["resetsAt"] = json!(now_ms() + 18000000);
        assert!(claude.local_usage(&config, &changed, false).await.is_null());
        let other = WslConfig {
            user: "other-user".into(),
            ..config.clone()
        };
        assert!(
            claude
                .local_usage(&other, &response["usage"], false)
                .await
                .is_null()
        );
        assert!(
            claude.usage_prices.lock().await.attempted.is_none(),
            "普通额度读取不下载价格、不扫描日志"
        );
        claude
            .local_usage_cache
            .lock()
            .await
            .get_mut(&config.instance_id())
            .unwrap()
            .checked = Instant::now() - CACHE_TTL;
        assert!(
            claude
                .subscription_usage(config.clone(), false, false)
                .await
                .unwrap()["usage"]["fiveHour"]["metrics"]
                .is_null(),
            "普通读取不返回已过期的详情缓存"
        );
    }

    #[tokio::test]
    async fn local_details_negative_cache_keeps_retry_backoff_without_loading_prices() {
        let directory = tempfile::tempdir().unwrap();
        let claude = Claude::new(directory.path()).unwrap();
        let config = WslConfig::default();
        let usage = json!({"fiveHour":{"usedPercentage":24,"resetsAt":now_ms()+9000000}});
        claude.local_usage_cache.lock().await.insert(
            config.instance_id(),
            CachedLocalUsage {
                config: config.clone(),
                checked: Instant::now(),
                windows: local_windows(&usage),
                metrics: Value::Null,
            },
        );
        assert_eq!(
            claude.cached_local_usage(&config, &usage).await,
            Some(Value::Null),
            "已缓存的失败与从未读取必须区分，避免反复扫描"
        );
        assert!(claude.local_usage(&config, &usage, true).await.is_null());
        assert!(claude.usage_prices.lock().await.attempted.is_none());
        let changed = WslConfig {
            executable: "/other/claude".into(),
            ..config
        };
        assert!(claude.cached_local_usage(&changed, &usage).await.is_none());
    }
}
