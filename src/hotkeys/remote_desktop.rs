use crate::config::{get_data_dir, LINUX_DESKTOP_APP_ID};
use gtk::glib::variant::{ObjectPath, ToVariant};
use gtk::{gio, glib};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::rc::Rc;
use std::sync::{mpsc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REGISTRY_INTERFACE: &str = "org.freedesktop.host.portal.Registry";
const REMOTE_DESKTOP_INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";
const SESSION_INTERFACE: &str = "org.freedesktop.portal.Session";
const PORTAL_CALL_TIMEOUT_MS: i32 = 5_000;
const PORTAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const PASTE_DELAY: Duration = Duration::from_millis(150);
const KEY_RELEASE_DELAY: Duration = Duration::from_millis(20);
const RETRY_COOLDOWN: Duration = Duration::from_secs(60);
const DEVICE_TYPE_KEYBOARD: u32 = 1;
const PERSIST_MODE_UNTIL_REVOKED: u32 = 2;
const KEYSYM_CONTROL_L: i32 = 0xffe3;
const KEYSYM_V: i32 = 0x0076;
const STATE_RELEASED: u32 = 0;
const STATE_PRESSED: u32 = 1;
const PERMISSION_STATE_FILE: &str = "wayland-portal.json";

static PASTE_WORKER: OnceLock<Option<mpsc::Sender<()>>> = OnceLock::new();

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct PortalPermissionState {
    #[serde(default)]
    remote_desktop_restore_token: Option<String>,
}

struct RemoteDesktopState {
    session: Option<String>,
    restore_token: Option<String>,
    retry_after: Option<Instant>,
}

impl RemoteDesktopState {
    fn load() -> Self {
        Self {
            session: None,
            restore_token: load_permission_state(&permission_state_path())
                .remote_desktop_restore_token,
            retry_after: None,
        }
    }

    fn can_retry(&self) -> bool {
        self.retry_after
            .is_none_or(|retry_after| Instant::now() >= retry_after)
    }

    fn mark_retry_cooldown(&mut self) {
        self.retry_after = Some(Instant::now() + RETRY_COOLDOWN);
    }

    fn ensure_session(
        &mut self,
        context: &glib::MainContext,
        connection: &gio::DBusConnection,
    ) -> Result<&str, String> {
        if self.session.is_none() {
            let (session, restore_token) =
                create_remote_desktop_session(context, connection, self.restore_token.as_deref())?;
            self.session = Some(session);
            self.restore_token = restore_token;
            if let Err(error) = save_restore_token(self.restore_token.as_deref()) {
                tracing::warn!("Unable to persist Wayland keyboard permission: {error}");
            }
            self.retry_after = None;
        }

        self.session
            .as_deref()
            .ok_or_else(|| "RemoteDesktop session was not created".to_string())
    }

    fn invalidate_session(&mut self, connection: &gio::DBusConnection) {
        if let Some(session) = self.session.take() {
            close_session(connection, &session);
        }
    }
}

pub(super) fn paste_to_active_window() {
    let Some(sender) = PASTE_WORKER.get_or_init(start_worker).as_ref() else {
        tracing::warn!("Wayland automatic paste worker could not be started");
        return;
    };

    if sender.send(()).is_err() {
        tracing::warn!("Wayland automatic paste worker is unavailable");
    }
}

fn start_worker() -> Option<mpsc::Sender<()>> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("jameprompt-wayland-paste".into())
        .spawn(move || remote_desktop_worker(rx))
        .ok()?;
    Some(tx)
}

