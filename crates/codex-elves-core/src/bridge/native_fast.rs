//! 为本机 Codex 模块注册原生 Fast 身份门禁适配；不改写安装包或服务等级。

use super::{CdpSession, next_message_id, runtime_evaluate_params_with_await_promise};
use crate::settings::BackendSettings;
use anyhow::Context;
use serde_json::{Value, json};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Session = CdpSession<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>;
const SOURCE_LIMIT: usize = 24 * 1024 * 1024;

async fn evaluate(session: &mut Session, expression: &str) -> anyhow::Result<Value> {
    let mut params = runtime_evaluate_params_with_await_promise(expression, true);
    params["returnByValue"] = json!(true);
    let reply = session
        .send_command_with_timeout(
            next_message_id(),
            "Runtime.evaluate",
            params,
            std::time::Duration::from_secs(20),
        )
        .await?;
    anyhow::ensure!(
        reply.pointer("/result/exceptionDetails").is_none(),
        "原生 Fast 模块适配执行失败"
    );
    Ok(reply)
}

pub(super) async fn install(
    session: &mut Session,
    settings: &BackendSettings,
    script_identifiers: &mut Vec<String>,
) -> anyhow::Result<()> {
    let bootstrap = crate::assets::native_fast_bootstrap(settings);
    evaluate(
        session,
        &format!(
            "{bootstrap}\nwindow.__codexElvesNativeFastAssetPatchReady=false;\
             window.__codexElvesNativeFastPatchState='preparing';"
        ),
    )
    .await?;
    // app:// 不经过 Chromium Fetch 拦截器。在下一文档解析前通过 import map
    // 复用同一模块的补丁副本；后台重注入只注册，不刷新正在使用的页面。
    let prepared = evaluate(
        session,
        r#"(async()=>{
          if(location.protocol!=='app:'||location.host!=='-')return {ok:false};
          const urls=new Set();
          const entries=[...document.scripts].map(s=>s.src).filter(s=>{
            try{const u=new URL(s);return u.origin===location.origin&&/^\/assets\/index-[\w-]+\.js$/.test(u.pathname)}catch{return false}
          });
          for(const entry of entries.slice(0,4)){
            const response=await fetch(entry);if(!response.ok)continue;
            const source=await response.text();if(source.length>24*1024*1024)return {ok:false};
            for(const match of source.matchAll(/["'`]\.\/app-initial-([\w-]+)\.js["'`]/g)){
              urls.add(new URL('./app-initial-'+match[1]+'.js',entry).href);
            }
          }
          if(urls.size!==1)return {ok:false};
          const url=[...urls][0],response=await fetch(url);if(!response.ok)return {ok:false};
          const source=await response.text();if(source.length>24*1024*1024)return {ok:false};
          const result=window.__codexElvesNativeFastUnlock.prepareModule(source,url);
          return result.ok?{ok:true,url,patchedSource:result.patchedSource}:{ok:false};
        })()"#,
    )
    .await?;
    let prepared = prepared
        .pointer("/result/result/value")
        .context("原生 Fast 模块适配未返回结果")?;
    anyhow::ensure!(
        prepared["ok"] == true,
        "当前 Codex 资产不支持原生 Fast 模块适配"
    );
    let url = prepared["url"]
        .as_str()
        .context("原生 Fast 模块 URL 缺失")?;
    let parsed = reqwest::Url::parse(url)?;
    anyhow::ensure!(
        parsed.scheme() == "app"
            && parsed.host_str() == Some("-")
            && parsed.path().starts_with("/assets/app-initial-")
            && parsed.path().ends_with(".js"),
        "原生 Fast 仅允许本机应用模块"
    );
    let source = prepared["patchedSource"]
        .as_str()
        .context("原生 Fast 模块源码缺失")?;
    anyhow::ensure!(source.len() <= SOURCE_LIMIT, "原生 Fast 模块过大");
    let script = format!(
        "{bootstrap}\nwindow.__codexElvesNativeFastUnlock.installModule({},{});",
        json!(source),
        json!(url),
    );
    let registration = session
        .send_command(
            next_message_id(),
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": script}),
        )
        .await?;
    let identifier = registration
        .pointer("/result/identifier")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .context("原生 Fast 模块适配未注册")?;
    // 与现有 bridge 共用连接和清理流程，不增加独立的后台拦截器。
    script_identifiers.push(identifier.to_string());
    evaluate(
        session,
        "window.__codexElvesNativeFastAssetPatchReady=true;\
         if(window.__codexElvesNativeFastCompiled!=='v3')window.__codexElvesNativeFastPatchState='armed';",
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    #[tokio::test]
    async fn native_fast_registers_only_supported_modules_without_reloading() {
        for supported in [false, true] {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let mut commands = Vec::new();
                while let Some(Ok(Message::Text(text))) = socket.next().await {
                    let request: Value = serde_json::from_str(&text).unwrap();
                    let result = match request["method"].as_str().unwrap() {
                        "Runtime.evaluate" => {
                            let expression = request["params"]["expression"].as_str().unwrap();
                            let value = if expression.contains("prepareModule(source,url)") {
                                if supported {
                                    json!({
                                        "ok": true,
                                        "url": "app://-/assets/app-initial-fixture.js",
                                        "patchedSource": "native-module",
                                    })
                                } else {
                                    json!({"ok": false})
                                }
                            } else {
                                json!(true)
                            };
                            json!({"result": {"value": value}})
                        }
                        "Page.addScriptToEvaluateOnNewDocument" => {
                            let source = request["params"]["source"].as_str().unwrap();
                            assert!(source.contains(
                                "installModule(\"native-module\",\"app://-/assets/app-initial-fixture.js\")"
                            ));
                            assert!(source.contains("\"codexAppNativeFastUnlock\":true"));
                            assert!(!source.contains("private-relay-key"));
                            json!({"identifier": "native-fast-script"})
                        }
                        unexpected => panic!("unexpected command: {unexpected}"),
                    };
                    socket
                        .send(Message::Text(
                            json!({"id": request["id"], "result": result})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap();
                    commands.push(request);
                }
                commands
            });
            let socket = super::super::connect_cdp_websocket(&format!("ws://{address}"))
                .await
                .unwrap();
            let mut session = CdpSession::new(socket);
            let mut identifiers = Vec::new();
            let settings = BackendSettings {
                enhancements_enabled: true,
                codex_app_native_fast_unlock: true,
                relay_api_key: "private-relay-key".to_string(),
                ..BackendSettings::default()
            };
            assert_eq!(
                install(&mut session, &settings, &mut identifiers)
                    .await
                    .is_ok(),
                supported
            );
            assert_eq!(identifiers.len(), usize::from(supported));
            drop(session);
            let commands = server.await.unwrap();
            assert_eq!(
                commands
                    .iter()
                    .filter(|request| request["method"] == "Page.addScriptToEvaluateOnNewDocument")
                    .count(),
                usize::from(supported)
            );
        }
    }
}
