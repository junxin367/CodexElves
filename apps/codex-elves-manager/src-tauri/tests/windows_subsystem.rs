#[cfg(windows)]
#[test]
fn manager_binary_uses_windows_gui_subsystem_in_debug_and_release() {
    let main_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"))
        .expect("read manager main.rs");

    assert!(
        main_rs.contains("#![cfg_attr(windows, windows_subsystem = \"windows\")]"),
        "manager binary should not allocate a console window on Windows"
    );
}

#[test]
fn manager_release_binary_uses_embedded_frontend_assets() {
    let cargo_toml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read manager Cargo.toml");

    assert!(
        cargo_toml.contains("custom-protocol"),
        "release manager binary should use Tauri custom protocol instead of devUrl localhost"
    );
}

#[test]
fn manager_uses_single_instance_guard_before_starting_tauri() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read manager lib.rs");

    assert!(lib_rs.contains("acquire_single_instance_guard(show_update)"));
    assert!(lib_rs.contains("manager_guard_port()"));
    assert!(lib_rs.contains("manager.already_running"));
}

#[test]
fn manager_dev_mode_has_separate_title_and_window_state() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read manager lib.rs");

    assert!(lib_rs.contains("CODEX_ELVES_MANAGER_DEV"));
    assert!(lib_rs.contains("CodexElves 管理工具 Dev"));
    assert!(lib_rs.contains("manager-window-state-dev.json"));
    assert!(lib_rs.contains("manager_window_title()"));
    assert!(lib_rs.contains("manager_window_state_file()"));
}

#[test]
fn manager_dev_mode_loads_vite_dev_url_for_manual_window() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read manager lib.rs");

    assert!(lib_rs.contains("manager_webview_url(show_update)?"));
    assert!(lib_rs.contains("tauri::WebviewUrl::External"));
    assert!(lib_rs.contains("http://localhost:1420/"));
    assert!(lib_rs.contains("tauri::WebviewUrl::App(url.into())"));
}

#[test]
fn manager_default_capability_allows_vite_dev_origin() {
    let capability = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/capabilities/default.json"
    ))
    .expect("read default capability");

    assert!(capability.contains("\"local\": true"));
    assert!(capability.contains("\"remote\""));
    assert!(capability.contains("\"urls\""));
    assert!(capability.contains("\"http://localhost:1420\""));
    assert!(capability.contains("\"http://localhost:1420/*\""));
}

#[test]
fn dev_manager_script_sets_isolated_dev_environment() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let script = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/dev-manager.ps1");
    let script = std::fs::read_to_string(&script).expect("read dev manager script");

    assert!(script.contains("CODEX_ELVES_MANAGER_DEV"));
    assert!(script.contains("CODEX_ELVES_MANAGER_GUARD_PORT"));
    assert!(script.contains("[int]$GuardPort = 45229"));
    assert!(script.contains("npm run dev"));
}

#[cfg(windows)]
#[test]
fn cargo_cache_cleanup_skips_when_debug_target_is_below_limit() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let cleanup_script = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/cleanup-cargo-cache.ps1");
    let temp_dir = tempfile::tempdir().expect("create cleanup test directory");
    let debug_dir = temp_dir.path().join("target/debug");
    std::fs::create_dir_all(&debug_dir).expect("create debug target directory");
    std::fs::write(debug_dir.join("small.bin"), [0_u8; 16]).expect("write small debug artifact");
    let cargo_marker = temp_dir.path().join("cargo-args.txt");
    let fake_cargo = temp_dir.path().join("fake-cargo.cmd");
    std::fs::write(
        &fake_cargo,
        format!(
            "@echo off\r\necho %*>>\"{}\"\r\nexit /b 0\r\n",
            cargo_marker.display()
        ),
    )
    .expect("write fake cargo command");

    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            cleanup_script.to_str().expect("cleanup script path"),
            "-RepoRoot",
            temp_dir.path().to_str().expect("temporary repo path"),
            "-MaxSizeGiB",
            "1",
            "-CargoPath",
            fake_cargo.to_str().expect("fake cargo path"),
        ])
        .output()
        .expect("run cleanup script");

    assert!(
        output.status.success(),
        "cleanup script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("[SKIP]"),
        "cleanup script should report that cleanup was skipped"
    );
    assert!(
        !cargo_marker.exists(),
        "cargo clean must not run below the configured limit"
    );
}

#[cfg(windows)]
#[test]
fn cargo_cache_cleanup_serializes_overlapping_foreground_requests() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let cleanup_script = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/cleanup-cargo-cache.ps1");
    let temp_dir = tempfile::tempdir().expect("create cleanup test directory");
    let repo_root = temp_dir.path().join("repo with spaces");
    let debug_dir = repo_root.join("target/debug");
    std::fs::create_dir_all(&debug_dir).expect("create debug target directory");
    std::fs::write(debug_dir.join("large.bin"), [0_u8; 2048]).expect("write large debug artifact");
    let cargo_started = repo_root.join("cargo-started.txt");
    let cargo_marker = repo_root.join("cargo-args.txt");
    let fake_cargo = repo_root.join("fake-cargo.cmd");
    std::fs::write(
        &fake_cargo,
        format!(
            "@echo off\r\necho started>\"{}\"\r\npowershell.exe -NoProfile -Command \"Start-Sleep -Milliseconds 2000\"\r\n(\r\n  echo %~1\r\n  echo %~2\r\n  echo %~3\r\n  echo %~4\r\n  echo %~5\r\n)>>\"{}\"\r\nexit /b 0\r\n",
            cargo_started.display(),
            cargo_marker.display()
        ),
    )
    .expect("write fake cargo command");

    let cleanup_command = || {
        let mut command = std::process::Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                cleanup_script.to_str().expect("cleanup script path"),
                "-RepoRoot",
                repo_root.to_str().expect("temporary repo path"),
                "-MaxSizeGiB",
                "0.000001",
                "-CargoPath",
                fake_cargo.to_str().expect("fake cargo path"),
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        command
    };

    let first = cleanup_command()
        .spawn()
        .expect("start first foreground cleanup");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !cargo_started.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        cargo_started.exists(),
        "first cleanup did not acquire the lock"
    );

    let second = cleanup_command()
        .spawn()
        .expect("start overlapping foreground cleanup");
    let first_output = first.wait_with_output().expect("wait for first cleanup");
    let second_output = second.wait_with_output().expect("wait for second cleanup");
    assert!(
        first_output.status.success(),
        "first cleanup failed: {}",
        String::from_utf8_lossy(&first_output.stderr)
    );
    assert!(
        second_output.status.success(),
        "second cleanup failed: {}",
        String::from_utf8_lossy(&second_output.stderr)
    );
    let combined_stdout = format!(
        "{}{}",
        String::from_utf8_lossy(&first_output.stdout),
        String::from_utf8_lossy(&second_output.stdout)
    );
    assert_eq!(combined_stdout.matches("[OK]").count(), 1);
    assert_eq!(
        combined_stdout
            .matches("[SKIP] Cargo cache cleanup is already running")
            .count(),
        1
    );

    let cargo_calls = std::fs::read_to_string(&cargo_marker).expect("read cargo invocations");
    let calls = cargo_calls.lines().collect::<Vec<_>>();
    assert_eq!(
        calls,
        [
            "clean",
            "--profile",
            "dev",
            "--target-dir",
            repo_root.join("target").to_str().expect("target path")
        ],
        "concurrent cleanup requests should invoke cargo clean exactly once"
    );
}

