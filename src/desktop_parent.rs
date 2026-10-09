//! Linux-only parent-window lease for portal dialogs.
//!
//! The owned [`ParentLease`] keeps the exported `WindowIdentifier` alive
//! from portal interaction start until cancellation/completion, before the
//! toplevel is destroyed. The ashpd Wayland export destroys its
//! `zxdg_exported` object on drop, so the lease owns the identifier and
//! only derives the `wayland:...` string for portal parameters. Raw display
//! pointers never cross this boundary.
//!
//! Export runs inside `iced::window::run` where the raw-handle borrows are
//! valid, completed with a blocking executor while the callback owns the
//! borrow. ashpd's helper performs its own blocking Wayland roundtrips; an
//! elapsed-clock measurement is recorded by the caller but cannot enforce
//! a deadline on the borrowing operation, so a hung export is a D0 design
//! issue, not a timeout to swallow.

use crate::desktop::DesktopErrorCode;
use std::sync::Arc;
use std::time::Instant;

/// Owned parent lease: valid only for its window generation.
#[derive(Debug)]
pub struct ParentLease {
    window_generation: u64,
    identifier: ashpd::WindowIdentifier,
    export_elapsed_ms: u64,
}

impl ParentLease {
    /// Wraps an already-exported identifier, rejecting non-Wayland or
    /// blank handles so a failed export can never become a silent blank
    /// parent.
    pub fn from_identifier(
        window_generation: u64,
        identifier: ashpd::WindowIdentifier,
        export_elapsed_ms: u64,
    ) -> Result<Self, DesktopErrorCode> {
        validate_wayland_handle(&identifier.to_string())?;
        Ok(Self {
            window_generation,
            identifier,
            export_elapsed_ms,
        })
    }

    pub fn generation(&self) -> u64 {
        self.window_generation
    }

    /// Owned exported handle (`wayland:...`), passed as portal parent.
    pub fn handle(&self) -> String {
        self.identifier.to_string()
    }

    /// Measured export latency for diagnostics (observation, not a
    /// deadline): the D0 gate requires p95 ≤ 100 ms over 30 exports.
    pub fn export_elapsed_ms(&self) -> u64 {
        self.export_elapsed_ms
    }

    /// Returns true when this lease still belongs to the given generation.
    pub fn is_current_for(&self, generation: u64) -> bool {
        self.window_generation == generation
    }
}

pub type SharedParentLease = Arc<ParentLease>;

/// Validates an exported Wayland parent handle string.
pub fn validate_wayland_handle(handle: &str) -> Result<(), DesktopErrorCode> {
    const PREFIX: &str = "wayland:";
    if handle.len() <= PREFIX.len() || !handle.starts_with(PREFIX) {
        return Err(DesktopErrorCode::ParentExportFailed);
    }
    Ok(())
}

/// Exports the toplevel `id` as a portal parent for `generation`.
///
/// Must be called on the iced main thread. The returned lease owns the
/// export; drop it only after outstanding portal interaction is
/// cancelled/completed and before the toplevel is destroyed.
pub fn export_parent(
    id: iced::window::Id,
    generation: u64,
) -> iced::Task<Result<SharedParentLease, DesktopErrorCode>> {
    iced::window::run(id, move |window| {
        let window_handle = window
            .window_handle()
            .map_err(|_| DesktopErrorCode::ParentUnavailable)?;
        let display_handle = window
            .display_handle()
            .map_err(|_| DesktopErrorCode::ParentUnavailable)?;
        let raw_window = window_handle.as_raw();
        let raw_display = display_handle.as_raw();
        let started = Instant::now();
        let identifier = futures_executor::block_on(ashpd::WindowIdentifier::from_raw_handle(
            &raw_window,
            Some(&raw_display),
        ))
        .ok_or(DesktopErrorCode::ParentExportFailed)?;
        let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        ParentLease::from_identifier(generation, identifier, elapsed_ms).map(Arc::new)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wayland_identifier(handle: &str) -> ashpd::WindowIdentifier {
        ashpd::WindowIdentifierType::Wayland(handle.to_string()).into()
    }

    #[test]
    fn valid_wayland_handle_is_accepted() {
        assert!(validate_wayland_handle("wayland:12345").is_ok());
    }

    #[test]
    fn empty_parent_handle_is_rejected() {
        assert_eq!(
            validate_wayland_handle(""),
            Err(DesktopErrorCode::ParentExportFailed)
        );
    }

    #[test]
    fn x11_handle_is_rejected_on_wayland_lease() {
        assert_eq!(
            validate_wayland_handle("x11:12345"),
            Err(DesktopErrorCode::ParentExportFailed)
        );
    }

    #[test]
    fn bare_prefix_without_suffix_is_rejected() {
        assert_eq!(
            validate_wayland_handle("wayland:"),
            Err(DesktopErrorCode::ParentExportFailed)
        );
    }

    #[test]
    fn lease_rejects_stale_generation() {
        let lease =
            ParentLease::from_identifier(3, wayland_identifier("abc"), 4).expect("valid lease");
        assert!(lease.is_current_for(3));
        assert!(!lease.is_current_for(4));
        assert_eq!(lease.handle(), "wayland:abc");
        assert_eq!(lease.export_elapsed_ms(), 4);
    }

    #[test]
    fn lease_construction_rejects_blank_export() {
        let error =
            ParentLease::from_identifier(1, wayland_identifier(""), 0).expect_err("must fail");
        assert_eq!(error, DesktopErrorCode::ParentExportFailed);
    }
}
