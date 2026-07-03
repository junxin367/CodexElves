use std::path::PathBuf;

pub fn default_codex_home_dir() -> PathBuf {
    saved_codex_home_dir()
        .or_else(codex_home_env_dir)
        .unwrap_or_else(default_user_codex_home_dir)
}

pub fn codex_home_dir_for_settings(settings: &crate::settings::BackendSettings) -> PathBuf {
    configured_codex_home_dir(settings)
        .or_else(codex_home_env_dir)
        .unwrap_or_else(default_user_codex_home_dir)
}

pub fn configured_codex_home_dir(settings: &crate::settings::BackendSettings) -> Option<PathBuf> {
    let path = crate::settings::normalize_codex_home_path(&settings.codex_home_path);
    if path.is_empty() {
        None
    } else {
        Some(expand_user_home_path(&path))
    }
}

fn saved_codex_home_dir() -> Option<PathBuf> {
    crate::settings::SettingsStore::default()
        .load()
        .ok()
        .and_then(|settings| configured_codex_home_dir(&settings))
}

fn codex_home_env_dir() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(codex_home_env_dir_is_valid)
}

fn codex_home_env_dir_is_valid(path: &PathBuf) -> bool {
    !path.as_os_str().is_empty() && !path.to_string_lossy().trim().is_empty() && path.is_dir()
}

fn default_user_codex_home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".codex"))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn expand_user_home_path(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
            return home;
        }
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::Mutex;

    static CODEX_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct CodexHomeEnvGuard {
        previous: Option<OsString>,
    }

    struct SettingsPathGuard {
        previous: Option<PathBuf>,
    }

    impl SettingsPathGuard {
        fn set(path: PathBuf) -> Self {
            let previous = crate::paths::set_settings_path_for_tests(Some(path));
            Self { previous }
        }
    }

    impl Drop for SettingsPathGuard {
        fn drop(&mut self) {
            crate::paths::set_settings_path_for_tests(self.previous.take());
        }
    }

    impl CodexHomeEnvGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("CODEX_HOME");
            unsafe {
                std::env::set_var("CODEX_HOME", path);
            }
            Self { previous }
        }

        fn set_raw(value: &str) -> Self {
            let previous = std::env::var_os("CODEX_HOME");
            unsafe {
                std::env::set_var("CODEX_HOME", value);
            }
            Self { previous }
        }
    }

    impl Drop for CodexHomeEnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var("CODEX_HOME", value),
                    None => std::env::remove_var("CODEX_HOME"),
                }
            }
        }
    }

    #[test]
    fn default_codex_home_dir_uses_existing_codex_home_env_dir() {
        let _lock = CODEX_HOME_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _settings_guard = SettingsPathGuard::set(temp.path().join("missing-settings.json"));
        let codex_home = temp.path().join("custom-codex-home");
        std::fs::create_dir_all(&codex_home).unwrap();
        let _guard = CodexHomeEnvGuard::set(&codex_home);

        assert_eq!(default_codex_home_dir(), codex_home);
        assert_eq!(crate::relay_config::default_codex_home_dir(), codex_home);
        assert_eq!(crate::codex_sqlite::default_codex_home_dir(), codex_home);
    }

    #[test]
    fn default_codex_home_dir_ignores_empty_or_missing_codex_home_env() {
        let _lock = CODEX_HOME_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let _settings_guard = SettingsPathGuard::set(temp.path().join("missing-settings.json"));
        let missing = temp.path().join("missing-codex-home");
        let expected = default_user_codex_home_dir();

        {
            let _guard = CodexHomeEnvGuard::set_raw("   ");
            assert_eq!(default_codex_home_dir(), expected);
            assert_eq!(crate::relay_config::default_codex_home_dir(), expected);
            assert_eq!(crate::codex_sqlite::default_codex_home_dir(), expected);
        }

        {
            let _guard = CodexHomeEnvGuard::set(&missing);
            assert_eq!(default_codex_home_dir(), expected);
            assert_eq!(crate::relay_config::default_codex_home_dir(), expected);
            assert_eq!(crate::codex_sqlite::default_codex_home_dir(), expected);
        }
    }

    #[test]
    fn default_codex_home_dir_uses_saved_codex_home_path_before_env() {
        let _lock = CODEX_HOME_ENV_LOCK.lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        let _settings_guard = SettingsPathGuard::set(settings_path.clone());
        let saved_home = temp.path().join("saved-codex-home");
        let env_home = temp.path().join("env-codex-home");
        std::fs::create_dir_all(&env_home).unwrap();
        let _guard = CodexHomeEnvGuard::set(&env_home);

        let settings = crate::settings::BackendSettings {
            codex_home_path: format!(" \"{}\" ", saved_home.to_string_lossy()),
            ..crate::settings::BackendSettings::default()
        };
        crate::settings::SettingsStore::new(settings_path)
            .save(&settings)
            .unwrap();

        assert_eq!(default_codex_home_dir(), saved_home);
        assert_eq!(crate::relay_config::default_codex_home_dir(), saved_home);
        assert_eq!(crate::codex_sqlite::default_codex_home_dir(), saved_home);
    }
}
