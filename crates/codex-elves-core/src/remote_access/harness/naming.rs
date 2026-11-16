use serde_json::{Value, json};

use super::Harness;

impl Harness {
    pub async fn naming_context(&self, id: &str, initial: bool) -> anyhow::Result<Value> {
        super::execution::Scope::service(self.audit.clone(), "naming-context", &json!({"threadId":id}), None)
            .run(async {
                let raw = self.runtime.request("thread/read", json!({"threadId":id,"includeTurns":false}), None).await?;
                let thread = crate::codex_service::normalize_thread_response(&raw)?;
                let page = self.runtime.request("thread/turns/list", json!({
                    "threadId":id,"limit":if initial {1} else {8},"itemsView":"full",
                    "sortDirection":if initial {"asc"} else {"desc"}
                }), None).await?;
                let mut turns = page["data"].as_array().cloned().unwrap_or_default();
                if !initial { turns.reverse(); }
                let name = raw["thread"]["name"].as_str().unwrap_or("");
                Ok(json!({"title":thread["title"],"expectedName":name,"eligible":name.trim().is_empty(),
                    "messages":super::super::conversation_naming::messages(&turns)}))
            }).await
    }
}
