//! Shared desktop-session contracts for Linux Wayland/GNOME integration.
//!
//! Single owner for [`DesktopErrorCode`], [`DesktopConfig`], shortcut
//! snapshot/acknowledgement types and parent-window metadata. No business
//! logic lives here; producers (tray, window lifecycle, portal worker) map
//! their outcomes into these stable classifications so UI and tests share
//! one vocabulary.

use std::collections::HashMap;

/// Bounded, user-presentable classification of desktop integration failures.
///
/// Diagnostic strings may carry sanitized details elsewhere; UI copy must
/// branch on these variants, never on free-form message substrings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesktopErrorCode {
    BusUnavailable,
    BackendUnavailable,
    UnsupportedVersion,
    ParentExportFailed,
    ParentUnavailable,
    RequestCancelled,
    RequestDenied,
    RequestTimeout,
    InvalidResponse,
    Disconnected,
    QueueOverflow,
    ShutdownTimeout,
    TrayHostMissing,
}

impl DesktopErrorCode {
    /// Short stable identifier for telemetry and controller responses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BusUnavailable => "bus_unavailable",
            Self::BackendUnavailable => "backend_unavailable",
            Self::UnsupportedVersion => "unsupported_version",
            Self::ParentExportFailed => "parent_export_failed",
            Self::ParentUnavailable => "parent_unavailable",
            Self::RequestCancelled => "request_cancelled",
            Self::RequestDenied => "request_denied",
            Self::RequestTimeout => "request_timeout",
            Self::InvalidResponse => "invalid_response",
            Self::Disconnected => "disconnected",
            Self::QueueOverflow => "queue_overflow",
            Self::ShutdownTimeout => "shutdown_timeout",
            Self::TrayHostMissing => "tray_host_missing",
        }
    }
}

impl std::fmt::Display for DesktopErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Validated runtime timing/policy configuration for desktop integration.
///
/// Sourced once at launch from `JAME_PROMPT_DESKTOP_*` environment
/// variables; environment overrides declared defaults. Invalid values fail
/// before the database is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopConfig {
    /// D-Bus method call timeout in milliseconds (100..=30000).
    pub method_timeout_ms: u64,
    /// Portal consent response timeout in milliseconds (1000..=120000).
    pub response_timeout_ms: u64,
    /// Shutdown/cancellation acknowledgement deadline in ms (100..=10000).
    pub shutdown_timeout_ms: u64,
    /// Shortcut snapshot coalescing debounce in milliseconds (0..=1000).
    pub reconcile_debounce_ms: u64,
    /// Bounded channel capacity for tray/shortcut events (1..=1024).
    pub queue_capacity: usize,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            method_timeout_ms: 5_000,
            response_timeout_ms: 120_000,
            shutdown_timeout_ms: 2_000,
            reconcile_debounce_ms: 100,
            queue_capacity: 64,
        }
    }
}

/// Error describing a rejected desktop configuration value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// Environment variable name that failed validation.
    pub variable: &'static str,
    /// Human-actionable reason (includes observed value, never secrets).
    pub reason: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.variable, self.reason)
    }
}

impl std::error::Error for ConfigError {}

