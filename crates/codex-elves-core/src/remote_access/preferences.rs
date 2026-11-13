//! 远程设置管理实例的用户级全局指令，并为文件预览确认引擎目录，不提供任意路径写入。
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::Mutex;

use super::{ApiError, WebState};

// 用户要求直接编辑用户级全局指令；128 KiB 只限制单个 UTF-8 指令文件，避免把整份日志误当指令载入。
const MAX_INSTRUCTION_BYTES: usize = 128 * 1024;
const DETECTION_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Environment {
    pub id: String,
    pub label: String,
    pub distribution: String,
    pub user: String,
    pub home: String,
    pub claude_executable: String,
    #[serde(default)]
    pub executables: BTreeMap<String, String>,
    pub configured: bool,
    pub error: Option<String>,
    pub(super) codex_home: String,
    pub(super) claude_home: String,
}

type DetectionCache = Option<(PathBuf, Instant, Vec<Environment>)>;
static ENVIRONMENTS: OnceLock<Mutex<DetectionCache>> = OnceLock::new();
static INSTRUCTION_WRITE: OnceLock<Mutex<()>> = OnceLock::new();

fn response(value: Value) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

// 用户要求所有 WSL 实例按同一规则接入；这里只校验名称格式，不按具体发行版名称限制产品能力。
pub(super) fn valid_distribution_name(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | "..")
        && name.len() <= 200
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn native_environment() -> Environment {
    let home = directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_default();
    let executables: BTreeMap<_, _> = super::instances::TYPES
        .iter()
        // 用户要求暂不识别本机 Claude；在扫描 PATH 和安装目录前按注册表筛选，WSL 不受影响。
        .filter(|kind| kind.detect_local)
        .filter_map(|kind| {
            let executable = std::env::var_os("PATH")
                .into_iter()
                .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
                .flat_map(|path| {
                    if cfg!(windows) {
                        vec![
                            path.join(format!("{}.exe", kind.command)),
                            path.join(format!("{}.cmd", kind.command)),
                            path.join(format!("{}.bat", kind.command)),
                        ]
                    } else {
                        vec![path.join(kind.command)]
                    }
                })
                .chain([
                    home.join(format!(".local/bin/{}.exe", kind.command)),
                    home.join(format!(".local/bin/{}", kind.command)),
                ])
                .chain(if kind.id == "codex" {
                    crate::cli_wrapper::default_user_runtime_candidates()
                } else {
                    vec![]
                })
                .find(|path| path.is_file());
            executable.map(|path| (kind.id.to_owned(), path.to_string_lossy().into_owned()))
        })
        .collect();
    Environment {
        id: "local".into(),
        label: "本机".into(),
        distribution: String::new(),
        user: String::new(),
        home: home.to_string_lossy().into_owned(),
        claude_executable: executables.get("claude").cloned().unwrap_or_default(),
        executables,
        configured: false,
        error: None,
        codex_home: crate::codex_home::default_codex_home_dir()
            .to_string_lossy()
            .into_owned(),
        claude_home: std::env::var("CLAUDE_CONFIG_DIR")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| home.join(".claude").to_string_lossy().into_owned()),
    }
}

pub(super) async fn output(mut command: Command) -> Result<Vec<u8>, ApiError> {
    command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_create_hidden_process());
    let output = tokio::time::timeout(DETECTION_TIMEOUT, command.output())
        .await
        .map_err(|_| ApiError::new(504, "环境检测超时"))?
        .map_err(|_| ApiError::new(503, "无法读取运行环境"))?;
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return Err(ApiError::new(503, "环境检测未成功"));
    }
    Ok(output.stdout)
}

fn wsl_names(bytes: &[u8]) -> Vec<String> {
    let text = if bytes.contains(&0) {
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}').to_owned())
        .filter(|name| valid_distribution_name(name) && !name.starts_with("docker-desktop"))
        .collect()
}

