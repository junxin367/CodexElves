use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub fn proxied_client(user_agent: &str) -> anyhow::Result<reqwest::Client> {
    let ua = if user_agent.trim().is_empty() {
        format!("CodexElves/{}", env!("CARGO_PKG_VERSION"))
    } else {
        user_agent.trim().to_string()
    };
    Ok(reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(ua)
        .build()?)
}