fn parse_bounded_u64(
    variable: &'static str,
    raw: Option<String>,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, ConfigError> {
    let Some(text) = raw else {
        return Ok(default);
    };
    let trimmed = text.trim();
    let value: u64 = trimmed.parse().map_err(|_| ConfigError {
        variable,
        reason: format!("expected integer in {min}..={max}, got {trimmed:?}"),
    })?;
    if !(min..=max).contains(&value) {
        return Err(ConfigError {
            variable,
            reason: format!("expected value in {min}..={max}, got {value}"),
        });
    }
    Ok(value)
}

fn parse_bounded_usize(
    variable: &'static str,
    raw: Option<String>,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, ConfigError> {
    let Some(text) = raw else {
        return Ok(default);
    };
    let trimmed = text.trim();
    let value: usize = trimmed.parse().map_err(|_| ConfigError {
        variable,
        reason: format!("expected integer in {min}..={max}, got {trimmed:?}"),
    })?;
    if !(min..=max).contains(&value) {
        return Err(ConfigError {
            variable,
            reason: format!("expected value in {min}..={max}, got {value}"),
        });
    }
    Ok(value)
}

impl DesktopConfig {
    /// Reads and validates configuration from the process environment.
    ///
    /// Fails early with an actionable [`ConfigError`] naming the offending
    /// variable; never falls back silently to defaults on invalid input.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with(|name| std::env::var(name).ok())
    }

    fn from_env_with(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        Ok(Self {
            method_timeout_ms: parse_bounded_u64(
                "JAME_PROMPT_DESKTOP_METHOD_TIMEOUT_MS",
                get("JAME_PROMPT_DESKTOP_METHOD_TIMEOUT_MS"),
                5_000,
                100,
                30_000,
            )?,
            response_timeout_ms: parse_bounded_u64(
                "JAME_PROMPT_DESKTOP_RESPONSE_TIMEOUT_MS",
                get("JAME_PROMPT_DESKTOP_RESPONSE_TIMEOUT_MS"),
                120_000,
                1_000,
                120_000,
            )?,
            shutdown_timeout_ms: parse_bounded_u64(
                "JAME_PROMPT_DESKTOP_SHUTDOWN_TIMEOUT_MS",
                get("JAME_PROMPT_DESKTOP_SHUTDOWN_TIMEOUT_MS"),
                2_000,
                100,
                10_000,
            )?,
            reconcile_debounce_ms: parse_bounded_u64(
                "JAME_PROMPT_DESKTOP_RECONCILE_DEBOUNCE_MS",
                get("JAME_PROMPT_DESKTOP_RECONCILE_DEBOUNCE_MS"),
                100,
                0,
                1_000,
            )?,
            queue_capacity: parse_bounded_usize(
                "JAME_PROMPT_DESKTOP_QUEUE_CAPACITY",
                get("JAME_PROMPT_DESKTOP_QUEUE_CAPACITY"),
                64,
                1,
                1024,
            )?,
        })
    }
}

/// Identity and storage context resolved once for one desktop process.
/// Consumers receive this immutable value instead of reading environment
/// variables or selecting production paths independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopRuntime {
    config: DesktopConfig,
    app_id: String,
    data_root: std::path::PathBuf,
    diagnostics_path: Option<std::path::PathBuf>,
    mode: RuntimeMode,
    native_consent_authorized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Production,
    Synthetic,
}

impl DesktopRuntime {
    /// Resolves production configuration before opening the database or
    /// constructing desktop services.
    pub fn production() -> Result<Self, ConfigError> {
        let config = DesktopConfig::from_env()?;
        let data_root = crate::config::get_data_dir();
        let diagnostics_path =
            std::env::var_os("JAME_PROMPT_DESKTOP_DIAGNOSTICS_PATH").map(std::path::PathBuf::from);
        Self::from_parts(
            config,
            crate::config::LINUX_DESKTOP_APP_ID,
            data_root,
            diagnostics_path,
            RuntimeMode::Production,
            false,
        )
    }

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    pub fn synthetic(
        profile: crate::desktop_test_support::SyntheticProfile,
        production_data_dir: &std::path::Path,
        config: DesktopConfig,
    ) -> Result<Self, ConfigError> {
        Self::from_parts(
            config,
            profile.test_app_id,
            profile.data_dir,
            profile.diagnostics_path,
            RuntimeMode::Synthetic,
            profile.native_consent_authorized,
        )
        .and_then(|runtime| {
            let production = canonicalize_for_runtime(production_data_dir)?;
            if runtime.data_root == production {
                return Err(ConfigError {
                    variable: "JAME_PROMPT_DESKTOP_DATA_DIR",
                    reason: "synthetic data root resolves to production data".to_string(),
                });
            }
            Ok(runtime)
        })
    }

    pub(crate) fn from_parts(
        config: DesktopConfig,
        app_id: impl Into<String>,
        data_root: std::path::PathBuf,
        diagnostics_path: Option<std::path::PathBuf>,
        mode: RuntimeMode,
        native_consent_authorized: bool,
    ) -> Result<Self, ConfigError> {
        let app_id = app_id.into();
        if app_id.is_empty() || !app_id.contains('.') {
            return Err(ConfigError {
                variable: "JAME_PROMPT_DESKTOP_APP_ID",
                reason: "app identity must be a non-empty reverse-DNS value".to_string(),
            });
        }
        if !data_root.is_absolute() {
            return Err(ConfigError {
                variable: "JAME_PROMPT_DESKTOP_DATA_DIR",
                reason: "data root must be absolute".to_string(),
            });
        }
        let data_root = canonicalize_for_runtime(&data_root)?;
        let diagnostics_path = diagnostics_path
            .map(|path| {
                if !path.is_absolute() {
                    return Err(ConfigError {
                        variable: "JAME_PROMPT_DESKTOP_DIAGNOSTICS_PATH",
                        reason: "diagnostics path must be absolute".to_string(),
                    });
                }
                let canonical = canonicalize_for_runtime(&path)?;
                if !canonical.starts_with(&data_root) {
                    return Err(ConfigError {
                        variable: "JAME_PROMPT_DESKTOP_DIAGNOSTICS_PATH",
                        reason: "diagnostics path must remain under data root".to_string(),
                    });
                }
                Ok(canonical)
            })
            .transpose()?;
        Ok(Self {
            config,
            app_id,
            data_root,
            diagnostics_path,
            mode,
            native_consent_authorized,
        })
    }

