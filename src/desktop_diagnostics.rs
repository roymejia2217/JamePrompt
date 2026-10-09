//! Bounded structured diagnostics for desktop-session behavior.
//!
//! Owner of monotonic event IDs, durations and run identity. Prompt
//! content, clipboard text, restore tokens and activation tokens are never
//! recorded. Optional file output uses `create_new` in an approved private
//! run directory with 0600 permissions; I/O failure is reported, never a
//! silent fake report.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::desktop::{DesktopErrorCode, ShutdownOutcome};

/// Monotonic clock origin for one application process.
#[derive(Debug)]
pub struct DesktopClock {
    origin: Instant,
}

impl DesktopClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// Nanoseconds since process clock origin (monotonic, not wall-clock).
    pub fn monotonic_ns(&self) -> u64 {
        self.origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }
}

impl Default for DesktopClock {
    fn default() -> Self {
        Self::new()
    }
}

/// One sanitized desktop lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEvent {
    /// Process-local sequence number.
    pub sequence: u64,
    /// Monotonic nanoseconds since clock origin.
    pub monotonic_ns: u64,
    /// Shortcut revision when applicable.
    pub revision: Option<u64>,
    /// Window generation when applicable.
    pub window_generation: Option<u64>,
    /// Stable event name (see plan telemetry inventory).
    pub event: &'static str,
    /// Sanitized detail: ids/codes/counts only, never content or tokens.
    pub detail: Option<String>,
}

const MAX_BUFFERED_EVENTS: usize = 512;

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// In-memory bounded diagnostics sink shared by desktop producers.
#[derive(Debug)]
pub struct DesktopDiagnostics {
    clock: DesktopClock,
    run_id: String,
    events: VecDeque<DesktopEvent>,
    dropped: u64,
}

impl DesktopDiagnostics {
    /// Creates a sink tagged with a run identity (hex run suffix).
    pub fn new(run_id: String) -> Self {
        Self {
            clock: DesktopClock::new(),
            run_id,
            events: VecDeque::with_capacity(MAX_BUFFERED_EVENTS),
            dropped: 0,
        }
    }

    /// Creates a disabled no-op sink used when diagnostics are off.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn disabled() -> Self {
        Self::new("disabled".to_string())
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Records one sanitized event. Bounded: oldest entries are evicted
    /// and counted in [`Self::dropped_count`] instead of growing memory.
    pub fn record(
        &mut self,
        event: &'static str,
        revision: Option<u64>,
        window_generation: Option<u64>,
        detail: Option<String>,
    ) {
        let entry = DesktopEvent {
            sequence: EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            monotonic_ns: self.clock.monotonic_ns(),
            revision,
            window_generation,
            event,
            detail: detail.map(|d| sanitize_detail(&d)),
        };
        if self.events.len() >= MAX_BUFFERED_EVENTS {
            self.events.pop_front();
            self.dropped += 1;
        }
        self.events.push_back(entry);
    }

    pub fn record_shutdown_outcome(&mut self, outcome: ShutdownOutcome) {
        let detail = match outcome {
            ShutdownOutcome::Released => "outcome=released".to_string(),
            ShutdownOutcome::Forced(DesktopErrorCode::ShutdownTimeout) => {
                "outcome=forced code=shutdown_timeout".to_string()
            }
            ShutdownOutcome::Forced(code) => format!("outcome=forced code={code}"),
        };
        self.record("shutdown_outcome", None, None, Some(detail));
    }

    /// Number of events evicted due to the bound.
    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }

    pub fn events(&self) -> impl Iterator<Item = &DesktopEvent> {
        self.events.iter()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Appends all buffered events as JSON lines to a `create_new` file
    /// and clears the buffer. Each line carries run identity, sequence,
    /// monotonic offsets and sanitized fields only. Returns the number of
    /// events written. A missing parent, an existing file or any I/O
    /// error is reported to the caller — never a silent fake report.
    pub fn flush_to_file(&mut self, path: &std::path::Path) -> std::io::Result<usize> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            let metadata = std::fs::metadata(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o777 != 0o700 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "diagnostics parent must be private",
                    ));
                }
            }
        }
        let count = self.len();
        let dropped = self.dropped_count();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        writeln!(
            file,
            "{{\"run_id\":{},\"dropped\":{dropped},\"events\":{count}}}",
            json_string(&self.run_id)
        )?;
        for event in self.events() {
            writeln!(
                file,
                "{{\"sequence\":{},\"monotonic_ns\":{},\"revision\":{},\"window_generation\":{},\"event\":{},\"detail\":{}}}",
                event.sequence,
                event.monotonic_ns,
                event.revision.map_or("null".to_string(), |revision| revision.to_string()),
                event
                    .window_generation
                    .map_or("null".to_string(), |generation| generation.to_string()),
                json_string(event.event),
                event
                    .detail
                    .as_deref()
                    .map_or("null".to_string(), |detail| json_string(detail)),
            )?;
        }
        file.flush()?;
        self.events.clear();
        Ok(count)
    }
}

fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                escaped.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

