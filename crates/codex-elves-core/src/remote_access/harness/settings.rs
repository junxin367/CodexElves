//! 设置页通过独立引擎读取用户 Home 下的扩展，不借用当前会话或 Codex App。
use serde_json::{Value, json};

use super::{Harness, error};
use crate::remote_access::{RemoteAuthorization, execution};

impl Harness {
    pub(in crate::remote_access) async fn settings_skills(
        &self,
        home: &str,
        change: Option<&Value>,
        authorization: &RemoteAuthorization,
    ) -> anyhow::Result<Value> {
        execution::Scope::service(self.audit.clone(), "settings-skills", &json!({}), None)
            .run(async {
                let response = self
                    .runtime
                    .request(
                        "skills/list",
                        json!({"cwds":[home],"forceReload":true}),
                        None,
                    )
                    .await?;
                let entry = response["data"]
                    .as_array()
                    .and_then(|entries| entries.iter().find(|entry| entry["cwd"] == home))
                    .ok_or_else(|| error(503, "原生技能列表未返回所选用户目录"))?;
                let rows = entry["skills"]
                    .as_array()
                    .ok_or_else(|| error(503, "原生技能列表格式无效"))?;
                if let Some(change) = change {
                    let path = change["id"]
                        .as_str()
                        .ok_or_else(|| error(400, "请选择原生技能"))?;
                    let enabled = change["enabled"]
                        .as_bool()
                        .ok_or_else(|| error(400, "技能开关必须为布尔值"))?;
                    let row = rows
                        .iter()
                        .find(|row| row["path"] == path)
                        .ok_or_else(|| error(409, "技能已不在当前实例中，请重新读取"))?;
                    if row["scope"] == "admin" {
                        return Err(error(403, "组织管理的技能需由原生策略调整"));
                    }
                    // 用户要求 Skill 设置属于所选实例；路径必须来自该实例刚返回的列表，不接受任意文件路径。
                    self.runtime
                        .request(
                            "skills/config/write",
                            json!({"path":path,"enabled":enabled}),
                            Some(authorization),
                        )
                        .await?;
                    self.runtime.invalidate_read("skills").await;
                    return Ok(json!({"updated":true}));
                }
                let rows = rows
                    .iter()
                    .map(|row| {
                        json!({
                            "id":row["path"],"name":row["name"],"description":row["description"],
                            "enabled":row["enabled"],"canToggle":row["scope"]!="admin"
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(json!({"data":rows,"message":
                    if entry["errors"].as_array().is_some_and(|errors|!errors.is_empty()) {
                        "部分技能读取失败，请在原生 CLI 中核对技能文件。"
                    } else {"显示所选实例用户目录中的原生技能，组织管理的技能保持只读。"}}))
            })
            .await
    }

    pub(in crate::remote_access) async fn settings_hooks(
        &self,
        home: &str,
    ) -> anyhow::Result<Value> {
        execution::Scope::service(self.audit.clone(), "settings-hooks", &json!({}), None)
            .run(async {
                let response = self
                    .runtime
                    .request("hooks/list", json!({"cwds":[home]}), None)
                    .await?;
                let entry = response["data"]
                    .as_array()
                    .and_then(|entries| entries.iter().find(|entry| entry["cwd"] == home))
                    .ok_or_else(|| error(503, "原生钩子列表未返回所选用户目录"))?;
                let rows = entry["hooks"]
                    .as_array()
                    .ok_or_else(|| error(503, "原生钩子列表格式无效"))?
                    .iter()
                    .map(|hook| {
                        json!({
                            "id":hook["key"],"name":hook["eventName"],"enabled":hook["enabled"],
                            "description":format!("{} · {}", match hook["source"].as_str() {
                                Some("plugin")=>"插件",Some("user")=>"用户",Some("project")=>"项目",
                                Some("system")=>"系统",_=>"原生策略"
                            },match hook["trustStatus"].as_str() {
                                Some("managed")=>"组织管理",Some("trusted")=>"已信任",
                                Some("modified")=>"内容已变化",_=>"尚未信任"
                            }),
                            "canToggle":false
                        })
                    })
                    .collect::<Vec<_>>();
                // 只展示状态，不返回命令、提示词、Hash，也不替用户信任新增或已变化的钩子。
                Ok(json!({"data":rows,"message":
                    if entry["errors"].as_array().is_some_and(|errors|!errors.is_empty()) {
                        "部分钩子读取失败，请在原生 CLI 中核对。"
                    } else {"单个钩子的信任与组织策略继续由原生 CLI 管理。"}}))
            })
            .await
    }
}
