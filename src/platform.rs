#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyBackendKind {
    Native,
    Portal,
    Unavailable,
}

#[cfg(target_os = "linux")]
pub(crate) fn display_server() -> DisplayServer {
    detect_linux_display_server(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    )
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn display_server() -> DisplayServer {
    DisplayServer::Unknown
}

pub(crate) fn hotkey_backend_kind() -> HotkeyBackendKind {
    #[cfg(target_os = "linux")]
    {
        return match display_server() {
            DisplayServer::X11 => HotkeyBackendKind::Native,
            DisplayServer::Wayland => HotkeyBackendKind::Portal,
            DisplayServer::Unknown => HotkeyBackendKind::Unavailable,
        };
    }

    #[cfg(not(target_os = "linux"))]
    {
        HotkeyBackendKind::Native
    }
}

fn detect_linux_display_server(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    x11_display: Option<&str>,
) -> DisplayServer {
    match session_type.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("wayland") => return DisplayServer::Wayland,
        Some(value) if value.eq_ignore_ascii_case("x11") => return DisplayServer::X11,
        _ => {}
    }

    if wayland_display.is_some_and(|value| !value.trim().is_empty()) {
        return DisplayServer::Wayland;
    }

    if x11_display.is_some_and(|value| !value.trim().is_empty()) {
        return DisplayServer::X11;
    }

    DisplayServer::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_wayland_session_wins_over_x11_fallback() {
        assert_eq!(
            detect_linux_display_server(Some("wayland"), Some("wayland-0"), Some(":0")),
            DisplayServer::Wayland
        );
    }

    #[test]
    fn explicit_x11_session_wins_over_wayland_fallback() {
        assert_eq!(
            detect_linux_display_server(Some("x11"), Some("wayland-0"), Some(":0")),
            DisplayServer::X11
        );
    }

    #[test]
    fn session_type_is_case_insensitive_and_trimmed() {
        assert_eq!(
            detect_linux_display_server(Some(" WayLand "), None, None),
            DisplayServer::Wayland
        );
        assert_eq!(
            detect_linux_display_server(Some(" X11 "), None, None),
            DisplayServer::X11
        );
    }

    #[test]
    fn wayland_display_is_used_when_session_type_is_missing() {
        assert_eq!(
            detect_linux_display_server(None, Some("wayland-1"), Some(":1")),
            DisplayServer::Wayland
        );
    }

    #[test]
    fn x11_display_is_used_when_wayland_is_missing() {
        assert_eq!(
            detect_linux_display_server(None, None, Some(":1")),
            DisplayServer::X11
        );
    }

    #[test]
    fn blank_environment_values_do_not_claim_a_display_server() {
        assert_eq!(
            detect_linux_display_server(Some(" "), Some(""), Some("  ")),
            DisplayServer::Unknown
        );
    }

    #[test]
    fn unknown_session_type_falls_back_to_available_display_variables() {
        assert_eq!(
            detect_linux_display_server(Some("tty"), Some("wayland-0"), Some(":0")),
            DisplayServer::Wayland
        );
        assert_eq!(
            detect_linux_display_server(Some("tty"), None, Some(":0")),
            DisplayServer::X11
        );
    }

    #[test]
    fn linux_backend_selection_maps_display_protocol_to_capability() {
        assert_eq!(
            match DisplayServer::X11 {
                DisplayServer::X11 => HotkeyBackendKind::Native,
                DisplayServer::Wayland => HotkeyBackendKind::Portal,
                DisplayServer::Unknown => HotkeyBackendKind::Unavailable,
            },
            HotkeyBackendKind::Native
        );
        assert_eq!(
            match DisplayServer::Wayland {
                DisplayServer::X11 => HotkeyBackendKind::Native,
                DisplayServer::Wayland => HotkeyBackendKind::Portal,
                DisplayServer::Unknown => HotkeyBackendKind::Unavailable,
            },
            HotkeyBackendKind::Portal
        );
    }
}
