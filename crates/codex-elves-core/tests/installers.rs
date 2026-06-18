use codex_elves_core::install::{
    InstallOptions, SILENT_BINARY, app_bundle_names, build_macos_app_bundle,
    build_windows_entrypoint_plan, companion_binary_path_from_exe, default_install_root_strategy,
    shortcut_names,
};

#[test]
fn windows_entrypoint_plan_contains_silent_and_manager_entrypoints() {
    let options = InstallOptions {
        install_root: Some("C:/Users/A/Desktop".into()),
        launcher_path: Some("C:/Tools/codex-elves.exe".into()),
        manager_path: Some("C:/Tools/codex-elves-manager.exe".into()),
        remove_owned_data: false,
    };

    let plan = build_windows_entrypoint_plan(&options);

    assert!(plan.silent_shortcut.ends_with("CodexElves.lnk"));
    assert!(plan.manager_shortcut.ends_with("CodexElves 管理工具.lnk"));
    assert_eq!(plan.launcher_path, "C:/Tools/codex-elves.exe");
    assert_eq!(plan.manager_path, "C:/Tools/codex-elves-manager.exe");
    assert_eq!(plan.silent_icon_path, "C:/Tools/codex-elves.exe");
    assert_eq!(plan.manager_icon_path, "C:/Tools/codex-elves-manager.exe");
    assert_eq!(plan.uninstall_key, "CodexElves");
}

#[test]
fn windows_entrypoint_plan_can_request_owned_data_removal_without_shell_script() {
    let options = InstallOptions {
        install_root: Some("C:/Users/A/Desktop".into()),
        launcher_path: None,
        manager_path: None,
        remove_owned_data: true,
    };

    let plan = build_windows_entrypoint_plan(&options);

    assert!(plan.silent_shortcut.ends_with("CodexElves.lnk"));
    assert!(plan.manager_shortcut.ends_with("CodexElves 管理工具.lnk"));
    assert!(plan.remove_owned_data);
}

#[test]
fn macos_bundle_metadata_contains_silent_and_manager_apps() {
    let options = InstallOptions {
        install_root: Some("/Applications".into()),
        launcher_path: Some("/opt/CodexElves/codex-elves".into()),
        manager_path: Some("/opt/CodexElves/codex-elves-manager".into()),
        remove_owned_data: false,
    };

    let silent = build_macos_app_bundle(&options, false);
    let manager = build_macos_app_bundle(&options, true);

    assert!(silent.app_path.ends_with("CodexElves.app"));
    assert!(manager.app_path.ends_with("CodexElves 管理工具.app"));
    assert!(silent.info_plist.contains("<string>CodexElves</string>"));
    assert!(
        manager
            .info_plist
            .contains("<string>CodexElves 管理工具</string>")
    );
    assert!(silent.launch_script.contains("codex-elves"));
    assert!(manager.launch_script.contains("codex-elves-manager"));
}

#[test]
fn installer_exports_expected_two_entrypoint_names() {
    assert_eq!(
        shortcut_names(),
        ("CodexElves.lnk", "CodexElves 管理工具.lnk")
    );
    assert_eq!(
        app_bundle_names(),
        ("CodexElves.app", "CodexElves 管理工具.app")
    );
}

#[test]
fn macos_dmg_includes_applications_shortcut_for_drag_install() {
    let script = std::fs::read_to_string("../../scripts/installer/macos/package-dmg.sh")
        .expect("read macOS DMG packaging script");

    assert!(script.contains("ln -s /Applications \"$STAGE/Applications\""));
}

#[test]
fn companion_binary_path_resolves_macos_silent_app_next_to_manager_app() {
    let manager_exe = std::path::Path::new(
        "/Applications/CodexElves 管理工具.app/Contents/MacOS/CodexElvesManager",
    );

    let companion = companion_binary_path_from_exe(manager_exe, SILENT_BINARY);

    assert_eq!(
        companion,
        std::path::PathBuf::from("/Applications/CodexElves.app/Contents/MacOS/CodexElves")
    );
    assert_ne!(
        companion,
        std::path::PathBuf::from(
            "/Applications/CodexElves 管理工具.app/Contents/MacOS/codex-elves"
        )
    );
}

#[test]
fn macos_bundle_does_not_wrap_the_bundle_executable_in_itself() {
    let options = InstallOptions {
        install_root: Some("/Applications".into()),
        launcher_path: Some("/Applications/CodexElves.app/Contents/MacOS/CodexElves".into()),
        manager_path: Some(
            "/Applications/CodexElves 管理工具.app/Contents/MacOS/CodexElvesManager".into(),
        ),
        remove_owned_data: false,
    };

    let silent = build_macos_app_bundle(&options, false);
    let manager = build_macos_app_bundle(&options, true);

    assert!(!silent.launch_script.contains("CodexElves\""));
    assert!(!manager.launch_script.contains("CodexElvesManager\""));
    assert!(silent.launch_script.contains("codex-elves"));
    assert!(manager.launch_script.contains("codex-elves-manager"));
}

#[test]
fn windows_default_install_root_uses_known_folder_before_userprofile_desktop() {
    let strategy = default_install_root_strategy();

    if cfg!(windows) {
        assert_eq!(strategy, "windows-known-folder");
    } else if cfg!(target_os = "macos") {
        assert_eq!(strategy, "macos-applications");
    } else {
        assert_eq!(strategy, "user-dirs-desktop");
    }
}
