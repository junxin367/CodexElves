use std::io::Write;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 860.0,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub server_url: String,
    pub window: WindowState,
}

pub fn normalize_server_url(input: &str) -> anyhow::Result<Url> {
    let input = input.trim();
    if input.is_empty() || input.len() > 2048 || input.chars().any(char::is_control) {
        bail!("请填写有效的远程服务地址。");
    }
    let value = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let mut url = Url::parse(&value).context("远程服务地址格式无效。")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("远程服务地址只支持 HTTP 或 HTTPS。");
    }
    // 只保存服务首页，不把配对密钥、凭据或会话路由写入原生客户端配置。
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        bail!("请使用服务首页地址，不包含账号、密码、查询参数或会话路径。");
    }
    if matches!(
        url.host_str(),
        Some("tauri.localhost" | "ipc.localhost" | "asset.localhost")
    ) || url.port() == Some(0)
    {
        bail!("此地址不能用作远程服务。");
    }
    if url
        .host_str()
        .is_some_and(|host| host.len() == 64 && host.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        bail!("这里填写远程服务地址；访问密钥请在连接后的登录页输入。");
    }
    url.set_path("/");
    Ok(url)
}

pub fn is_connection_page(url: &Url) -> bool {
    // 命令白名单之外再核对真实调用来源，不能将远程 HTTP 页面当成内置连接页。
    ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https")
            && url.host_str() == Some("tauri.localhost")
            && url.port().is_none()))
        && matches!(url.path(), "/" | "/index.html")
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

pub fn is_titlebar_page(url: &Url) -> bool {
    // 标题栏只允许自身的本地页面；它与连接页分开授权，任何远程 URL 都不能进入标题栏。
    let mut home = url.clone();
    home.set_path("/");
    url.path() == "/titlebar.html" && is_connection_page(&home)
}

pub fn session_window_url(source: &Url, thread_id: &str) -> anyhow::Result<Url> {
    // 用户要求独立窗口打开同一会话；只接收会话标识，服务地址从已校验来源派生，不能传任意 URL 或凭据。
    ensure_session_id(thread_id)?;
    let mut url = source.clone();
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    normalize_server_url(url.as_str())?;
    url.set_query(Some("sessionWindow=1"));
    url.set_fragment(Some(&format!("thread={thread_id}")));
    Ok(url)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewWindowTarget {
    pub kind: String,
    pub path: String,
    pub thread_id: Option<String>,
    pub project_id: Option<String>,
    pub line: Option<u32>,
    pub section: Option<String>,
    pub repository: Option<String>,
}

pub fn preview_window_url(source: &Url, target: &PreviewWindowTarget) -> anyhow::Result<Url> {
    let valid_text = |value: &str, max| {
        !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
    };
    if !valid_text(&target.path, 32768)
        || !matches!(target.kind.as_str(), "file" | "change")
        || target.line == Some(0)
    {
        bail!("文件预览参数无效。");
    }
    let mut fragment = url::form_urlencoded::Serializer::new(String::new());
    fragment
        .append_pair("kind", &target.kind)
        .append_pair("path", &target.path);
    match (&target.thread_id, &target.project_id) {
        (Some(id), None) => {
            ensure_session_id(id)?;
            fragment.append_pair("threadId", id);
        }
        (None, Some(id)) if valid_text(id, 4096) => {
            fragment.append_pair("projectId", id);
        }
        _ => bail!("预览必须指定唯一的会话或项目。"),
    }
    if target.kind == "change" {
        let section = target.section.as_deref().unwrap_or("");
        let repository = target.repository.as_deref().unwrap_or("");
        if !matches!(section, "staged" | "unstaged" | "untracked")
            || repository.len() > 16384
            || repository.contains(['\\', ':', '\0'])
            || repository.chars().any(char::is_control)
            || !repository.is_empty()
                && repository.split('/').any(|part| {
                    part.is_empty()
                        || matches!(part, "." | "..")
                        || part.eq_ignore_ascii_case(".git")
                })
        {
            bail!("文件差异归属无效。");
        }
        fragment
            .append_pair("section", section)
            .append_pair("repository", repository);
    } else if target.section.is_some() || target.repository.is_some() {
        bail!("文件预览不能携带差异分区。");
    }
    if let Some(line) = target.line {
        fragment.append_pair("line", &line.to_string());
    }
    let mut url = source.clone();
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    normalize_server_url(url.as_str())?;
    // 文件只通过原会话/项目接口读取；原生仅构造同来源预览页，不读取网页指定的本机文件。
    url.set_path("/file-preview.html");
    url.set_fragment(Some(&fragment.finish()));
    Ok(url)
}

fn ensure_session_id(value: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || value.len() > 120
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        bail!("会话标识无效。");
    }
    Ok(())
}

pub fn load(path: &Path) -> anyhow::Result<Settings> {
    match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.len() > 32 * 1024 {
                bail!("远程客户端设置文件过大。");
            }
            serde_json::from_slice(&bytes).context("远程客户端设置文件损坏。")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(error).context("无法读取远程客户端设置。"),
    }
}

