use serde::{Deserialize, Serialize};

pub type PromptId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub id: PromptId,
    pub name: String,
    pub content: String,
    pub hotkey: Option<String>,
    pub hotkey_enabled: bool,
    pub favorite: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
    pub use_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptFilter {
    All,
    Favorites,
}

impl std::fmt::Display for PromptFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => write!(f, "All"),
            Self::Favorites => write!(f, "Favorites"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptSort {
    NameAsc,
    RecentlyUsed,
    RecentlyUpdated,
    MostUsed,
}

impl std::fmt::Display for PromptSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NameAsc => write!(f, "Name"),
            Self::RecentlyUsed => write!(f, "Recently used"),
            Self::RecentlyUpdated => write!(f, "Recently updated"),
            Self::MostUsed => write!(f, "Most used"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptQuery {
    pub search: String,
    pub filter: PromptFilter,
    pub sort: PromptSort,
}

impl Default for PromptQuery {
    fn default() -> Self {
        Self {
            search: String::new(),
            filter: PromptFilter::All,
            sort: PromptSort::NameAsc,
        }
    }
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            content: String::new(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        }
    }
}

impl Prompt {
    pub fn preview(&self, max_chars: usize) -> String {
        let mut s = self.content.trim().to_string();
        if s.len() > max_chars {
            s.truncate(max_chars);
            s.push('\u{2026}');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_prompt(content: &str) -> Prompt {
        Prompt {
            id: String::from("11111111-1111-4111-8111-111111111111"),
            name: "Test".to_string(),
            content: content.to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        }
    }

    #[test]
    fn test_preview_short_content_unchanged() {
        let p = make_prompt("Hello");
        assert_eq!(p.preview(10), "Hello");
    }

    #[test]
    fn test_preview_exact_length_unchanged() {
        let p = make_prompt("Hello");
        assert_eq!(p.preview(5), "Hello");
    }

    #[test]
    fn test_preview_truncates_long_content() {
        let p = make_prompt("Hello, World! This is a long prompt.");
        let result = p.preview(5);
        assert_eq!(result, "Hello\u{2026}");
        assert!(
            result.chars().count() == 6,
            "Truncated result should be max_chars + 1 ellipsis char"
        );
    }

    #[test]
    fn test_preview_trims_whitespace() {
        let p = make_prompt("  Hello  ");
        assert_eq!(p.preview(10), "Hello");
    }

    #[test]
    fn test_preview_empty_content() {
        let p = make_prompt("");
        assert_eq!(p.preview(10), "");
    }

    #[test]
    fn test_preview_whitespace_only() {
        let p = make_prompt("   ");
        assert_eq!(p.preview(10), "");
    }

    #[test]
    fn test_preview_single_char() {
        let p = make_prompt("A");
        assert_eq!(p.preview(1), "A");
    }

    #[test]
    fn test_preview_truncate_to_zero() {
        let p = make_prompt("Hello");
        let result = p.preview(0);
        assert_eq!(
            result, "\u{2026}",
            "Truncating to 0 should produce just ellipsis"
        );
    }

    #[test]
    fn test_preview_unicode_content() {
        let p = make_prompt("\u{3053}\u{3093}\u{306b}\u{3061}\u{306f}\u{4e16}\u{754c}");
        let result = p.preview(3);
        assert!(
            result.ends_with('\u{2026}'),
            "Should truncate and add ellipsis"
        );
    }

    #[test]
    fn test_prompt_default() {
        let p = Prompt::default();
        assert!(p.id.is_empty());
        assert_eq!(p.name, "");
        assert_eq!(p.content, "");
        assert!(p.hotkey.is_none());
        assert!(p.hotkey_enabled, "Default hotkey_enabled should be true");
        assert!(!p.favorite, "Default favorite should be false");
        assert_eq!(p.use_count, 0, "Default use_count should be zero");
        assert!(
            p.last_used_at.is_none(),
            "Default last_used_at should be none"
        );
    }

    #[test]
    fn test_prompt_default_hotkey_enabled_is_true() {
        let p = Prompt::default();
        assert!(p.hotkey_enabled, "Default hotkey_enabled should be true");
    }

    #[test]
    fn test_prompt_preview_handles_multiline_content() {
        let p = make_prompt("Line one\nLine two\nLine three");

        assert_eq!(p.preview(30), "Line one\nLine two\nLine three");
    }

    #[test]
    fn test_prompt_query_default_is_all_sorted_by_name() {
        let query = PromptQuery::default();

        assert_eq!(query.search, "");
        assert_eq!(query.filter, PromptFilter::All);
        assert_eq!(query.sort, PromptSort::NameAsc);
    }
}
