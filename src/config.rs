use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "JamePrompt";
pub const APP_ID: &str = "jame-prompt";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
pub const OLD_PROJECT_QUALIFIER: &str = "prompt-manager";
pub const NEW_PROJECT_QUALIFIER: &str = "jame-prompt";
pub const WINDOW_INITIAL_WIDTH: f32 = 1000.0;
pub const WINDOW_INITIAL_HEIGHT: f32 = 640.0;
pub const WINDOW_MIN_WIDTH: f32 = 720.0;
pub const WINDOW_MIN_HEIGHT: f32 = 520.0;

/// Returns the current UTC year using only std (no external dependencies).
pub fn get_current_year() -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before UNIX epoch");
    let total_seconds = duration.as_secs();

    let days = total_seconds / (24 * 60 * 60);
    let mut year = 1970i32;
    let mut remaining_days = days;

    loop {
        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if is_leap { 366 } else { 365 };
        if remaining_days < days_in_year as u64 {
            break;
        }
        remaining_days -= days_in_year as u64;
        year += 1;
    }
    year
}

pub fn get_data_dir() -> PathBuf {
    let desired_dir =
        if let Some(proj_dirs) = ProjectDirs::from("com", NEW_PROJECT_QUALIFIER, APP_NAME) {
            let new_dir = proj_dirs.data_local_dir().to_path_buf();

            // Automatic migration: if new dir doesn't exist but old dir does, copy data.
            if !new_dir.exists() {
                if let Some(old_proj_dirs) =
                    ProjectDirs::from("com", OLD_PROJECT_QUALIFIER, "PromptManager")
                {
                    let old_dir = old_proj_dirs.data_local_dir();
                    if old_dir.exists() {
                        migrate_data_dir(old_dir, &new_dir);
                    }
                }
            }

            new_dir
        } else {
            let mut dir = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
            dir.push("data");
            dir
        };

    resolve_writable_data_dir(&desired_dir)
}

fn migrate_data_dir(old_dir: &Path, new_dir: &Path) {
    if std::fs::create_dir_all(new_dir).is_err() {
        return;
    }

    for filename in ["prompts.db", "settings.json"] {
        let old_file = old_dir.join(filename);
        let new_file = new_dir.join(filename);
        if old_file.exists() && !new_file.exists() {
            let _ = std::fs::copy(&old_file, &new_file);
        }
    }
}

fn resolve_writable_data_dir(dir: &Path) -> PathBuf {
    if std::fs::create_dir_all(dir).is_ok() {
        return dir.to_path_buf();
    }

    let fallback = std::env::temp_dir().join(APP_ID);
    let _ = std::fs::create_dir_all(&fallback);
    fallback
}

pub fn get_db_path() -> PathBuf {
    get_data_dir().join("prompts.db")
}

