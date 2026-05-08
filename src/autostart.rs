use crate::config::{APP_DESCRIPTION, APP_ID, APP_NAME};
use crate::launch::START_MINIMIZED_ARG;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum AutostartError {
    BaseDirsUnavailable,
    ExecutableUnavailable(std::io::Error),
    CreateAutostartDir(std::io::Error),
    WriteDesktopEntry(std::io::Error),
    RemoveDesktopEntry(std::io::Error),
    #[cfg(target_os = "windows")]
    RegistryCreate(String),
    #[cfg(target_os = "windows")]
    RegistryWrite(String),
    #[cfg(target_os = "windows")]
    RegistryRemove(String),
}

impl fmt::Display for AutostartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BaseDirsUnavailable => {
                write!(f, "unable to resolve the user configuration directory")
            }
            Self::ExecutableUnavailable(error) => {
                write!(f, "unable to resolve the running executable: {}", error)
            }
            Self::CreateAutostartDir(error) => {
                write!(f, "unable to create the autostart directory: {}", error)
            }
            Self::WriteDesktopEntry(error) => {
                write!(f, "unable to write the autostart desktop entry: {}", error)
            }
            Self::RemoveDesktopEntry(error) => {
                write!(f, "unable to remove the autostart desktop entry: {}", error)
            }
            #[cfg(target_os = "windows")]
            Self::RegistryCreate(error) => {
                write!(
                    f,
                    "unable to open or create the Windows run registry key: {}",
                    error
                )
            }
            #[cfg(target_os = "windows")]
            Self::RegistryWrite(error) => {
                write!(
                    f,
                    "unable to write the Windows run registry value: {}",
                    error
                )
            }
            #[cfg(target_os = "windows")]
            Self::RegistryRemove(error) => {
                write!(
                    f,
                    "unable to remove the Windows run registry value: {}",
                    error
                )
            }
        }
    }
}

impl std::error::Error for AutostartError {}

#[cfg(all(target_os = "linux", not(test)))]
pub fn sync(enabled: bool) -> Result<(), AutostartError> {
    use directories::BaseDirs;

    let base_dirs = BaseDirs::new().ok_or(AutostartError::BaseDirsUnavailable)?;
    let executable = if enabled {
        Some(std::env::current_exe().map_err(AutostartError::ExecutableUnavailable)?)
    } else {
        None
    };

    sync_at(base_dirs.config_dir(), enabled, executable.as_deref())
}

#[cfg(all(not(target_os = "linux"), not(test)))]
pub fn sync(_enabled: bool) -> Result<(), AutostartError> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn sync_at(
    config_dir: &Path,
    enabled: bool,
    executable: Option<&Path>,
) -> Result<(), AutostartError> {
    let desktop_entry_path = desktop_entry_path(config_dir);

    if enabled {
        let executable = executable.ok_or_else(|| {
            AutostartError::ExecutableUnavailable(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "running executable unavailable",
            ))
        })?;
        let autostart_dir = desktop_entry_path
            .parent()
            .ok_or(AutostartError::BaseDirsUnavailable)?;
        std::fs::create_dir_all(autostart_dir).map_err(AutostartError::CreateAutostartDir)?;
        std::fs::write(&desktop_entry_path, desktop_entry_contents(executable))
            .map_err(AutostartError::WriteDesktopEntry)?;
    } else if desktop_entry_path.exists() {
        std::fs::remove_file(&desktop_entry_path).map_err(AutostartError::RemoveDesktopEntry)?;
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn sync_at(
    _config_dir: &Path,
    _enabled: bool,
    _executable: Option<&Path>,
) -> Result<(), AutostartError> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn sync_windows(enabled: bool) -> Result<(), AutostartError> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    let executable = if enabled {
        Some(std::env::current_exe().map_err(AutostartError::ExecutableUnavailable)?)
    } else {
        None
    };

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let run_key_path = r"Software\Microsoft\Windows\CurrentVersion\Run";

    if enabled {
        let (run_key, _) = current_user
            .create_subkey(run_key_path)
            .map_err(|error| AutostartError::RegistryCreate(error.to_string()))?;
        let command = windows_run_command(
            executable
                .as_deref()
                .expect("executable is available when autostart is enabled"),
        );
        run_key
            .set_value(APP_ID, &command)
            .map_err(|error| AutostartError::RegistryWrite(error.to_string()))?;
    } else if let Ok(run_key) = current_user.open_subkey_with_flags(run_key_path, KEY_SET_VALUE) {
        if let Err(error) = run_key.delete_value(APP_ID) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(AutostartError::RegistryRemove(error.to_string()));
            }
        }
    }

    Ok(())
}
pub(crate) fn desktop_entry_path(config_dir: &Path) -> PathBuf {
    config_dir
        .join("autostart")
        .join(format!("{APP_ID}.desktop"))
}

