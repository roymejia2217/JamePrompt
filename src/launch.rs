use std::ffi::OsStr;

pub(crate) const START_MINIMIZED_ARG: &str = "--start-minimized";
pub(crate) const PERF_SMOKE_ARG: &str = "--perf-smoke";
pub(crate) const UI_SMOKE_ARG: &str = "--ui-smoke";
pub(crate) const NATIVE_HOTKEY_SMOKE_ARG: &str = "--native-hotkey-smoke";
pub(crate) const DESKTOP_TEST_CONTROLLER_ARG: &str = "--desktop-test-controller";
pub(crate) const DESKTOP_TEST_PROFILE_ARG: &str = "--desktop-test-profile";

pub(crate) fn should_start_minimized() -> bool {
    should_start_minimized_from_args(std::env::args_os())
}

pub(crate) fn should_start_minimized_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(START_MINIMIZED_ARG))
}

pub(crate) fn should_run_perf_smoke_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(PERF_SMOKE_ARG))
}

pub(crate) fn should_run_ui_smoke_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(UI_SMOKE_ARG))
}

pub(crate) fn should_run_native_hotkey_smoke_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(NATIVE_HOTKEY_SMOKE_ARG))
}

/// Strict test-mode selection: both switches are required together, the
/// socket path must be absolute, and the profile must be an absolute JSON
/// path. Production binaries (feature off) reject both switches before
/// any application state is constructed. Returns the parsed selection or
/// a precise CLI error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesktopTestMode {
    pub controller_socket: std::path::PathBuf,
    pub profile_path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DesktopTestModeError {
    /// Test switches present but the binary was built without the
    /// `desktop-integration-tests` feature.
    FeatureDisabled,
    ControllerWithoutProfile,
    ProfileWithoutController,
    RelativeSocketPath,
    RelativeProfilePath,
    OversizeArgument,
}

pub(crate) const MAX_TEST_ARG_LEN: usize = 4096;

pub(crate) fn desktop_test_mode_from_args<I, S>(
    args: I,
    feature_enabled: bool,
) -> Result<Option<DesktopTestMode>, DesktopTestModeError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect();
    let mut controller: Option<String> = None;
    let mut profile: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        for (flag, slot) in [
            (DESKTOP_TEST_CONTROLLER_ARG, &mut controller),
            (DESKTOP_TEST_PROFILE_ARG, &mut profile),
        ] {
            if arg == flag {
                let value = args.get(index + 1).cloned().unwrap_or_default();
                if value.len() > MAX_TEST_ARG_LEN {
                    return Err(DesktopTestModeError::OversizeArgument);
                }
                *slot = Some(value);
            } else if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
                if value.len() > MAX_TEST_ARG_LEN {
                    return Err(DesktopTestModeError::OversizeArgument);
                }
                *slot = Some(value.to_string());
            }
        }
        index += 1;
    }
    match (controller, profile) {
        (None, None) => Ok(None),
        (Some(_), None) => Err(DesktopTestModeError::ControllerWithoutProfile),
        (None, Some(_)) => Err(DesktopTestModeError::ProfileWithoutController),
        (Some(socket), Some(profile)) => {
            if !feature_enabled {
                return Err(DesktopTestModeError::FeatureDisabled);
            }
            let socket = std::path::PathBuf::from(socket);
            let profile = std::path::PathBuf::from(profile);
            if !socket.is_absolute() {
                return Err(DesktopTestModeError::RelativeSocketPath);
            }
            if !profile.is_absolute() {
                return Err(DesktopTestModeError::RelativeProfilePath);
            }
            Ok(Some(DesktopTestMode {
                controller_socket: socket,
                profile_path: profile,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn desktop_test_mode_absent_without_switches() {
        let args = [OsString::from("jame-prompt")];
        assert_eq!(desktop_test_mode_from_args(args, true), Ok(None));
    }

    #[test]
    fn desktop_test_mode_requires_both_switches() {
        let args = [
            OsString::from("jame-prompt"),
            OsString::from("--desktop-test-controller"),
            OsString::from("/tmp/run/controller.sock"),
        ];
        assert_eq!(
            desktop_test_mode_from_args(args, true),
            Err(DesktopTestModeError::ControllerWithoutProfile)
        );
        let args = [
            OsString::from("jame-prompt"),
            OsString::from("--desktop-test-profile"),
            OsString::from("/tmp/run/profile.json"),
        ];
        assert_eq!(
            desktop_test_mode_from_args(args, true),
            Err(DesktopTestModeError::ProfileWithoutController)
        );
    }

    #[test]
    fn desktop_test_mode_rejects_relative_paths() {
        let args = [
            OsString::from("jame-prompt"),
            OsString::from("--desktop-test-controller=relative.sock"),
            OsString::from("--desktop-test-profile=/tmp/run/profile.json"),
        ];
        assert_eq!(
            desktop_test_mode_from_args(args, true),
            Err(DesktopTestModeError::RelativeSocketPath)
        );
    }

    #[test]
    fn desktop_test_mode_rejects_feature_off_before_any_state() {
        let args = [
            OsString::from("jame-prompt"),
            OsString::from("--desktop-test-controller=/tmp/run/controller.sock"),
            OsString::from("--desktop-test-profile=/tmp/run/profile.json"),
        ];
        assert_eq!(
            desktop_test_mode_from_args(args, false),
            Err(DesktopTestModeError::FeatureDisabled)
        );
    }

    #[test]
    fn desktop_test_mode_accepts_absolute_pair() {
        let root = std::env::temp_dir().join("jameprompt-launch-test");
        let controller_socket = root.join("controller.sock");
        let profile_path = root.join("profile.json");
        let args = [
            OsString::from("jame-prompt"),
            OsString::from("--desktop-test-controller"),
            controller_socket.clone().into_os_string(),
            OsString::from("--desktop-test-profile"),
            profile_path.clone().into_os_string(),
        ];
        assert_eq!(
            desktop_test_mode_from_args(args, true),
            Ok(Some(DesktopTestMode {
                controller_socket,
                profile_path,
            }))
        );
    }

    #[test]
    fn desktop_test_mode_rejects_oversize_arguments() {
        let huge = "x".repeat(MAX_TEST_ARG_LEN + 1);
        let args = [
            OsString::from("jame-prompt"),
            OsString::from(format!("--desktop-test-controller={huge}")),
            OsString::from("--desktop-test-profile=/tmp/run/profile.json"),
        ];
        assert_eq!(
            desktop_test_mode_from_args(args, true),
            Err(DesktopTestModeError::OversizeArgument)
        );
    }
}

#[cfg(test)]
pub(crate) fn initial_window_visible(start_minimized: bool) -> bool {
    !start_minimized
}

pub(crate) fn should_show_window_after_hidden_start(
    start_minimized: bool,
    tray_available: bool,
) -> bool {
    start_minimized && !tray_available
}
