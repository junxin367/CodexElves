fn main() {
    prepare_bundled_resources();
    #[cfg(windows)]
    {
        // 用户要求独立远程 EXE；它只作为客户端运行，所有构建均沿用 asInvoker，不请求管理器的管理员权限。
        let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!(
            "../../codex-elves-manager/src-tauri/windows-dev-app-manifest.xml"
        ));
        let attributes = tauri_build::Attributes::new()
            .windows_attributes(windows)
            .app_manifest(tauri_build::AppManifest::new().commands(&[
                "remote_client_state",
                "connect_remote",
                "remote_titlebar_theme",
                "remote_unread_badge",
                "remote_cache_status",
                "remote_cache_refresh",
                "remote_cache_activity",
                "remote_open_session_window",
            ]));
        tauri_build::try_build(attributes).expect("failed to build CodexElves Remote");
    }
}

fn prepare_bundled_resources() {
    use std::fmt::Write;
    use std::path::Path;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/remote-access");
    println!("cargo:rerun-if-changed={}", root.display());
    let revision: u64 = std::fs::read_to_string(root.join("resource-revision.txt"))
        .expect("missing bundled resource revision")
        .trim()
        .parse()
        .expect("invalid resource revision");
    let mut resources = Vec::new();
    fn walk(root: &Path, directory: &Path, files: &mut Vec<(String, String, String)>) {
        for entry in std::fs::read_dir(directory).expect("read bundled resources") {
            let path = entry.expect("read resource entry").path();
            if path.is_dir() {
                walk(root, &path, files);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // 与 Android 的公开网页包一致；缓存恢复页及开发/许可证文本不作为可执行缓存入口。
            if matches!(
                relative.as_str(),
                "cache-recovery.html" | "cache-recovery.js"
            ) {
                continue;
            }
            let mime = match path.extension().and_then(|value| value.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("js" | "mjs") => "application/javascript; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                Some("png") => "image/png",
                Some("webmanifest") => "application/manifest+json; charset=utf-8",
                _ => continue,
            };
            let route = if relative == "index.html" {
                "/".to_owned()
            } else {
                format!("/{relative}")
            };
            files.push((
                route,
                mime.into(),
                path.canonicalize().unwrap().to_string_lossy().into_owned(),
            ));
        }
    }
    walk(&root, &root, &mut resources);
    resources.sort_by(|left, right| left.0.cmp(&right.0));
    let mut code = format!(
        "pub const BUNDLED_REVISION: u64 = {revision};\npub fn bundled_files() -> Vec<(&'static str, &'static str, &'static [u8])> {{ vec![\n"
    );
    for (route, mime, path) in resources {
        writeln!(
            &mut code,
            "({route:?}, {mime:?}, include_bytes!({path:?})),"
        )
        .unwrap();
    }
    code.push_str("] }\n");
    std::fs::write(
        Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("bundled_resources.rs"),
        code,
    )
    .expect("write bundled resource index");
}
