use global_hotkey::hotkey::HotKey;
use iced::keyboard::{key::Named, Key, Modifiers};

use crate::desktop::{DesktopRuntime, RuntimeMode};
use crate::perf;
use crate::platform::HotkeyBackendKind;

#[cfg(target_os = "linux")]
mod clipboard_transfer;
#[cfg(target_os = "linux")]
mod connection;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteRequest {
    Started,
    ClipboardRequired(String),
    Busy,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteOutcome {
    Completed,
    Failed,
}

enum HotkeyBackend {
    Native(native::NativeHotkeyService),
    #[cfg(target_os = "linux")]
    Portal(portal::PortalHotkeyService),
}

pub struct HotkeyService {
    backend: HotkeyBackend,
    instance_id: u64,
    runtime: std::sync::Arc<DesktopRuntime>,
}

/// Slot registry for backend event streams. Iced may rebuild subscriptions
/// after window lifecycle changes, so the receiver must outlive each stream
/// instance instead of being consumed by the first builder.
struct EventSlot {
    receiver: std::sync::Arc<
        std::sync::Mutex<futures_channel::mpsc::UnboundedReceiver<crate::desktop::HotkeyEvent>>,
    >,
}

static EVENT_SLOTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<u64, EventSlot>>,
> = std::sync::OnceLock::new();

fn event_slots() -> &'static std::sync::Mutex<std::collections::HashMap<u64, EventSlot>> {
    EVENT_SLOTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

impl HotkeyService {
    pub fn new(runtime: std::sync::Arc<DesktopRuntime>) -> Option<Self> {
        if runtime.mode() == RuntimeMode::Synthetic && !runtime.native_consent_authorized() {
            return None;
        }
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let (event_tx, event_rx) = futures_channel::mpsc::unbounded();
        let backend = match crate::platform::hotkey_backend_kind() {
            HotkeyBackendKind::Native => {
                HotkeyBackend::Native(native::NativeHotkeyService::new(event_tx)?)
            }
            #[cfg(target_os = "linux")]
            HotkeyBackendKind::Portal => HotkeyBackend::Portal(
                portal::PortalHotkeyService::new_with_event(event_tx, runtime.clone())?,
            ),
            #[cfg(not(target_os = "linux"))]
            HotkeyBackendKind::Portal => return None,
            HotkeyBackendKind::Unavailable => return None,
        };
        let instance_id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Both backends expose one owned stream through the same slot. The
        // portal worker publishes asynchronously, but its receiver belongs to
        // the facade from construction time and must never be discarded.
        event_slots().lock().ok()?.insert(
            instance_id,
            EventSlot {
                receiver: std::sync::Arc::new(std::sync::Mutex::new(event_rx)),
            },
        );
        Some(Self {
            backend,
            instance_id,
            runtime,
        })
    }

    /// Queues one versioned desired snapshot. A queued command is never
    /// Active: readiness arrives through the subscription as
    /// [`crate::desktop::HotkeyEvent::Acknowledged`].
    pub fn reconcile(
        &self,
        snapshot: &crate::desktop::ShortcutSnapshot,
    ) -> Result<crate::desktop::ReconcileReceipt, crate::desktop::DesktopErrorCode> {
        if self.runtime.mode() == RuntimeMode::Synthetic
            && !self.runtime.native_consent_authorized()
        {
            return Ok(crate::desktop::ReconcileReceipt {
                rejected_prompt_ids: snapshot
                    .shortcuts
                    .iter()
                    .map(|spec| spec.prompt_id.clone())
                    .collect(),
            });
        }
        perf::measure("hotkeys.reconcile", || match &self.backend {
            HotkeyBackend::Native(service) => {
                service.reconcile(snapshot.revision, &snapshot.shortcuts)
            }
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => {
                service.reconcile(snapshot.revision, &snapshot.shortcuts)
            }
        })
    }

    pub fn set_parent(
        &self,
        parent: Option<crate::desktop::ParentContext>,
    ) -> Result<(), crate::desktop::DesktopErrorCode> {
        match &self.backend {
            HotkeyBackend::Native(service) => service.set_parent(parent),
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => service.set_parent(parent),
        }
    }

    pub fn cancel_interaction(
        &self,
        generation: u64,
    ) -> futures_channel::oneshot::Receiver<crate::desktop::CancelOutcome> {
        match &self.backend {
            HotkeyBackend::Native(service) => service.cancel_interaction(generation),
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => service.cancel_interaction(generation),
        }
    }

    pub fn retry_pending(&self) -> Result<(), crate::desktop::DesktopErrorCode> {
        match &self.backend {
            HotkeyBackend::Native(service) => service.retry_pending(),
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => service.retry_pending(),
        }
    }

    /// Backend event subscription: acknowledgements, activations and
    /// backend loss. Native activations stay on the existing polled path
    /// (X11 smoke contract); portal activations arrive here without
    /// polling dependence.
    pub fn subscription(&self) -> iced::Subscription<crate::desktop::HotkeyEvent> {
        iced::Subscription::run_with(self.instance_id, stream_hotkey_events)
    }

    pub fn shutdown(&self) -> iced::Task<crate::ui::Message> {
        let pending = match &self.backend {
            HotkeyBackend::Native(service) => service.shutdown(),
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => service.shutdown(),
        };
        iced::Task::perform(
            async move {
                match pending.await {
                    Ok(()) => crate::desktop::ShutdownOutcome::Released,
                    Err(_) => crate::desktop::ShutdownOutcome::Forced(
                        crate::desktop::DesktopErrorCode::ShutdownTimeout,
                    ),
                }
            },
            crate::ui::Message::DesktopShutdownFinished,
        )
    }