fn remote_desktop_worker(rx: mpsc::Receiver<()>) {
    let context = glib::MainContext::new();
    let result = context.with_thread_default(|| {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .map_err(|error| format!("cannot connect to the session bus: {error}"))?;
        register_host_application(&connection)?;
        ensure_remote_desktop_available(&connection)?;

        let mut state = RemoteDesktopState::load();
        while rx.recv().is_ok() {
            thread::sleep(PASTE_DELAY);

            if !state.can_retry() {
                tracing::debug!(
                    "Skipping Wayland automatic paste during permission retry cooldown"
                );
                continue;
            }

            let session = match state.ensure_session(&context, &connection) {
                Ok(session) => session.to_string(),
                Err(error) => {
                    tracing::warn!("Wayland automatic paste permission unavailable: {error}");
                    state.mark_retry_cooldown();
                    continue;
                }
            };

            if let Err(error) = paste_ctrl_v(&connection, &session) {
                tracing::warn!("Wayland automatic paste failed: {error}");
                state.invalidate_session(&connection);
            }
        }

        state.invalidate_session(&connection);
        Ok::<(), String>(())
    });

    if let Err(error) = result {
        tracing::warn!("Wayland automatic paste worker stopped: {error}");
    }
}

fn register_host_application(connection: &gio::DBusConnection) -> Result<(), String> {
    let parameters =
        glib::Variant::tuple_from_iter([LINUX_DESKTOP_APP_ID.to_variant(), empty_options()]);

    match connection.call_sync(
        Some(PORTAL_BUS),
        PORTAL_PATH,
        REGISTRY_INTERFACE,
        "Register",
        Some(&parameters),
        None,
        gio::DBusCallFlags::NONE,
        PORTAL_CALL_TIMEOUT_MS,
        gio::Cancellable::NONE,
    ) {
        Ok(_) => Ok(()),
        Err(error) => {
            let message = error.to_string();
            if message.contains("UnknownMethod") || message.contains("UnknownInterface") {
                tracing::debug!(
                    "Host portal registry is unavailable for RemoteDesktop; continuing with legacy portal discovery"
                );
                Ok(())
            } else {
                Err(format!("host portal registration failed: {error}"))
            }
        }
    }
}

fn ensure_remote_desktop_available(connection: &gio::DBusConnection) -> Result<(), String> {
    let parameters = (REMOTE_DESKTOP_INTERFACE, "version").to_variant();
    connection
        .call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .map_err(|error| format!("RemoteDesktop portal is unavailable: {error}"))?;
    Ok(())
}

fn create_remote_desktop_session(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    restore_token: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let session = create_session(context, connection)?;

    if let Err(error) = select_keyboard(context, connection, &session, restore_token) {
        close_session(connection, &session);
        return Err(error);
    }

    match start_session(context, connection, &session) {
        Ok(new_restore_token) => Ok((session, new_restore_token)),
        Err(error) => {
            close_session(connection, &session);
            Err(error)
        }
    }
}

fn create_session(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
) -> Result<String, String> {
    let handle_token = portal_token("rd_create");
    let session_token = portal_token("rd_session");
    let request_path = request_path(connection, &handle_token)?;
    let options = glib::VariantDict::new(None);
    options.insert("handle_token", handle_token.as_str());
    options.insert("session_handle_token", session_token.as_str());
    let parameters = glib::Variant::tuple_from_iter([options.end()]);

    let results = portal_request(context, connection, &request_path, || {
        connection.call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            REMOTE_DESKTOP_INTERFACE,
            "CreateSession",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
    })?;

    let results = glib::VariantDict::new(Some(&results));
    results
        .lookup::<String>("session_handle")
        .map_err(|error| format!("invalid RemoteDesktop CreateSession response: {error}"))?
        .ok_or_else(|| "RemoteDesktop CreateSession did not include session_handle".to_string())
}

fn select_keyboard(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    session: &str,
    restore_token: Option<&str>,
) -> Result<(), String> {
    let handle_token = portal_token("rd_select");
    let request_path = request_path(connection, &handle_token)?;
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| format!("invalid RemoteDesktop session path: {error}"))?
        .to_variant();
    let options = glib::VariantDict::new(None);
    options.insert("handle_token", handle_token.as_str());
    options.insert("types", DEVICE_TYPE_KEYBOARD);
    options.insert("persist_mode", PERSIST_MODE_UNTIL_REVOKED);
    if let Some(token) = restore_token.filter(|token| !token.is_empty()) {
        options.insert("restore_token", token);
    }
    let parameters = glib::Variant::tuple_from_iter([session_path, options.end()]);

    portal_request(context, connection, &request_path, || {
        connection.call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            REMOTE_DESKTOP_INTERFACE,
            "SelectDevices",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
    })?;
    Ok(())
}

