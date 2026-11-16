use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

use super::super::RemoteAuthorization;
use super::super::claude::{Claude, WslConfig};
use super::Settings;

const TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT: u64 = 256 * 1024;

fn schema() -> Value {
    json!({"type":"object","properties":{"title":{"type":"string"},"description":{"type":"string"}},
        "required":["title","description"],"additionalProperties":false})
}

async fn write(
    service: &crate::codex_service::RunningService,
    frame: Value,
    authorization: &RemoteAuthorization,
) -> anyhow::Result<()> {
    let mut bytes = serde_json::to_vec(&frame)?;
    bytes.push(b'\n');
    crate::service_transport::guarded_write(&service.stdin, bytes, Some(authorization), TIMEOUT)
        .await
}

async fn rpc(
    service: &mut crate::codex_service::RunningService,
    id: u64,
    method: &str,
    params: Value,
    authorization: &RemoteAuthorization,
    mut events: Option<&mut std::collections::VecDeque<Value>>,
) -> anyhow::Result<Value> {
    write(
        service,
        json!({"id":id,"method":method,"params":params}),
        authorization,
    )
    .await?;
    loop {
        let frame = frame(service).await?;
        if frame["id"] != id {
            if let Some(events) = events.as_mut() {
                anyhow::ensure!(events.len() < 256, "命名引擎提前返回过多事件");
                events.push_back(frame);
            }
            continue;
        }
        if frame.get("error").is_some() {
            bail!(
                "命名引擎拒绝请求：{}",
                frame["error"]["message"].as_str().unwrap_or("原生请求失败")
            );
        }
        return Ok(frame["result"].clone());
    }
}

async fn frame(service: &mut crate::codex_service::RunningService) -> anyhow::Result<Value> {
    let mut line = String::new();
    let bytes = (&mut service.stdout)
        .take(MAX_OUTPUT + 1)
        .read_line(&mut line)
        .await?;
    anyhow::ensure!(
        bytes > 0 && bytes as u64 <= MAX_OUTPUT,
        "命名引擎已退出或输出超过限制"
    );
    Ok(serde_json::from_str(&line)?)
}

pub(super) async fn codex(
    settings: &Settings,
    input: &Value,
    effort: Option<&str>,
    authorization: &RemoteAuthorization,
) -> anyhow::Result<String> {
    let service = crate::codex_service::spawn_service().await?;
    codex_with_service(service, settings, input, effort, authorization).await
}

