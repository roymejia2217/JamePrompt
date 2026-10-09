//! Feature-only desktop integration test support (Linux).
//!
//! Compiled exclusively with `desktop-integration-tests` on Linux and
//! activated only through the explicit `--desktop-test-controller` CLI
//! switch. Absent from default and release builds: no socket, no test
//! identity, no synthetic configuration surface.
//!
//! The controller is a local Unix-socket NDJSON server (version 1) that
//! translates reviewed operations into the production [`crate::ui::Message`]
//! handlers on the iced main thread — never direct database writes, never
//! a parallel save path. Replies report the production handler outcome,
//! including the database row identity and shortcut status snapshot, but a
//! controller acknowledgement is NOT portal consent acceptance.

use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

/// Maximum controller packet: 64 KiB, bounded pending queue 64.
pub const MAX_PACKET_BYTES: usize = 64 * 1024;
pub const MAX_PENDING_COMMANDS: usize = 64;
pub const PROTOCOL_VERSION: u32 = 1;

/// Validated synthetic startup profile. Production paths, autostart and
/// permission prewarm are never borrowed: distinct app identity, private
/// data dir, autostart suppressed, no prewarm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticProfile {
    pub test_app_id: String,
    pub data_dir: PathBuf,
    pub diagnostics_path: Option<PathBuf>,
    pub fixture_seed: String,
    pub native_consent_authorized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError {
    pub reason: String,
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid desktop test profile: {}", self.reason)
    }
}

impl std::error::Error for ProfileError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    #[serde(default)]
    test_app_id: Option<String>,
    #[serde(default)]
    data_dir: Option<PathBuf>,
    #[serde(default)]
    diagnostics_path: Option<PathBuf>,
    #[serde(default)]
    fixture_seed: Option<String>,
    #[serde(default)]
    native_consent_authorized: Option<bool>,
    #[serde(default)]
    autostart: Option<bool>,
    #[serde(default)]
    prewarm: Option<bool>,
}

fn is_reverse_dns(app_id: &str) -> bool {
    let labels: Vec<&str> = app_id.split('.').collect();
    if labels.len() < 3 {
        return false;
    }
    labels.iter().all(|label| {
        let mut chars = label.chars();
        matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    })
}

fn canonicalize_for_validation(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return std::fs::canonicalize(path).map_err(|error| error.to_string());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "path has no existing parent".to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "path has no final component".to_string())?;
    Ok(std::fs::canonicalize(parent)
        .map_err(|error| error.to_string())?
        .join(file_name))
}

fn validate_private_run_path(
    profile_path: &Path,
    path: &Path,
    run_root: &Path,
    label: &str,
) -> Result<PathBuf, ProfileError> {
    if !path.is_absolute() {
        return Err(ProfileError {
            reason: format!("{label} must be absolute"),
        });
    }
    let canonical = canonicalize_for_validation(path).map_err(|error| ProfileError {
        reason: format!(
            "cannot resolve {label} for {}: {error}",
            profile_path.display()
        ),
    })?;
    if !canonical.starts_with(run_root) {
        return Err(ProfileError {
            reason: format!("{label} must remain under private run root"),
        });
    }
    Ok(canonical)
}