#[cfg(windows)]
#[test]
fn cargo_cache_cleanup_background_mode_is_non_blocking_and_overwrites_status_log() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let cleanup_script = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/cleanup-cargo-cache.ps1");
    let temp_dir = tempfile::tempdir().expect("create cleanup test directory");
    let repo_root = temp_dir.path().join("background repo with spaces");
    let debug_dir = repo_root.join("target/debug");
    std::fs::create_dir_all(&debug_dir).expect("create debug target directory");
    std::fs::write(debug_dir.join("large.bin"), [0_u8; 2048]).expect("write large debug artifact");
    let cargo_finished = repo_root.join("cargo-finished.txt");
    let fake_cargo = repo_root.join("fake cargo.cmd");
    std::fs::write(
        &fake_cargo,
        format!(
            "@echo off\r\npowershell.exe -NoProfile -Command \"Start-Sleep -Milliseconds 3000\"\r\necho finished>\"{}\"\r\nexit /b 0\r\n",
            cargo_finished.display()
        ),
    )
    .expect("write fake cargo command");

    let started = std::time::Instant::now();
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            cleanup_script.to_str().expect("cleanup script path"),
            "-RepoRoot",
            repo_root.to_str().expect("temporary repo path"),
            "-MaxSizeGiB",
            "0.000001",
            "-CargoPath",
            fake_cargo.to_str().expect("fake cargo path"),
            "-Background",
        ])
        .output()
        .expect("schedule background cleanup");
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "background cleanup launcher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "background launcher blocked for {elapsed:?}"
    );

    let status_log = repo_root.join("target/cargo-cache-cleanup.log");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while (!cargo_finished.exists() || !status_log.exists()) && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        cargo_finished.exists(),
        "background cargo command did not finish"
    );
    assert!(
        status_log.exists(),
        "background cleanup did not write status"
    );

    let skip_output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            cleanup_script.to_str().expect("cleanup script path"),
            "-RepoRoot",
            repo_root.to_str().expect("temporary repo path"),
            "-MaxSizeGiB",
            "1",
            "-CargoPath",
            fake_cargo.to_str().expect("fake cargo path"),
        ])
        .output()
        .expect("run below-limit cleanup");
    assert!(skip_output.status.success());

    let status = std::fs::read_to_string(&status_log).expect("read cleanup status");
    assert_eq!(
        status.lines().count(),
        1,
        "cleanup status log should overwrite its previous result"
    );
    assert!(status.contains("skipped:"));
}

#[test]
fn manager_second_launch_requests_existing_window_to_show() {
    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read manager lib.rs");

    assert!(lib_rs.contains("spawn_manager_wake_listener"));
    assert!(lib_rs.contains("request_existing_manager_to_show"));
    assert!(lib_rs.contains("MANAGER_WAKE_SHOW"));
    assert!(lib_rs.contains("MANAGER_WAKE_SHOW_UPDATE"));
    assert!(lib_rs.contains("MANAGER_WAKE_ACK"));
    assert!(lib_rs.contains("stream.write_all(MANAGER_WAKE_ACK)"));
    assert!(lib_rs.contains("fallback_single_instance_guard()"));
    assert!(lib_rs.contains("wake_requested"));
    assert!(lib_rs.contains("show_main_window(&app_handle)"));
}

#[test]
fn launcher_binary_embeds_codex_icon_resource() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let launcher_build = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("codex-elves-launcher/build.rs");
    let build_rs = std::fs::read_to_string(&launcher_build).expect("read launcher build.rs");

    assert!(build_rs.contains("WindowsResource"));
    assert!(build_rs.contains("icons/icon.ico"));
}

#[test]
fn task_board_runs_as_a_separate_window_with_persistent_placement() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo_toml =
        std::fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read Cargo.toml");
    let main_rs = std::fs::read_to_string(manifest_dir.join("src/task_board_main.rs"))
        .expect("read task board main.rs");
    let task_board_rs = std::fs::read_to_string(manifest_dir.join("src/task_board.rs"))
        .expect("read task board backend");
    let permissions = std::fs::read_to_string(manifest_dir.join("permissions/default.toml"))
        .expect("read manager permissions");

    assert!(cargo_toml.contains("name = \"codex-elves-task-board\""));
    assert!(cargo_toml.contains("\"image-png\""));
    assert!(main_rs.contains("#![cfg_attr(windows, windows_subsystem = \"windows\")]"));
    assert!(task_board_rs.contains("include_bytes!(\"../icons/task-board.png\")"));
    assert!(task_board_rs.contains("set_default_window_icon(Some(task_board_icon.clone()))"));
    assert!(task_board_rs.contains(".icon(task_board_icon.clone())?"));
    assert!(task_board_rs.contains("set_task_board_windows_taskbar_icon(&window)?"));
    assert!(task_board_rs.contains("const WM_SETICON: u32 = 0x0080"));
    assert!(task_board_rs.contains("const ICON_BIG: usize = 1"));
    assert!(task_board_rs.contains("task-board-webview2"));
    assert!(task_board_rs.contains("task-board-window-state.json"));
    assert!(task_board_rs.contains(".center()"));
    assert!(task_board_rs.contains("window_state_is_visible"));
    assert!(task_board_rs.contains("persist_window_state"));
    assert!(task_board_rs.contains("task_board_guard_port()"));
    assert!(task_board_rs.contains("JSON.stringify(result ?? null)"));
    assert!(task_board_rs.contains("task_board_load_host_appearance"));
    assert!(task_board_rs.contains("task_board_load_conversation_statuses"));
    assert!(task_board_rs.contains("task_board_delete_task"));
    assert!(task_board_rs.contains("TaskBoardDeleteCommand"));
    assert!(task_board_rs.contains("task_board_rename_task"));
    assert!(task_board_rs.contains("TaskBoardRenameTaskCommand"));
    assert!(task_board_rs.contains("call_codex_host_operation("));
    assert!(task_board_rs.contains("call_codex_host_with_min_runtime("));
    assert!(task_board_rs.contains("TASK_BOARD_MIN_CONVERSATION_STATUS_RUNTIME_VERSION: u64 = 58"));
    assert!(task_board_rs.contains("__codexElvesTaskBoardStandaloneOperations"));
    assert!(task_board_rs.contains("task_board_host_operation_abandon_script"));
    assert!(task_board_rs.contains("host_version_unsupported"));
    assert!(task_board_rs.contains("host_outcome_unknown"));
    assert!(task_board_rs.contains("TASK_BOARD_HOST_OPERATION_TIMEOUT"));
    assert!(task_board_rs.contains("Duration::from_secs(120)"));
    assert!(task_board_rs.contains("nativeCreateLease"));
    assert!(task_board_rs.contains("nativeCreateRuntime"));
    assert!(!task_board_rs.contains("skip_initial_composer"));
    assert!(!task_board_rs.contains("TASK_BOARD_HOST_OPERATION_POLL_SLICE_MS"));
    assert!(!task_board_rs.contains("taskBoardStandaloneQuerySelector"));
    assert!(!task_board_rs.contains("taskBoardStandaloneDispatchEvent"));
    assert!(permissions.contains("\"task_board_load_create_options\""));
    assert!(permissions.contains("\"task_board_load_host_appearance\""));
    assert!(permissions.contains("\"task_board_load_conversation_statuses\""));
    assert!(permissions.contains("\"task_board_delete_task\""));
    assert!(permissions.contains("\"task_board_rename_task\""));
}

