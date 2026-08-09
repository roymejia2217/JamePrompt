use global_hotkey::hotkey::HotKey;
use iced::keyboard::{key::Named, Key, Modifiers};

use crate::perf;
use crate::platform::HotkeyBackendKind;

mod native;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
mod remote_desktop;

/// Formats a keyboard key and modifiers into JamePrompt's portable hotkey notation.
pub fn format_hotkey(key: &Key, modifiers: Modifiers) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();

    if modifiers.control() {
        parts.push("Ctrl");
    }
    if modifiers.shift() {
        parts.push("Shift");
    }
    if modifiers.alt() {
        parts.push("Alt");
    }
    if modifiers.logo() {
        parts.push("Super");
    }
    if parts.is_empty() {
        return None;
    }

    let key_str = format_key(key)?;
    parts.push(&key_str);
    Some(parts.join("+"))
}

fn format_key(key: &Key) -> Option<String> {
    match key {
        Key::Character(c) => {
            let ch = c.chars().next()?;
            Some(ch.to_uppercase().collect::<String>())
        }
        Key::Named(named) => Some(format_named_key(named)),
        Key::Unidentified => None,
    }
}

fn format_named_key(named: &Named) -> String {
    match named {
        Named::F1 => "F1".to_string(),
        Named::F2 => "F2".to_string(),
        Named::F3 => "F3".to_string(),
        Named::F4 => "F4".to_string(),
        Named::F5 => "F5".to_string(),
        Named::F6 => "F6".to_string(),
        Named::F7 => "F7".to_string(),
        Named::F8 => "F8".to_string(),
        Named::F9 => "F9".to_string(),
        Named::F10 => "F10".to_string(),
        Named::F11 => "F11".to_string(),
        Named::F12 => "F12".to_string(),
        Named::F13 => "F13".to_string(),
        Named::F14 => "F14".to_string(),
        Named::F15 => "F15".to_string(),
        Named::F16 => "F16".to_string(),
        Named::F17 => "F17".to_string(),
        Named::F18 => "F18".to_string(),
        Named::F19 => "F19".to_string(),
        Named::F20 => "F20".to_string(),
        Named::F21 => "F21".to_string(),
        Named::F22 => "F22".to_string(),
        Named::F23 => "F23".to_string(),
        Named::F24 => "F24".to_string(),
        Named::F25 => "F25".to_string(),
        Named::ArrowUp => "ArrowUp".to_string(),
        Named::ArrowDown => "ArrowDown".to_string(),
        Named::ArrowLeft => "ArrowLeft".to_string(),
        Named::ArrowRight => "ArrowRight".to_string(),
        Named::Home => "Home".to_string(),
        Named::End => "End".to_string(),
        Named::PageUp => "PageUp".to_string(),
        Named::PageDown => "PageDown".to_string(),
        Named::Insert => "Insert".to_string(),
        Named::Delete => "Delete".to_string(),
        Named::Enter => "Enter".to_string(),
        Named::Space => "Space".to_string(),
        Named::Tab => "Tab".to_string(),
        Named::Escape => "Escape".to_string(),
        Named::Backspace => "Backspace".to_string(),
        Named::CapsLock => "CapsLock".to_string(),
        Named::NumLock => "NumLock".to_string(),
        Named::ScrollLock => "ScrollLock".to_string(),
        Named::PrintScreen => "PrintScreen".to_string(),
        Named::Pause => "Pause".to_string(),
        Named::ContextMenu => "ContextMenu".to_string(),
        _ => "Unknown".to_string(),
    }
}

pub fn validate_hotkey(hotkey_str: &str) -> bool {
    hotkey_str.parse::<HotKey>().is_ok()
}

enum HotkeyBackend {
    Native(native::NativeHotkeyService),
    #[cfg(target_os = "linux")]
    Portal(portal::PortalHotkeyService),
}

pub struct HotkeyService {
    backend: HotkeyBackend,
}

impl HotkeyService {
    pub fn new() -> Option<Self> {
        let backend = match crate::platform::hotkey_backend_kind() {
            HotkeyBackendKind::Native => HotkeyBackend::Native(native::NativeHotkeyService::new()?),
            #[cfg(target_os = "linux")]
            HotkeyBackendKind::Portal => HotkeyBackend::Portal(portal::PortalHotkeyService::new()?),
            #[cfg(not(target_os = "linux"))]
            HotkeyBackendKind::Portal => return None,
            HotkeyBackendKind::Unavailable => return None,
        };
        Some(Self { backend })
    }

    pub fn register(&self, prompt_id: &str, prompt_name: &str, key_str: &str) -> Option<u32> {
        perf::measure("hotkeys.register", || match &self.backend {
            HotkeyBackend::Native(service) => service.register(key_str),
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => service.register(prompt_id, prompt_name, key_str),
        })
    }

    pub fn unregister(&self, hotkey_id: u32) -> bool {
        perf::measure("hotkeys.unregister", || match &self.backend {
            HotkeyBackend::Native(service) => service.unregister(hotkey_id),
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => service.unregister(hotkey_id),
        })
    }

    pub fn poll_events() -> Vec<u32> {
        perf::measure("hotkeys.poll_events", || {
            let mut triggered = native::poll_events();
            #[cfg(target_os = "linux")]
            triggered.extend(portal::poll_events());
            triggered
        })
    }
}

/// Injects Ctrl+V into the previously active application using the platform
/// backend. Native platforms keep the existing rdev path. Wayland uses the
/// permissioned XDG RemoteDesktop portal and never falls back to X11 injection.
pub fn paste_to_active_window() {
    perf::measure("hotkeys.paste_to_active_window_spawn", || {
        #[cfg(target_os = "linux")]
        if crate::platform::display_server() == crate::platform::DisplayServer::Wayland {
            remote_desktop::paste_to_active_window();
            return;
        }
        native::paste_to_active_window();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hotkey_ctrl_shift_p() {
        let key = Key::Character("p".into());
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        modifiers |= Modifiers::SHIFT;
        assert_eq!(format_hotkey(&key, modifiers), Some("Ctrl+Shift+P".into()));
    }

    #[test]
    fn test_format_hotkey_no_modifiers_returns_none() {
        assert!(format_hotkey(&Key::Character("a".into()), Modifiers::empty()).is_none());
    }

    #[test]
    fn test_format_hotkey_function_key() {
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        assert_eq!(
            format_hotkey(&Key::Named(Named::F1), modifiers),
            Some("Ctrl+F1".into())
        );
    }

    #[test]
    fn test_format_hotkey_all_modifiers() {
        let hotkey = format_hotkey(&Key::Character("k".into()), Modifiers::all()).unwrap();
        for expected in ["Ctrl", "Shift", "Alt", "Super", "K"] {
            assert!(hotkey.contains(expected));
        }
    }

    #[test]
    fn test_format_hotkey_unidentified_returns_none() {
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        assert!(format_hotkey(&Key::Unidentified, modifiers).is_none());
    }

    #[test]
    fn test_validate_hotkey_valid() {
        assert!(validate_hotkey("Ctrl+Shift+P"));
        assert!(validate_hotkey("Ctrl+G"));
        assert!(validate_hotkey("Alt+F1"));
    }

    #[test]
    fn test_validate_hotkey_invalid() {
        assert!(!validate_hotkey("InvalidHotkey"));
        assert!(!validate_hotkey("++P"));
        assert!(!validate_hotkey(""));
    }

    #[test]
    fn test_unregister_nonexistent_returns_false_when_service_is_available() {
        if let Some(service) = HotkeyService::new() {
            assert!(!service.unregister(99999));
        }
    }
}
