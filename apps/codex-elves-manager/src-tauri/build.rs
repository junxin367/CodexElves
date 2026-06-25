fn main() {
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let manifest = if profile == "release" {
        include_str!("windows-app-manifest.xml")
    } else {
        include_str!("windows-dev-app-manifest.xml")
    };
    let windows = tauri_build::WindowsAttributes::new().app_manifest(manifest);
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("failed to run Tauri build script");
}
