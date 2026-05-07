use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager};
use rdev::{simulate, EventType, Key as RdevKey};
use std::cell::RefCell;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use crate::perf;
use iced::keyboard::{key::Named, Key, Modifiers};

/// Formats a keyboard key and modifiers into a hotkey string suitable for
/// `global_hotkey::HotKey::from_str()`.
///
/// Returns `None` if no modifiers are pressed (to prevent single-key global hotkeys).
/// The output format uses `Ctrl`, `Shift`, `Alt`, `Super` modifier names
/// joined with `+`, e.g. `"Ctrl+Shift+P"`.
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

    // Require at least one modifier for a global hotkey
    if parts.is_empty() {
        return None;
    }

    let key_str = format_key(key)?;
    parts.push(&key_str);

    Some(parts.join("+"))
}

/// Formats an iced `Key` into its string representation for hotkey notation.
fn format_key(key: &Key) -> Option<String> {
    match key {
        Key::Character(c) => {
            let ch = c.chars().next()?;
            // Uppercase single characters for hotkey display
            Some(ch.to_uppercase().collect::<String>())
        }
        Key::Named(named) => Some(format_named_key(named)),
        Key::Unidentified => None,
    }
}

/// Maps an iced `Named` key to its string representation for hotkey notation.
fn format_named_key(named: &Named) -> String {
    match named {
        // Function keys
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
        // Navigation keys
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
        // Action keys
        Named::Enter => "Enter".to_string(),
        Named::Space => "Space".to_string(),
        Named::Tab => "Tab".to_string(),
        Named::Escape => "Escape".to_string(),
        Named::Backspace => "Backspace".to_string(),
        // Other common keys
        Named::CapsLock => "CapsLock".to_string(),
        Named::NumLock => "NumLock".to_string(),
        Named::ScrollLock => "ScrollLock".to_string(),
        Named::PrintScreen => "PrintScreen".to_string(),
        Named::Pause => "Pause".to_string(),
        Named::ContextMenu => "ContextMenu".to_string(),
        _ => "Unknown".to_string(),
    }
}

/// Validates whether a hotkey string can be parsed by `global_hotkey::HotKey::from_str()`.
pub fn validate_hotkey(hotkey_str: &str) -> bool {
    hotkey_str.parse::<HotKey>().is_ok()
}

pub struct HotkeyService {
    manager: GlobalHotKeyManager,
    registered: RefCell<HashMap<u32, HotKey>>,
}

impl HotkeyService {
    pub fn new() -> Option<Self> {
        GlobalHotKeyManager::new().ok().map(|manager| Self {
            manager,
            registered: RefCell::new(HashMap::new()),
        })
    }

    pub fn register(&self, key_str: &str) -> Option<u32> {
        perf::measure("hotkeys.register", || {
            if let Ok(hotkey) = key_str.parse::<HotKey>() {
                if self.manager.register(hotkey).is_ok() {
                    self.registered.borrow_mut().insert(hotkey.id(), hotkey);
                    return Some(hotkey.id());
                }
            }
            None
        })
    }

    pub fn unregister(&self, hotkey_id: u32) -> bool {
        perf::measure("hotkeys.unregister", || {
            if let Some(hotkey) = self.registered.borrow_mut().remove(&hotkey_id) {
                self.manager.unregister(hotkey).is_ok()
            } else {
                false
            }
        })
    }

    /// Polls the global hotkey event receiver for any pending events.
    /// Returns a list of triggered hotkey IDs (only Pressed events).
    pub fn poll_events() -> Vec<u32> {
        perf::measure("hotkeys.poll_events", || {
            let receiver = GlobalHotKeyEvent::receiver();
            let mut triggered = Vec::new();
            while let Ok(event) = receiver.try_recv() {
                if event.state == global_hotkey::HotKeyState::Pressed {
                    triggered.push(event.id());
                }
            }
            triggered
        })
    }
}

