#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    Gpt,
    Claude,
    Other,
}

impl ModelFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gpt => "gpt",
            Self::Claude => "claude",
            Self::Other => "other",
        }
    }
}

pub fn model_family(model: &str) -> ModelFamily {
    let normalized = normalized_model_slug(model);
    let slug = normalized.as_str();
    if slug == "gpt"
        || slug.starts_with("gpt-")
        || slug
            .strip_prefix('o')
            .and_then(|rest| rest.chars().next())
            .is_some_and(|ch| ch.is_ascii_digit())
    {
        return ModelFamily::Gpt;
    }
    if slug.starts_with("claude-") {
        return ModelFamily::Claude;
    }
    ModelFamily::Other
}

pub fn known_model_context_window(model: &str) -> Option<u64> {
    let normalized = normalized_model_slug(model);
    let slug = normalized.as_str();
    let exact = match slug {
        "codex-auto-review" => 272_000,
        "gpt-4.1" | "gpt-4.1-mini" | "gpt-4.1-nano" => 1_047_576,
        "gpt-4o" | "gpt-4o-mini" => 128_000,
        "gpt-5" | "gpt-5-mini" | "gpt-5-nano" | "gpt-5.2" | "gpt-5.3-codex" | "gpt-5.4-mini"
        | "gpt-5.5" => 272_000,
        "gpt-5.4" => 1_000_000,
        "gpt-5.6" => 372_000,
        "o3" | "o3-mini" | "o4" | "o4-mini" => 200_000,
        "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" | "claude-sonnet-4-6" => {
            1_000_000
        }
        "claude-opus-4-5" | "claude-opus-4-1" | "claude-opus-4" | "claude-sonnet-4-5"
        | "claude-sonnet-4" | "claude-3-7-sonnet" | "claude-3-5-sonnet" | "claude-3-opus"
        | "claude-3-haiku" => 200_000,
        "deepseek-v4-flash" | "deepseek-v4-pro" => 1_000_000,
        "deepseek-chat" | "deepseek-reasoner" | "deepseek-coder" | "deepseek-r1"
        | "deepseek-v3" => 128_000,
        "qwen3-coder-plus"
        | "qwen3-coder-plus-2025-09-23"
        | "qwen3-coder-plus-2025-07-22"
        | "qwen3-coder-flash"
        | "qwen3-coder-flash-2025-07-28"
        | "qwen3.7-max"
        | "qwen3.6-plus"
        | "qwen3.5-plus"
        | "qwen-plus"
        | "qwen-flash" => 1_000_000,
        "qwen3-max" | "qwen3-max-2026-01-23" | "qwen3.6-max-preview" => 262_144,
        _ => 0,
    };
    if exact > 0 {
        return Some(exact);
    }

    if slug.starts_with("gpt-4.1") {
        return Some(1_047_576);
    }
    if slug.starts_with("gpt-4o") {
        return Some(128_000);
    }
    if slug.starts_with("gpt-5.6") {
        return Some(372_000);
    }
    if slug.starts_with("gpt-5.5")
        || slug.starts_with("gpt-5.4-mini")
        || slug.starts_with("gpt-5.3-codex")
        || slug.starts_with("gpt-5.2")
        || slug.starts_with("gpt-5-mini")
        || slug.starts_with("gpt-5-nano")
        || slug == "gpt-5"
    {
        return Some(272_000);
    }
    if slug.starts_with("gpt-5.4") {
        return Some(1_000_000);
    }
    if slug
        .strip_prefix('o')
        .and_then(|rest| rest.chars().next())
        .is_some_and(|ch| matches!(ch, '3' | '4'))
    {
        return Some(200_000);
    }
    if slug.starts_with("claude-opus-4-6")
        || slug.starts_with("claude-opus-4-7")
        || slug.starts_with("claude-opus-4-8")
        || slug.starts_with("claude-sonnet-4-6")
    {
        return Some(1_000_000);
    }
    if slug.starts_with("claude-opus-4-5")
        || slug.starts_with("claude-opus-4-1")
        || slug == "claude-opus-4"
        || slug.starts_with("claude-sonnet-4-5")
        || slug == "claude-sonnet-4"
        || slug.starts_with("claude-3-7-sonnet")
        || slug.starts_with("claude-3-5-sonnet")
        || slug.starts_with("claude-3-opus")
        || slug.starts_with("claude-3-haiku")
    {
        return Some(200_000);
    }
    if slug.starts_with("deepseek-v4-flash") || slug.starts_with("deepseek-v4-pro") {
        return Some(1_000_000);
    }
    if slug.starts_with("deepseek-chat")
        || slug.starts_with("deepseek-reasoner")
        || slug.starts_with("deepseek-coder")
        || slug.starts_with("deepseek-r1")
        || slug.starts_with("deepseek-v3")
    {
        return Some(128_000);
    }
    if slug.starts_with("qwen3-coder-plus")
        || slug.starts_with("qwen3-coder-flash")
        || slug.starts_with("qwen3.7-max")
        || slug.starts_with("qwen3.6-plus")
        || slug.starts_with("qwen3.5-plus")
        || slug.starts_with("qwen-plus")
        || slug.starts_with("qwen-flash")
    {
        return Some(1_000_000);
    }
    if slug.starts_with("qwen3-max") || slug.starts_with("qwen3.6-max") {
        return Some(262_144);
    }
    None
}

fn normalized_model_slug(model: &str) -> String {
    let normalized = model.trim().to_ascii_lowercase();
    normalized
        .rsplit('/')
        .next()
        .filter(|slug| !slug.is_empty())
        .unwrap_or(normalized.as_str())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_model_families_from_last_path_segment() {
        assert_eq!(model_family("openai/gpt-5.6-sol"), ModelFamily::Gpt);
        assert_eq!(
            model_family("anthropic/claude-opus-4-8"),
            ModelFamily::Claude
        );
        assert_eq!(model_family("zai-org/glm-5.1"), ModelFamily::Other);
    }

    #[test]
    fn resolves_known_context_windows_for_family_models() {
        assert_eq!(known_model_context_window("gpt-5.4"), Some(1_000_000));
        assert_eq!(
            known_model_context_window("openai/gpt-5.6-custom"),
            Some(372_000)
        );
        assert_eq!(
            known_model_context_window("claude-opus-4-8"),
            Some(1_000_000)
        );
        assert_eq!(
            known_model_context_window("deepseek-v4-flash"),
            Some(1_000_000)
        );
        assert_eq!(known_model_context_window("glm-5.1"), None);
        assert_eq!(known_model_context_window("gpt-5.7"), None);
        assert_eq!(known_model_context_window("claude-opus-5"), None);
        assert_eq!(known_model_context_window("deepseek-v5"), None);
    }
}