impl SyntheticProfile {
    pub fn load(path: &Path, production_data_dir: &Path) -> Result<Self, ProfileError> {
        let body = std::fs::read_to_string(path).map_err(|error| ProfileError {
            reason: format!("cannot read {}: {error}", path.display()),
        })?;
        if body.len() > MAX_PACKET_BYTES {
            return Err(ProfileError {
                reason: format!("profile exceeds {} bytes", MAX_PACKET_BYTES),
            });
        }
        let raw: RawProfile = serde_json::from_str(&body).map_err(|error| ProfileError {
            reason: format!("profile schema rejected: {error}"),
        })?;
        let test_app_id = raw.test_app_id.ok_or_else(|| ProfileError {
            reason: "test_app_id is required".to_string(),
        })?;
        if !is_reverse_dns(&test_app_id) {
            return Err(ProfileError {
                reason: format!("test_app_id {test_app_id:?} is not reverse-DNS"),
            });
        }
        if test_app_id == crate::config::LINUX_DESKTOP_APP_ID {
            return Err(ProfileError {
                reason: "test_app_id must differ from the production app id".to_string(),
            });
        }
        let data_dir = raw.data_dir.ok_or_else(|| ProfileError {
            reason: "data_dir is required".to_string(),
        })?;
        let run_root = path
            .parent()
            .ok_or_else(|| ProfileError {
                reason: "profile must have a private parent directory".to_string(),
            })
            .and_then(|parent| {
                canonicalize_for_validation(parent).map_err(|error| ProfileError {
                    reason: format!("cannot resolve private run root: {error}"),
                })
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&run_root)
                .map_err(|error| ProfileError {
                    reason: format!("cannot inspect private run root: {error}"),
                })?
                .permissions()
                .mode()
                & 0o777;
            if mode != 0o700 {
                return Err(ProfileError {
                    reason: format!("private run root must be mode 0700, got {mode:o}"),
                });
            }
        }
        let data_dir = validate_private_run_path(path, &data_dir, &run_root, "data_dir")?;
        let production_data_dir =
            canonicalize_for_validation(production_data_dir).map_err(|error| ProfileError {
                reason: format!("cannot resolve production data directory: {error}"),
            })?;
        if data_dir == production_data_dir {
            return Err(ProfileError {
                reason: "data_dir must not be the production data directory".to_string(),
            });
        }
        let diagnostics_path = raw
            .diagnostics_path
            .as_deref()
            .map(|diagnostics_path| {
                validate_private_run_path(path, diagnostics_path, &run_root, "diagnostics_path")
            })
            .transpose()?;
        if raw.autostart == Some(true) {
            return Err(ProfileError {
                reason: "autostart must be suppressed in test profiles".to_string(),
            });
        }
        if raw.prewarm == Some(true) {
            return Err(ProfileError {
                reason: "permission prewarm is disabled in test profiles".to_string(),
            });
        }
        Ok(Self {
            test_app_id,
            data_dir,
            diagnostics_path,
            fixture_seed: raw.fixture_seed.unwrap_or_else(|| "seed".to_string()),
            native_consent_authorized: raw.native_consent_authorized.unwrap_or(false),
        })
    }
}

/// Controller operations. No arbitrary command, path or shell field
/// exists; import carries a bounded backup object validated by the
/// existing [`crate::prompt_backup::PromptBackup`] rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControllerOperation {
    Hello {},
    State {},
    CreatePrompt {
        name: String,
        content: String,
        hotkey: Option<String>,
        hotkey_enabled: Option<bool>,
    },
    EditPrompt {
        id: String,
        name: Option<String>,
        content: Option<String>,
        hotkey: Option<String>,
        hotkey_enabled: Option<bool>,
    },
    DeletePrompt {
        id: String,
    },
    ToggleGlobalHotkeys {
        enabled: bool,
    },
    ImportFixture {
        backup: serde_json::Value,
        mode: Option<String>,
    },
    OpenExportDialog {},
    OpenImportDialog {},
    CloseWindow {},
    ShowWindow {},
    Quit {},
}

