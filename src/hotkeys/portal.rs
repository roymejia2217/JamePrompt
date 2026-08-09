use crate::config::LINUX_DESKTOP_APP_ID;
use gtk::glib::variant::{ObjectPath, ToVariant};
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REGISTRY_INTERFACE: &str = "org.freedesktop.host.portal.Registry";
const GLOBAL_SHORTCUTS_INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";
const SESSION_INTERFACE: &str = "org.freedesktop.portal.Session";
const PORTAL_CALL_TIMEOUT_MS: i32 = 5_000;
const PORTAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const REBIND_DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
struct PortalBinding {
    preferred_trigger: String,
    description: String,
}

pub(super) struct PortalHotkeyService {
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    refresh_tx: mpsc::Sender<()>,
    next_id: AtomicU32,
}

impl PortalHotkeyService {
    pub(super) fn new() -> Option<Self> {
        let bindings = Arc::new(Mutex::new(HashMap::new()));
        let (refresh_tx, refresh_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::sync_channel(1);
        let worker_bindings = Arc::clone(&bindings);

        thread::Builder::new()
            .name("jameprompt-wayland-shortcuts".into())
            .spawn(move || portal_worker(worker_bindings, refresh_rx, init_tx))
            .ok()?;

        match init_rx.recv_timeout(Duration::from_secs(6)) {
            Ok(Ok(())) => Some(Self {
                bindings,
                refresh_tx,
                next_id: AtomicU32::new(1),
            }),
            Ok(Err(error)) => {
                tracing::warn!("Wayland global shortcuts unavailable: {error}");
                None
            }
            Err(error) => {
                tracing::warn!("Wayland global shortcuts initialization timed out: {error}");
                None
            }
        }
    }

    pub(super) fn register(&self, key_str: &str) -> Option<u32> {
        let preferred_trigger = to_portal_trigger(key_str)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let binding = PortalBinding {
            preferred_trigger,
            description: format!("Paste JamePrompt prompt ({key_str})"),
        };

        self.bindings.lock().ok()?.insert(id, binding);
        if self.refresh_tx.send(()).is_err() {
            if let Ok(mut bindings) = self.bindings.lock() {
                bindings.remove(&id);
            }
            return None;
        }
        Some(id)
    }

    pub(super) fn unregister(&self, hotkey_id: u32) -> bool {
        let removed = self
            .bindings
            .lock()
            .ok()
            .and_then(|mut bindings| bindings.remove(&hotkey_id))
            .is_some();
        if removed {
            let _ = self.refresh_tx.send(());
        }
        removed
    }
}

pub(super) fn poll_events() -> Vec<u32> {
    let Some(queue) = PORTAL_EVENTS.get() else {
        return Vec::new();
    };
    let Ok(mut queue) = queue.lock() else {
        return Vec::new();
    };
    queue.drain(..).collect()
}

fn event_queue() -> &'static Mutex<VecDeque<u32>> {
    PORTAL_EVENTS.get_or_init(|| Mutex::new(VecDeque::new()))
}

static PORTAL_EVENTS: OnceLock<Mutex<VecDeque<u32>>> = OnceLock::new();

fn portal_worker(
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    refresh_rx: mpsc::Receiver<()>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
) {
    let context = glib::MainContext::new();
    let result = context.with_thread_default(|| {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .map_err(|error| format!("cannot connect to the session bus: {error}"))?;

        register_host_application(&connection)?;
        ensure_global_shortcuts_available(&connection)?;
        subscribe_activated(&connection);
        let _ = init_tx.send(Ok(()));

        run_worker_loop(&context, &connection, bindings, refresh_rx);
        Ok::<(), String>(())
    });

    if let Err(error) = result {
        let _ = init_tx.send(Err(error.to_string()));
    }
}