fn inherited_windows_executable(path: &str) -> bool {
    let Some(relative) = path.strip_prefix("/mnt/").filter(|relative| {
        cfg!(windows)
            && relative
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
            && relative.as_bytes().get(1) == Some(&b'/')
    }) else {
        return false;
    };
    let native = PathBuf::from(format!("{}:/{}", &relative[..1], &relative[2..]));
    // WSL 会继承 Windows PATH；Windows PE 或 npm 的 cmd/exe 同名垫片仍属于本机实例，不能重复登记。
    // 只读文件头和同目录标记；共享磁盘中的真实 Linux ELF 仍按 WSL 程序识别。
    let mut magic = [0; 4];
    if std::fs::File::open(&native)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok()
    {
        if magic == *b"\x7fELF" {
            return false;
        }
        if magic[..2] == *b"MZ" {
            return true;
        }
    }
    ["cmd", "bat", "exe"].iter().any(|extension| {
        let mut sibling = native.as_os_str().to_owned();
        sibling.push(format!(".{extension}"));
        Path::new(&sibling).is_file()
    })
}

async fn probe_wsl(distribution: String, configured: &Value) -> Environment {
    let selected = configured["distribution"]
        .as_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(&distribution));
    let mut environment = Environment {
        id: format!("wsl:{distribution}"),
        label: format!("WSL · {distribution}"),
        distribution,
        user: String::new(),
        home: String::new(),
        claude_executable: String::new(),
        executables: BTreeMap::new(),
        configured: selected,
        error: None,
        codex_home: String::new(),
        claude_home: String::new(),
    };
    let mut command = Command::new("wsl.exe");
    command.args(["--distribution", &environment.distribution]);
    if selected && let Some(user) = configured["user"].as_str().filter(|user| !user.is_empty()) {
        command.args(["--user", user]);
    }
    // 用户要求按检测到的类型实例控制；命令仅来自静态注册表，探测不执行模型或恢复会话。
    let mut script = String::from(
        "PATH=\"$HOME/.local/bin:$HOME/.npm-global/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; printf '%s\\n' \"$HOME\" \"$(id -un)\" \"${CODEX_HOME:-$HOME/.codex}\" \"${CLAUDE_CONFIG_DIR:-$HOME/.claude}\"",
    );
    for kind in super::instances::TYPES {
        script.push_str(&format!(" \"$(command -v {} 2>/dev/null)\"", kind.command));
    }
    script.push_str(" 'end'");
    command.args(["--exec", "sh", "-c", &script]);
    match output(command).await {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let fields: Vec<_> = text.lines().collect();
            if fields.len() >= 5 + super::instances::TYPES.len() && fields[0].starts_with('/') {
                environment.home = fields[0].into();
                environment.user = fields[1].into();
                environment.codex_home = fields[2].into();
                environment.claude_home = fields[3].into();
                for (index, kind) in super::instances::TYPES.iter().enumerate() {
                    let path = fields[4 + index];
                    environment.executables.insert(
                        kind.id.into(),
                        if inherited_windows_executable(path) {
                            String::new()
                        } else {
                            path.into()
                        },
                    );
                }
                environment.claude_executable = environment
                    .executables
                    .get("claude")
                    .cloned()
                    .unwrap_or_default();
            } else {
                environment.error = Some("无法识别该环境的用户目录".into());
            }
        }
        Err(error) => environment.error = Some(error.message),
    }
    environment
}