    pub fn config(&self) -> DesktopConfig {
        self.config
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn data_root(&self) -> &std::path::Path {
        &self.data_root
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.data_root.join("prompts.db")
    }

    pub fn settings_path(&self) -> std::path::PathBuf {
        self.data_root.join("settings.json")
    }

    pub fn diagnostics_path(&self) -> Option<&std::path::Path> {
        self.diagnostics_path.as_deref()
    }

    pub fn mode(&self) -> RuntimeMode {
        self.mode
    }

    pub fn native_consent_authorized(&self) -> bool {
        self.native_consent_authorized
    }
}

fn canonicalize_for_runtime(path: &std::path::Path) -> Result<std::path::PathBuf, ConfigError> {
    if path.exists() {
        return std::fs::canonicalize(path).map_err(|error| ConfigError {
            variable: "JAME_PROMPT_DESKTOP_DATA_DIR",
            reason: format!("cannot resolve {}: {error}", path.display()),
        });
    }
    let parent = path.parent().ok_or_else(|| ConfigError {
        variable: "JAME_PROMPT_DESKTOP_DATA_DIR",
        reason: format!("{} has no parent", path.display()),
    })?;
    let file_name = path.file_name().ok_or_else(|| ConfigError {
        variable: "JAME_PROMPT_DESKTOP_DATA_DIR",
        reason: format!("{} has no final component", path.display()),
    })?;
    Ok(std::fs::canonicalize(parent)
        .map_err(|error| ConfigError {
            variable: "JAME_PROMPT_DESKTOP_DATA_DIR",
            reason: format!("cannot resolve {}: {error}", parent.display()),
        })?
        .join(file_name))
}

/// One desired global shortcut: stable prompt identity plus the user's
/// preferred trigger text. GNOME owns the actually assigned trigger; the
/// preferred value is a request, never authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutSpec {
    /// Stable prompt UUID (never regenerated for key edits).
    pub prompt_id: String,
    /// Human-readable prompt name for the portal dialog.
    pub description: String,
    /// User-requested trigger text, e.g. `Ctrl+Alt+F8`.
    pub preferred_trigger: String,
}

/// Complete versioned desired-shortcut state. Revision increments only on
/// effective desired-set/metadata change; content-only edits are no-ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutSnapshot {
    /// Monotonic revision starting at 1.
    pub revision: u64,
    /// Desired shortcuts sorted by `prompt_id` for determinism.
    pub shortcuts: Vec<ShortcutSpec>,
}

impl ShortcutSnapshot {
    /// Builds a deterministic snapshot (sorted by `prompt_id`).
    pub fn new(revision: u64, mut shortcuts: Vec<ShortcutSpec>) -> Self {
        shortcuts.sort_by(|a, b| a.prompt_id.cmp(&b.prompt_id));
        Self {
            revision,
            shortcuts,
        }
    }

    /// Returns true when the effective desired set (id + description +
    /// trigger) is identical, ignoring revision numbers.
    pub fn same_effective_set(&self, other: &Self) -> bool {
        self.shortcuts == other.shortcuts
    }
}

/// Computes the next revision: keeps the previous revision when the
/// effective desired set is unchanged; otherwise advances to at least
/// `previous.revision + 1` (never below 1).
pub fn next_snapshot_revision(
    last: Option<&ShortcutSnapshot>,
    desired: &[ShortcutSpec],
    next_revision: u64,
) -> (ShortcutSnapshot, u64) {
    let mut sorted = desired.to_vec();
    sorted.sort_by(|a, b| a.prompt_id.cmp(&b.prompt_id));
    let revision = match last {
        Some(previous) if previous.shortcuts == sorted => previous.revision,
        Some(previous) => previous
            .revision
            .saturating_add(1)
            .max(next_revision)
            .max(1),
        None => next_revision.max(1),
    };
    (
        ShortcutSnapshot {
            revision,
            shortcuts: sorted,
        },
        revision,
    )
}

/// Readiness of a single desired shortcut from the backend's perspective.
///
/// `Pending` means queued, never assumed active. `Active` requires an
/// accepted backend acknowledgement for the current revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutStatus {
    Pending,
    AwaitingWindow,
    AwaitingConsent,
    Active,
    Inactive,
    Cancelled,
    Unavailable,
    Failed,
}

