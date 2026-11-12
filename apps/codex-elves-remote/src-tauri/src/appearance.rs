use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TitlebarTheme {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
    pub border: [u8; 3],
    pub dark: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ThemeSnapshot {
    pub version: u64,
    pub theme: TitlebarTheme,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_bridge_accepts_only_rgb_bytes_and_a_theme_mode() {
        let value = serde_json::json!({
            "background": [24, 24, 24], "foreground": [237, 237, 237],
            "border": [54, 54, 54], "dark": true
        });
        assert!(serde_json::from_value::<TitlebarTheme>(value.clone()).is_ok());
        for (key, invalid) in [
            ("background", serde_json::json!("url(file:///C:/private)")),
            ("foreground", serde_json::json!([256, 0, 0])),
            ("border", serde_json::json!([-1, 0, 0])),
            ("background", serde_json::json!([0, 0])),
            ("dark", serde_json::json!("dark")),
            ("serverUrl", serde_json::json!("https://other.example/")),
        ] {
            let mut payload = value.clone();
            payload[key] = invalid;
            assert!(
                serde_json::from_value::<TitlebarTheme>(payload).is_err(),
                "{key}"
            );
        }
    }
}
