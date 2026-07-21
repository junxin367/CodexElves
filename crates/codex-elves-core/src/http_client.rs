use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub fn proxied_client(user_agent: &str) -> anyhow::Result<reqwest::Client> {
    Ok((*shared_proxied_client(user_agent, None)?).clone())
}

pub fn proxied_client_with_timeout(
    user_agent: &str,
    timeout: Duration,
) -> anyhow::Result<reqwest::Client> {
    Ok((*shared_proxied_client(user_agent, Some(timeout))?).clone())
}

fn shared_proxied_client(
    user_agent: &str,
    timeout: Option<Duration>,
) -> anyhow::Result<Arc<reqwest::Client>> {
    let user_agent = effective_user_agent(user_agent);
    let cache_key = (user_agent.clone(), timeout);
    let cache = client_cache();
    if let Some(client) = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("HTTP Client 缓存锁已损坏"))?
        .get(&cache_key)
        .cloned()
    {
        return Ok(client);
    }

    let mut builder = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(&user_agent);
    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
    }
    let client = Arc::new(builder.build()?);
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("HTTP Client 缓存锁已损坏"))?;
    Ok(cache
        .entry(cache_key)
        .or_insert_with(|| Arc::clone(&client))
        .clone())
}

fn effective_user_agent(user_agent: &str) -> String {
    if user_agent.trim().is_empty() {
        format!("CodexElves/{}", env!("CARGO_PKG_VERSION"))
    } else {
        user_agent.trim().to_string()
    }
}

fn client_cache() -> &'static Mutex<HashMap<(String, Option<Duration>), Arc<reqwest::Client>>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, Option<Duration>), Arc<reqwest::Client>>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::{effective_user_agent, shared_proxied_client};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn reuses_client_for_same_effective_user_agent() {
        let first = shared_proxied_client(" Codex-Test/1 ", None).unwrap();
        let second = shared_proxied_client("Codex-Test/1", None).unwrap();

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn isolates_clients_for_different_user_agents() {
        let first = shared_proxied_client("Codex-Test/1", None).unwrap();
        let second = shared_proxied_client("Codex-Test/2", None).unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn isolates_clients_for_different_request_timeouts() {
        let first = shared_proxied_client("Codex-Test/1", Some(Duration::from_secs(30))).unwrap();
        let second = shared_proxied_client("Codex-Test/1", Some(Duration::from_secs(90))).unwrap();

        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn empty_user_agent_uses_product_default() {
        assert_eq!(
            effective_user_agent("  "),
            format!("CodexElves/{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
