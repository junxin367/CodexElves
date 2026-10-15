#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--remote-access-service") {
        let result = tokio::runtime::Runtime::new()
            .map_err(anyhow::Error::from)
            .and_then(|runtime| runtime.block_on(codex_elves_core::remote_access::run_service()));
        if let Err(error) = result {
            let _ = codex_elves_core::diagnostic_log::append_diagnostic_log(
                "remote_access.service_failed",
                serde_json::json!({"message":format!("{error:#}")}),
            );
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--show-update") {
        unsafe {
            std::env::set_var("CODEX_ELVES_SHOW_UPDATE", "1");
        }
    }
    codex_elves_manager_lib::run();
}
