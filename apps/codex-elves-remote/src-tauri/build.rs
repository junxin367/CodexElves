fn main() {
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
                "remote_titlebar_state",
            ]));
        tauri_build::try_build(attributes).expect("failed to build CodexElves Remote");
    }
}