fn run_worker_loop(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    refresh_rx: mpsc::Receiver<()>,
) {
    let mut active_session: Option<String> = None;
    let mut pending_refresh: Option<Instant> = None;

    loop {
        while context.pending() {
            context.iteration(false);
        }

        match refresh_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(()) => pending_refresh = Some(Instant::now()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(session) = active_session.take() {
                    close_session(connection, &session);
                }
                return;
            }
        }

        if pending_refresh.is_some_and(|started| started.elapsed() >= REBIND_DEBOUNCE) {
            pending_refresh = None;
            if let Some(session) = active_session.take() {
                close_session(connection, &session);
            }

            let snapshot = bindings
                .lock()
                .map(|bindings| bindings.clone())
                .unwrap_or_default();
            if snapshot.is_empty() {
                continue;
            }

            match create_and_bind_session(context, connection, &snapshot) {
                Ok(session) => active_session = Some(session),
                Err(error) => tracing::warn!("Wayland shortcut binding failed: {error}"),
            }
        }
    }
}

fn register_host_application(connection: &gio::DBusConnection) -> Result<(), String> {
    let options = empty_options();
    let parameters = glib::Variant::tuple_from_iter([LINUX_DESKTOP_APP_ID.to_variant(), options]);

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
                    "Host portal registry is unavailable; continuing with legacy portal discovery"
                );
                Ok(())
            } else {
                Err(format!("host portal registration failed: {error}"))
            }
        }
    }
}

fn ensure_global_shortcuts_available(connection: &gio::DBusConnection) -> Result<(), String> {
    let parameters = (GLOBAL_SHORTCUTS_INTERFACE, "version").to_variant();
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
        .map_err(|error| format!("GlobalShortcuts portal is unavailable: {error}"))?;
    Ok(())
}

