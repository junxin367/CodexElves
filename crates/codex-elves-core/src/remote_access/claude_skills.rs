//! Claude 编辑区的斜杠菜单目录：由所选实例的原版 CLI 报告，不扫描或复制其他实例的 Skill。
use super::{Claude, MODEL_CATALOG_TTL, WslConfig, error, normalize_models};
use anyhow::Context;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

fn catalog_key(config: &WslConfig, cwd: &str) -> String {
    // Skill 与模型不同：项目技能依赖工作目录，不能只按实例缓存或混用 Windows/其他 WSL 的目录。
    json!([config.instance_id(), cwd]).to_string()
}

fn normalize_skills(response: &Value, initialization: &Value) -> anyhow::Result<Vec<Value>> {
    let reported = response["skills"]
        .as_array()
        .context("当前 Claude CLI 未返回原生 Skill 目录")?;
    let commands: HashSet<_> = initialization["commands"]
        .as_array()
        .context("当前 Claude CLI 未返回可调用命令目录")?
        .iter()
        .filter_map(|command| command["name"].as_str())
        .collect();
    let mut seen = HashSet::new();
    let mut data = Vec::new();
    for skill in reported {
        let Some(name) = skill["name"].as_str().filter(|name| {
            !name.is_empty()
                && name.len() <= 256
                && name
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.'))
                && !matches!(*name, "compact" | "goal")
                && commands.contains(name)
        }) else {
            continue;
        };
        if skill["userInvocable"] == false || !seen.insert(name) {
            continue;
        }
        // reload_skills 区分提示技能和 clear/model 等管理命令，再与可手动调用目录取交集。
        // path 仅是菜单稳定标识，不是文件路径，也不作为 Codex Skill 参数发送。
        data.push(json!({
            "name":name,"command":name,"path":format!("claude-skill:{name}"),
            "backend":"claude","scope":"native",
            "description":skill["description"].as_str().unwrap_or(""),
            "argumentHint":skill["argumentHint"].as_str().unwrap_or("")
        }));
    }
    Ok(data)
}

impl Claude {
    pub async fn session_skills(self: &Arc<Self>, id: &str, force: bool) -> anyhow::Result<Value> {
        if Self::is_subagent_id(id) {
            return Ok(json!({"ok":true,"backend":"claude","data":[],"errors":[]}));
        }
        let conversation = self.conversation(id).await?;
        self.skills_in(conversation.config, &conversation.cwd, force)
            .await
    }

    pub async fn skills_in(
        self: &Arc<Self>,
        config: WslConfig,
        cwd: &str,
        force: bool,
    ) -> anyhow::Result<Value> {
        config.validate()?;
        let key = catalog_key(&config, cwd);
        let lock = self.lock(&format!("skills:{key}")).await;
        let _guard = lock.lock().await;
        if !force
            && let Some((at, data)) = self.skill_metadata.lock().await.get(&key)
            && at.elapsed() < MODEL_CATALOG_TTL
        {
            return Ok(data.clone());
        }
        let instance = config.instance_id();
        let process = self.catalog_process(config, cwd).await?;
        // 仅刷新独立目录探测进程，不向正在执行的共享 CLI 发送 /reload-skills 或其他命令。
        let response = process
            .control(json!({"subtype":"reload_skills"}), None)
            .await
            .map_err(|_| {
                error(
                    409,
                    "无法读取所选 Claude 实例的 Skill 列表，请检查 CLI 版本或稍后重试",
                )
            })?;
        let data = json!({
            "ok":true,"backend":"claude","instanceId":instance,"cwd":cwd,
            "data":normalize_skills(&response, &process.initialization)?,"errors":[],
            "nativeCapabilities":normalize_models(&process.initialization)["nativeCapabilities"]
        });
        let mut cache = self.skill_metadata.lock().await;
        // 与网页目录缓存一致最多保留 64 个项目目录；探测失败不覆盖已有成功数据。
        if cache.len() >= 64 && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.insert(key, (Instant::now(), data.clone()));
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_menu_uses_native_invocable_skills_not_management_commands() {
        let commands = json!({"commands":[{"name":"review"},{"name":"plugin:check"},{"name":"clear"},
            {"name":"compact"},{"name":"goal"},{"name":"hidden"},{"name":"bad\nname"}]});
        let response = json!({"skills":[
            {"name":"review","builtin":true,"description":"Review code","argumentHint":"[path]"},
            {"name":"plugin:check","description":"Plugin skill"},
            {"name":"review"},{"name":"model-only"},{"name":"hidden","userInvocable":false},
            {"name":"compact"},{"name":"goal"},{"name":"bad\nname"}
        ]});
        let skills = normalize_skills(&response, &commands).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0]["command"], "review");
        assert_eq!(skills[0]["argumentHint"], "[path]");
        assert_eq!(skills[1]["path"], "claude-skill:plugin:check");
        assert!(normalize_skills(&json!({}), &commands).is_err());
    }

    #[test]
    fn skill_catalog_keys_separate_instances_and_projects() {
        let a = WslConfig::default();
        let b = WslConfig {
            distribution: "other-test".into(),
            ..a.clone()
        };
        assert_ne!(catalog_key(&a, "/project/a"), catalog_key(&a, "/project/b"));
        assert_ne!(catalog_key(&a, "/project/a"), catalog_key(&b, "/project/a"));
    }
}
