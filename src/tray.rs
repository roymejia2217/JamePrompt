//! Platform tray facade.
//!
//! Linux uses the event-driven StatusNotifierItem service
//! ([`linux`], ksni + DBusMenu callbacks, no GTK pumping). Windows keeps
//! the existing tray-icon polling path ([`windows`]). Every tray callback
//! reaches the same canonical application routes via [`TraySignal`].

#![cfg_attr(
    all(target_os = "linux", feature = "desktop-integration-tests"),
    allow(dead_code)
)]

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "windows")]
pub use windows::{TrayEvent, TrayHandle};

use crate::desktop::{DesktopErrorCode, DesktopRuntime};
use std::sync::Arc;

/// Typed tray event delivered to the iced subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraySignal {
    ShowRequested,
    QuitRequested,
    /// Service registered with the SNI watcher and ready.
    Ready,
    /// No usable tray host or service spawn failure.
    Unavailable(DesktopErrorCode),
    /// Watcher went offline after being available.
    HostOffline,
    /// Watcher came back online.
    HostOnline,
}

/// Availability from the application's perspective.
///
/// `Ready` requires a registered host/service, never the mere presence of
/// a handle. `Starting` is not ready: hidden startup waits for `Ready` or
/// the bounded initial failure before choosing the fallback window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayState {
    #[default]
    Starting,
    Ready,
    Unavailable,
    HostOffline,
}

/// Persistent tray subscription. The `instance` id gives the subscription a
/// stable identity across window open/close cycles: the service is created
/// once and cancelled only on shutdown, never rebuilt per status change.
pub fn subscription(instance: u64, runtime: Arc<DesktopRuntime>) -> iced::Subscription<TraySignal> {
    #[cfg(target_os = "linux")]
    {
        linux::subscription(instance, runtime)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (instance, runtime);
        iced::Subscription::none()
    }
}

/// Legacy platform event pumping hook. The Linux tray-icon path (and its
/// GTK pumping) is removed: the ksni service has no GTK loop integration
/// and rfd owns its separate GTK thread. Retained for the unchanged
/// Windows polling path only.
#[cfg(target_os = "windows")]
pub fn pump_platform_events() {
    // Windows tray-icon path requires no extra pumping.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ready_counts_as_available() {
        assert!(!matches!(TrayState::Starting, TrayState::Ready));
        assert!(!matches!(TrayState::Unavailable, TrayState::Ready));
        assert!(!matches!(TrayState::HostOffline, TrayState::Ready));
        assert!(matches!(TrayState::Ready, TrayState::Ready));
    }

    #[test]
    fn default_tray_state_is_starting() {
        assert_eq!(TrayState::default(), TrayState::Starting);
    }
}
