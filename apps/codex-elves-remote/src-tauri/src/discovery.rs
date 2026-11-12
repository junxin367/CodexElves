use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use url::Url;

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LocalSettings {
    remote_access: LocalRemote,
}

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LocalRemote {
    bind_host: String,
    port: u16,
}

impl Default for LocalRemote {
    fn default() -> Self {
        Self {
            bind_host: "127.0.0.1".into(),
            port: 45222,
        }
    }
}

pub fn candidates(path: &Path) -> Vec<Url> {
    // 用户要求自动预填本机正在运行的服务；只提取监听配置，不读取访问密钥或设备授权文件，也不扫描端口。
    let settings = std::fs::File::open(path)
        .ok()
        .and_then(|file| {
            let mut bytes = Vec::new();
            file.take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            (bytes.len() <= 2 * 1024 * 1024).then_some(bytes)
        })
        .and_then(|bytes| serde_json::from_slice::<LocalSettings>(&bytes).ok())
        .unwrap_or_default();
    let host = if settings.remote_access.bind_host == "::1" {
        "[::1]"
    } else {
        "127.0.0.1"
    };
    let mut urls = Vec::new();
    if settings.remote_access.port != 0 {
        urls.push(Url::parse(&format!("http://{host}:{}/", settings.remote_access.port)).unwrap());
    }
    let fallback = Url::parse("http://127.0.0.1:45222/").unwrap();
    if !urls.contains(&fallback) {
        urls.push(fallback);
    }
    urls
}

pub async fn detect(path: &Path) -> Option<Url> {
    let client = reqwest::Client::builder()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(350))
        .timeout(Duration::from_millis(1200))
        .build()
        .ok()?;
    for url in candidates(path) {
        // 仅公开资源清单通过协议和摘要校验才认为是 CodexElves 服务，避免把其他本地 HTTP 程序误填进去。
        if crate::resource_cache::fetch_manifest(&client, &url)
            .await
            .is_ok()
        {
            return Some(url);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_uses_configured_loopback_only_and_never_a_remote_bind_host() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            br#"{"remoteAccess":{"port":49123,"bindHost":"192.0.2.12"},"apiKey":"not-used"}"#,
        )
        .unwrap();
        let urls = candidates(&path);
        assert_eq!(urls[0].as_str(), "http://127.0.0.1:49123/");
        assert!(
            urls.iter()
                .all(|url| matches!(url.host_str(), Some("127.0.0.1" | "[::1]")))
        );
        std::fs::write(
            &path,
            br#"{"remoteAccess":{"port":49124,"bindHost":"::1"}}"#,
        )
        .unwrap();
        assert_eq!(candidates(&path)[0].as_str(), "http://[::1]:49124/");
        std::fs::write(&path, b"broken").unwrap();
        assert_eq!(candidates(&path).len(), 1);
    }
}
