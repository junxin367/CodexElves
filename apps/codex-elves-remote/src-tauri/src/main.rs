#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    if let Err(error) = codex_elves_remote::desktop::run() {
        codex_elves_remote::desktop::show_startup_error(&error.to_string());
        std::process::exit(1);
    }

    #[cfg(not(windows))]
    eprintln!("CodexElves Remote currently supports Windows.");
}