pub(crate) fn desktop_entry_contents(executable: &Path) -> String {
    format!(
        concat!(
            "[Desktop Entry]\n",
            "Type=Application\n",
            "Name={name}\n",
            "Comment={description}\n",
            "Exec={exec} {start_minimized_arg}\n",
            "Icon={icon}\n",
            "Terminal=false\n",
            "Categories=Utility;\n",
            "StartupNotify=true\n"
        ),
        name = APP_NAME,
        description = APP_DESCRIPTION,
        exec = quote_desktop_argument(executable),
        start_minimized_arg = START_MINIMIZED_ARG,
        icon = APP_ID,
    )
}

fn quote_desktop_argument(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let escaped = raw.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_run_command(executable: &Path) -> String {
    format!(
        "{} {}",
        quote_windows_argument(executable),
        START_MINIMIZED_ARG
    )
}

#[cfg(any(target_os = "windows", test))]
fn quote_windows_argument(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_desktop_entry_path_uses_app_id() {
        let base = Path::new("/tmp/example");
        let path = desktop_entry_path(base);
        assert!(
            path.ends_with(Path::new(&format!("autostart/{APP_ID}.desktop"))),
            "Desktop entry path should use the app id"
        );
    }

    #[test]
    fn test_desktop_entry_contents_includes_app_metadata() {
        let contents = desktop_entry_contents(Path::new("/opt/jame-prompt/jame-prompt"));
        assert!(contents.contains(&format!("Name={APP_NAME}")));
        assert!(contents.contains(&format!("Comment={APP_DESCRIPTION}")));
        assert!(contents.contains("Exec=\"/opt/jame-prompt/jame-prompt\""));
        assert!(contents.contains(&format!("Icon={APP_ID}")));
    }

    #[test]
    fn test_desktop_entry_contents_starts_minimized_from_autostart() {
        let contents = desktop_entry_contents(Path::new("/opt/jame-prompt/jame-prompt"));

        assert!(contents.contains("Exec=\"/opt/jame-prompt/jame-prompt\" --start-minimized"));
    }

    #[test]
    fn test_desktop_entry_contents_quotes_path_without_quoting_start_argument() {
        let contents = desktop_entry_contents(Path::new("/opt/Jame Prompt/jame-prompt"));

        assert!(contents.contains("Exec=\"/opt/Jame Prompt/jame-prompt\" --start-minimized"));
    }

    #[test]
    fn test_windows_run_command_appends_start_minimized_argument() {
        let command =
            windows_run_command(Path::new(r"C:\Program Files\JamePrompt\jame-prompt.exe"));

        assert_eq!(
            command,
            format!(
                "\"C:\\Program Files\\JamePrompt\\jame-prompt.exe\" {}",
                START_MINIMIZED_ARG
            )
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_sync_at_writes_desktop_entry() {
        let temp = tempdir().expect("Failed to create temp dir");
        let executable = temp.path().join("bin/jame-prompt");

        sync_at(temp.path(), true, Some(&executable)).expect("Autostart sync should succeed");

        let desktop_entry = desktop_entry_path(temp.path());
        let contents =
            std::fs::read_to_string(&desktop_entry).expect("Desktop entry should be written");
        assert!(contents.contains("Type=Application"));
        assert!(contents.contains(&format!("Exec=\"{}\"", executable.display())));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_sync_at_removes_desktop_entry() {
        let temp = tempdir().expect("Failed to create temp dir");
        let executable = temp.path().join("bin/jame-prompt");

        sync_at(temp.path(), true, Some(&executable)).expect("Autostart sync should succeed");
        let desktop_entry = desktop_entry_path(temp.path());
        assert!(desktop_entry.exists());

        sync_at(temp.path(), false, None).expect("Autostart removal should succeed");
        assert!(
            !desktop_entry.exists(),
            "Desktop entry should be removed when autostart is disabled"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_sync_at_fails_when_autostart_dir_is_blocked() {
        use std::fs;

        let temp = tempdir().expect("Failed to create temp dir");
        let blocked_autostart_dir = temp.path().join("autostart");
        fs::write(&blocked_autostart_dir, b"blocked").expect("Failed to block autostart dir");

        let executable = temp.path().join("bin/jame-prompt");
        let result = sync_at(temp.path(), true, Some(&executable));

        assert!(
            result.is_err(),
            "Blocking the autostart directory should surface an error"
        );
    }
}