pub fn get_settings_path() -> PathBuf {
    get_data_dir().join("settings.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hotkeys_enabled: bool,
    pub autostart_enabled: bool,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkeys_enabled: false,
            autostart_enabled: false,
            theme: "Dark".to_string(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_settings_default_values() {
        let s = Settings::default();
        assert!(
            !s.hotkeys_enabled,
            "Default hotkeys_enabled should be false"
        );
        assert!(
            !s.autostart_enabled,
            "Default autostart_enabled should be false"
        );
        assert_eq!(s.theme, "Dark", "Default theme should be Dark");
    }

    #[test]
    fn test_settings_serialization_roundtrip() {
        let s = Settings {
            hotkeys_enabled: true,
            autostart_enabled: true,
            theme: "Light".to_string(),
        };
        let json = serde_json::to_string(&s).expect("Serialization failed");
        let deserialized: Settings = serde_json::from_str(&json).expect("Deserialization failed");

        assert!(deserialized.hotkeys_enabled);
        assert!(deserialized.autostart_enabled);
        assert_eq!(deserialized.theme, "Light");
    }

    #[test]
    fn test_settings_deserialization_from_json() {
        let json = r#"{"hotkeys_enabled":true,"autostart_enabled":true,"theme":"Light"}"#;
        let s: Settings = serde_json::from_str(json).expect("Deserialization failed");

        assert!(s.hotkeys_enabled);
        assert!(s.autostart_enabled);
        assert_eq!(s.theme, "Light");
    }

    #[test]
    fn test_settings_deserialization_missing_field_uses_default() {
        let json = r#"{"hotkeys_enabled":true}"#;
        let s: Settings = serde_json::from_str(json).expect("Deserialization failed");

        assert!(s.hotkeys_enabled);
        assert!(
            !s.autostart_enabled,
            "Missing autostart_enabled should use the default value"
        );
        assert_eq!(s.theme, "Dark");
    }

    #[test]
    fn test_get_data_dir_returns_path() {
        let path = get_data_dir();
        assert!(
            !path.to_string_lossy().is_empty(),
            "Data dir should not be empty"
        );
    }

    #[test]
    fn test_get_db_path_ends_with_db() {
        let path = get_db_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.ends_with("prompts.db"),
            "DB path should end with prompts.db, got: {}",
            path_str
        );
    }

    #[test]
    fn test_get_settings_path_ends_with_json() {
        let path = get_settings_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.ends_with("settings.json"),
            "Settings path should end with settings.json, got: {}",
            path_str
        );
    }

    #[test]
    fn test_settings_save_and_load_roundtrip() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("test_settings.json");

        let original = Settings {
            hotkeys_enabled: true,
            autostart_enabled: true,
            theme: "Light".to_string(),
        };

        original.save(&path).expect("Save failed");
        let loaded = Settings::load(&path);

        assert_eq!(loaded.hotkeys_enabled, original.hotkeys_enabled);
        assert_eq!(loaded.autostart_enabled, original.autostart_enabled);
        assert_eq!(loaded.theme, original.theme);
    }

    #[test]
    fn test_settings_save_to_directory_fails() {
        let dir = tempdir().expect("Failed to create temp dir");
        let result = Settings::default().save(dir.path());

        assert!(
            result.is_err(),
            "Saving settings to a directory path should fail"
        );
    }

    #[test]
    fn test_settings_load_missing_file_returns_default() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("nonexistent.json");

        let loaded = Settings::load(&path);
        assert!(!loaded.hotkeys_enabled);
        assert!(!loaded.autostart_enabled);
        assert_eq!(loaded.theme, "Dark");
    }

    #[test]
    fn test_settings_load_corrupt_file_returns_default() {
        use std::fs;

        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("corrupt.json");
        fs::write(&path, b"{not valid json").expect("Failed to write corrupt settings");

        let loaded = Settings::load(&path);
        assert!(
            !loaded.hotkeys_enabled,
            "Corrupt settings should fall back to the default hotkeys state"
        );
        assert!(
            !loaded.autostart_enabled,
            "Corrupt settings should fall back to the default autostart state"
        );
        assert_eq!(loaded.theme, "Dark");
    }

    #[test]
    fn test_resolve_writable_data_dir_falls_back_when_target_is_blocked() {
        use std::fs;

        let dir = tempdir().expect("Failed to create temp dir");
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, b"not a directory").expect("Failed to block directory path");

        let resolved = resolve_writable_data_dir(&blocked);

        assert_ne!(
            resolved, blocked,
            "Blocked data directory should fall back to a different writable location"
        );
        assert!(
            resolved.starts_with(std::env::temp_dir()),
            "Fallback data directory should live under the system temp directory"
        );
        assert!(
            resolved.exists(),
            "Fallback data directory should be created if possible"
        );
    }

    #[test]
    fn test_app_description_matches_cargo_toml() {
        assert_eq!(
            APP_DESCRIPTION,
            "JamePrompt — Lightweight and minimal local prompt manager"
        );
    }

    #[test]
    fn test_app_name_constant() {
        assert_eq!(APP_NAME, "JamePrompt");
    }

    #[test]
    fn test_data_migration_copies_files_from_old_to_new_dir() {
        use std::fs;

        let temp_base = tempdir().expect("Failed to create temp base dir");
        let old_dir = temp_base.path().join("old");
        let new_dir = temp_base.path().join("new");

        // Simulate old data directory with files
        fs::create_dir_all(&old_dir).expect("Failed to create old dir");
        fs::write(old_dir.join("prompts.db"), b"fake-db-content").expect("Failed to write fake db");
        fs::write(old_dir.join("settings.json"), b"{}").expect("Failed to write fake settings");

        // Run migration
        migrate_data_dir(&old_dir, &new_dir);

        // Verify files were copied
        assert!(
            new_dir.join("prompts.db").exists(),
            "prompts.db should be migrated"
        );
        assert!(
            new_dir.join("settings.json").exists(),
            "settings.json should be migrated"
        );
        assert_eq!(
            fs::read_to_string(new_dir.join("prompts.db")).unwrap(),
            "fake-db-content",
            "Migrated db content should match"
        );
    }

    #[test]
    fn test_data_migration_skips_if_new_file_already_exists() {
        use std::fs;

        let temp_base = tempdir().expect("Failed to create temp base dir");
        let old_dir = temp_base.path().join("old");
        let new_dir = temp_base.path().join("new");

        fs::create_dir_all(&old_dir).expect("Failed to create old dir");
        fs::write(old_dir.join("prompts.db"), b"old-content").expect("Failed to write old db");
        fs::create_dir_all(&new_dir).expect("Failed to create new dir");
        fs::write(new_dir.join("prompts.db"), b"new-content").expect("Failed to write new db");

        migrate_data_dir(&old_dir, &new_dir);

        // New file should NOT be overwritten
        assert_eq!(
            fs::read_to_string(new_dir.join("prompts.db")).unwrap(),
            "new-content",
            "Existing new file should not be overwritten"
        );
    }

    #[test]
    fn test_data_migration_handles_missing_old_dir() {
        let temp_base = tempdir().expect("Failed to create temp base dir");
        let old_dir = temp_base.path().join("nonexistent");
        let new_dir = temp_base.path().join("new");

        // Should not panic even if old dir doesn't exist
        migrate_data_dir(&old_dir, &new_dir);

        // New dir may or may not exist (create_dir_all runs), but no files should exist
        assert!(
            !new_dir.join("prompts.db").exists(),
            "No file should be migrated from missing dir"
        );
    }

    #[test]
    fn test_get_current_year_returns_reasonable_value() {
        let year = get_current_year();
        assert!(
            (2024..=2100).contains(&year),
            "Current year should be in a reasonable range, got: {}",
            year
        );
    }
}