async fn codex_with_service(
    mut service: crate::codex_service::RunningService,
    settings: &Settings,
    input: &Value,
    effort: Option<&str>,
    authorization: &RemoteAuthorization,
) -> anyhow::Result<String> {
    let outcome = tokio::time::timeout(TIMEOUT, async {
        rpc(&mut service, 1, "initialize", json!({"clientInfo":{"name":"codex-elves-naming",
            "version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}), authorization, None).await?;
        write(&service, json!({"method":"initialized","params":{}}), authorization).await?;
        let saved = rpc(&mut service, 2, "config/read", json!({"includeLayers":false}), authorization, None).await?;
        let disabled_mcp = saved["config"]["mcp_servers"].as_object().into_iter().flatten()
            .map(|(name,_)| (name.clone(),json!({"enabled":false}))).collect::<serde_json::Map<_,_>>();
        // 参考 App 的临时元数据请求隔离配置：不加载项目指令、记忆、Hooks、MCP 或可执行工具，不污染原会话。
        let mut config = json!({"project_doc_max_bytes":0,"include_environment_context":false,
            "include_permissions_instructions":false,"include_apps_instructions":false,
            "mcp_servers":disabled_mcp,"apps":{"_default":{"enabled":false}},
            "skills":{"bundled":{"enabled":false},"config":[]},
            "memories":{"use_memories":false,"generate_memories":false},"web_search":"disabled",
            "features.hooks":false,"features.apps":false,"features.plugins":false,
            "features.shell_tool":false,"features.unified_exec":false,"features.apply_patch_freeform":false,
            "features.js_repl":false,"features.code_mode":false,"features.multi_agent":false,
            "features.multi_agent_v2":false,"features.memories":false,"features.image_generation":false});
        if let Some(effort) = effort { config["model_reasoning_effort"] = json!(effort); }
        let thread = rpc(&mut service, 3, "thread/start", json!({
            "model":settings.model,"ephemeral":true,"threadSource":"thread_title",
            "baseInstructions":settings.system_prompt,"developerInstructions":"","config":config,
            "approvalPolicy":"never","sandbox":"read-only","cwd":std::env::temp_dir()
        }), authorization, None).await?;
        let id = thread["thread"]["id"].as_str().context("命名引擎未返回临时会话 ID")?.to_owned();
        let mut early = std::collections::VecDeque::new();
        let started = rpc(&mut service, 4, "turn/start", json!({
            "threadId":id,"model":settings.model,"effort":effort,"outputSchema":schema(),
            "input":[{"type":"text","text":input.to_string(),"text_elements":[]}]
        }), authorization, Some(&mut early)).await?;
        let mut text = String::new();
        if let Some(items) = started["turn"]["items"].as_array() {
            for item in items { if item["type"] == "agentMessage" { text = item["text"].as_str().unwrap_or("").into(); } }
        }
        loop {
            // 原生完成事件可先于 turn/start 回执；必须按接收顺序处理，不能漏掉已完成的标题结果。
            let frame = if let Some(frame) = early.pop_front() { frame } else { frame(&mut service).await? };
            if frame.get("id").is_some() && frame.get("method").is_some() {
                bail!("命名模型请求了交互或工具，已停止本次命名");
            }
            if frame["params"]["threadId"] != id { continue; }
            if frame["method"] == "item/completed" && frame["params"]["item"]["type"] == "agentMessage" {
                text = frame["params"]["item"]["text"].as_str().unwrap_or("").into();
            }
            if frame["method"] == "turn/completed" {
                anyhow::ensure!(frame["params"]["turn"]["status"] == "completed", "命名模型未成功完成");
                anyhow::ensure!(!text.is_empty(), "命名模型未返回标题");
                return Ok(text);
            }
        }
    }).await.unwrap_or_else(|_| Err(anyhow::anyhow!("Codex 会话命名超时")));
    let _ = service.child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), service.child.wait()).await;
    outcome
}

pub(super) async fn claude(
    config: &WslConfig,
    settings: &Settings,
    input: &Value,
    authorization: &RemoteAuthorization,
) -> anyhow::Result<String> {
    let mut command = Claude::wsl(config)?;
    command.args(["--cd", "~"]);
    Claude::wsl_login_exec(&mut command, &config.executable);
    command.args([
        "--print",
        "--output-format",
        "json",
        "--safe-mode",
        "--no-session-persistence",
        "--model",
        &settings.model,
        "--system-prompt",
        &settings.system_prompt,
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
        r#"{"mcpServers":{}}"#,
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ]);
    command.args(["--json-schema", &schema().to_string()]);
    let permit = authorization.prepare_dispatch().await?;
    let mut child = permit.commit(|| {
        Ok(command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?)
    })?;
    let outcome = tokio::time::timeout(TIMEOUT, async {
        let mut writer = child.stdin.take().context("Claude 命名输入不可用")?;
        writer.write_all(input.to_string().as_bytes()).await?;
        writer.shutdown().await?;
        drop(writer);
        let output = child.stdout.take().context("Claude 命名输出不可用")?;
        let mut bytes = Vec::new();
        output.take(MAX_OUTPUT + 1).read_to_end(&mut bytes).await?;
        anyhow::ensure!(bytes.len() as u64 <= MAX_OUTPUT, "Claude 命名输出超过限制");
        anyhow::ensure!(child.wait().await?.success(), "Claude 命名进程未成功结束");
        let value: Value = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(value["is_error"] != true, "Claude 命名返回失败");
        if value["structured_output"].is_object() {
            Ok(value["structured_output"].to_string())
        } else {
            value["result"]
                .as_str()
                .map(str::to_owned)
                .context("Claude 命名没有返回文本结果")
        }
    })
    .await
    .unwrap_or_else(|_| Err(anyhow::anyhow!("Claude 会话命名超时")));
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn conversation_naming_inference_keeps_events_before_start_reply_and_isolates_tools() {
        let (_directory, authorization) =
            super::super::super::tests::service_dispatch_fixture().await;
        let script = r#"
const readline = require('node:readline');
const out = value => process.stdout.write(JSON.stringify(value)+'\n');
readline.createInterface({input:process.stdin}).on('line', line => {
 const q=JSON.parse(line); if(q.id==null)return;
 if(q.method==='initialize')return out({id:q.id,result:{}});
 if(q.method==='config/read')return out({id:q.id,result:{config:{mcp_servers:{fixture:{enabled:true}}}}});
 if(q.method==='thread/start'){
  const p=q.params,c=p.config;
  if(p.ephemeral!==true||p.sandbox!=='read-only'||c.mcp_servers.fixture.enabled!==false
    ||c['features.hooks']!==false||c['features.shell_tool']!==false||p.baseInstructions.length===0)
   return out({id:q.id,error:{message:'unsafe naming thread'}});
  return out({id:q.id,result:{thread:{id:'isolated-naming'}}});
 }
 if(q.method==='turn/start'){
  out({method:'item/completed',params:{threadId:'isolated-naming',item:{type:'agentMessage',text:'{"title":"验证标题","description":"独立命名"}'}}});
  out({method:'turn/completed',params:{threadId:'isolated-naming',turn:{id:'t',status:'completed'}}});
  return out({id:q.id,result:{turn:{id:'t',items:[]}}});
 }
 out({id:q.id,error:{message:'unexpected request'}});
});
"#;
        let mut command = tokio::process::Command::new("node");
        command
            .args(["-e", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(crate::windows_create_hidden_process());
        let service =
            crate::codex_service::RunningService::for_test(command.spawn().unwrap()).unwrap();
        let settings = Settings {
            model: "fixture-model".into(),
            ..Default::default()
        };
        let output = codex_with_service(
            service,
            &settings,
            &json!({"firstUserMessage":"测试命名"}),
            Some("low"),
            &authorization,
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap()["title"],
            "验证标题"
        );
    }
}
