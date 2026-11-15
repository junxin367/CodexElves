// 远程会话网页资源表：按 scripts/remote-access-resources.mjs 的统一规则扫描 assets/remote-access，
// 生成服务端内嵌资源（路由、MIME、字节）。新增或拆分网页模块无需在 remote_access.rs 中逐个登记；
// 规则变化须同步 Android build.gradle、Windows 客户端 build.rs 与调试服务器。
use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/remote-access");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut resources = Vec::new();
    walk(&root, &root, &mut resources);
    resources.sort();
    let mut code = String::from(
        "/// 由 build.rs 生成：(路由, MIME, 内容)，按路由排序。\npub(crate) static REMOTE_WEB_ASSETS: &[(&str, &str, &[u8])] = &[\n",
    );
    for (route, mime, file) in resources {
        writeln!(
            &mut code,
            "    ({route:?}, {mime:?}, include_bytes!({file:?})),"
        )
        .unwrap();
    }
    code.push_str("];\n");
    let target =
        Path::new(&std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("remote_web_assets.rs");
    std::fs::write(target, code).expect("write remote web asset table");
}

fn walk(root: &Path, directory: &Path, resources: &mut Vec<(String, &'static str, String)>) {
    for entry in std::fs::read_dir(directory).expect("read remote web assets") {
        let path = entry.expect("read remote web asset entry").path();
        if path.is_dir() {
            walk(root, &path, resources);
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("remote web asset under root")
            .to_string_lossy()
            .replace('\\', "/");
        // 缓存恢复页及脚本独立提供（no-store），不进入版本壳清单。
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
        let file = path
            .canonicalize()
            .expect("canonical remote web asset path")
            .to_string_lossy()
            .into_owned();
        resources.push((route, mime, file));
    }
}