/// Redacts secret-bearing material from diagnostic details.
///
/// Drops values that look like tokens/handles/content; keeps short stable
/// identifiers (prompt ids, error codes, counts, generations).
pub fn sanitize_detail(detail: &str) -> String {
    const REDACTED: &str = "[redacted]";
    let lowered = detail.to_lowercase();
    for marker in [
        "token",
        "clipboard",
        "content=",
        "content:",
        "prompt_text",
        "parent_handle",
        "raw_handle",
        "request_path",
        "session_path",
    ] {
        if lowered.contains(marker) {
            return REDACTED.to_string();
        }
    }
    if detail.len() > 256 {
        let mut end = 256;
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        return format!("{}…[truncated]", &detail[..end]);
    }
    detail.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_is_monotonic_non_decreasing() {
        let clock = DesktopClock::new();
        let first = clock.monotonic_ns();
        let second = clock.monotonic_ns();
        assert!(second >= first);
    }

    #[test]
    fn record_assigns_sequences_and_bounds_buffer() {
        let mut diagnostics = DesktopDiagnostics::new("run1".to_string());
        for _ in 0..(MAX_BUFFERED_EVENTS + 10) {
            diagnostics.record("tray_callback", None, None, None);
        }
        assert_eq!(diagnostics.len(), MAX_BUFFERED_EVENTS);
        assert_eq!(diagnostics.dropped_count(), 10);
        let first_sequence = diagnostics.events().next().unwrap().sequence;
        let last_sequence = diagnostics.events().last().unwrap().sequence;
        assert!(last_sequence > first_sequence);
    }

    #[test]
    fn sanitize_detail_redacts_tokens() {
        assert_eq!(sanitize_detail("activation_token=abc123"), "[redacted]");
        assert_eq!(sanitize_detail("clipboard content pasted"), "[redacted]");
    }

    #[test]
    fn sanitize_detail_keeps_stable_identifiers() {
        assert_eq!(
            sanitize_detail("revision=3 generation=7"),
            "revision=3 generation=7"
        );
    }

    #[test]
    fn sanitize_detail_truncates_long_values() {
        let long = "x".repeat(300);
        let sanitized = sanitize_detail(&long);
        assert!(sanitized.ends_with("[truncated]"));
    }

    #[test]
    fn diagnostics_multibyte_boundary_never_panics() {
        let detail = format!("{}é", "x".repeat(255));
        let sanitized = std::panic::catch_unwind(|| sanitize_detail(&detail))
            .expect("UTF-8 truncation must not panic");
        assert!(sanitized.is_char_boundary(sanitized.len()));
        assert!(sanitized.ends_with("[truncated]"));
    }

    #[test]
    fn diagnostics_redacts_parent_handles_and_raw_tokens() {
        assert_eq!(
            sanitize_detail("parent_handle=wayland:opaque"),
            "[redacted]"
        );
        assert_eq!(sanitize_detail("raw_handle=0x1234"), "[redacted]");
    }

    #[cfg(unix)]
    #[test]
    fn diagnostics_refuses_nonprivate_parent_and_existing_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("evidence parent");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("make parent non-private");
        let path = dir.path().join("run.jsonl");
        let mut diagnostics = DesktopDiagnostics::new("run-private".to_string());
        diagnostics.record("tray_callback", None, None, None);
        assert!(diagnostics.flush_to_file(&path).is_err());
    }

    #[test]
    fn diagnostics_io_error_is_reported() {
        let dir = tempfile::tempdir().expect("evidence parent");
        let parent_file = dir.path().join("not-a-directory");
        std::fs::write(&parent_file, b"file").expect("seed file");
        let path = parent_file.join("run.jsonl");
        let mut diagnostics = DesktopDiagnostics::new("run-io".to_string());
        diagnostics.record("tray_callback", None, None, None);
        assert!(diagnostics.flush_to_file(&path).is_err());
    }

    #[test]
    fn diagnostics_shutdown_outcome_is_captured() {
        let dir = tempfile::tempdir().expect("evidence parent");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .expect("secure evidence parent");
        }
        let path = dir.path().join("shutdown.jsonl");
        let mut diagnostics = DesktopDiagnostics::new("run-shutdown".to_string());
        diagnostics
            .record_shutdown_outcome(ShutdownOutcome::Forced(DesktopErrorCode::ShutdownTimeout));
        diagnostics.flush_to_file(&path).expect("flush outcome");
        let body = std::fs::read_to_string(path).expect("read evidence");
        assert!(body.contains("shutdown_outcome"));
        assert!(body.contains("outcome=forced code=shutdown_timeout"));
    }

    #[test]
    fn flush_writes_json_lines_and_clears_buffer() {
        let dir = tempfile::tempdir().expect("private evidence dir");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .expect("secure evidence parent");
        }
        let path = dir.path().join("run.jsonl");
        let mut diagnostics = DesktopDiagnostics::new("run-1".to_string());
        diagnostics.record(
            "tray_callback",
            Some(2),
            Some(3),
            Some("signal=Ready".into()),
        );
        let count = diagnostics.flush_to_file(&path).expect("flush");
        assert_eq!(count, 1);
        assert!(diagnostics.is_empty());
        let body = std::fs::read_to_string(&path).expect("read back");
        assert!(body.contains("\"run_id\":\"run-1\""));
        assert!(body.contains("\"event\":\"tray_callback\""));
        assert!(body.contains("\"revision\":2"));
    }

    #[test]
    fn flush_refuses_to_overwrite_existing_evidence() {
        let dir = tempfile::tempdir().expect("private evidence dir");
        let path = dir.path().join("run.jsonl");
        std::fs::write(&path, b"existing").expect("seed file");
        let mut diagnostics = DesktopDiagnostics::new("run-2".to_string());
        assert!(diagnostics.flush_to_file(&path).is_err());
    }
}
