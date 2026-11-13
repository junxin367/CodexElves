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

pub struct AppBarAppearance {
    pub theme: TitlebarTheme,
    version: u64,
}

impl AppBarAppearance {
    pub fn new(dark: bool) -> Self {
        Self {
            version: 0,
            theme: if dark {
                TitlebarTheme {
                    background: [32, 32, 32],
                    foreground: [240, 240, 240],
                    border: [48, 48, 48],
                    dark: true,
                }
            } else {
                TitlebarTheme {
                    background: [243, 243, 243],
                    foreground: [23, 23, 23],
                    border: [222, 222, 222],
                    dark: false,
                }
            },
        }
    }

    pub fn apply(&mut self, snapshot: ThemeSnapshot) -> bool {
        // 同一页面的渐变色帧可能异步到达；原生顶栏只接替更晚的帧，不另起动画或倒退颜色。
        if snapshot.version <= self.version {
            return false;
        }
        self.version = snapshot.version;
        self.theme = snapshot.theme;
        true
    }
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

    #[test]
    fn native_app_bar_keeps_latest_color_frame_without_starting_another_transition() {
        let mut appearance = AppBarAppearance::new(false);
        let dark = AppBarAppearance::new(true).theme;
        assert!(appearance.apply(ThemeSnapshot {
            version: 3,
            theme: dark.clone(),
        }));
        assert!(!appearance.apply(ThemeSnapshot {
            version: 2,
            theme: AppBarAppearance::new(false).theme,
        }));
        assert_eq!(appearance.theme, dark);
        let middle = TitlebarTheme {
            background: [96, 120, 144],
            foreground: [220, 230, 240],
            border: [110, 130, 150],
            dark: false,
        };
        assert!(appearance.apply(ThemeSnapshot {
            version: 4,
            theme: middle.clone(),
        }));
        assert_eq!(appearance.theme, middle);
    }
}