/// Backend acknowledgement for one shortcut at a given revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutAcknowledgement {
    pub revision: u64,
    pub prompt_id: String,
    pub runtime_id: Option<u32>,
    pub status: ShortcutStatus,
    /// Backend-assigned trigger text (`trigger_description`); may differ
    /// from the requested `preferred_trigger` on GNOME 50.
    pub trigger_description: Option<String>,
    pub error_code: Option<DesktopErrorCode>,
}

/// Events flowing from the shortcut backend to the iced subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    Acknowledged(ShortcutAcknowledgement),
    Activated { revision: u64, runtime_id: u32 },
    BackendUnavailable(DesktopErrorCode),
}

/// Outcome of a cancel_interaction request. `Cancelled` is reported only
/// after the remote Request was closed, never for merely enqueueing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    Released,
    Failed(DesktopErrorCode),
    NoPendingRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownOutcome {
    Released,
    Forced(DesktopErrorCode),
}

/// Receipt for a reconcile call: which prompts were rejected before any
/// backend contact, so the UI marks them Failed instead of Pending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReceipt {
    pub rejected_prompt_ids: Vec<String>,
}

/// Portable owned parent-window metadata passed to the shortcut worker.
///
/// The actual exported handle string (e.g. `wayland:...`) is produced by
/// the Linux-only lease; this struct never carries raw display pointers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentContext {
    /// Window generation that owns the export.
    pub generation: u64,
    /// Owned exported handle string.
    pub handle: String,
}