pub fn save(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    let parent = path.parent().context("设置目录无效。")?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temporary, settings)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_preview_windows_keep_project_repository_and_partition_without_native_file_access() {
        let source = Url::parse("https://remote.example.com:444/?old=1#old").unwrap();
        let mut target: PreviewWindowTarget = serde_json::from_value(serde_json::json!({
            "kind":"change","projectId":"project-1","path":"目录/说明 & #.md",
            "repository":"backend/nested","section":"staged"
        }))
        .unwrap();
        let url = preview_window_url(&source, &target).unwrap();
        assert_eq!(url.origin(), source.origin());
        assert_eq!(url.path(), "/file-preview.html");
        assert!(url.query().is_none());
        let values: std::collections::HashMap<_, _> =
            url::form_urlencoded::parse(url.fragment().unwrap().as_bytes())
                .into_owned()
                .collect();
        assert_eq!(values["path"], target.path);
        assert_eq!(values["projectId"], "project-1");
        assert_eq!(values["repository"], "backend/nested");
        assert_eq!(values["section"], "staged");
        target.thread_id = Some("another-thread".into());
        assert!(preview_window_url(&source, &target).is_err());
        target.thread_id = None;
        target.repository = Some("../outside".into());
        assert!(preview_window_url(&source, &target).is_err());
        target.repository = Some("backend".into());
        target.section = Some("unknown".into());
        assert!(preview_window_url(&source, &target).is_err());
        target.kind = "file".into();
        assert!(preview_window_url(&source, &target).is_err());
        target.section = None;
        target.repository = None;
        assert!(preview_window_url(&source, &target).is_ok());
    }

    #[test]
    fn session_windows_keep_origin_and_only_accept_native_session_ids() {
        let source = Url::parse("https://remote.example.com:444/?old=1#thread=previous").unwrap();
        assert_eq!(
            session_window_url(&source, "claude-abc_123")
                .unwrap()
                .as_str(),
            "https://remote.example.com:444/?sessionWindow=1#thread=claude-abc_123"
        );
        for id in ["", "../file", "id&key=value", "会话", &"a".repeat(121)] {
            assert!(session_window_url(&source, id).is_err(), "{id}");
        }
        for source in [
            "file:///C:/x",
            "http://tauri.localhost/",
            "https://u:p@example.com/",
        ] {
            assert!(session_window_url(&Url::parse(source).unwrap(), "valid-id").is_err());
        }
    }

    #[test]
    fn service_addresses_are_normalized_without_persisting_credentials_or_routes() {
        for (input, expected) in [
            (
                " https://remote.example.com ",
                "https://remote.example.com/",
            ),
            ("remote.example.com:443/", "https://remote.example.com/"),
            ("http://127.0.0.1:39399", "http://127.0.0.1:39399/"),
            ("http://[::1]:39399/", "http://[::1]:39399/"),
        ] {
            assert_eq!(normalize_server_url(input).unwrap().as_str(), expected);
        }
        for input in [
            "",
            "file:///C:/test",
            "javascript:alert(1)",
            "https://u:p@example.com/",
            "https://example.com/?key=secret",
            "https://example.com/#thread=x",
            "https://example.com/session",
            "http://tauri.localhost",
            "http://ipc.localhost",
            "http://example.com:0",
            "http://exa\nmple.com",
        ] {
            assert!(normalize_server_url(input).is_err(), "{input}");
        }
    }

    #[test]
    fn remote_pages_and_origin_lookalikes_cannot_call_local_settings_commands() {
        for url in [
            "http://tauri.localhost/",
            "http://tauri.localhost/index.html",
            "tauri://localhost/index.html",
        ] {
            assert!(is_connection_page(&Url::parse(url).unwrap()));
        }
        for url in [
            "https://remote.example.com/",
            "http://tauri.localhost.evil.example/",
            "http://tauri.localhost:39399/",
            "http://tauri.localhost/index.html?x=1",
            "http://tauri.localhost/other.html",
        ] {
            assert!(!is_connection_page(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn access_keys_are_not_treated_as_server_addresses() {
        assert!(
            normalize_server_url(&"a".repeat(64))
                .unwrap_err()
                .to_string()
                .contains("访问密钥")
        );
    }

    #[test]
    fn titlebar_and_connection_pages_have_separate_local_origins_and_routes() {
        for value in [
            "http://tauri.localhost/titlebar.html",
            "tauri://localhost/titlebar.html",
        ] {
            let url = Url::parse(value).unwrap();
            assert!(is_titlebar_page(&url));
            assert!(!is_connection_page(&url));
        }
        for value in [
            "http://tauri.localhost/index.html",
            "http://tauri.localhost:39399/titlebar.html",
            "https://remote.example.com/titlebar.html",
            "http://tauri.localhost.evil.example/titlebar.html",
            "http://tauri.localhost/titlebar.html?remote=1",
        ] {
            assert!(!is_titlebar_page(&Url::parse(value).unwrap()), "{value}");
        }
    }

    #[test]
    fn settings_replace_atomically_and_corrupt_input_is_not_silently_reset() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remote.json");
        let mut value = Settings::default();
        save(&path, &value).unwrap();
        value.server_url = "https://remote.example.com/".into();
        value.window.maximized = true;
        save(&path, &value).unwrap();
        let restored = load(&path).unwrap();
        assert_eq!(restored.server_url, value.server_url);
        assert!(restored.window.maximized);
        std::fs::write(&path, b"broken").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
    }
}
