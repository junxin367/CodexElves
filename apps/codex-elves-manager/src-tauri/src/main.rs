#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--show-update") {
        unsafe {
            std::env::set_var("CODEX_ELVES_SHOW_UPDATE", "1");
        }
    }
    codex_elves_manager_lib::run();
}