#[test]
fn standalone_task_board_icon_has_transparent_corners_and_windows_sizes() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let png_bytes =
        std::fs::read(manifest_dir.join("icons/task-board.png")).expect("read task board PNG");
    let png = tauri::image::Image::from_bytes(&png_bytes).expect("decode task board PNG");

    assert_eq!((png.width(), png.height()), (256, 256));
    let alpha_at = |x: u32, y: u32| {
        let index = ((y * png.width() + x) * 4 + 3) as usize;
        png.rgba()[index]
    };
    for point in [(0, 0), (255, 0), (0, 255), (255, 255)] {
        assert_eq!(alpha_at(point.0, point.1), 0);
    }
    assert_eq!(alpha_at(128, 128), 255);

    let ico_bytes =
        std::fs::read(manifest_dir.join("icons/task-board.ico")).expect("read task board ICO");
    assert_eq!(&ico_bytes[..4], &[0, 0, 1, 0]);
    assert_eq!(u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]), 9);
}

#[test]
fn standalone_task_board_reuses_codex_board_visual_language() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let frontend_dir = manifest_dir.parent().expect("manager frontend directory");
    let app = std::fs::read_to_string(frontend_dir.join("src/TaskBoardApp.tsx"))
        .expect("read standalone task board");
    let styles = std::fs::read_to_string(frontend_dir.join("src/task-board.css"))
        .expect("read standalone task board styles");
    let standalone_styles =
        std::fs::read_to_string(frontend_dir.join("src/task-board-standalone.css"))
            .expect("read standalone task board entry styles");
    let main = std::fs::read_to_string(frontend_dir.join("src/main.tsx"))
        .expect("read standalone task board entry");
    let task_board_commands = std::fs::read_to_string(manifest_dir.join("src/task_board.rs"))
        .expect("read standalone task board commands");

    assert!(app.contains("跨项目观察任务状态，并集中关联项目下的多个会话"));
    assert!(app.contains("搜索任务、项目或关联会话"));
    assert!(app.contains("拖动任务卡片可调整顺序或切换状态"));
    assert!(app.contains("formatSessionUpdatedTime(session.updatedAtMs)"));
    assert!(app.contains("task-board-session-time"));
    assert!(app.contains("\"task_board_load_host_appearance\""));
    assert!(app.contains("\"task_board_load_conversation_statuses\""));
    assert!(app.contains("nativeCreateAvailable: null"));
    assert!(app.contains("probe.canStart !== true"));
    assert!(app.contains("disabled={editor.busy || !editor.projectCwd}"));
    assert!(!app.contains("正在确认 Codex 新会话能力…"));
    let native_create_panel = app
        .split("<div className=\"task-board-new-session\">")
        .nth(1)
        .and_then(|section| {
            section
                .split("<label className=\"task-board-field task-board-instruction\">")
                .next()
        })
        .expect("standalone native create panel");
    assert!(!native_create_panel.contains("task-board-create-availability"));
    let instruction_heading = app
        .split("<span className=\"task-board-instruction-heading\">")
        .nth(1)
        .and_then(|section| {
            section
                .split("<div className=\"task-board-create-composer\">")
                .next()
        })
        .expect("standalone native create instruction heading");
    assert!(instruction_heading.contains("editor.nativeCreateAvailable === false ? ("));
    assert!(instruction_heading.contains("task-board-create-availability"));
    let instruction_heading_styles = styles
        .split(".task-board-instruction-heading {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone native create instruction heading styles");
    assert!(instruction_heading_styles.contains("display: flex;"));
    assert!(instruction_heading_styles.contains("justify-content: space-between;"));
    let availability_styles = styles
        .split(".task-board-create-availability {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone native create availability styles");
    assert!(availability_styles.contains("overflow: hidden;"));
    assert!(availability_styles.contains("text-overflow: ellipsis;"));
    assert!(availability_styles.contains("white-space: nowrap;"));
    assert!(app.contains("function taskProjectRef(project: TaskProject)"));
    assert!(app.contains("project: taskProjectRef(project)"));
    assert!(app.contains("const hostProject = taskProjectRef(project)"));
    assert!(app.contains("task-board-conversation-state"));
    assert!(app.contains("const [catalogReady, setCatalogReady] = useState(false);"));
    assert!(app.contains("function taskBoardConversationShouldDisplay("));
    assert!(app.contains("const visibleConversations = task.conversations.filter"));
    assert!(app.contains("setCatalogReady(true);"));
    assert!(app.contains(r#"return { id: "unread", label: "未读" }"#));
    assert!(!app.contains("已完成 · 未读"));
    assert!(app.contains(r#"const showStatus = status.id !== "completed";"#));
    assert!(app.contains(r#"status.id === "running" || status.id === "unread""#));
    assert!(app.contains("{iconOnlyStatus ? null : status.label}"));
    assert!(app.contains("conversationReadSuppressionsRef.current.add(sessionKey)"));
    let status_refresh = app
        .split("const refreshStatuses = async () => {")
        .nth(1)
        .and_then(|section| section.split("void refreshStatuses();").next())
        .expect("standalone conversation status refresh");
    assert!(status_refresh.contains("sessionAliases: catalogSessionAliases.get("));
    assert!(app.contains("TaskBoardAppearanceOverlay"));
    assert!(app.contains("appearanceRefreshIntervalMs = 20_000"));
    assert!(app.contains("codexElvesAppearance="));
    assert!(app.contains("TaskBoardDetachConfirmation"));
    assert!(app.contains("TaskBoardDeleteConfirmation"));
    assert!(app.contains("\"task_board_delete_task\""));
    assert!(app.contains("\"task_board_rename_task\""));
    assert!(app.contains("\"task_board_create_board\""));
    assert!(app.contains("\"task_board_delete_board\""));
    assert!(app.contains("\"task_board_rename_board\""));
    assert!(app.contains("\"task_board_move_board\""));
    assert!(app.contains(r##"{ id: "planning", label: "规划", color: "#60a5fa" }"##));
    assert!(app.contains(r##"{ id: "executing", label: "执行", color: "#c084fc" }"##));
    assert!(app.contains(r##"{ id: "review", label: "验收", color: "#fbbf24" }"##));
    assert!(app.contains(r##"{ id: "done", label: "完成", color: "#34d399" }"##));
    assert!(app.contains("className=\"task-board-manage\""));
    assert!(app.contains("function TaskBoardManager("));
    assert!(app.contains("添加、改名、排序或删除任务状态列；“未分配”是固定的系统列。"));
    assert!(app.contains("visibleStatusDefinitions"));
    let visible_statuses = app
        .split("const visibleStatusDefinitions = useMemo(")
        .nth(1)
        .and_then(|section| section.split("const unassignedDropActive").next())
        .expect("standalone visible task-board statuses");
    assert!(!visible_statuses.contains("dragTaskId"));
    assert!(!app.contains("Boolean(dragTaskId),"));
    assert!(app.contains("taskBoardColumnsStyle(visibleStatusDefinitions.length)"));
    assert!(app.contains("className=\"task-board-unassigned-drop-zone\""));
    assert!(app.contains("aria-hidden={!unassignedDropActive}"));
    assert!(app.contains("className=\"task-board-sticky-header\""));
    assert!(app.contains("aria-label=\"任务看板，可横向和纵向滚动\""));
    assert!(!app.contains("aria-label=\"任务看板列，可横向和纵向滚动\""));
    assert!(app.contains("event.dataTransfer.setData(\"text/plain\", task.id);"));
    assert!(styles.contains(".task-board-unassigned-drop-zone"));
    assert!(styles.contains("position: absolute;"));
    assert!(app.contains("taskBoardMoveBoardTargetIndex("));
    assert!(app.contains("const taskBoardStatusIconPaths = ["));
    assert!(app.contains("function taskBoardStatusIconIndex("));
    assert!(app.contains("data-task-board-status-icon={iconIndex}"));
    assert!(app.contains("data-searchable={searchable || undefined}"));
    assert!(app.contains("searchPlaceholder=\"搜索项目名称或路径\""));
    assert!(app.matches("searchable").count() >= 4);
    assert!(!app.contains("task-board-dropdown-option-marker"));
    assert!(app.contains("className=\"task-board-board-action-slot\""));
    assert!(app.contains("task-board-board-mode-action"));
    assert!(app.contains("value={editingBoardId ? editingLabel : manager.label}"));
    assert!(app.contains("event.dataTransfer.setDragImage("));
    assert!(!app.contains("task-board-board-edit-form"));
    assert!(app.contains("aria-label={`拖动看板 ${board.label} 调整排序`}"));
    assert!(app.contains("aria-label={`编辑看板 ${board.label}`}"));
    assert!(task_board_commands.contains("task_board_create_board"));
    assert!(task_board_commands.contains("task_board_delete_board"));
    assert!(task_board_commands.contains("task_board_rename_board"));
    assert!(task_board_commands.contains("task_board_move_board"));
    assert!(task_board_commands.contains("task_board_rename_task"));
    assert!(app.contains("aria-label={`删除任务 ${task.title}`}"));
    assert!(app.contains("task-board-card-title-input"));
    assert!(app.contains("titleLength < 1 || titleLength > 120"));
    assert!(app.contains("event.key === \"Enter\""));
    assert!(app.contains("event.key === \"Escape\""));
    assert!(app.contains("function taskBoardMoveTaskTargetIndex("));
    assert!(app.contains("function taskBoardTaskMoveIsNoOp("));
    assert!(app.contains("event.clientY >= bounds.top + bounds.height / 2"));
    assert!(app.contains("data-drop-position={dropPosition}"));
    assert!(app.contains("拖动任务卡片可调整顺序或切换状态"));
    assert!(app.contains("<X size={13} strokeWidth={1.35} aria-hidden=\"true\" />"));
    assert!(!app.contains("Trash2"));
    assert!(app.contains("不会删除 Codex 中的原始会话"));
    assert!(app.contains("task-board-confirm-backdrop"));
    assert!(app.contains("仅解除与任务“{confirmation.task.title || \"未命名任务\"}”的关联"));
    assert!(!app.contains("window.confirm("));
    assert!(!app.contains("独立 WebView2 进程"));
    assert!(styles.contains("grid-template-columns: repeat(5, minmax(0, 1fr))"));
    assert!(styles.contains("min-width: 1580px"));
    assert!(styles.contains("align-content: start"));
    assert!(styles.contains("grid-auto-rows: max-content"));
    assert!(styles.contains(".task-board-session-copy"));
    assert!(styles.contains(".task-board-session-time"));
    assert!(styles.contains("--task-board-modal-background"));
    assert!(styles.contains(".task-board-dropdown-status-icon"));
    assert!(styles.contains(".task-board-dropdown-search"));
    assert!(styles.contains(".task-board-dropdown-options"));
    let card_title_button_styles = styles
        .split(".task-board-app .task-board-card-title-button {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-card title button styles should override global buttons");
    assert!(card_title_button_styles.contains("cursor: text"));
    assert!(styles.contains(".task-board-app .task-board-card-title-button:focus-visible {"));
    let card_title_input_styles = styles
        .split(".task-board-card-title-input {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-card title input styles should be present");
    assert!(card_title_input_styles.contains("border: 1px solid"));
    let card_title_focus_styles = styles
        .split(".task-board-app .task-board-card-title-input:focus {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-card title focus styles should be present");
    assert!(card_title_focus_styles.contains("border-color: var(--task-board-accent)"));
    assert!(card_title_focus_styles.contains("box-shadow: none"));
    assert!(card_title_focus_styles.contains("outline: none"));
    assert!(card_title_focus_styles.contains("outline-offset: 0"));
    assert!(!card_title_focus_styles.contains("box-shadow: 0 0 0"));
    assert!(styles.contains(".task-board-card[data-drop-position=\"before\"]"));
    assert!(styles.contains(".task-board-card[data-drop-position=\"after\"]"));
    let page_styles = styles
        .split(".task-board-page {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board page styles should be present");
    assert!(page_styles.contains("overflow: auto"));
    assert!(page_styles.contains("overscroll-behavior: contain"));
    assert!(!page_styles.contains("padding: 24px 28px"));
    let sticky_header_styles = styles
        .split(".task-board-sticky-header {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board sticky header styles should be present");
    assert!(sticky_header_styles.contains("position: sticky"));
    assert!(sticky_header_styles.contains("top: 0"));
    assert!(sticky_header_styles.contains("left: 0"));
    assert!(sticky_header_styles.contains("padding: 12px 14px 8px"));
    let board_scroll_styles = styles
        .split(".task-board-scroll {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board content styles should be present");
    assert!(board_scroll_styles.contains("overflow: visible"));
    assert!(board_scroll_styles.contains("padding: 8px 14px 12px"));
    assert!(!board_scroll_styles.contains("overflow: auto"));
    assert!(styles.contains(".task-board-page::-webkit-scrollbar"));
    assert!(!styles.contains(".task-board-scroll::-webkit-scrollbar"));
    assert_eq!(
        styles.matches("--task-board-status-icon-color: #").count(),
        10
    );
    assert!(styles.contains(".task-board-dropdown-menu[data-searchable=\"true\"]"));
    assert!(styles.contains("margin-bottom: 5px"));
    assert!(!styles.contains("--task-board-status-color"));
    assert!(styles.contains(".task-board-conversation-state"));
    assert!(styles.contains(r#"[data-conversation-status="unread"]"#));
    assert!(styles.contains("animation: task-board-status-spin 1.1s linear infinite;"));
    assert!(styles.contains("animation: none !important;"));
    assert!(!styles.contains(r#"[data-conversation-status="completed-unread"]"#));
    assert!(styles.contains(".task-board-card-delete"));
    assert!(styles.contains(".task-board-card {\n  position: relative;"));
    assert!(styles.contains(".task-board-card-delete {\n  position: absolute;"));
    assert!(styles.contains("top: 5px;\n  right: 5px;"));
    let conversation_row_styles = styles
        .split(".task-board-conversation-row {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board conversation row styles should be present");
    assert!(conversation_row_styles.contains("position: relative"));
    assert!(conversation_row_styles.contains("gap: 0"));
    let conversation_styles = styles
        .split(".task-board-conversation {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board conversation styles should be present");
    assert!(conversation_styles.contains("font: 11px/1.3 system-ui, sans-serif"));
    let conversation_remove_styles = styles
        .split(".task-board-conversation-remove {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board conversation remove styles should be present");
    assert!(conversation_remove_styles.contains("position: absolute"));
    assert!(conversation_remove_styles.contains("left: 4px"));
    assert!(conversation_remove_styles.contains("opacity: 0"));
    assert!(conversation_remove_styles.contains("pointer-events: none"));
    let conversation_icon_styles = styles
        .split(".task-board-conversation-icon {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board conversation icon styles should be present");
    assert!(conversation_icon_styles.contains("width: 24px"));
    assert!(conversation_icon_styles.contains("height: 24px"));
    assert!(styles.contains(".task-board-conversation-row:hover .task-board-conversation-icon,"));
    assert!(
        styles
            .contains(".task-board-conversation-row:focus-within .task-board-conversation-remove")
    );
    assert!(styles.contains("background: #1f1f1f"));
    assert!(styles.contains("font-size: 24px"));
    assert!(!app.contains("<select"));
    assert!(app.contains("function TaskBoardDropdown"));
    assert!(app.contains("showChevron = true"));
    assert_eq!(app.matches("showChevron={false}").count(), 1);
    assert!(app.contains("function TaskBoardCreateSettings"));
    assert!(app.contains("function taskBoardDropdownLeft("));
    assert!(app.contains("function taskBoardCreateSubmenuLeft("));
    assert!(app.contains("const rightLeft = menuRight + gap;"));
    assert!(app.contains("if (rightLeft + submenuWidth <= viewportWidth - edge)"));
    assert!(app.contains("menuLeft - gap - submenuWidth"));
    assert!(app.contains("function taskBoardCreateSubmenuTop("));
    assert!(app.contains("const centeredTop = anchorTop + (anchorHeight - submenuHeight) / 2;"));
    assert!(app.contains("taskBoardCreateSubmenuTop("));
    assert!(!app.contains("parentRect.top - 5"));
    assert!(!app.contains("align?: \"start\" | \"end\""));
    assert!(!app.contains("align=\"start\""));
    assert!(app.contains("function taskBoardModalFocusableElements"));
    assert!(app.contains("为“${editor.targetTask?.title || \"未命名任务\"}”关联已有会话"));
    assert!(app.contains("只可追加当前任务所属项目中的会话。"));
    assert!(styles.contains(".task-board-dropdown-chevron"));
    let card_move_styles = styles
        .split(".task-board-card-move {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-card status trigger styles should be present");
    assert!(card_move_styles.contains("width: auto"));
    assert!(card_move_styles.contains("border: 0"));
    assert!(card_move_styles.contains("background: transparent"));
    assert!(card_move_styles.contains("padding: 0 2px"));
    assert!(styles.contains(".task-board-card-move[aria-expanded=\"true\"]"));
    let dropdown_button_styles = styles
        .split(".task-board-dropdown-menu button {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone dropdown button styles should be present");
    assert!(dropdown_button_styles.contains("align-items: center"));
    assert!(!dropdown_button_styles.contains("align-items: flex-start"));
    assert!(!styles.contains(".task-board-dropdown-option-marker"));
    assert!(styles.contains(".task-board-create-settings-chevron svg"));
    assert!(
        styles.contains(".task-board-board-edit,\n.task-board-board-delete {\n  cursor: pointer;")
    );
    let icon_button_styles = styles
        .split(".task-board-icon-button {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board icon button styles should be present");
    assert!(icon_button_styles.contains("cursor: pointer"));
    let modal_button_styles = styles
        .split("\n.task-board-button {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board modal button styles should be present");
    assert!(modal_button_styles.contains("cursor: pointer"));
    assert!(styles.contains("grid-template-columns: 18px 16px minmax(0, 1fr)"));
    assert!(styles.contains("--task-board-modal-overlay-background: rgb(0 0 0 / 45%)"));
    assert!(styles.contains("--task-board-modal-radius: 18px"));
    assert!(styles.contains("--task-board-modal-shadow: 0 24px 80px rgb(0 0 0 / 45%)"));
    assert!(styles.contains("--task-board-modal-viewport-gap: 48px"));
    assert!(
        styles.contains("height: min(650px, calc(100vh - var(--task-board-modal-viewport-gap)))")
    );
    assert!(
        styles.contains("width: min(420px, calc(100vw - var(--task-board-modal-viewport-gap)))")
    );
    assert!(
        styles.contains("width: min(600px, calc(100vw - var(--task-board-modal-viewport-gap)))")
    );
    assert!(!styles.contains("backdrop-filter: blur(4px)"));
    assert!(!styles.contains("background: rgb(0 0 0 / 58%)"));
    let search_focus_styles = styles
        .split(".task-board-search:focus-within {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board search focus styles should be present");
    assert!(search_focus_styles.contains("outline: 1px solid #38bdf8"));
    assert!(!search_focus_styles.contains("outline: 2px"));
    let search_input_focus_styles = styles
        .split(".task-board-app .task-board-search input:focus-visible {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board search input focus reset should be present");
    assert!(search_input_focus_styles.contains("outline: none"));
    assert!(search_input_focus_styles.contains("box-shadow: none"));
    assert!(!standalone_styles.contains("task-board-search"));
    let create_mode_tabs_styles = styles
        .split(".task-board-mode-tabs {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone create-mode group styles should be present");
    assert!(create_mode_tabs_styles.contains("box-sizing: border-box"));
    assert!(create_mode_tabs_styles.contains("height: 35px"));
    let create_mode_button_styles = styles
        .split(".task-board-mode-tabs button {")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone create-mode button styles should be present");
    assert!(create_mode_button_styles.contains("height: 100%"));
    assert!(create_mode_button_styles.contains("min-height: 0"));
    assert!(styles.contains(".task-board-app :where(button, input, textarea)"));
    assert!(styles.contains(
        ".task-board-app .task-board-field input:focus-visible,\n.task-board-app .task-board-create-select:focus-visible,\n.task-board-app .task-board-field textarea:focus-visible"
    ));
    let field_focus_styles = styles
        .split(".task-board-field input:focus,")
        .nth(1)
        .and_then(|section| section.split('}').next())
        .expect("standalone task-board field focus styles should be present");
    assert!(field_focus_styles.contains("border-color: var(--task-board-accent)"));
    assert!(field_focus_styles.contains("box-shadow: 0 0 0 1px"));
    assert!(!field_focus_styles.contains("box-shadow: 0 0 0 2px"));
    assert!(styles.contains(".task-board-appearance-overlay"));
    assert!(styles.contains("z-index: 2147483646"));
    assert!(styles.contains(".task-board-confirm-backdrop"));
    assert!(styles.contains("z-index: 2147483200"));
    assert!(styles.contains(".task-board-board-manager"));
    assert!(styles.contains(".task-board-board-list"));
    assert!(styles.contains(".task-board-board-drag-handle"));
    assert!(styles.contains("grid-template-columns: minmax(0, 1fr) 98px"));
    assert!(styles.contains(".task-board-board-action-slot"));
    assert!(styles.contains(".task-board-board-mode-action"));
    assert!(styles.contains("flex: 1 1 0"));
    assert!(!styles.contains(".task-board-board-edit-form"));
    assert!(styles.contains(".task-board-board-item[data-drop-position=\"before\"]"));
    assert!(styles.contains(".task-board-board-manager *"));
    assert!(styles.contains(".task-board-board-delete-confirm"));
    assert!(standalone_styles.contains(".task-board-app button"));
    assert!(standalone_styles.contains("transition: none"));
    assert!(main.contains("if (taskBoardMode)"));
    assert!(main.contains("void import(\"./task-board-standalone.css\")"));
    assert!(main.contains("void import(\"./styles.css\")"));
}

#[test]
fn standalone_native_create_recovery_is_private_and_idempotent() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let frontend_dir = manifest_dir.parent().expect("manager frontend directory");
    let app = std::fs::read_to_string(frontend_dir.join("src/TaskBoardApp.tsx"))
        .expect("read standalone task board");

    let submit = app
        .split("const submitEditor = useCallback(async () => {")
        .nth(1)
        .and_then(|value| value.split("const sessionsForEditor = useMemo").next())
        .expect("standalone submitEditor body");
    assert!(
        submit
            .trim_start()
            .starts_with("if (submitEditorBusyRef.current) return;")
    );
    assert!(submit.contains("submitEditorBusyRef.current = true;"));
    assert!(submit.contains("submitEditorBusyRef.current = false;"));
    assert!(submit.contains("if (editor.mode === \"new\" && recovery)"));
    assert!(submit.contains("} else if (editor.mode === \"new\") {"));
    assert_eq!(
        submit.matches("\"task_board_start_conversation\"").count(),
        1
    );
    assert!(submit.contains("recovery.semanticKey === semanticKey"));
    assert!(submit.contains("taskId = recovery.taskId"));
    assert!(submit.contains("taskBoardRecoveryAlreadyApplied(recovery, snapshot)"));
    assert!(
        submit.find("let taskId =").expect("stable task id")
            < submit
                .find("\"task_board_start_conversation\"")
                .expect("start conversation")
    );
    assert!(
        submit
            .find("saveNativeCreateRecovery(recovery)")
            .expect("save recovery")
            < submit.find("const command =").expect("task mutation")
    );

    let save_recovery = app
        .split("function saveNativeCreateRecovery(")
        .nth(1)
        .and_then(|value| value.split("function clearNativeCreateRecovery()").next())
        .expect("standalone recovery writer");
    assert!(save_recovery.contains("sessionStorage.setItem"));
    assert!(!save_recovery.contains("instruction"));
    assert!(!save_recovery.contains("firstInstruction"));
    assert!(!save_recovery.contains("modelId"));
    assert!(!save_recovery.contains("effortId"));

    let automatic_recovery = app
        .split("const attemptNativeCreateRecovery = useCallback(")
        .nth(1)
        .and_then(|value| value.split("const refresh = useCallback").next())
        .expect("standalone automatic recovery");
    assert!(automatic_recovery.contains("nativeCreateRecoveryAttemptedRef.current = true"));
    assert!(automatic_recovery.contains("\"task_board_create_task\""));
    assert!(automatic_recovery.contains("\"task_board_attach_conversations\""));
    assert!(automatic_recovery.contains("expectedRevision: effectiveSnapshot.revision"));
    assert!(automatic_recovery.contains("invokeSessionMutationWithRetry"));
    assert!(automatic_recovery.contains("clearNativeCreateRecovery()"));
    assert!(automatic_recovery.contains("taskBoardRecoveryAlreadyApplied"));
    assert!(!automatic_recovery.contains("task_board_start_conversation"));

    assert!(app.contains("nativeCreateRecoveryTtlMs = 24 * 60 * 60 * 1000"));
    assert!(app.contains("kind: NativeCreateRecoveryKind"));
    assert!(app.contains("targetTaskId?: string"));
    assert!(app.contains("semanticKey: string"));
    assert!(app.contains("taskBoardCreateTaskIdIsValid"));
    assert!(app.contains("now - createdAtMs > nativeCreateRecoveryTtlMs"));
    assert!(app.contains("if (!record) sessionStorage.removeItem(nativeCreateRecoveryKey)"));
}

#[test]
fn windows_binaries_request_administrator_privileges() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manager_build =
        std::fs::read_to_string(manifest_dir.join("build.rs")).expect("read manager build.rs");
    let windows_manifest = std::fs::read_to_string(manifest_dir.join("windows-app-manifest.xml"))
        .expect("read windows app manifest");
    let windows_dev_manifest =
        std::fs::read_to_string(manifest_dir.join("windows-dev-app-manifest.xml"))
            .expect("read windows dev app manifest");
    let launcher_build = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("codex-elves-launcher/build.rs");
    let launcher_build = std::fs::read_to_string(&launcher_build).expect("read launcher build.rs");
    let windows_installer = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/installer/windows/CodexElves.nsi");
    let windows_installer =
        std::fs::read_to_string(&windows_installer).expect("read windows installer");

    assert!(manager_build.contains("windows-app-manifest.xml"));
    assert!(manager_build.contains("windows-dev-app-manifest.xml"));
    assert!(manager_build.contains("PROFILE"));
    assert!(launcher_build.contains("windows-app-manifest.xml"));
    assert!(windows_manifest.contains("requireAdministrator"));
    assert!(windows_manifest.contains("Microsoft.Windows.Common-Controls"));
    assert!(windows_dev_manifest.contains("asInvoker"));
    assert!(windows_dev_manifest.contains("Microsoft.Windows.Common-Controls"));
    assert!(windows_installer.contains("RequestExecutionLevel admin"));
    assert!(
        windows_installer
            .contains("File \"${ROOT}\\dist\\windows\\app\\codex-elves-task-board.exe\"")
    );
    assert!(windows_installer.contains("taskkill /IM codex-elves-task-board.exe /F"));
    assert!(windows_installer.contains("Delete \"$INSTDIR\\codex-elves-task-board.exe\""));
}

#[test]
fn manager_launch_button_spawns_silent_launcher_binary() {
    let commands_rs =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands.rs"))
            .expect("read manager commands.rs");

    assert!(commands_rs.contains("SILENT_BINARY"));
    assert!(commands_rs.contains("std::process::Command::new"));
    assert!(!commands_rs.contains("launch_and_inject_with_hooks(options"));
}

#[test]
fn frontend_literal_tauri_commands_are_registered_for_invocation() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib_rs = std::fs::read_to_string(manifest_dir.join("src/lib.rs")).expect("read lib.rs");
    let commands_rs =
        std::fs::read_to_string(manifest_dir.join("src/commands.rs")).expect("read commands.rs");
    let app_tsx = std::fs::read_to_string(manifest_dir.parent().unwrap().join("src/App.tsx"))
        .expect("read App.tsx");
    let default_capability =
        std::fs::read_to_string(manifest_dir.join("capabilities/default.json"))
            .expect("read default capability");
    let default_permissions =
        std::fs::read_to_string(manifest_dir.join("permissions/default.toml"))
            .expect("read default permissions");

    let frontend_commands = literal_tauri_commands(&app_tsx);
    assert!(default_capability.contains("\"allow-manager-commands\""));
    for expected in [
        "load_ccs_providers",
        "import_ccs_providers",
        "plugin_marketplace_status",
        "repair_plugin_marketplace",
    ] {
        assert!(
            frontend_commands.contains(expected),
            "expected frontend command {expected} to be covered"
        );
    }

    for command in frontend_commands {
        let sync_fn = format!("pub fn {command}");
        let async_fn = format!("pub async fn {command}");
        assert!(
            commands_rs.contains(&sync_fn) || commands_rs.contains(&async_fn),
            "frontend command {command} should have a backend command implementation"
        );
        assert!(
            lib_rs.contains(&format!("commands::{command}")),
            "frontend command {command} should be registered in Tauri invoke_handler"
        );
        assert!(
            default_permissions.contains(&format!("\"{command}\"")),
            "frontend command {command} should be allowed by app permissions"
        );
    }
}

fn literal_tauri_commands(source: &str) -> std::collections::BTreeSet<String> {
    let mut commands = std::collections::BTreeSet::new();

    for marker in ["call", "invoke"] {
        let mut offset = 0;
        while let Some(relative_start) = source[offset..].find(marker) {
            let start = offset + relative_start;
            let after_marker = start + marker.len();
            if source[after_marker..]
                .chars()
                .next()
                .is_some_and(|next| next.is_ascii_alphanumeric() || next == '_')
            {
                offset = after_marker;
                continue;
            }

            let Some(relative_open_paren) = source[after_marker..].find('(') else {
                break;
            };
            let open_paren = after_marker + relative_open_paren;
            let argument = source[open_paren + 1..].trim_start();
            let Some(quote) = argument.chars().next() else {
                offset = open_paren + 1;
                continue;
            };
            if quote != '"' && quote != '\'' {
                offset = open_paren + 1;
                continue;
            }

            let rest = &argument[quote.len_utf8()..];
            if let Some(end) = rest.find(quote) {
                commands.insert(rest[..end].to_string());
            }
            offset = open_paren + 1;
        }
    }

    commands
}

#[test]
fn macos_packager_hides_silent_launcher_but_not_manager() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let packager = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/installer/macos/package-dmg.sh");
    let script = std::fs::read_to_string(&packager).expect("read macOS packager");

    assert!(script.contains("<key>LSUIElement</key>"));
    assert!(script.contains("ARCH=\"${2:-$(uname -m)}\""));
    assert!(script.contains("BINARY_DIR=\"${BINARY_DIR:-$ROOT/target/release}\""));
    assert!(script.contains("CodexElves-${VERSION}-macos-${ARCH}.dmg"));
    assert!(script.contains(
        "create_app \"CodexElves\" \"CodexElves\" \"$BINARY_DIR/codex-elves\" \"com.bigpizzav3.codexelves\" \"true\""
    ));
    assert!(script.contains(
        "create_app \"CodexElves 管理工具\" \"CodexElvesManager\" \"$BINARY_DIR/codex-elves-manager\" \"com.bigpizzav3.codexelves.manager\" \"false\""
    ));
}

#[test]
fn github_release_workflow_builds_separate_macos_x64_and_arm64_dmgs() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(".github/workflows/release-assets.yml");
    let workflow = std::fs::read_to_string(&workflow).expect("read release assets workflow");

    assert!(workflow.contains("macos-15-intel"));
    assert!(workflow.contains("x86_64-apple-darwin"));
    assert!(workflow.contains("macos-14"));
    assert!(workflow.contains("aarch64-apple-darwin"));
    assert!(workflow.contains("package-dmg.sh \"$VERSION\" \"${{ matrix.arch }}\""));
    assert!(workflow.contains("target/${{ matrix.target }}/release"));
}

#[test]
fn github_release_workflow_can_build_assets_from_tags_and_manual_dispatch() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(".github/workflows/release-assets.yml");
    let workflow = std::fs::read_to_string(&workflow).expect("read release assets workflow");

    assert!(workflow.contains("workflow_dispatch:"));
    assert!(workflow.contains("tags:"));
    assert!(workflow.contains("- \"v*\""));
    assert!(workflow.contains("ensure-release:"));
    assert!(workflow.contains("gh release create \"$TAG\""));
    assert!(workflow.contains("release-notes.md"));
    assert!(workflow.contains("Release already exists; preserving maintained notes"));
    assert!(
        workflow.contains(
            "node scripts/generate-release-notes.mjs \"$TAG\" \"$REPO\" > release-notes.md"
        )
    );
    assert!(!workflow.contains("CodexElves $VERSION 发布版本。"));
    assert!(!workflow.contains("gh release edit \"$TAG\" --repo \"$REPO\" --notes-file"));
    assert!(workflow.contains("ref: ${{ needs.ensure-release.outputs.tag }}"));
    assert!(workflow.contains("TAG: ${{ needs.ensure-release.outputs.tag }}"));
    assert!(workflow.contains("gh release upload $env:TAG @($files.FullName) --clobber"));
    assert!(workflow.contains("gh release upload \"$TAG\" dist/macos/*.dmg --clobber"));
    assert!(!workflow.contains("softprops/action-gh-release"));
}

#[test]
fn release_notes_generator_removes_release_metadata_from_feature_titles() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let generator = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join("scripts/generate-release-notes.mjs");
    let generator = std::fs::read_to_string(&generator).expect("read release notes generator");

    assert!(generator.contains("const fallbackTitle = sanitizeBullet"));
    assert!(generator.contains(".github/release-notes/${version}.md"));
    assert!(generator.contains("releaseNoteTopic(parsed.type, parsed.scope, commit.subject)"));
    assert!(generator.contains("lines.push(`- ${note.topic}: ${note.text}`)"));
    assert!(generator.contains("(?:\\s*并\\s*|[，,]\\s*)"));
    assert!(!generator.contains("lines.push(\"### 主要更新\")"));
}

#[test]
fn github_workflows_install_frontend_dependencies_from_lockfile() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(".github/workflows");

    for workflow_name in ["release-assets.yml", "pr-build.yml"] {
        let workflow = std::fs::read_to_string(workflow_root.join(workflow_name))
            .unwrap_or_else(|error| panic!("read {workflow_name}: {error}"));
        assert!(
            workflow.contains("run: npm ci"),
            "{workflow_name} should install frontend dependencies from package-lock.json"
        );
        assert!(
            !workflow.contains("npm install --package-lock=false"),
            "{workflow_name} should not ignore the committed package-lock.json"
        );
    }
}

#[test]
fn github_release_workflow_uploads_static_latest_json() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workflow = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .unwrap()
        .join(".github/workflows/release-assets.yml");
    let workflow = std::fs::read_to_string(&workflow).expect("read release assets workflow");

    assert!(workflow.contains("latest-json:"));
    assert!(workflow.contains("latest.json"));
    assert!(workflow.contains("- ensure-release"));
    assert!(workflow.contains("TAG: ${{ needs.ensure-release.outputs.tag }}"));
    assert!(workflow.contains("gh release upload \"$TAG\" latest.json --clobber"));
}

#[test]
fn relay_settings_uses_structured_config_and_isolated_auth() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");
    let commands_rs = manifest_dir.join("src/commands.rs");
    let commands_rs = std::fs::read_to_string(&commands_rs).expect("read manager commands.rs");

    assert!(app_tsx.contains("switch_relay_profile"));
    assert!(app_tsx.contains("previousActiveRelayId"));
    assert!(app_tsx.contains("relayProfileSwitchValidation(selectedBeforeSave, switchSettings)"));
    assert!(app_tsx.contains("RelayActivationPanel"));
    assert!(app_tsx.contains("启用后会修改"));
    assert!(app_tsx.contains("auth.json 存档"));
    assert!(app_tsx.contains("saveRelayAuthFile"));
    assert!(!app_tsx.contains("RelayFileEditors"));
    assert!(!app_tsx.contains("config.toml 预览"));
    assert!(!app_tsx.contains("提取当前供应商配置"));
    assert!(!app_tsx.contains("启用目标功能"));
    assert!(commands_rs.contains("供应商配置不再支持直接保存 config.toml"));
    assert!(commands_rs.contains("backfill_relay_profile_from_live"));
    assert!(commands_rs.contains("switch_relay_profile_in_home"));
}

#[test]
fn manager_user_script_controls_are_registered_and_visible() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");
    let commands_rs = manifest_dir.join("src/commands.rs");
    let commands_rs = std::fs::read_to_string(&commands_rs).expect("read manager commands.rs");
    let lib_rs =
        std::fs::read_to_string(manifest_dir.join("src/lib.rs")).expect("read manager lib.rs");

    assert!(app_tsx.contains("setUserScriptsEnabled"));
    assert!(app_tsx.contains("reloadUserScripts"));
    assert!(app_tsx.contains("关闭全部"));
    assert!(app_tsx.contains("立即重载"));
    assert!(app_tsx.contains("禁用或删除已执行脚本仍需重载 Codex 页面"));
    assert!(commands_rs.contains("pub fn set_user_scripts_enabled"));
    assert!(commands_rs.contains("pub async fn reload_user_scripts"));
    assert!(commands_rs.contains("reload_user_scripts_into_running_codex"));
    assert!(lib_rs.contains("commands::set_user_scripts_enabled"));
    assert!(lib_rs.contains("commands::reload_user_scripts"));
}

#[test]
fn relay_context_management_is_global_not_supplier_scoped() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");
    let styles = manifest_dir.parent().unwrap().join("src/styles.css");
    let styles = std::fs::read_to_string(&styles).expect("read manager styles.css");

    assert!(app_tsx.contains("作为全局配置独立管理"));
    assert!(app_tsx.contains("label: \"工具与插件\""));
    assert!(app_tsx.contains("<strong>Codex 工具与插件</strong>"));
    assert!(!app_tsx.contains("<CardHead title=\"Codex 工具与插件\""));
    assert!(app_tsx.contains("className=\"relay-context-content\""));
    assert!(!app_tsx.contains("className=\"relay-context-panel\""));
    assert!(!app_tsx.contains("label: \"上下文配置\""));
    assert!(!app_tsx.contains("title=\"上下文配置\""));
    assert!(!app_tsx.contains("<strong>Codex 上下文</strong>"));
    assert!(app_tsx.contains("id: \"context\""));
    assert!(app_tsx.contains("function ContextScreen"));
    assert!(app_tsx.contains("route === \"context\""));
    assert!(app_tsx.contains("if (next === \"context\")"));
    assert!(app_tsx.contains("contextConfigTextFromConfig(configContents, entries)"));
    assert!(app_tsx.contains("toggleContextEntryEnabled"));
    assert!(app_tsx.contains("relayFiles={relayFiles}"));
    assert!(app_tsx.contains("read_live_context_entries"));
    assert!(app_tsx.contains("sync_live_context_entries"));
    assert!(app_tsx.contains("refreshLiveContextEntries"));
    assert!(app_tsx.contains("syncLiveContextEntries(next, true, { kind"));
    assert!(app_tsx.contains("function contextEntriesWithLiveEntries"));
    assert!(app_tsx.contains("liveByKind"));
    assert!(app_tsx.contains("mergeLiveContextEntries"));
    assert!(app_tsx.contains("withLiveEntryState"));
    assert!(app_tsx.contains("contextEnabledSwitch"));
    assert!(!app_tsx.contains("entry.enabled ? \"已启用\" : \"已禁用\""));
    assert!(!app_tsx.contains("空配置体"));
    assert!(app_tsx.contains("relay-context-delete"));
    assert!(!app_tsx.contains("切换供应商时只合并勾选项"));
    assert!(!app_tsx.contains("未勾选的条目不会写入"));
    assert!(!app_tsx.contains("className=\"context-switch\""));
    assert!(!styles.contains(".context-switch {"));
    assert!(styles.contains(".context-enabled-switch"));
    assert!(styles.contains(".context-switch-track"));
    assert!(styles.contains(".context-switch-thumb"));
    assert!(!styles.contains(".relay-context-row code"));
    assert!(styles.contains(".relay-context-delete"));
}

#[test]
fn manager_window_and_relay_detail_header_stay_usable() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");
    let styles = manifest_dir.parent().unwrap().join("src/styles.css");
    let styles = std::fs::read_to_string(&styles).expect("read manager styles.css");
    let lib_rs =
        std::fs::read_to_string(manifest_dir.join("src/lib.rs")).expect("read manager lib.rs");
    let tauri_conf =
        std::fs::read_to_string(manifest_dir.join("tauri.conf.json")).expect("read tauri config");

    assert!(app_tsx.contains("relay-detail-sticky"));
    assert!(!app_tsx.contains("CardHead title=\"供应商详情\""));
    assert!(styles.contains(".relay-detail-sticky"));
    assert!(styles.contains("position: sticky"));
    assert!(styles.contains("top: 0"));
    assert!(styles.contains("margin: 0"));
    assert!(lib_rs.contains("DEFAULT_WINDOW_WIDTH"));
    assert!(lib_rs.contains("DEFAULT_WINDOW_HEIGHT"));
    assert!(lib_rs.contains("MIN_WINDOW_WIDTH"));
    assert!(lib_rs.contains("MIN_WINDOW_HEIGHT"));
    assert!(lib_rs.contains("MANAGER_WINDOW_STATE_FILE"));
    assert!(lib_rs.contains("visible(false)"));
    assert!(lib_rs.contains("apply_manager_window_state"));
    assert!(lib_rs.contains("manager_window_state_is_visible"));
    assert!(lib_rs.contains("persist_manager_window_state"));
    assert!(!lib_rs.contains("minimized_window.hide()"));
    assert!(tauri_conf.contains("\"width\": 1180"));
    assert!(tauri_conf.contains("\"height\": 820"));
    assert!(tauri_conf.contains("\"minWidth\": 960"));
    assert!(tauri_conf.contains("\"minHeight\": 750"));
}

#[test]
fn relay_preview_deduplicates_root_keys_when_merging_common_config() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_tsx = manifest_dir.parent().unwrap().join("src/App.tsx");
    let app_tsx = std::fs::read_to_string(&app_tsx).expect("read manager App.tsx");

    assert!(app_tsx.contains("dedupeTomlRootLines"));
    assert!(app_tsx.contains("rootSeen.add(key)"));
    assert!(app_tsx.contains("joinTomlSectionsRootFirst"));
}