pub(super) async fn detect(directory: &Path, force: bool) -> Vec<Environment> {
    let mut cache = ENVIRONMENTS.get_or_init(|| Mutex::new(None)).lock().await;
    if !force
        && let Some((path, at, values)) = cache.as_ref()
        && path == directory
        && at.elapsed() < Duration::from_secs(60)
    {
        return values.clone();
    }
    let mut values = vec![native_environment()];
    if cfg!(windows) {
        let configured: Value = std::fs::read(directory.join("remote-claude-wsl.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(
                || json!({"distribution":"codex-elves-claude-test","user":"elves-test"}),
            );
        let mut command = Command::new("wsl.exe");
        command.args(["--list", "--quiet"]);
        if let Ok(bytes) = output(command).await {
            let probes = wsl_names(&bytes)
                .into_iter()
                .map(|name| probe_wsl(name, &configured));
            values.extend(stream::iter(probes).buffered(3).collect::<Vec<_>>().await);
        }
    }
    *cache = Some((directory.to_owned(), Instant::now(), values.clone()));
    values
}

pub(super) async fn environments(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let authorization = state.controller.authorization(device, None);
    let values = detect(&state.controller.state_dir, payload["force"] == true).await;
    let mut instances = super::instances::catalog(&values);
    state
        .controller
        .claude
        .add_known_instances(&mut instances)
        .await;
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok(response(
        json!({"ok":true,"environments":values,"types":super::instances::TYPES,"instances":instances}),
    ))
}

// /mnt/<盘符> 与 WSL UNC 指向同一原生项目。只接受服务器确认的发行版，绝不放开普通网络共享。
pub(super) fn wsl_path(distribution: &str, path: &str) -> Result<PathBuf, ApiError> {
    if !valid_distribution_name(distribution)
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\0')
        || path.contains('\\')
        || path.split('/').any(|part| part == "..")
    {
        return Err(ApiError::new(400, "WSL 文件路径无效"));
    }
    if !cfg!(windows) {
        return Ok(PathBuf::from(path));
    }
    let bytes = path.as_bytes();
    if path.starts_with("/mnt/")
        && bytes.get(5).is_some_and(u8::is_ascii_alphabetic)
        && (bytes.len() == 6 || bytes.get(6) == Some(&b'/'))
    {
        return Ok(PathBuf::from(format!(
            "{}:/{}",
            &path[5..6],
            path.get(7..).unwrap_or("")
        )));
    }
    Ok(PathBuf::from(format!(
        r"\\wsl.localhost\{}\{}",
        distribution,
        path[1..].replace('/', "\\")
    )))
}

pub(super) fn wsl_distribution(path: &Path) -> Option<String> {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text
        .strip_prefix("//?/UNC/")
        .map(|value| format!("//{value}"))
        .unwrap_or(text);
    let mut parts = text.strip_prefix("//")?.split('/');
    let host = parts.next()?;
    if !["wsl.localhost", "wsl$"]
        .iter()
        .any(|value| host.eq_ignore_ascii_case(value))
    {
        return None;
    }
    let distribution = parts.next()?;
    valid_distribution_name(distribution).then(|| distribution.to_owned())
}

pub(super) async fn harness_file_roots(context: &Value) -> Result<Vec<PathBuf>, ApiError> {
    // 用户要求文件访问覆盖当前 harness 的配置、指令、Skill 和插件；
    // 目录只由服务端确认的实例推导，不能接受客户端指定用户目录或借用另一实例。
    let backend = context["backend"].as_str().unwrap_or("codex");
    let environment = match backend {
        "codex"
            if context["instanceId"]
                .as_str()
                .is_none_or(|id| id == "codex:local") =>
        {
            native_environment()
        }
        "claude" => {
            let distribution = context["wslDistribution"]
                .as_str()
                .filter(|name| valid_distribution_name(name))
                .ok_or_else(|| ApiError::new(403, "无法确认会话所属的 WSL 实例"))?;
            let user = context["wslUser"]
                .as_str()
                .filter(|user| !user.is_empty())
                .ok_or_else(|| ApiError::new(403, "无法确认会话所属的 WSL 用户"))?;
            if context["instanceId"] != super::instances::id("claude", distribution, user) {
                return Err(ApiError::new(403, "会话实例身份已变化，请重新打开会话"));
            }
            probe_wsl(
                distribution.to_owned(),
                &json!({"distribution":distribution,"user":user}),
            )
            .await
        }
        _ => return Err(ApiError::new(403, "该会话实例尚不支持引擎文件访问")),
    };
    harness_roots(&environment, backend)
}

fn harness_roots(environment: &Environment, backend: &str) -> Result<Vec<PathBuf>, ApiError> {
    if environment.error.is_some() {
        return Err(ApiError::new(503, "无法读取会话引擎的用户目录"));
    }
    let home = match backend {
        "codex" => &environment.codex_home,
        "claude" => &environment.claude_home,
        _ => return Err(ApiError::new(403, "会话引擎类型无效")),
    };
    if home.is_empty() || environment.home.is_empty() {
        return Err(ApiError::new(403, "会话引擎目录无效"));
    }
    let mut roots = Vec::new();
    // 引擎目录包含其配置与扩展资源；通用 .agents 目录承载跨引擎 Skill，不放开整个用户主目录。
    for value in [
        home.to_owned(),
        format!("{}/.agents", environment.home.trim_end_matches(['/', '\\'])),
    ] {
        let path = if environment.distribution.is_empty() {
            PathBuf::from(&value)
        } else {
            wsl_path(&environment.distribution, &value)?
        };
        if !path.is_absolute() {
            return Err(ApiError::new(403, "会话引擎目录无效"));
        }
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    Ok(roots)
}

fn instruction_target(
    environments: &[Environment],
    payload: &Value,
) -> Result<(PathBuf, String), ApiError> {
    // 用户要求指令仅为实例的用户级全局设置；旧客户端也不能再通过作用范围或项目参数访问项目文件。
    if payload
        .get("scope")
        .is_some_and(|value| value.as_str() != Some("global"))
        || payload
            .get("projectId")
            .is_some_and(|value| !value.is_null())
    {
        return Err(ApiError::new(400, "自定义指令仅支持用户级全局设置"));
    }
    let id = payload["instanceId"]
        .as_str()
        .ok_or_else(|| ApiError::new(400, "请选择会话实例"))?;
    let instance = super::instances::catalog(environments)
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| ApiError::new(404, "会话实例暂不可用"))?;
    let environment = environments
        .iter()
        .find(|item| item.id == instance.environment_id && item.error.is_none())
        .ok_or_else(|| ApiError::new(404, "会话实例暂不可用"))?;
    let kind = super::instances::TYPES
        .iter()
        .find(|kind| kind.id == instance.backend)
        .ok_or_else(|| ApiError::new(400, "指令会话类型无效"))?;
    let name = kind.instruction_file;
    let home = match kind.id {
        "codex" => &environment.codex_home,
        "claude" => &environment.claude_home,
        _ => return Err(ApiError::new(400, "该会话类型尚未支持自定义指令")),
    };
    let display = format!("{}/{name}", home.trim_end_matches(['/', '\\']));
    let root = if environment.distribution.is_empty() {
        PathBuf::from(home)
    } else {
        wsl_path(&environment.distribution, home)?
    };
    if !root.is_absolute() || root.as_os_str().is_empty() {
        return Err(ApiError::new(400, "指令目录不是绝对路径"));
    }
    Ok((root.join(name), display))
}

fn read_instruction(path: &Path) -> Result<(String, bool, String), ApiError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(value) => Some(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(ApiError::new(403, "无法读取指令文件")),
    };
    if let Some(metadata) = &metadata {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ApiError::new(403, "指令文件不能是链接、目录或特殊文件"));
        }
        if metadata.len() > MAX_INSTRUCTION_BYTES as u64 {
            return Err(ApiError::new(413, "指令文件超过 128 KiB"));
        }
    }
    let exists = metadata.is_some();
    let bytes = if exists {
        std::fs::read(path).map_err(|_| ApiError::new(403, "无法读取指令文件"))?
    } else {
        Vec::new()
    };
    if bytes.len() > MAX_INSTRUCTION_BYTES {
        return Err(ApiError::new(413, "指令文件超过 128 KiB"));
    }
    let revision = super::hash(&format!(
        "{}:{}",
        exists,
        super::hash(&String::from_utf8_lossy(&bytes))
    ));
    let text =
        String::from_utf8(bytes).map_err(|_| ApiError::new(422, "指令文件需要使用 UTF-8 编码"))?;
    Ok((text, exists, revision))
}

