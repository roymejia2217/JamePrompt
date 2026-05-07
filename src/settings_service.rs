use crate::config::Settings;
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTheme {
    Dark,
    Light,
}

#[derive(Debug)]
pub enum SettingsServiceError {
    UnknownTheme(String),
    Save(Box<dyn std::error::Error>),
    Autostart(String),
}

impl fmt::Display for SettingsServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTheme(theme) => write!(f, "unknown theme: {}", theme),
            Self::Save(error) => write!(f, "settings save failed: {}", error),
            Self::Autostart(error) => write!(f, "autostart sync failed: {}", error),
        }
    }
}

impl std::error::Error for SettingsServiceError {}

pub struct SettingsLoad {
    pub settings: Settings,
    pub warning: Option<String>,
}

pub struct SettingsService;

impl SettingsService {
    pub fn parse_theme(theme: &str) -> Result<AppTheme, SettingsServiceError> {
        match theme {
            "Dark" => Ok(AppTheme::Dark),
            "Light" => Ok(AppTheme::Light),
            other => Err(SettingsServiceError::UnknownTheme(other.to_string())),
        }
    }

    pub fn load(path: &Path) -> SettingsLoad {
        match std::fs::read_to_string(path) {
            Ok(data) => match serde_json::from_str::<Settings>(&data) {
                Ok(settings) => SettingsLoad {
                    settings,
                    warning: None,
                },
                Err(error) => SettingsLoad {
                    settings: Settings::default(),
                    warning: Some(format!("Settings file is invalid: {}", error)),
                },
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SettingsLoad {
                settings: Settings::default(),
                warning: None,
            },
            Err(error) => SettingsLoad {
                settings: Settings::default(),
                warning: Some(format!("Settings file could not be read: {}", error)),
            },
        }
    }

    pub fn save_and_apply<F>(
        settings: &Settings,
        path: &Path,
        sync_autostart: F,
    ) -> Result<(), SettingsServiceError>
    where
        F: FnOnce(bool) -> Result<(), String>,
    {
        settings.save(path).map_err(SettingsServiceError::Save)?;
        sync_autostart(settings.autostart_enabled).map_err(SettingsServiceError::Autostart)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn settings_service_rejects_unknown_theme() {
        let error = SettingsService::parse_theme("Solarized")
            .expect_err("Unknown theme should be rejected");

        assert!(matches!(error, SettingsServiceError::UnknownTheme(theme) if theme == "Solarized"));
    }

    #[test]
    fn settings_service_loads_default_with_warning_for_corrupt_json() {
        let dir = tempdir().expect("Temp dir should be created");
        let path = dir.path().join("settings.json");
        fs::write(&path, "{not valid json").expect("Corrupt settings should be written");

        let load = SettingsService::load(&path);

        assert!(load.warning.is_some());
        assert_eq!(load.settings.theme, Settings::default().theme);
    }

    #[test]
    fn settings_service_saves_before_autostart_sync() {
        let dir = tempdir().expect("Temp dir should be created");
        let path = dir.path().join("settings.json");
        let settings = Settings {
            autostart_enabled: true,
            ..Settings::default()
        };

        SettingsService::save_and_apply(&settings, &path, |enabled| {
            assert!(enabled);
            assert!(
                path.exists(),
                "Settings should be saved before autostart sync"
            );
            Ok(())
        })
        .expect("Save and autostart sync should succeed");
    }

    #[test]
    fn settings_service_reports_autostart_failure_without_losing_settings() {
        let dir = tempdir().expect("Temp dir should be created");
        let path = dir.path().join("settings.json");
        let settings = Settings {
            autostart_enabled: true,
            ..Settings::default()
        };

        let error = SettingsService::save_and_apply(&settings, &path, |_| {
            Err("autostart unavailable".to_string())
        })
        .expect_err("Autostart failure should be reported");

        assert!(
            matches!(error, SettingsServiceError::Autostart(message) if message == "autostart unavailable")
        );
        assert!(
            path.exists(),
            "Settings should remain saved after autostart failure"
        );
    }
}