/// Simulates Ctrl+V to inject text into the active window.
pub fn paste_to_active_window() {
    perf::measure("hotkeys.paste_to_active_window_spawn", || {
        thread::spawn(|| {
            thread::sleep(Duration::from_millis(150));
            let _ = simulate(&EventType::KeyPress(RdevKey::ControlLeft));
            let _ = simulate(&EventType::KeyPress(RdevKey::KeyV));
            thread::sleep(Duration::from_millis(20));
            let _ = simulate(&EventType::KeyRelease(RdevKey::KeyV));
            let _ = simulate(&EventType::KeyRelease(RdevKey::ControlLeft));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- format_hotkey tests ---

    #[test]
    fn test_format_hotkey_ctrl_shift_p() {
        let key = Key::Character("p".into());
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        modifiers |= Modifiers::SHIFT;
        let result = format_hotkey(&key, modifiers);
        assert_eq!(result, Some("Ctrl+Shift+P".to_string()));
    }

    #[test]
    fn test_format_hotkey_no_modifiers_returns_none() {
        let key = Key::Character("a".into());
        let modifiers = Modifiers::empty();
        let result = format_hotkey(&key, modifiers);
        assert!(
            result.is_none(),
            "Single key without modifiers should return None"
        );
    }

    #[test]
    fn test_format_hotkey_single_modifier() {
        let key = Key::Character("g".into());
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        let result = format_hotkey(&key, modifiers);
        assert_eq!(result, Some("Ctrl+G".to_string()));
    }

    #[test]
    fn test_format_hotkey_function_key() {
        let key = Key::Named(Named::F1);
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        let result = format_hotkey(&key, modifiers);
        assert_eq!(result, Some("Ctrl+F1".to_string()));
    }

    #[test]
    fn test_format_hotkey_alt_modifier() {
        let key = Key::Character("x".into());
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::ALT;
        let result = format_hotkey(&key, modifiers);
        assert_eq!(result, Some("Alt+X".to_string()));
    }

    #[test]
    fn test_format_hotkey_super_modifier() {
        let key = Key::Character("l".into());
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::LOGO;
        let result = format_hotkey(&key, modifiers);
        assert_eq!(result, Some("Super+L".to_string()));
    }

    #[test]
    fn test_format_hotkey_all_modifiers() {
        let key = Key::Character("k".into());
        let modifiers = Modifiers::all();
        let result = format_hotkey(&key, modifiers);
        assert!(result.is_some());
        let hotkey_str = result.unwrap();
        assert!(hotkey_str.contains("Ctrl"));
        assert!(hotkey_str.contains("Shift"));
        assert!(hotkey_str.contains("Alt"));
        assert!(hotkey_str.contains("Super"));
        assert!(hotkey_str.contains("K"));
    }

    #[test]
    fn test_format_hotkey_named_enter() {
        let key = Key::Named(Named::Enter);
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        let result = format_hotkey(&key, modifiers);
        assert_eq!(result, Some("Ctrl+Enter".to_string()));
    }

    #[test]
    fn test_format_hotkey_named_space() {
        let key = Key::Named(Named::Space);
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        let result = format_hotkey(&key, modifiers);
        assert_eq!(result, Some("Ctrl+Space".to_string()));
    }

    #[test]
    fn test_format_hotkey_unidentified_returns_none() {
        let key = Key::Unidentified;
        let mut modifiers = Modifiers::empty();
        modifiers |= Modifiers::CTRL;
        let result = format_hotkey(&key, modifiers);
        assert!(result.is_none(), "Unidentified key should return None");
    }

    // --- validate_hotkey tests ---

    #[test]
    fn test_validate_hotkey_valid() {
        assert!(
            validate_hotkey("Ctrl+Shift+P"),
            "Ctrl+Shift+P should be valid"
        );
        assert!(validate_hotkey("Ctrl+G"), "Ctrl+G should be valid");
        assert!(validate_hotkey("Alt+F1"), "Alt+F1 should be valid");
    }

    #[test]
    fn test_validate_hotkey_invalid() {
        assert!(
            !validate_hotkey("InvalidHotkey"),
            "Random string should be invalid"
        );
        assert!(!validate_hotkey("++P"), "Double plus should be invalid");
    }

    #[test]
    fn test_validate_hotkey_empty() {
        assert!(!validate_hotkey(""), "Empty string should be invalid");
    }

    // --- HotkeyService tests ---

    #[test]
    fn test_unregister_removes_from_map() {
        if let Some(svc) = HotkeyService::new() {
            if let Some(hotkey_id) = svc.register("Ctrl+Shift+T") {
                let result = svc.unregister(hotkey_id);
                assert!(
                    result,
                    "Unregister should return true for registered hotkey"
                );
                assert!(
                    !svc.registered.borrow().contains_key(&hotkey_id),
                    "Hotkey should be removed from registered map after unregister"
                );
            }
            // If register fails (e.g., in CI without a display server), skip the assertion
        }
        // If HotkeyService::new() fails (e.g., in CI without a display server), skip the test
    }

    #[test]
    fn test_unregister_nonexistent_returns_false() {
        if let Some(svc) = HotkeyService::new() {
            let result = svc.unregister(99999);
            assert!(
                !result,
                "Unregistering a non-existent hotkey ID should return false"
            );
        }
    }

    #[test]
    fn test_unregister_twice_returns_false_second_time() {
        if let Some(svc) = HotkeyService::new() {
            if let Some(hotkey_id) = svc.register("Ctrl+Shift+U") {
                let first = svc.unregister(hotkey_id);
                assert!(first, "First unregister should return true");
                let second = svc.unregister(hotkey_id);
                assert!(!second, "Second unregister of same ID should return false");
            }
        }
    }
}