    /// Native polled activation only (X11/Windows). Portal activations
    /// arrive through [`HotkeyService::subscription`].
    pub fn poll_native_event() -> Option<u32> {
        perf::measure("hotkeys.poll_native_event", || native::poll_event())
    }
}

fn stream_hotkey_events(
    instance_id: &u64,
) -> impl futures_util::Stream<Item = crate::desktop::HotkeyEvent> {
    let receiver = event_slots()
        .lock()
        .expect("hotkey event slots")
        .get_mut(instance_id)
        .expect("hotkey event slot registered")
        .receiver
        .clone();
    futures_util::stream::poll_fn(move |cx| {
        let mut receiver = receiver.lock().expect("hotkey event receiver");
        futures_util::stream::Stream::poll_next(std::pin::Pin::new(&mut *receiver), cx)
    })
}

/// Starts automatic paste using the backend selected for the active platform.
pub fn request_paste(runtime: std::sync::Arc<DesktopRuntime>, content: String) -> PasteRequest {
    if runtime.mode() == RuntimeMode::Synthetic && !runtime.native_consent_authorized() {
        return PasteRequest::Unavailable;
    }
    perf::measure(
        "hotkeys.request_paste",
        || match crate::platform::hotkey_backend_kind() {
            HotkeyBackendKind::Native => PasteRequest::ClipboardRequired(content),
            #[cfg(target_os = "linux")]
            HotkeyBackendKind::Portal => {
                if remote_desktop::paste_to_active_window(runtime, content) {
                    PasteRequest::Started
                } else {
                    PasteRequest::Busy
                }
            }
            #[cfg(not(target_os = "linux"))]
            HotkeyBackendKind::Portal => PasteRequest::Unavailable,
            HotkeyBackendKind::Unavailable => PasteRequest::Unavailable,
        },
    )
}

/// Injects Ctrl+V after the UI runtime has written and verified the clipboard.
pub fn paste_from_prepared_clipboard() -> bool {
    perf::measure("hotkeys.paste_from_prepared_clipboard_spawn", || {
        if crate::platform::hotkey_backend_kind() != HotkeyBackendKind::Native {
            return false;
        }
        native::paste_to_active_window()
    })
}

pub fn poll_paste_outcome() -> Option<PasteOutcome> {
    if let Some(outcome) = native::poll_paste_outcome() {
        return Some(outcome);
    }

    #[cfg(target_os = "linux")]
    if crate::platform::hotkey_backend_kind() == HotkeyBackendKind::Portal {
        return remote_desktop::poll_paste_outcome();
    }

    None
}

#[cfg(target_os = "linux")]
pub fn prewarm_permission(runtime: std::sync::Arc<DesktopRuntime>) {
    if crate::platform::display_server() == crate::platform::DisplayServer::Wayland {
        remote_desktop::prewarm_permission(runtime);
    }
}

#[cfg(not(target_os = "linux"))]
pub fn prewarm_permission(_runtime: std::sync::Arc<DesktopRuntime>) {}

#[cfg(target_os = "linux")]
pub fn is_paste_permission_denied() -> bool {
    if crate::platform::display_server() == crate::platform::DisplayServer::Wayland {
        return remote_desktop::is_permission_denied();
    }
    false
}

#[cfg(not(target_os = "linux"))]
pub fn is_paste_permission_denied() -> bool {
    false
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
    fn test_reconcile_rejects_unparseable_trigger_before_backend_contact() {
        use crate::desktop::{ShortcutSnapshot, ShortcutSpec};
        if let Some(service) = HotkeyService::new(std::sync::Arc::new(
            DesktopRuntime::from_parts(
                crate::desktop::DesktopConfig::default(),
                "io.github.roymejia2217.JamePrompt.Test",
                std::env::temp_dir(),
                None,
                RuntimeMode::Production,
                false,
            )
            .expect("test runtime"),
        )) {
            let receipt = service
                .reconcile(&ShortcutSnapshot::new(
                    1,
                    vec![ShortcutSpec {
                        prompt_id: "prompt-bad".to_string(),
                        description: "Bad".to_string(),
                        preferred_trigger: "Not A Key".to_string(),
                    }],
                ))
                .expect("reconcile queues");
            assert_eq!(receipt.rejected_prompt_ids, vec!["prompt-bad".to_string()]);
        }
    }

    #[test]
    fn test_empty_snapshot_reconciles_without_rejections() {
        use crate::desktop::ShortcutSnapshot;
        if let Some(service) = HotkeyService::new(std::sync::Arc::new(
            DesktopRuntime::from_parts(
                crate::desktop::DesktopConfig::default(),
                "io.github.roymejia2217.JamePrompt.Test",
                std::env::temp_dir(),
                None,
                RuntimeMode::Production,
                false,
            )
            .expect("test runtime"),
        )) {
            let receipt = service
                .reconcile(&ShortcutSnapshot::new(1, vec![]))
                .expect("reconcile queues");
            assert!(receipt.rejected_prompt_ids.is_empty());
        }
    }

    #[test]
    fn unauthorized_synthetic_run_never_requests_consent_or_paste() {
        let runtime = std::sync::Arc::new(
            DesktopRuntime::from_parts(
                crate::desktop::DesktopConfig::default(),
                "io.github.roymejia2217.JamePrompt.Test",
                std::env::temp_dir(),
                None,
                RuntimeMode::Synthetic,
                false,
            )
            .expect("test runtime"),
        );

        assert_eq!(
            request_paste(runtime, "synthetic content".to_string()),
            PasteRequest::Unavailable
        );
    }
}
