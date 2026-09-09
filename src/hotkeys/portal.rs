use crate::config::LINUX_DESKTOP_APP_ID;
use gtk::glib::variant::{ObjectPath, ToVariant};
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
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
const MAX_PENDING_EVENTS: usize = 64;

#[derive(Debug, Clone)]
struct PortalBinding {
    shortcut_id: String,
    preferred_trigger: String,
    description: String,
}

#[derive(Default)]
struct ActiveRoutes {
    session: Option<String>,
    routes: HashMap<String, u32>,
}

fn activated_route(active: &ActiveRoutes, session: &str, shortcut_id: &str) -> Option<u32> {
    if active.session.as_deref() != Some(session) {
        return None;
    }
    active.routes.get(shortcut_id).copied()
}

fn clear_closed_session(active: &mut ActiveRoutes, session: &str) -> bool {
    if active.session.as_deref() != Some(session) {
        return false;
    }
    *active = ActiveRoutes::default();
    true
}

pub(super) struct PortalHotkeyService {
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    runtime_ids: Mutex<HashMap<String, u32>>,
    refresh_tx: mpsc::Sender<()>,
    next_id: AtomicU32,
}

impl PortalHotkeyService {
    pub(super) fn new() -> Option<Self> {
        let bindings = Arc::new(Mutex::new(HashMap::new()));
        let active_routes = Arc::new(Mutex::new(ActiveRoutes::default()));
        let (refresh_tx, refresh_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::sync_channel(1);
        let worker_bindings = Arc::clone(&bindings);
        let worker_active_routes = Arc::clone(&active_routes);

        thread::Builder::new()
            .name("jameprompt-wayland-shortcuts".into())
            .spawn(move || {
                portal_worker(worker_bindings, worker_active_routes, refresh_rx, init_tx)
            })
            .ok()?;

        match init_rx.recv_timeout(Duration::from_secs(6)) {
            Ok(Ok(())) => Some(Self {
                bindings,
                runtime_ids: Mutex::new(HashMap::new()),
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

    pub(super) fn register(
        &self,
        prompt_id: &str,
        prompt_name: &str,
        key_str: &str,
    ) -> Option<u32> {
        let prompt_id = prompt_id.trim();
        let preferred_trigger = to_portal_trigger(key_str)?;
        let shortcut_id = portal_shortcut_id(prompt_id)?;
        let id = {
            let mut runtime_ids = self.runtime_ids.lock().ok()?;
            runtime_id_for_prompt(prompt_id, &mut runtime_ids, &self.next_id)?
        };
        let binding = PortalBinding {
            shortcut_id,
            preferred_trigger,
            description: portal_description(prompt_name),
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

pub(super) fn poll_event() -> Option<u32> {
    let queue = PORTAL_EVENTS.get()?;
    queue.lock().ok()?.pop_front()
}

fn event_queue() -> &'static Mutex<VecDeque<u32>> {
    PORTAL_EVENTS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn enqueue_event(queue: &mut VecDeque<u32>, id: u32) -> bool {
    if queue.len() >= MAX_PENDING_EVENTS {
        return false;
    }
    queue.push_back(id);
    true
}

static PORTAL_EVENTS: OnceLock<Mutex<VecDeque<u32>>> = OnceLock::new();

fn portal_worker(
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    active_routes: Arc<Mutex<ActiveRoutes>>,
    refresh_rx: mpsc::Receiver<()>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
) {
    let context = glib::MainContext::new();
    let result = context.with_thread_default(|| {
        let connection = super::connection::session_connection()?;

        register_host_application(&connection)?;
        ensure_global_shortcuts_available(&connection)?;
        let (closed_tx, closed_rx) = mpsc::sync_channel(1);
        subscribe_session_closed(&connection, Arc::clone(&active_routes), closed_tx);
        subscribe_activated(&connection, Arc::clone(&active_routes));
        let _ = init_tx.send(Ok(()));

        run_worker_loop(
            &context,
            &connection,
            bindings,
            active_routes,
            refresh_rx,
            closed_rx,
        );
        Ok::<(), String>(())
    });

    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = init_tx.send(Err(error));
        }
        Err(error) => {
            let _ = init_tx.send(Err(error.to_string()));
        }
    }
}

fn run_worker_loop(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    active_routes: Arc<Mutex<ActiveRoutes>>,
    refresh_rx: mpsc::Receiver<()>,
    closed_rx: mpsc::Receiver<String>,
) {
    let mut active_session: Option<String> = None;
    let mut pending_refresh: Option<Instant> = None;

    loop {
        while context.pending() {
            context.iteration(false);
        }
        while let Ok(closed_session) = closed_rx.try_recv() {
            if active_session.as_deref() == Some(closed_session.as_str()) {
                active_session = None;
                pending_refresh = Some(Instant::now());
            }
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

            let snapshot = bindings
                .lock()
                .map(|bindings| bindings.clone())
                .unwrap_or_default();
            if snapshot.is_empty() {
                if let Some(session) = active_session.take() {
                    close_session(connection, &session);
                }
                if let Ok(mut routes) = active_routes.lock() {
                    *routes = ActiveRoutes::default();
                }
                continue;
            }

            match create_and_bind_session(context, connection, &snapshot) {
                Ok((session, accepted)) => {
                    let routes = snapshot
                        .iter()
                        .filter(|(_, binding)| accepted.contains(&binding.shortcut_id))
                        .map(|(id, binding)| (binding.shortcut_id.clone(), *id))
                        .collect();
                    if let Ok(mut active) = active_routes.lock() {
                        *active = ActiveRoutes {
                            session: Some(session.clone()),
                            routes,
                        };
                    }
                    if let Some(previous) = active_session.replace(session) {
                        close_session(connection, &previous);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        "Wayland shortcut binding failed; keeping previous bindings active: {error}"
                    );
                }
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

fn subscribe_activated(connection: &gio::DBusConnection, active_routes: Arc<Mutex<ActiveRoutes>>) {
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
            let Some(session) = parameters.child_value(0).get::<ObjectPath>() else {
                return;
            };
            let shortcut_id = parameters.child_value(1);
            let Some(shortcut_id) = shortcut_id.str() else {
                return;
            };
            let id = {
                let Ok(routes) = active_routes.lock() else {
                    return;
                };
                let Some(id) = activated_route(&routes, session.as_str(), shortcut_id) else {
                    return;
                };
                id
            };
            if let Ok(mut queue) = event_queue().lock() {
                if !enqueue_event(&mut queue, id) {
                    tracing::warn!("Wayland portal event queue is full; dropping event");
                }
            }
        },
    );
}

fn subscribe_session_closed(
    connection: &gio::DBusConnection,
    active_routes: Arc<Mutex<ActiveRoutes>>,
    closed_tx: mpsc::SyncSender<String>,
) {
    #[allow(deprecated)]
    connection.signal_subscribe(
        Some(PORTAL_BUS),
        Some(SESSION_INTERFACE),
        Some("Closed"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |_connection, _sender, path, _interface, _signal, _parameters| {
            let session = path;
            let was_current = active_routes
                .lock()
                .map(|mut routes| clear_closed_session(&mut routes, session))
                .unwrap_or(false);
            if was_current {
                let _ = closed_tx.try_send(session.to_string());
            }
        },
    );
}

fn create_and_bind_session(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    bindings: &HashMap<u32, PortalBinding>,
) -> Result<(String, HashSet<String>), String> {
    let session = create_session(context, connection)?;
    match bind_shortcuts(context, connection, &session, bindings) {
        Ok(accepted) => Ok((session, accepted)),
        Err(error) => {
            close_session(connection, &session);
            Err(error)
        }
    }
}

fn bound_shortcut_ids(results: &glib::Variant) -> Result<HashSet<String>, String> {
    let results = glib::VariantDict::new(Some(results));
    let shortcuts = results
        .lookup_value("shortcuts", None)
        .ok_or_else(|| "BindShortcuts response did not include shortcuts".to_string())?;
    if shortcuts.type_().as_str() != "a(sa{sv})" {
        return Err("BindShortcuts response shortcuts had invalid type".to_string());
    }
    let mut accepted = HashSet::new();
    for index in 0..shortcuts.n_children() {
        let entry = shortcuts.child_value(index);
        if entry.n_children() != 2 {
            return Err("BindShortcuts response contained an invalid shortcut".to_string());
        }
        let id = entry
            .child_value(0)
            .get::<String>()
            .ok_or_else(|| "BindShortcuts response shortcut id was invalid".to_string())?;
        accepted.insert(id);
    }
    Ok(accepted)
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
) -> Result<HashSet<String>, String> {
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

    let results = portal_request(context, connection, &request_path, || {
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
    bound_shortcut_ids(&results)
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
            binding.shortcut_id.to_variant(),
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

fn stable_hash(seed: u64, values: &[&str]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = seed;
    for value in values {
        for byte in value.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn runtime_id_for_prompt(
    prompt_id: &str,
    runtime_ids: &mut HashMap<String, u32>,
    next_id: &AtomicU32,
) -> Option<u32> {
    let prompt_id = prompt_id.trim();
    if prompt_id.is_empty() {
        return None;
    }
    if let Some(id) = runtime_ids.get(prompt_id) {
        return Some(*id);
    }

    let id = next_id.fetch_add(1, Ordering::Relaxed);
    runtime_ids.insert(prompt_id.to_string(), id);
    Some(id)
}

fn portal_shortcut_id(prompt_id: &str) -> Option<String> {
    let prompt_id = prompt_id.trim();
    if prompt_id.is_empty() {
        return None;
    }
    let values = [prompt_id];
    let primary = stable_hash(0xcbf2_9ce4_8422_2325, &values);
    let secondary = stable_hash(0x8422_2325_cbf2_9ce4, &values);
    Some(format!("prompt_{primary:016x}{secondary:016x}"))
}

fn portal_description(prompt_name: &str) -> String {
    const MAX_LABEL_CHARS: usize = 64;
    let normalized = prompt_name.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = if normalized.is_empty() {
        "Untitled prompt".to_string()
    } else {
        normalized
    };

    let mut chars = normalized.chars();
    let mut label: String = chars.by_ref().take(MAX_LABEL_CHARS).collect();
    if chars.next().is_some() {
        label.push('…');
    }
    format!("Paste prompt: {label}")
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
    fn initialization_reports_bus_connection_error() {
        const CHILD: &str = "JAME_PROMPT_TEST_BUS_FAILURE";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().expect("isolated bus directory");
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "hotkeys::portal::tests::initialization_reports_bus_connection_error",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(
                    "DBUS_SESSION_BUS_ADDRESS",
                    format!(
                        "unix:path={}",
                        directory.path().join("absent-bus").display()
                    ),
                )
                .output()
                .expect("run isolated initialization test");
            assert!(
                output.status.success(),
                "isolated worker failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let (_refresh_tx, refresh_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::sync_channel(1);
        portal_worker(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(ActiveRoutes::default())),
            refresh_rx,
            init_tx,
        );
        let result = init_rx
            .try_recv()
            .expect("worker must report its initialization error");
        let error = result.expect_err("missing bus must fail initialization");
        assert!(
            error.contains("cannot connect to the session bus"),
            "{error}"
        );
    }

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
    fn shortcut_id_is_stable_for_prompt_identity() {
        let original = portal_shortcut_id("prompt-123");
        assert_eq!(original, portal_shortcut_id("prompt-123"));
        assert_eq!(original, portal_shortcut_id("  prompt-123  "));
        assert_ne!(original, portal_shortcut_id("prompt-456"));
    }

    #[test]
    fn runtime_id_is_reused_for_prompt_edits() {
        let mut runtime_ids = HashMap::new();
        let next_id = AtomicU32::new(1);
        let original = runtime_id_for_prompt("prompt-123", &mut runtime_ids, &next_id);

        assert_eq!(
            original,
            runtime_id_for_prompt("  prompt-123  ", &mut runtime_ids, &next_id)
        );
        assert_ne!(
            original,
            runtime_id_for_prompt("prompt-456", &mut runtime_ids, &next_id)
        );
        assert_eq!(runtime_ids.len(), 2);
    }

    #[test]
    fn portal_description_identifies_the_prompt_without_duplicating_the_trigger() {
        let description = portal_description("  continuar-proyecto  ");
        assert_eq!(description, "Paste prompt: continuar-proyecto");
        assert!(!description.contains("Shift+Alt+C"));
    }

    #[test]
    fn portal_shortcut_requires_persistent_prompt_identity() {
        assert_eq!(portal_shortcut_id(""), None);
        assert_eq!(portal_shortcut_id("   "), None);
    }

    #[test]
    fn shortcut_array_has_portal_signature() {
        let bindings = HashMap::from([(
            7,
            PortalBinding {
                shortcut_id: "prompt_test".into(),
                preferred_trigger: "CTRL+SHIFT+p".into(),
                description: "Paste prompt: Test".into(),
            },
        )]);
        let variant = shortcuts_variant(&bindings).expect("shortcut variant");
        assert_eq!(variant.type_().as_str(), "a(sa{sv})");
    }

    #[test]
    fn polling_one_event_preserves_the_next_event() {
        let queue = event_queue();
        let mut queue = queue.lock().unwrap();
        queue.clear();
        queue.push_back(7);
        queue.push_back(9);
        drop(queue);
        assert_eq!(poll_event(), Some(7));
        assert_eq!(poll_event(), Some(9));
        assert_eq!(poll_event(), None);
    }

    #[test]
    fn bound_shortcut_ids_uses_only_portal_accepted_subset() {
        let first = glib::Variant::tuple_from_iter([
            "prompt_a".to_variant(),
            glib::VariantDict::new(None).end(),
        ]);
        let second = glib::Variant::tuple_from_iter([
            "prompt_b".to_variant(),
            glib::VariantDict::new(None).end(),
        ]);
        let shortcuts = glib::Variant::array_from_iter_with_type(first.type_(), [&first, &second]);
        let response = glib::VariantDict::new(None);
        response.insert_value("shortcuts", &shortcuts);
        assert_eq!(
            bound_shortcut_ids(&response.end()).unwrap(),
            std::collections::HashSet::from(["prompt_a".to_string(), "prompt_b".to_string()])
        );
    }

    #[test]
    fn bound_shortcut_ids_accepts_empty_subset() {
        let template = glib::Variant::tuple_from_iter([
            "template".to_variant(),
            glib::VariantDict::new(None).end(),
        ]);
        let shortcuts = glib::Variant::array_from_iter_with_type(
            template.type_(),
            std::iter::empty::<&glib::Variant>(),
        );
        let response = glib::VariantDict::new(None);
        response.insert_value("shortcuts", &shortcuts);
        assert!(bound_shortcut_ids(&response.end()).unwrap().is_empty());
    }

    #[test]
    fn enqueue_event_rejects_65th_and_preserves_fifo() {
        let mut queue = VecDeque::new();
        for id in 0..MAX_PENDING_EVENTS as u32 {
            assert!(enqueue_event(&mut queue, id));
        }
        assert!(!enqueue_event(&mut queue, 64));
        assert_eq!(
            queue.iter().copied().collect::<Vec<_>>(),
            (0..64).collect::<Vec<_>>()
        );
    }

    #[test]
    fn enqueue_event_accepts_after_pop_and_appends() {
        let mut queue = VecDeque::new();
        for id in 0..MAX_PENDING_EVENTS as u32 {
            assert!(enqueue_event(&mut queue, id));
        }
        assert_eq!(queue.pop_front(), Some(0));
        assert!(enqueue_event(&mut queue, 64));
        assert_eq!(queue.back(), Some(&64));
    }

    #[test]
    fn activated_route_requires_exact_current_session() {
        let active = ActiveRoutes {
            session: Some("/org/freedesktop/portal/desktop/session/test/current".into()),
            routes: HashMap::from([("prompt_a".into(), 7)]),
        };
        assert_eq!(
            activated_route(
                &active,
                "/org/freedesktop/portal/desktop/session/test/current",
                "prompt_a"
            ),
            Some(7)
        );
        assert_eq!(
            activated_route(
                &active,
                "/org/freedesktop/portal/desktop/session/test/old",
                "prompt_a"
            ),
            None
        );
        assert_eq!(
            activated_route(
                &active,
                "/org/freedesktop/portal/desktop/session/test/current",
                "prompt_missing"
            ),
            None
        );
    }

    #[test]
    fn closed_session_clears_only_matching_routes() {
        let mut active = ActiveRoutes {
            session: Some("/org/freedesktop/portal/desktop/session/test/current".into()),
            routes: HashMap::from([("prompt_a".into(), 7)]),
        };
        assert!(!clear_closed_session(
            &mut active,
            "/org/freedesktop/portal/desktop/session/test/old"
        ));
        assert_eq!(
            activated_route(
                &active,
                "/org/freedesktop/portal/desktop/session/test/current",
                "prompt_a"
            ),
            Some(7)
        );
        assert!(clear_closed_session(
            &mut active,
            "/org/freedesktop/portal/desktop/session/test/current"
        ));
        assert!(active.session.is_none());
        assert!(active.routes.is_empty());
    }
}