fn start_session(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    session: &str,
) -> Result<Option<String>, String> {
    let handle_token = portal_token("rd_start");
    let request_path = request_path(connection, &handle_token)?;
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| format!("invalid RemoteDesktop session path: {error}"))?
        .to_variant();
    let options = glib::VariantDict::new(None);
    options.insert("handle_token", handle_token.as_str());
    let parameters = glib::Variant::tuple_from_iter([session_path, "".to_variant(), options.end()]);

    let results = portal_request(context, connection, &request_path, || {
        connection.call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            REMOTE_DESKTOP_INTERFACE,
            "Start",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
    })?;

    let results = glib::VariantDict::new(Some(&results));
    let devices = results
        .lookup::<u32>("devices")
        .map_err(|error| format!("invalid RemoteDesktop device response: {error}"))?
        .unwrap_or_default();
    if devices & DEVICE_TYPE_KEYBOARD == 0 {
        return Err("RemoteDesktop session did not grant keyboard access".to_string());
    }

    results
        .lookup::<String>("restore_token")
        .map_err(|error| format!("invalid RemoteDesktop restore token: {error}"))
}

fn paste_ctrl_v(connection: &gio::DBusConnection, session: &str) -> Result<(), String> {
    notify_keysym(connection, session, KEYSYM_CONTROL_L, STATE_PRESSED)?;
    if let Err(error) = notify_keysym(connection, session, KEYSYM_V, STATE_PRESSED) {
        let _ = notify_keysym(connection, session, KEYSYM_CONTROL_L, STATE_RELEASED);
        return Err(error);
    }

    thread::sleep(KEY_RELEASE_DELAY);
    let release_v = notify_keysym(connection, session, KEYSYM_V, STATE_RELEASED);
    let release_ctrl = notify_keysym(connection, session, KEYSYM_CONTROL_L, STATE_RELEASED);
    release_v?;
    release_ctrl?;
    Ok(())
}

fn notify_keysym(
    connection: &gio::DBusConnection,
    session: &str,
    keysym: i32,
    state: u32,
) -> Result<(), String> {
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| format!("invalid RemoteDesktop session path: {error}"))?
        .to_variant();
    let parameters = glib::Variant::tuple_from_iter([
        session_path,
        empty_options(),
        keysym.to_variant(),
        state.to_variant(),
    ]);

    connection
        .call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            REMOTE_DESKTOP_INTERFACE,
            "NotifyKeyboardKeysym",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .map_err(|error| format!("NotifyKeyboardKeysym failed: {error}"))?;
    Ok(())
}