/// Applies an acknowledgement to the UI map only when it belongs to the
/// current desired revision and the prompt is still desired.
///
/// Returns true when the map was updated; stale/unknown acks are rejected
/// without mutation so a late consent cannot resurrect deleted bindings.
pub fn apply_acknowledgement(
    states: &mut HashMap<String, ShortcutAcknowledgement>,
    desired_ids: &std::collections::HashSet<String>,
    current_revision: u64,
    ack: ShortcutAcknowledgement,
) -> bool {
    if ack.revision != current_revision {
        return false;
    }
    if !desired_ids.contains(&ack.prompt_id) && ack.status == ShortcutStatus::Active {
        return false;
    }
    states.insert(ack.prompt_id.clone(), ack);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn spec(id: &str, trigger: &str) -> ShortcutSpec {
        ShortcutSpec {
            prompt_id: id.to_string(),
            description: format!("prompt {id}"),
            preferred_trigger: trigger.to_string(),
        }
    }

    #[test]
    fn config_defaults_match_documented_policy() {
        let config = DesktopConfig::from_env_with(|_| None);
        let config = config.expect("defaults must validate");
        assert_eq!(config.method_timeout_ms, 5_000);
        assert_eq!(config.response_timeout_ms, 120_000);
        assert_eq!(config.shutdown_timeout_ms, 2_000);
        assert_eq!(config.reconcile_debounce_ms, 100);
        assert_eq!(config.queue_capacity, 64);
    }

    #[test]
    fn config_rejects_out_of_bounds_method_timeout() {
        let result = DesktopConfig::from_env_with(|name| {
            (name == "JAME_PROMPT_DESKTOP_METHOD_TIMEOUT_MS").then(|| "5".to_string())
        });
        let error = result.expect_err("below-minimum timeout must fail");
        assert_eq!(error.variable, "JAME_PROMPT_DESKTOP_METHOD_TIMEOUT_MS");
    }

    #[test]
    fn config_rejects_non_numeric_capacity() {
        let result = DesktopConfig::from_env_with(|name| {
            (name == "JAME_PROMPT_DESKTOP_QUEUE_CAPACITY").then(|| "many".to_string())
        });
        assert!(result.is_err());
    }

    #[test]
    fn runtime_keeps_one_validated_config_and_private_destinations() {
        let dir = tempfile::tempdir().expect("runtime root");
        let data_root = dir.path().join("data");
        std::fs::create_dir(&data_root).expect("data root");
        let diagnostics = data_root.join("diagnostics.jsonl");
        let config = DesktopConfig::default();
        let runtime = DesktopRuntime::from_parts(
            config,
            "io.github.roymejia2217.JamePrompt.Test",
            data_root.clone(),
            Some(diagnostics.clone()),
            RuntimeMode::Synthetic,
            false,
        )
        .expect("valid runtime");

        assert_eq!(runtime.config(), config);
        assert_eq!(runtime.app_id(), "io.github.roymejia2217.JamePrompt.Test");
        assert_eq!(runtime.data_root(), data_root);
        assert_eq!(runtime.db_path(), data_root.join("prompts.db"));
        assert_eq!(runtime.settings_path(), data_root.join("settings.json"));
        assert_eq!(runtime.diagnostics_path(), Some(diagnostics.as_path()));
        assert_eq!(runtime.mode(), RuntimeMode::Synthetic);
        assert!(!runtime.native_consent_authorized());
    }

    #[test]
    fn runtime_rejects_diagnostics_outside_data_root() {
        let dir = tempfile::tempdir().expect("runtime root");
        let data_root = dir.path().join("data");
        std::fs::create_dir(&data_root).expect("data root");
        let outside = dir.path().join("outside.jsonl");

        let error = DesktopRuntime::from_parts(
            DesktopConfig::default(),
            "io.github.roymejia2217.JamePrompt.Test",
            data_root,
            Some(outside),
            RuntimeMode::Synthetic,
            false,
        )
        .expect_err("diagnostics must remain private");

        assert_eq!(error.variable, "JAME_PROMPT_DESKTOP_DIAGNOSTICS_PATH");
    }

    #[test]
    fn snapshot_sorts_by_prompt_id_for_determinism() {
        let snapshot = ShortcutSnapshot::new(3, vec![spec("b", "Ctrl+A"), spec("a", "Ctrl+B")]);
        assert_eq!(snapshot.shortcuts[0].prompt_id, "a");
        assert_eq!(snapshot.shortcuts[1].prompt_id, "b");
        assert_eq!(snapshot.revision, 3);
    }

    #[test]
    fn same_effective_set_ignores_revision() {
        let first = ShortcutSnapshot::new(1, vec![spec("a", "Ctrl+A")]);
        let second = ShortcutSnapshot::new(2, vec![spec("a", "Ctrl+A")]);
        assert!(first.same_effective_set(&second));
    }

    #[test]
    fn content_only_change_is_detected_as_different_description() {
        let first = ShortcutSnapshot::new(1, vec![spec("a", "Ctrl+A")]);
        let mut changed = spec("a", "Ctrl+A");
        changed.description = "renamed".to_string();
        let second = ShortcutSnapshot::new(1, vec![changed]);
        assert!(!first.same_effective_set(&second));
    }

    #[test]
    fn stale_revision_ack_is_rejected_without_mutation() {
        let mut states = HashMap::new();
        let desired: HashSet<String> = ["p1".into()].into_iter().collect();
        let ack = ShortcutAcknowledgement {
            revision: 1,
            prompt_id: "p1".into(),
            runtime_id: Some(7),
            status: ShortcutStatus::Active,
            trigger_description: Some("Ctrl+A".into()),
            error_code: None,
        };
        assert!(!apply_acknowledgement(&mut states, &desired, 2, ack));
        assert!(states.is_empty());
    }

    #[test]
    fn active_ack_for_removed_prompt_is_rejected() {
        let mut states = HashMap::new();
        let desired: HashSet<String> = HashSet::new();
        let ack = ShortcutAcknowledgement {
            revision: 2,
            prompt_id: "deleted".into(),
            runtime_id: Some(7),
            status: ShortcutStatus::Active,
            trigger_description: None,
            error_code: None,
        };
        assert!(!apply_acknowledgement(&mut states, &desired, 2, ack));
        assert!(states.is_empty());
    }

    #[test]
    fn current_ack_for_desired_prompt_is_applied() {
        let mut states = HashMap::new();
        let desired: HashSet<String> = ["p1".into()].into_iter().collect();
        let ack = ShortcutAcknowledgement {
            revision: 2,
            prompt_id: "p1".into(),
            runtime_id: None,
            status: ShortcutStatus::Pending,
            trigger_description: None,
            error_code: None,
        };
        assert!(apply_acknowledgement(&mut states, &desired, 2, ack));
        assert_eq!(states["p1"].status, ShortcutStatus::Pending);
    }

    #[test]
    fn error_code_strings_are_stable() {
        assert_eq!(DesktopErrorCode::RequestTimeout.as_str(), "request_timeout");
        assert_eq!(
            DesktopErrorCode::TrayHostMissing.as_str(),
            "tray_host_missing"
        );
    }

    #[test]
    fn shutdown_outcome_distinguishes_forced_cleanup() {
        assert_eq!(ShutdownOutcome::Released, ShutdownOutcome::Released);
        assert_eq!(
            ShutdownOutcome::Forced(DesktopErrorCode::ShutdownTimeout),
            ShutdownOutcome::Forced(DesktopErrorCode::ShutdownTimeout)
        );
        assert_ne!(
            ShutdownOutcome::Released,
            ShutdownOutcome::Forced(DesktopErrorCode::ShutdownTimeout)
        );
    }
}