fn save_instruction(path: &Path, content: &str, expected: &str) -> anyhow::Result<()> {
    use std::io::Write;

    anyhow::ensure!(
        content.len() <= MAX_INSTRUCTION_BYTES,
        "指令内容超过 128 KiB"
    );
    if read_instruction(path)?.2 != expected {
        return Err(ApiError::new(409, "指令文件已变化，请重新读取").into());
    }
    let parent = path.parent().context("指令目录无效")?;
    std::fs::create_dir_all(parent)?;
    let permissions = std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    if permissions
        .as_ref()
        .is_some_and(std::fs::Permissions::readonly)
    {
        return Err(ApiError::new(403, "指令文件为只读，未修改文件权限").into());
    }
    // 用户要求直接保存原生文件；不能借用固定 AGENTS.md.tmp / CLAUDE.md.tmp，避免覆盖编辑器的临时文件或链接。
    let temporary = parent.join(format!(
        ".codex-elves-instructions-{}.tmp",
        uuid::Uuid::new_v4()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ApiError::new(503, "无法创建指令临时文件，请检查文件权限"))?;
    let result = (|| -> anyhow::Result<()> {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if read_instruction(path)?.2 != expected {
            return Err(ApiError::new(409, "指令文件已变化，请重新读取").into());
        }
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temporary);
    result.map_err(|failure| {
        if failure.downcast_ref::<ApiError>().is_some() {
            failure
        } else {
            ApiError::new(503, "保存指令失败，请检查所选环境的文件权限").into()
        }
    })
}

pub(super) async fn instructions(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, ApiError> {
    let device = super::authorized(&state, &headers).await?;
    let writing = payload.get("content").is_some();
    if writing && state.settings.read_only {
        return Err(ApiError::new(403, "当前为只读访问"));
    }
    let mut authorization = state.controller.authorization(device, None);
    if writing {
        authorization.write_action = Some("saveInstructions".into());
    }
    let environments = detect(&state.controller.state_dir, false).await;
    let (path, display) = instruction_target(&environments, &payload)?;
    let _lock = INSTRUCTION_WRITE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;
    let (mut text, mut exists, mut revision) = read_instruction(&path)?;
    if writing {
        let content = payload["content"]
            .as_str()
            .filter(|value| value.len() <= MAX_INSTRUCTION_BYTES)
            .ok_or_else(|| ApiError::new(400, "指令内容无效或超过 128 KiB"))?;
        if payload["expectedRevision"].as_str() != Some(&revision) {
            return Err(ApiError::new(
                409,
                "指令文件已被其他窗口或原生程序修改，请重新读取后保存",
            ));
        }
        let permit = authorization
            .prepare_dispatch()
            .await
            .map_err(super::authorization_error)?;
        permit
            .commit(|| {
                let parent = path
                    .parent()
                    .ok_or_else(|| ApiError::new(400, "指令目录无效"))?;
                std::fs::create_dir_all(parent).map_err(|_| {
                    ApiError::new(403, "无法创建指令目录，请检查所选环境的文件权限")
                })?;
                // 用户要求保存实际原生指令；同一目录原子替换，并用读取版本防止覆盖其他窗口的新内容。
                save_instruction(&path, content, &revision)?;
                Ok(())
            })
            .map_err(super::authorization_error)?;
        (text, exists, revision) = read_instruction(&path)?;
    }
    authorization
        .check()
        .await
        .map_err(super::authorization_error)?;
    Ok(response(
        json!({"ok":true,"path":display,"content":text,"exists":exists,"revision":revision}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn environment_detection_rejects_windows_interop_shims_but_keeps_linux_binaries() {
        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("codex");
        let text = program.to_string_lossy().replace('\\', "/");
        let wsl = format!("/mnt/{}/{}", text[..1].to_lowercase(), &text[3..]);
        std::fs::write(&program, b"#!/bin/sh\nexec node.exe codex.js\n").unwrap();
        assert!(!inherited_windows_executable(&wsl));
        std::fs::write(program.with_extension("cmd"), b"@node codex.js").unwrap();
        assert!(inherited_windows_executable(&wsl));
        std::fs::write(&program, b"\x7fELFfixture-linux").unwrap();
        assert!(!inherited_windows_executable(&wsl));
        std::fs::write(&program, b"MZfixture-windows").unwrap();
        assert!(inherited_windows_executable(&wsl));
        assert!(!inherited_windows_executable("/usr/bin/codex"));
    }

    #[test]
    fn session_instances_catalog_keeps_wsl_claude_and_skips_local_claude() {
        let environment = |distribution: &str, commands: &[&str], configured| Environment {
            id: if distribution.is_empty() {
                "local".into()
            } else {
                format!("wsl:{distribution}")
            },
            label: if distribution.is_empty() {
                "本机".into()
            } else {
                format!("WSL · {distribution}")
            },
            distribution: distribution.into(),
            user: if distribution.is_empty() {
                "".into()
            } else {
                "user".into()
            },
            home: "/home/user".into(),
            claude_executable: String::new(),
            executables: commands
                .iter()
                .map(|kind| ((*kind).into(), format!("/bin/{kind}")))
                .collect(),
            configured,
            error: None,
            codex_home: String::new(),
            claude_home: String::new(),
        };
        let environments = [
            environment("", &["claude", "codex"], false),
            environment("WSL1", &["claude"], true),
            environment("WSL2", &["claude"], false),
            environment("WSL3", &["codex"], false),
        ];
        let instances = super::super::instances::catalog(&environments);
        assert_eq!(instances.len(), 4);
        assert!(instances.iter().all(|value| value.id != "claude:local"));
        assert_eq!(
            instances
                .iter()
                .filter(|value| value.backend == "claude")
                .count(),
            2
        );
        assert_eq!(
            instances
                .iter()
                .filter(|value| value.backend == "codex")
                .count(),
            2
        );
        assert_eq!(instances.iter().filter(|value| value.ready).count(), 3);
        assert_eq!(
            instances
                .iter()
                .filter(|value| value.legacy_default)
                .count(),
            2
        );
        let ids: std::collections::HashSet<_> = instances.iter().map(|value| &value.id).collect();
        assert_eq!(ids.len(), 4);
        let id = super::super::instances::id;
        assert_eq!(id("claude", "WSL1", "user"), id("claude", "wsl1", "user"));
        assert_ne!(id("claude", "WSL1", "other"), id("claude", "WSL1", "user"));
        let mut upgraded = environments[1].clone();
        upgraded
            .executables
            .insert("claude".into(), "/new/version/claude".into());
        assert_eq!(
            super::super::instances::catalog(&[upgraded])[0].id,
            id("claude", "WSL1", "user")
        );
    }

    #[test]
    fn environment_detection_uses_the_same_rules_for_all_distribution_names() {
        let names = "Ubuntu\r\nclaude-us\r\ncodex-elves-claude-test\r\n";
        let bytes: Vec<_> = names.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(
            wsl_names(&bytes),
            ["Ubuntu", "claude-us", "codex-elves-claude-test"]
        );
        for name in ["Ubuntu", "claude-us", "CLAUDE-US", "work.profile_2"] {
            assert!(valid_distribution_name(name));
            assert!(wsl_path(name, "/home/fixture/file").is_ok());
        }
        assert!(wsl_path("Ubuntu", "/home/user/../outside").is_err());
        // 仅比较本机内存样例，不启动 WSL、访问真实发行版或执行 CLI。
        let environments: Vec<_> = ["Ubuntu", "claude-us", "CLAUDE-US"]
            .into_iter()
            .map(|name| Environment {
                id: format!("wsl:{name}"),
                label: format!("WSL · {name}"),
                distribution: name.into(),
                user: "fixture-user".into(),
                home: "/home/fixture".into(),
                claude_executable: "/fixture/bin/claude".into(),
                executables: BTreeMap::from([("claude".into(), "/fixture/bin/claude".into())]),
                configured: false,
                error: None,
                codex_home: "/home/fixture/.codex".into(),
                claude_home: "/home/fixture/.claude".into(),
            })
            .collect();
        let instances = super::super::instances::catalog(&environments);
        assert_eq!(instances.len(), 3);
        assert!(instances.iter().all(|instance| instance.ready));
        assert!(instances.iter().any(
            |instance| instance.backend == "codex" && instance.connection_modes == ["service"]
        ));
        assert!(
            instances
                .iter()
                .filter(|instance| instance.backend == "claude")
                .all(|instance| instance.connection_modes == ["native"])
        );
        assert!(instances.iter().all(|instance| instance.status == "可接入"));
    }

    #[test]
    fn instruction_target_resolves_only_the_selected_instances_global_file() {
        let temporary = tempfile::tempdir().unwrap();
        let local = Environment {
            id: "local".into(),
            label: "本机".into(),
            distribution: String::new(),
            user: String::new(),
            home: temporary.path().to_string_lossy().into_owned(),
            claude_executable: String::new(),
            executables: BTreeMap::new(),
            configured: false,
            error: None,
            codex_home: temporary
                .path()
                .join("custom-codex-home")
                .to_string_lossy()
                .into_owned(),
            claude_home: temporary
                .path()
                .join("custom-claude-home")
                .to_string_lossy()
                .into_owned(),
        };
        let wsl = Environment {
            id: "wsl:fixture-wsl".into(),
            distribution: "fixture-wsl".into(),
            user: "fixture-user".into(),
            home: "/home/fixture".into(),
            executables: BTreeMap::from([
                ("claude".into(), "/fixture/bin/claude".into()),
                ("codex".into(), "/fixture/bin/codex".into()),
            ]),
            codex_home: "/home/fixture/custom-codex".into(),
            claude_home: "/home/fixture/custom-claude".into(),
            ..local.clone()
        };
        let environments = [local.clone(), wsl.clone()];
        let local_target =
            instruction_target(&environments, &json!({"instanceId":"codex:local"})).unwrap();
        assert_eq!(
            local_target.0,
            PathBuf::from(&local.codex_home).join("AGENTS.md")
        );
        assert_eq!(
            harness_roots(&local, "codex").unwrap(),
            [
                PathBuf::from(&local.codex_home),
                temporary.path().join(".agents")
            ]
        );
        for (backend, home, name) in [
            ("claude", &wsl.claude_home, "CLAUDE.md"),
            ("codex", &wsl.codex_home, "AGENTS.md"),
        ] {
            let id = super::super::instances::id(backend, &wsl.distribution, &wsl.user);
            let target = instruction_target(&environments, &json!({"instanceId":id})).unwrap();
            assert_eq!(
                target.0,
                wsl_path(&wsl.distribution, home).unwrap().join(name)
            );
            assert_eq!(target.1, format!("{home}/{name}"));
            assert_eq!(
                harness_roots(&wsl, backend).unwrap(),
                [
                    wsl_path(&wsl.distribution, home).unwrap(),
                    wsl_path(&wsl.distribution, "/home/fixture/.agents").unwrap()
                ]
            );
        }
        for payload in [
            json!({"instanceId":"codex:local","scope":"project","projectId":"fixture"}),
            json!({"instanceId":"codex:local","projectId":"fixture"}),
            json!({"backend":"codex","environmentId":"local","scope":"global"}),
        ] {
            assert_eq!(
                instruction_target(&environments, &payload)
                    .unwrap_err()
                    .code,
                400
            );
        }
        for id in ["claude:local", "codex:wsl:unknown"] {
            assert_eq!(
                instruction_target(&environments, &json!({"instanceId":id}))
                    .unwrap_err()
                    .code,
                404
            );
        }
        let unavailable = Environment {
            error: Some("模拟不可用".into()),
            ..local
        };
        assert_eq!(harness_roots(&unavailable, "codex").unwrap_err().code, 503);
        assert_eq!(
            instruction_target(&[unavailable], &json!({"instanceId":"codex:local"}))
                .unwrap_err()
                .code,
            404
        );
    }

    #[tokio::test]
    async fn harness_file_roots_reject_unknown_or_mismatched_instances_before_probing() {
        for context in [
            json!({"backend":"unknown"}),
            json!({"backend":"codex","instanceId":"codex:wsl:other"}),
            json!({"backend":"claude","wslDistribution":"Other","wslUser":"fixture","instanceId":"claude:local"}),
            json!({"backend":"claude","wslDistribution":"../Other","wslUser":"fixture"}),
            json!({"backend":"claude","wslDistribution":"Other"}),
        ] {
            assert_eq!(harness_file_roots(&context).await.unwrap_err().code, 403);
        }
    }

    #[test]
    fn instruction_revision_distinguishes_missing_empty_and_changed_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("AGENTS.md");
        let missing = read_instruction(&path).unwrap();
        assert!(!missing.1);
        std::fs::write(&path, "").unwrap();
        let empty = read_instruction(&path).unwrap();
        assert!(empty.1);
        assert_ne!(missing.2, empty.2);
        std::fs::write(&path, "中文指令\n").unwrap();
        assert_ne!(empty.2, read_instruction(&path).unwrap().2);
    }

    #[test]
    fn instruction_save_preserves_external_edits_and_creates_native_utf8_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".claude/CLAUDE.md");
        let initial = read_instruction(&path).unwrap().2;
        save_instruction(&path, "原生中文指令\n", &initial).unwrap();
        let saved = read_instruction(&path).unwrap();
        assert_eq!(saved.0, "原生中文指令\n");
        std::fs::write(&path, "原生程序修改\n").unwrap();
        let error = save_instruction(&path, "旧网页草稿", &saved.2).unwrap_err();
        assert_eq!(error.downcast_ref::<ApiError>().unwrap().code, 409);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "原生程序修改\n");
    }

    #[test]
    fn instruction_save_does_not_overwrite_editor_temporary_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("AGENTS.md");
        let editor_backup = path.with_extension("md.tmp");
        std::fs::write(&editor_backup, "编辑器未保存的工作").unwrap();
        let revision = read_instruction(&path).unwrap().2;
        save_instruction(&path, "新的原生指令\r\n", &revision).unwrap();
        assert_eq!(
            std::fs::read_to_string(&editor_backup).unwrap(),
            "编辑器未保存的工作"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "新的原生指令\r\n");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
    }
}