fn portal_request<F>(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    request_path: &str,
    call: F,
) -> Result<glib::Variant, String>
where
    F: FnOnce() -> Result<glib::Variant, glib::Error>,
{
    let response = Rc::new(RefCell::new(None::<(u32, glib::Variant)>));
    let response_slot = Rc::clone(&response);

    #[allow(deprecated)]
    let subscription = connection.signal_subscribe(
        Some(PORTAL_BUS),
        Some(REQUEST_INTERFACE),
        Some("Response"),
        Some(request_path),
        None,
        gio::DBusSignalFlags::NONE,
        move |_connection, _sender, _path, _interface, _signal, parameters| {
            if parameters.n_children() < 2 {
                return;
            }
            let Some(code) = parameters.child_value(0).get::<u32>() else {
                return;
            };
            let results = parameters.child_value(1);
            *response_slot.borrow_mut() = Some((code, results));
        },
    );

    let call_result = call().map_err(|error| format!("portal request call failed: {error}"));
    if let Err(error) = call_result {
        #[allow(deprecated)]
        connection.signal_unsubscribe(subscription);
        return Err(error);
    }

    let started = Instant::now();
    loop {
        while context.pending() {
            context.iteration(false);
        }

        if let Some((code, results)) = response.borrow_mut().take() {
            #[allow(deprecated)]
            connection.signal_unsubscribe(subscription);
            return match code {
                0 => Ok(results),
                1 => Err("portal request was cancelled by the user".to_string()),
                2 => Err("portal request was denied or failed".to_string()),
                other => Err(format!("portal request failed with response code {other}")),
            };
        }

        if started.elapsed() >= PORTAL_RESPONSE_TIMEOUT {
            #[allow(deprecated)]
            connection.signal_unsubscribe(subscription);
            return Err("portal request timed out".to_string());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn close_session(connection: &gio::DBusConnection, session: &str) {
    if ObjectPath::try_from(session).is_err() {
        return;
    }
    let _ = connection.call_sync(
        Some(PORTAL_BUS),
        session,
        SESSION_INTERFACE,
        "Close",
        None,
        None,
        gio::DBusCallFlags::NONE,
        PORTAL_CALL_TIMEOUT_MS,
        gio::Cancellable::NONE,
    );
}

fn empty_options() -> glib::Variant {
    glib::VariantDict::new(None).end()
}

fn request_path(connection: &gio::DBusConnection, token: &str) -> Result<String, String> {
    let unique_name = connection
        .unique_name()
        .ok_or_else(|| "session bus connection has no unique name".to_string())?;
    let sender = unique_name.trim_start_matches(':').replace('.', "_");
    Ok(format!(
        "/org/freedesktop/portal/desktop/request/{sender}/{token}"
    ))
}

fn portal_token(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

fn permission_state_path() -> std::path::PathBuf {
    get_data_dir().join(PERMISSION_STATE_FILE)
}

fn load_permission_state(path: &Path) -> PortalPermissionState {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_restore_token(restore_token: Option<&str>) -> Result<(), String> {
    let path = permission_state_path();
    let state = PortalPermissionState {
        remote_desktop_restore_token: restore_token.map(str::to_owned),
    };
    save_permission_state(&path, &state)
}

fn save_permission_state(path: &Path, state: &PortalPermissionState) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Wayland portal state path has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create Wayland portal state directory: {error}"))?;

    let temp_path = path.with_extension("json.tmp");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options
        .open(&temp_path)
        .map_err(|error| format!("cannot open Wayland portal state: {error}"))?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("cannot protect Wayland portal state: {error}"))?;

    let content = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("cannot encode Wayland portal state: {error}"))?;
    file.write_all(&content)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot write Wayland portal state: {error}"))?;
    drop(file);

    fs::rename(&temp_path, path)
        .map_err(|error| format!("cannot install Wayland portal state: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn remote_desktop_requests_keyboard_only() {
        assert_eq!(DEVICE_TYPE_KEYBOARD, 1);
    }

    #[test]
    fn remote_desktop_uses_persistent_permission_mode() {
        assert_eq!(PERSIST_MODE_UNTIL_REVOKED, 2);
    }

    #[test]
    fn ctrl_v_uses_standard_x11_keysyms_expected_by_portal() {
        assert_eq!(KEYSYM_CONTROL_L, 0xffe3);
        assert_eq!(KEYSYM_V, 'v' as i32);
    }

    #[test]
    fn permission_state_roundtrips_restore_token() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("portal.json");
        let state = PortalPermissionState {
            remote_desktop_restore_token: Some("restore-token".into()),
        };

        save_permission_state(&path, &state).expect("save permission state");
        assert_eq!(load_permission_state(&path), state);
    }

    #[cfg(unix)]
    #[test]
    fn permission_state_is_owner_only() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("portal.json");
        save_permission_state(&path, &PortalPermissionState::default())
            .expect("save permission state");

        let mode = fs::metadata(path)
            .expect("permission state metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn corrupt_permission_state_falls_back_to_default() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("portal.json");
        fs::write(&path, "not-json").expect("write corrupt state");
        assert_eq!(
            load_permission_state(&path),
            PortalPermissionState::default()
        );
    }
}
