use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static DIAGNOSTIC_LOG_PATH_TEST_LOCK: Mutex<()> = Mutex::new(());

pub struct DiagnosticLogCapture {
    path: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl DiagnosticLogCapture {
    pub fn new(path: PathBuf) -> Self {
        let lock = DIAGNOSTIC_LOG_PATH_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(Some(path.clone()));
        Self { path, _lock: lock }
    }

    pub fn read(&self) -> String {
        let mut last_error = None;
        for _ in 0..500 {
            match std::fs::read_to_string(&self.path) {
                Ok(contents) => return contents,
                Err(error) => {
                    last_error = Some(error);
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
        panic!(
            "diagnostic log should become readable: {}",
            last_error.expect("diagnostic log read should fail before retry exhaustion")
        );
    }
}

impl Drop for DiagnosticLogCapture {
    fn drop(&mut self) {
        codex_elves_core::diagnostic_log::set_diagnostic_log_path_for_tests(None);
    }
}