fn subscribe_activated(connection: &gio::DBusConnection) {
    #[allow(deprecated)]
    connection.signal_subscribe(
        Some(PORTAL_BUS),
        Some(GLOBAL_SHORTCUTS_INTERFACE),
        Some("Activated"),
        Some(PORTAL_PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |_connection, _sender, _path, _interface, _signal, parameters| {
            if parameters.n_children() < 2 {
                return;
            }
            let shortcut_id = parameters.child_value(1);
            let Some(shortcut_id) = shortcut_id.str() else {
                return;
            };
            let Some(id) = shortcut_id
                .strip_prefix("jameprompt-")
                .and_then(|value| value.parse::<u32>().ok())
            else {
                return;
            };
            if let Ok(mut queue) = event_queue().lock() {
                queue.push_back(id);
            }
        },
    );
}

fn create_and_bind_session(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    bindings: &HashMap<u32, PortalBinding>,
) -> Result<String, String> {
    let session = create_session(context, connection)?;
    if let Err(error) = bind_shortcuts(context, connection, &session, bindings) {
        close_session(connection, &session);
        return Err(error);
    }
    Ok(session)
}

fn create_session(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
) -> Result<String, String> {
    let handle_token = portal_token("create");
    let session_token = portal_token("session");
    let request_path = request_path(connection, &handle_token)?;
    let options = glib::VariantDict::new(None);
    options.insert("handle_token", handle_token.as_str());
    options.insert("session_handle_token", session_token.as_str());
    let parameters = glib::Variant::tuple_from_iter([options.end()]);

    let results = portal_request(context, connection, &request_path, || {
        connection.call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            GLOBAL_SHORTCUTS_INTERFACE,
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
        .map_err(|error| format!("invalid CreateSession response: {error}"))?
        .ok_or_else(|| "CreateSession response did not include session_handle".to_string())
}

fn bind_shortcuts(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    session: &str,
    bindings: &HashMap<u32, PortalBinding>,
) -> Result<(), String> {
    let handle_token = portal_token("bind");
    let request_path = request_path(connection, &handle_token)?;
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| format!("invalid portal session path: {error}"))?
        .to_variant();
    let shortcuts = shortcuts_variant(bindings)?;
    let options = glib::VariantDict::new(None);
    options.insert("handle_token", handle_token.as_str());
    let parameters =
        glib::Variant::tuple_from_iter([session_path, shortcuts, "".to_variant(), options.end()]);

    portal_request(context, connection, &request_path, || {
        connection.call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            GLOBAL_SHORTCUTS_INTERFACE,
            "BindShortcuts",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
    })?;
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

fn shortcuts_variant(bindings: &HashMap<u32, PortalBinding>) -> Result<glib::Variant, String> {
    let mut ids: Vec<u32> = bindings.keys().copied().collect();
    ids.sort_unstable();
    let mut entries = Vec::with_capacity(ids.len());

    for id in ids {
        let binding = bindings
            .get(&id)
            .ok_or_else(|| format!("missing portal binding {id}"))?;
        let properties = glib::VariantDict::new(None);
        properties.insert("description", binding.description.as_str());
        properties.insert("preferred_trigger", binding.preferred_trigger.as_str());
        entries.push(glib::Variant::tuple_from_iter([
            format!("jameprompt-{id}").to_variant(),
            properties.end(),
        ]));
    }

    let Some(first) = entries.first() else {
        return Err("cannot bind an empty shortcut set".to_string());
    };
    Ok(glib::Variant::array_from_iter_with_type(
        first.type_(),
        entries.iter(),
    ))
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

pub(super) fn to_portal_trigger(hotkey: &str) -> Option<String> {
    let mut parts: Vec<&str> = hotkey.split('+').collect();
    if parts.len() < 2 {
        return None;
    }
    let key = parts.pop()?;
    let mut converted = Vec::with_capacity(parts.len() + 1);
    for modifier in parts {
        converted.push(match modifier {
            "Ctrl" => "CTRL".to_string(),
            "Shift" => "SHIFT".to_string(),
            "Alt" => "ALT".to_string(),
            "Super" => "LOGO".to_string(),
            _ => return None,
        });
    }

    let key = match key {
        "Enter" => "Return".to_string(),
        "Backspace" => "BackSpace".to_string(),
        "ArrowUp" => "Up".to_string(),
        "ArrowDown" => "Down".to_string(),
        "ArrowLeft" => "Left".to_string(),
        "ArrowRight" => "Right".to_string(),
        "Space" => "space".to_string(),
        "ContextMenu" => "Menu".to_string(),
        "Unknown" | "CapsLock" | "NumLock" | "ScrollLock" => return None,
        value if value.len() == 1 => value.to_ascii_lowercase(),
        value if value.chars().all(|ch| ch.is_ascii_alphanumeric()) => value.to_string(),
        _ => return None,
    };
    converted.push(key);
    Some(converted.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_trigger_uses_xdg_modifier_names() {
        assert_eq!(
            to_portal_trigger("Ctrl+Shift+P"),
            Some("CTRL+SHIFT+p".to_string())
        );
        assert_eq!(
            to_portal_trigger("Super+Alt+F8"),
            Some("LOGO+ALT+F8".to_string())
        );
    }

    #[test]
    fn portal_trigger_maps_xkb_key_names() {
        assert_eq!(
            to_portal_trigger("Ctrl+Enter"),
            Some("CTRL+Return".to_string())
        );
        assert_eq!(
            to_portal_trigger("Ctrl+ArrowLeft"),
            Some("CTRL+Left".to_string())
        );
        assert_eq!(
            to_portal_trigger("Alt+Space"),
            Some("ALT+space".to_string())
        );
    }

    #[test]
    fn portal_trigger_rejects_invalid_or_unsupported_values() {
        assert_eq!(to_portal_trigger("P"), None);
        assert_eq!(to_portal_trigger("Hyper+P"), None);
        assert_eq!(to_portal_trigger("Ctrl+Unknown"), None);
    }

    #[test]
    fn portal_token_is_valid_object_path_element() {
        let token = portal_token("test");
        assert!(token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_'));
    }

    #[test]
    fn shortcut_array_has_portal_signature() {
        let bindings = HashMap::from([(
            7,
            PortalBinding {
                preferred_trigger: "CTRL+SHIFT+p".into(),
                description: "Paste prompt".into(),
            },
        )]);
        let variant = shortcuts_variant(&bindings).expect("shortcut variant");
        assert_eq!(variant.type_().as_str(), "a(sa{sv})");
    }
}