#[derive(Debug, Clone)]
pub struct ControllerRequest {
    pub version: u32,
    pub request_id: u64,
    pub operation: ControllerOperation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawControllerRequest {
    version: u32,
    request_id: u64,
    operation: String,
    args: serde_json::Value,
}

impl<'de> Deserialize<'de> for ControllerRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawControllerRequest::deserialize(deserializer)?;
        if raw.request_id == 0 {
            return Err(D::Error::custom("request_id must be positive"));
        }
        let mut args = match raw.args {
            serde_json::Value::Object(args) => args,
            _ => return Err(D::Error::custom("controller args must be an object")),
        };
        args.insert(
            "operation".to_string(),
            serde_json::Value::String(raw.operation),
        );
        let operation =
            serde_json::from_value(serde_json::Value::Object(args)).map_err(D::Error::custom)?;
        Ok(Self {
            version: raw.version,
            request_id: raw.request_id,
            operation,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ControllerError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControllerResponse {
    pub version: u32,
    pub request_id: u64,
    pub ok: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<ControllerError>,
}

/// Channel bridge carrying one translated command plus its single reply
/// slot into the iced main thread.
#[derive(Debug, Clone)]
pub struct ControllerCommand {
    pub request_id: u64,
    pub operation: ControllerOperation,
    pub reply: Arc<Mutex<Option<futures_channel::oneshot::Sender<ControllerResponse>>>>,
}

static CONTROLLER_RECEIVER: std::sync::OnceLock<
    Arc<Mutex<futures_channel::mpsc::UnboundedReceiver<ControllerCommand>>>,
> = std::sync::OnceLock::new();

/// Registers the controller inbox for one subscription instance. Called
/// once from test-mode startup before the iced runtime starts.
pub fn register_inbox(
    _instance: u64,
    receiver: futures_channel::mpsc::UnboundedReceiver<ControllerCommand>,
) {
    let _ = CONTROLLER_RECEIVER.set(Arc::new(Mutex::new(receiver)));
}

/// Polls one queued command without waiting. The UI owns dispatch timing;
/// controller I/O remains on its dedicated socket thread.
pub fn poll_command() -> Option<ControllerCommand> {
    let receiver = CONTROLLER_RECEIVER.get()?.clone();
    let mut receiver = receiver.lock().ok()?;
    let waker = futures_util::task::noop_waker_ref();
    let mut context = std::task::Context::from_waker(waker);
    match futures_util::stream::Stream::poll_next(std::pin::Pin::new(&mut *receiver), &mut context)
    {
        std::task::Poll::Ready(command) => command,
        std::task::Poll::Pending => None,
    }
}

/// Sanitized lifecycle event for the scenario report aggregator.
#[derive(Debug, Clone, Serialize)]
pub struct ControllerEvent {
    pub version: u32,
    pub run_id: String,
    pub operation_id: u64,
    pub sequence: u64,
    pub monotonic_ns: u64,
    pub revision: Option<u64>,
    pub window_generation: Option<u64>,
    pub event: String,
}

/// Owned controller server: Unix socket bound with 0600 inside a private
/// 0700 directory, same-user only. No TCP listener exists.
pub struct TestController {
    _socket_path: PathBuf,
    _join: Option<std::thread::JoinHandle<()>>,
}

impl TestController {
    pub fn start(
        socket_path: &Path,
        run_id: String,
        inbox: futures_channel::mpsc::UnboundedSender<ControllerCommand>,
        event_out: Arc<Mutex<VecDeque<ControllerEvent>>>,
        clock_origin: Instant,
    ) -> Result<Self, String> {
        validate_socket_dir(socket_path)?;
        if socket_path.exists() {
            return Err(format!(
                "controller socket {} already exists; refusing to hijack it",
                socket_path.display()
            ));
        }
        let listener = UnixListener::bind(socket_path)
            .map_err(|error| format!("cannot bind controller socket: {error}"))?;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(socket_path, permissions)
            .map_err(|error| format!("cannot secure controller socket: {error}"))?;
        let socket_path = socket_path.to_path_buf();
        let join = std::thread::Builder::new()
            .name("jameprompt-test-controller".into())
            .spawn(move || {
                accept_loop(listener, &run_id, inbox, &event_out, clock_origin);
            })
            .map_err(|error| format!("cannot spawn controller thread: {error}"))?;
        Ok(Self {
            _socket_path: socket_path,
            _join: Some(join),
        })
    }
}

fn validate_socket_dir(socket_path: &Path) -> Result<(), String> {
    let parent = socket_path.parent().ok_or_else(|| {
        format!(
            "controller socket {} has no parent directory",
            socket_path.display()
        )
    })?;
    if !parent.is_absolute() {
        return Err(format!(
            "controller socket directory {} must be absolute",
            parent.display()
        ));
    }
    #[cfg(unix)]
    {
        let metadata = std::fs::metadata(parent)
            .map_err(|error| format!("cannot stat controller directory: {error}"))?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o700 {
            return Err(format!(
                "controller directory {} must be private (0700), got {mode:o}",
                parent.display()
            ));
        }
    }
    Ok(())
}

fn accept_loop(
    listener: UnixListener,
    _run_id: &str,
    inbox: futures_channel::mpsc::UnboundedSender<ControllerCommand>,
    event_out: &Arc<Mutex<VecDeque<ControllerEvent>>>,
    clock_origin: Instant,
) {
    let mut sequence = 1u64;
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        sequence += 1;
        serve_connection(stream, sequence, &inbox, event_out, clock_origin);
    }
}

/// One in-flight request: the reply receiver stays on this thread while
/// the sender travels with the command to the iced main thread.
struct PendingReply {
    request_id: u64,
    receiver: futures_channel::oneshot::Receiver<ControllerResponse>,
    deadline: Instant,
}

#[derive(Debug, PartialEq, Eq)]
enum FrameReadError {
    Io,
    Unterminated,
    Oversized,
}

fn read_bounded_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>, FrameReadError> {
    let mut frame = Vec::with_capacity(MAX_PACKET_BYTES.min(4096));
    let mut byte = [0u8; 1];

    loop {
        match reader.read(&mut byte) {
            Ok(0) if frame.is_empty() => return Ok(None),
            Ok(0) => return Err(FrameReadError::Unterminated),
            Ok(_) => {}
            Err(_) => return Err(FrameReadError::Io),
        }

        if frame.len() == MAX_PACKET_BYTES {
            if byte[0] != b'\n' {
                while byte[0] != b'\n' {
                    match reader.read(&mut byte) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                return Err(FrameReadError::Oversized);
            }
            frame.push(byte[0]);
            return Ok(Some(frame));
        }

        frame.push(byte[0]);
        if byte[0] == b'\n' {
            return Ok(Some(frame));
        }
    }
}

fn serve_connection(
    stream: UnixStream,
    _connection_id: u64,
    inbox: &futures_channel::mpsc::UnboundedSender<ControllerCommand>,
    event_out: &Arc<Mutex<VecDeque<ControllerEvent>>>,
    _clock_origin: Instant,
) {
    let Ok(read_stream) = stream.try_clone() else {
        return;
    };
    let mut writer = stream;
    let mut pending: Vec<PendingReply> = Vec::new();
    let mut reader = BufReader::new(read_stream);
    loop {
        flush_events(&mut writer, event_out);
        drain_replies(&mut writer, &mut pending);
        let frame = match read_bounded_frame(&mut reader) {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(FrameReadError::Unterminated) | Err(FrameReadError::Io) => break,
            Err(FrameReadError::Oversized) => {
                let _ = write_response(
                    &mut writer,
                    &ControllerResponse {
                        version: PROTOCOL_VERSION,
                        request_id: 0,
                        ok: false,
                        result: None,
                        error: Some(ControllerError {
                            code: "oversize_frame".to_string(),
                            message: format!("frame exceeds {MAX_PACKET_BYTES} bytes"),
                        }),
                    },
                );
                continue;
            }
        };
        let line = &frame[..frame.len() - 1];
        if line.is_empty() {
            let _ = write_response(
                &mut writer,
                &ControllerResponse {
                    version: PROTOCOL_VERSION,
                    request_id: 0,
                    ok: false,
                    result: None,
                    error: Some(ControllerError {
                        code: "oversize_frame".to_string(),
                        message: format!("frame exceeds {MAX_PACKET_BYTES} bytes"),
                    }),
                },
            );
            continue;
        }
        if pending.len() >= MAX_PENDING_COMMANDS {
            let _ = write_response(
                &mut writer,
                &ControllerResponse {
                    version: PROTOCOL_VERSION,
                    request_id: 0,
                    ok: false,
                    result: None,
                    error: Some(ControllerError {
                        code: "queue_overflow".to_string(),
                        message: "too many pending controller commands".to_string(),
                    }),
                },
            );
            continue;
        }
        let request: Result<ControllerRequest, _> = serde_json::from_slice(line);
        let Ok(request) = request else {
            let _ = write_response(
                &mut writer,
                &ControllerResponse {
                    version: PROTOCOL_VERSION,
                    request_id: 0,
                    ok: false,
                    result: None,
                    error: Some(ControllerError {
                        code: "invalid_frame".to_string(),
                        message: "request is not valid controller JSON".to_string(),
                    }),
                },
            );
            continue;
        };
        if request.version != PROTOCOL_VERSION {
            let _ = write_response(
                &mut writer,
                &ControllerResponse {
                    version: PROTOCOL_VERSION,
                    request_id: request.request_id,
                    ok: false,
                    result: None,
                    error: Some(ControllerError {
                        code: "unsupported_version".to_string(),
                        message: "controller version mismatch".to_string(),
                    }),
                },
            );
            continue;
        }
        let is_quit = matches!(&request.operation, ControllerOperation::Quit {});
        let (reply_tx, reply_rx) = futures_channel::oneshot::channel();
        let command = ControllerCommand {
            request_id: request.request_id,
            operation: request.operation,
            reply: Arc::new(Mutex::new(Some(reply_tx))),
        };
        if inbox.unbounded_send(command).is_err() {
            let _ = write_response(
                &mut writer,
                &ControllerResponse {
                    version: PROTOCOL_VERSION,
                    request_id: request.request_id,
                    ok: false,
                    result: None,
                    error: Some(ControllerError {
                        code: "shutting_down".to_string(),
                        message: "application is not accepting commands".to_string(),
                    }),
                },
            );
            continue;
        }
        // Quit closes the iced runtime immediately after dispatch. A direct
        // dispatch acknowledgement avoids racing the socket flush against
        // process teardown; normal operations still await their UI reply.
        if is_quit {
            let _ = write_response(
                &mut writer,
                &ControllerResponse {
                    version: PROTOCOL_VERSION,
                    request_id: request.request_id,
                    ok: true,
                    result: Some(serde_json::json!({ "dispatched": true })),
                    error: None,
                },
            );
            continue;
        }
        pending.push(PendingReply {
            request_id: request.request_id,
            receiver: reply_rx,
            deadline: Instant::now() + std::time::Duration::from_secs(30),
        });
        // Replies resolve on the iced main thread; poll them here with a
        // bounded deadline while forwarding queued events.
        loop {
            flush_events(&mut writer, event_out);
            drain_replies(&mut writer, &mut pending);
            if pending.is_empty() {
                break;
            }
            if pending.iter().all(|reply| Instant::now() >= reply.deadline) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Expired requests are reported instead of hanging the client.
        let now = Instant::now();
        let mut expired = Vec::new();
        pending.retain(|reply| {
            if now >= reply.deadline {
                expired.push(reply.request_id);
                return false;
            }
            true
        });
        for request_id in expired {
            let _ = write_response(
                &mut writer,
                &ControllerResponse {
                    version: PROTOCOL_VERSION,
                    request_id,
                    ok: false,
                    result: None,
                    error: Some(ControllerError {
                        code: "command_timeout".to_string(),
                        message: "application did not answer in time".to_string(),
                    }),
                },
            );
        }
    }
}

/// Writes every settled reply and drops it from the pending list.
fn drain_replies(writer: &mut UnixStream, pending: &mut Vec<PendingReply>) {
    let mut settled = Vec::new();
    for reply in pending.iter_mut() {
        match reply.receiver.try_recv() {
            Ok(Some(response)) => {
                let _ = write_response(writer, &response);
                settled.push(reply.request_id);
            }
            Ok(None) | Err(_) => {}
        }
    }
    pending.retain(|reply| !settled.contains(&reply.request_id));
}

fn flush_events(writer: &mut UnixStream, event_out: &Arc<Mutex<VecDeque<ControllerEvent>>>) {
    let events: Vec<ControllerEvent> = event_out
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default();
    for event in events {
        let mut body = serde_json::to_vec(&event).unwrap_or_else(|_| b"{}".to_vec());
        body.push(b'\n');
        if writer.write_all(&body).is_err() {
            break;
        }
    }
    let _ = writer.flush();
}

fn write_response(writer: &mut UnixStream, response: &ControllerResponse) -> std::io::Result<()> {
    let mut body = serde_json::to_vec(response).unwrap_or_else(|_| b"{}".to_vec());
    body.push(b'\n');
    writer.write_all(&body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production_dir() -> PathBuf {
        PathBuf::from("/tmp/jameprompt-test-production-data")
    }

    fn write_profile(dir: &Path, body: &str) -> PathBuf {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .expect("secure test profile directory");
        }
        let path = dir.join("profile.json");
        std::fs::write(&path, body).expect("write test profile");
        path
    }

    #[test]
    fn profile_rejects_production_app_id() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write_profile(
            dir.path(),
            r#"{"test_app_id":"io.github.roymejia2217.JamePrompt","data_dir":"/tmp/jameprompt-test-data-1"}"#,
        );
        let error = SyntheticProfile::load(&path, &production_dir()).expect_err("must reject");
        assert!(error.reason.contains("must differ"), "{error}");
    }

    #[test]
    fn profile_rejects_non_reverse_dns_identity() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write_profile(
            dir.path(),
            r#"{"test_app_id":"not-a-valid-id","data_dir":"/tmp/jameprompt-test-data-2"}"#,
        );
        assert!(SyntheticProfile::load(&path, &production_dir()).is_err());
    }

    #[test]
    fn profile_rejects_production_data_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let production = dir.path().join("production");
        std::fs::create_dir(&production).expect("production directory");
        let path = write_profile(
            dir.path(),
            &format!(
                "{{\"test_app_id\":\"io.github.roymejia2217.JamePrompt.Test.a1\",\"data_dir\":\"{}\"}}",
                production.display()
            ),
        );
        let error = SyntheticProfile::load(&path, &production).expect_err("must reject");
        assert!(error.reason.contains("production"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn profile_rejects_canonical_production_alias() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("private run dir");
        let production = dir.path().join("production");
        let alias = dir.path().join("production-alias");
        std::fs::create_dir(&production).expect("production directory");
        symlink(&production, &alias).expect("production alias");
        let path = write_profile(
            dir.path(),
            &format!(
                "{{\"test_app_id\":\"io.github.roymejia2217.JamePrompt.Test.alias\",\"data_dir\":\"{}\"}}",
                alias.display()
            ),
        );

        let error = SyntheticProfile::load(&path, &production).expect_err("alias must reject");
        assert!(error.reason.contains("production"), "{error}");
    }

    #[test]
    fn profile_rejects_data_and_diagnostics_outside_private_run_root() {
        let dir = tempfile::tempdir().expect("private run dir");
        let outside_data = std::env::temp_dir().join("jameprompt-profile-outside-data");
        let outside_diagnostics = std::env::temp_dir().join("jameprompt-profile-outside.jsonl");
        for body in [
            format!(
                "{{\"test_app_id\":\"io.github.roymejia2217.JamePrompt.Test.outside1\",\"data_dir\":\"{}\"}}",
                outside_data.display()
            ),
            format!(
                "{{\"test_app_id\":\"io.github.roymejia2217.JamePrompt.Test.outside2\",\"data_dir\":\"{}\",\"diagnostics_path\":\"{}\"}}",
                dir.path().join("data").display(),
                outside_diagnostics.display()
            ),
        ] {
            let path = write_profile(dir.path(), &body);
            assert!(
                SyntheticProfile::load(&path, &production_dir()).is_err(),
                "profile escaped its private root: {body}"
            );
        }
    }

    #[test]
    fn profile_rejects_enabled_autostart_and_prewarm() {
        let dir = tempfile::tempdir().expect("temp dir");
        for body in [
            r#"{"test_app_id":"io.github.roymejia2217.JamePrompt.Test.a2","data_dir":"/tmp/jameprompt-test-data-4","autostart":true}"#,
            r#"{"test_app_id":"io.github.roymejia2217.JamePrompt.Test.a3","data_dir":"/tmp/jameprompt-test-data-5","prewarm":true}"#,
        ] {
            let path = write_profile(dir.path(), body);
            assert!(SyntheticProfile::load(&path, &production_dir()).is_err());
        }
    }

    #[test]
    fn profile_rejects_unknown_fields() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write_profile(
            dir.path(),
            r#"{"test_app_id":"io.github.roymejia2217.JamePrompt.Test.a4","data_dir":"/tmp/jameprompt-test-data-6","shell":"true"}"#,
        );
        assert!(SyntheticProfile::load(&path, &production_dir()).is_err());
    }

    #[test]
    fn profile_accepts_minimal_synthetic_identity() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write_profile(
            dir.path(),
            &format!(
                "{{\"test_app_id\":\"io.github.roymejia2217.JamePrompt.Test.a5\",\"data_dir\":\"{}\"}}",
                dir.path().join("data").display()
            ),
        );
        let profile = SyntheticProfile::load(&path, &production_dir()).expect("valid profile");
        assert!(!profile.native_consent_authorized);
        assert_eq!(profile.fixture_seed, "seed");
    }

    #[test]
    fn requests_reject_unknown_operations_and_fields() {
        let bad_operation = r#"{"version":1,"request_id":1,"operation":"format_disk","args":{}}"#;
        assert!(serde_json::from_str::<ControllerRequest>(bad_operation).is_err());
        let bad_field =
            r#"{"version":1,"request_id":1,"operation":"hello","args":{"shell":"true"}}"#;
        assert!(serde_json::from_str::<ControllerRequest>(bad_field).is_err());
        // Unknown top-level fields are rejected, not ignored.
        let extra = r#"{"version":1,"request_id":1,"operation":"hello","args":{},"extra":1}"#;
        assert!(serde_json::from_str::<ControllerRequest>(extra).is_err());
    }

    #[test]
    fn requests_use_versioned_args_object() {
        let request = serde_json::from_str::<ControllerRequest>(
            r#"{"version":1,"request_id":7,"operation":"hello","args":{}}"#,
        )
        .expect("valid hello request");
        assert_eq!(request.version, PROTOCOL_VERSION);
        assert_eq!(request.request_id, 7);
        assert!(matches!(request.operation, ControllerOperation::Hello {}));
    }

    #[test]
    fn requests_reject_zero_request_ids() {
        let error = serde_json::from_str::<ControllerRequest>(
            r#"{"version":1,"request_id":0,"operation":"hello","args":{}}"#,
        )
        .expect_err("request id zero must be rejected");
        assert!(error.to_string().contains("request_id"), "{error}");
    }

    #[test]
    fn bounded_frames_require_newline_and_include_it_in_limit() {
        let mut missing_newline = std::io::Cursor::new(vec![b'x'; MAX_PACKET_BYTES]);
        assert_eq!(
            read_bounded_frame(&mut missing_newline),
            Err(FrameReadError::Unterminated)
        );

        let mut oversized = std::io::Cursor::new(vec![b'x'; MAX_PACKET_BYTES + 1]);
        assert_eq!(
            read_bounded_frame(&mut oversized),
            Err(FrameReadError::Oversized)
        );
    }

    #[test]
    fn responses_always_emit_nullable_result_and_error_fields() {
        let response = ControllerResponse {
            version: PROTOCOL_VERSION,
            request_id: 7,
            ok: true,
            result: None,
            error: None,
        };
        let value = serde_json::to_value(response).expect("response serializes");
        assert!(
            value.get("result").is_some(),
            "result must be explicit null"
        );
        assert!(value.get("error").is_some(), "error must be explicit null");
        assert!(value["result"].is_null());
        assert!(value["error"].is_null());
    }

    #[test]
    fn emits_rust_response_wire_frames() {
        let responses = [
            ControllerResponse {
                version: PROTOCOL_VERSION,
                request_id: 7,
                ok: true,
                result: Some(serde_json::json!({"ready": true})),
                error: None,
            },
            ControllerResponse {
                version: PROTOCOL_VERSION,
                request_id: 8,
                ok: false,
                result: None,
                error: Some(ControllerError {
                    code: "invalid_frame".to_string(),
                    message: "bad".to_string(),
                }),
            },
        ];

        for response in responses {
            let mut bytes = serde_json::to_vec(&response).expect("response serializes");
            bytes.push(b'\n');
            let hex = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            println!("RUST_WIRE_HEX={hex}");
        }
    }

    #[test]
    fn socket_dir_must_be_private_absolute() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("controller.sock");
        // The controller requires its parent directory to be explicitly private.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .expect("secure temporary controller directory");
            let mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        assert!(validate_socket_dir(&socket).is_ok());
        assert!(validate_socket_dir(Path::new("relative.sock")).is_err());
    }
}
