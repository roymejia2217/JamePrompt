use super::clipboard_transfer;
use super::PasteOutcome;
use crate::config::{get_data_dir, LINUX_DESKTOP_APP_ID};
use gtk::glib::variant::{ObjectPath, ToVariant};
use gtk::{gio, glib};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REGISTRY_INTERFACE: &str = "org.freedesktop.host.portal.Registry";
const REMOTE_DESKTOP_INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
const CLIPBOARD_INTERFACE: &str = "org.freedesktop.portal.Clipboard";
const CLIPBOARD_MIME_UTF8: &str = "text/plain;charset=utf-8";
const CLIPBOARD_MIME_TEXT: &str = "text/plain";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";
const SESSION_INTERFACE: &str = "org.freedesktop.portal.Session";
const PORTAL_CALL_TIMEOUT_MS: i32 = 5_000;
const PORTAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const PASTE_DELAY: Duration = Duration::from_millis(150);
const CLIPBOARD_TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);
const CLIPBOARD_OFFER_TIMEOUT: Duration = Duration::from_secs(5);
const KEY_RELEASE_DELAY: Duration = Duration::from_millis(20);
const RETRY_COOLDOWN: Duration = Duration::from_secs(60);
const DEVICE_TYPE_KEYBOARD: u32 = 1;
const PERSIST_MODE_UNTIL_REVOKED: u32 = 2;
const KEYSYM_CONTROL_L: i32 = 0xffe3;
const KEYSYM_V: i32 = 0x0076;
const STATE_RELEASED: u32 = 0;
const STATE_PRESSED: u32 = 1;
const PERMISSION_STATE_FILE: &str = "wayland-portal.json";

#[derive(Clone)]
struct ClipboardOffer {
    session: String,
    bytes: Arc<[u8]>,
    generation: u64,
    transferred: bool,
}

fn transfer_snapshot(
    offer: Option<&ClipboardOffer>,
    session: &str,
    mime_type: &str,
) -> Option<Arc<[u8]>> {
    let offer = offer?;
    if offer.session != session || !matches!(mime_type, CLIPBOARD_MIME_UTF8 | CLIPBOARD_MIME_TEXT) {
        return None;
    }
    Some(Arc::clone(&offer.bytes))
}

fn mark_offer_transferred(offer: &mut Option<ClipboardOffer>, session: &str) {
    if let Some(offer) = offer.as_mut().filter(|offer| offer.session == session) {
        offer.transferred = true;
    }
}

fn take_expired_offer(
    offer: &mut Option<ClipboardOffer>,
    session: &str,
    generation: u64,
) -> Option<ClipboardOffer> {
    if offer
        .as_ref()
        .is_some_and(|offer| offer.session == session && offer.generation == generation)
    {
        return offer.take();
    }
    None
}

fn schedule_offer_timeout(
    context: &glib::MainContext,
    timeout_tx: mpsc::Sender<(String, u64)>,
    session: String,
    generation: u64,
    timeout: Duration,
) {
    let source = glib::timeout_source_new(
        timeout,
        Some("jameprompt-clipboard-offer-timeout"),
        glib::Priority::DEFAULT,
        move || {
            let _ = timeout_tx.send((session.clone(), generation));
            glib::ControlFlow::Break
        },
    );
    source.attach(Some(context));
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkerCommand {
    Prewarm,
    Paste(Arc<[u8]>),
}

static PERMISSION_DENIED: AtomicBool = AtomicBool::new(false);
static PASTE_PENDING: AtomicBool = AtomicBool::new(false);
const MAX_PENDING_PASTE_OUTCOMES: usize = 8;
static PASTE_OUTCOMES: OnceLock<Mutex<VecDeque<PasteOutcome>>> = OnceLock::new();

fn paste_outcomes() -> &'static Mutex<VecDeque<PasteOutcome>> {
    PASTE_OUTCOMES.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn set_permission_denied(denied: bool) {
    PERMISSION_DENIED.store(denied, Ordering::Relaxed);
}

fn try_begin_paste() -> bool {
    PASTE_PENDING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

fn finish_paste() {
    PASTE_PENDING.store(false, Ordering::Release);
}

fn enqueue_paste_outcome(queue: &mut VecDeque<PasteOutcome>, outcome: PasteOutcome) {
    if queue.len() >= MAX_PENDING_PASTE_OUTCOMES {
        queue.pop_front();
    }
    queue.push_back(outcome);
}

fn finish_paste_with(outcome: PasteOutcome) {
    if PASTE_PENDING.swap(false, Ordering::AcqRel) {
        if let Ok(mut queue) = paste_outcomes().lock() {
            enqueue_paste_outcome(&mut queue, outcome);
        }
    }
}

pub(super) fn poll_paste_outcome() -> Option<PasteOutcome> {
    paste_outcomes().lock().ok()?.pop_front()
}

pub(crate) fn is_permission_denied() -> bool {
    PERMISSION_DENIED.load(Ordering::Relaxed)
}

static PASTE_WORKER: OnceLock<Mutex<Option<mpsc::Sender<WorkerCommand>>>> = OnceLock::new();

fn worker_slot() -> &'static Mutex<Option<mpsc::Sender<WorkerCommand>>> {
    PASTE_WORKER.get_or_init(|| Mutex::new(None))
}

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

fn send_command(command: WorkerCommand) -> bool {
    let slot = worker_slot();
    let mut sender_guard = match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if let Some(sender) = sender_guard.as_ref() {
        if sender.send(command.clone()).is_ok() {
            return true;
        }
        tracing::warn!("Wayland automatic paste worker was disconnected; restarting worker");
        *sender_guard = None;
    }

    match start_worker() {
        Some(sender) => {
            if let Err(error) = sender.send(command) {
                set_permission_denied(true);
                tracing::warn!("Wayland automatic paste worker is unavailable: {error}");
                *sender_guard = None;
                false
            } else {
                *sender_guard = Some(sender);
                true
            }
        }
        None => {
            set_permission_denied(true);
            tracing::warn!("Wayland automatic paste worker could not be started");
            false
        }
    }
}

pub fn prewarm_permission() {
    let _ = send_command(WorkerCommand::Prewarm);
}

pub(super) fn paste_to_active_window(content: String) -> bool {
    if !try_begin_paste() {
        return false;
    }
    if send_command(WorkerCommand::Paste(Arc::from(content.into_bytes()))) {
        true
    } else {
        finish_paste();
        false
    }
}

fn start_worker() -> Option<mpsc::Sender<WorkerCommand>> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("jameprompt-wayland-paste".into())
        .spawn(move || remote_desktop_worker(rx))
        .ok()?;
    Some(tx)
}

fn remote_desktop_worker(rx: mpsc::Receiver<WorkerCommand>) {
    let context = glib::MainContext::new();
    let result = context.with_thread_default(|| {
        let connection = super::connection::session_connection()?;
        register_host_application(&connection)?;
        ensure_remote_desktop_available(&connection)?;
        let offer = Rc::new(RefCell::new(None::<ClipboardOffer>));
        let (offer_timeout_tx, offer_timeout_rx) = mpsc::channel::<(String, u64)>();
        let _selection_transfer = subscribe_selection_transfer(&connection, Rc::clone(&offer));

        let mut state = RemoteDesktopState::load();
        let mut next_offer_generation = 1u64;
        loop {
            while context.pending() {
                context.iteration(false);
            }
            while let Ok((expired_session, generation)) = offer_timeout_rx.try_recv() {
                if let Some(expired_offer) =
                    take_expired_offer(&mut offer.borrow_mut(), &expired_session, generation)
                {
                    finish_paste_with(if expired_offer.transferred {
                        PasteOutcome::ClipboardTransferred
                    } else {
                        PasteOutcome::Failed
                    });
                }
            }
            let command = match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            match command {
                WorkerCommand::Prewarm => {
                    if !state.can_retry() {
                        tracing::debug!(
                            "Skipping Wayland RemoteDesktop prewarm during permission retry cooldown"
                        );
                        continue;
                    }

                    match state.ensure_session(&context, &connection) {
                        Ok(_) => {
                            set_permission_denied(false);
                            tracing::info!("Wayland RemoteDesktop keyboard permission prewarmed");
                        }
                        Err(error) => {
                            set_permission_denied(true);
                            tracing::warn!(
                                "Wayland RemoteDesktop prewarm permission unavailable: {error}"
                            );
                            state.mark_retry_cooldown();
                        }
                    }
                }
                WorkerCommand::Paste(snapshot) => {
                    if !state.can_retry() {
                        finish_paste_with(PasteOutcome::Failed);
                        set_permission_denied(true);
                        tracing::debug!(
                            "Skipping Wayland automatic paste during permission retry cooldown"
                        );
                        continue;
                    }

                    let session = match state.ensure_session(&context, &connection) {
                        Ok(session) => {
                            set_permission_denied(false);
                            session.to_string()
                        }
                        Err(error) => {
                            finish_paste_with(PasteOutcome::Failed);
                            set_permission_denied(true);
                            tracing::warn!("Wayland automatic paste permission unavailable: {error}");
                            state.mark_retry_cooldown();
                            continue;
                        }
                    };

                    *offer.borrow_mut() = Some(ClipboardOffer {
                        session: session.clone(),
                        bytes: snapshot,
                        generation: next_offer_generation,
                        transferred: false,
                    });
                    next_offer_generation = next_offer_generation.checked_add(1).unwrap_or(1);

                    if let Err(error) = set_clipboard_selection(&connection, &session) {
                        finish_paste_with(PasteOutcome::Failed);
                        set_permission_denied(true);
                        tracing::warn!("Wayland clipboard selection failed: {error}");
                        offer.borrow_mut().take();
                        state.invalidate_session(&connection);
                        continue;
                    }

                    thread::sleep(PASTE_DELAY);

                    if let Err(error) = paste_ctrl_v(&connection, &session) {
                        offer.borrow_mut().take();
                        finish_paste_with(PasteOutcome::Failed);
                        set_permission_denied(true);
                        tracing::warn!("Wayland automatic paste failed: {error}");
                        state.invalidate_session(&connection);
                    } else {
                        let timeout_generation = offer
                            .borrow()
                            .as_ref()
                            .map(|offer| offer.generation)
                            .unwrap_or(0);
                        schedule_offer_timeout(
                            &context,
                            offer_timeout_tx.clone(),
                            session.clone(),
                            timeout_generation,
                            CLIPBOARD_OFFER_TIMEOUT,
                        );
                    }
                }
            }
        }

        state.invalidate_session(&connection);
        Ok::<(), String>(())
    });

    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            finish_paste_with(PasteOutcome::Failed);
            set_permission_denied(true);
            tracing::warn!("Wayland automatic paste worker stopped: {error}");
        }
        Err(error) => {
            finish_paste_with(PasteOutcome::Failed);
            set_permission_denied(true);
            tracing::warn!("Wayland automatic paste worker stopped: {error}");
        }
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

    if let Err(error) = request_clipboard(connection, &session) {
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

fn request_clipboard(connection: &gio::DBusConnection, session: &str) -> Result<(), String> {
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| format!("invalid RemoteDesktop session path: {error}"))?
        .to_variant();
    let parameters = glib::Variant::tuple_from_iter([session_path, empty_options()]);
    connection
        .call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            CLIPBOARD_INTERFACE,
            "RequestClipboard",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .map_err(|error| format!("clipboard access request failed: {error}"))?;
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

    parse_start_response(&results)
}

fn parse_start_response(results: &glib::Variant) -> Result<Option<String>, String> {
    let results = glib::VariantDict::new(Some(results));
    let devices = results
        .lookup::<u32>("devices")
        .map_err(|error| format!("invalid RemoteDesktop device response: {error}"))?
        .unwrap_or_default();
    if devices & DEVICE_TYPE_KEYBOARD == 0 {
        return Err("RemoteDesktop session did not grant keyboard access".to_string());
    }

    let clipboard_enabled = results
        .lookup::<bool>("clipboard_enabled")
        .map_err(|error| format!("invalid RemoteDesktop clipboard response: {error}"))?
        .unwrap_or(false);
    if !clipboard_enabled {
        return Err("RemoteDesktop session did not grant clipboard access".to_string());
    }

    results
        .lookup::<String>("restore_token")
        .map_err(|error| format!("invalid RemoteDesktop restore token: {error}"))
}

fn set_clipboard_selection(connection: &gio::DBusConnection, session: &str) -> Result<(), String> {
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| format!("invalid RemoteDesktop session path: {error}"))?
        .to_variant();
    let options = glib::VariantDict::new(None);
    let mime_types = glib::Variant::array_from_iter::<String>([
        CLIPBOARD_MIME_UTF8.to_variant(),
        CLIPBOARD_MIME_TEXT.to_variant(),
    ]);
    options.insert_value("mime_types", &mime_types);
    let parameters = glib::Variant::tuple_from_iter([session_path, options.end()]);
    connection
        .call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            CLIPBOARD_INTERFACE,
            "SetSelection",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .map_err(|error| format!("clipboard selection failed: {error}"))?;
    Ok(())
}

fn subscribe_selection_transfer(
    connection: &gio::DBusConnection,
    offer: Rc<RefCell<Option<ClipboardOffer>>>,
) -> gio::SignalSubscriptionId {
    #[allow(deprecated)]
    connection.signal_subscribe(
        Some(PORTAL_BUS),
        Some(CLIPBOARD_INTERFACE),
        Some("SelectionTransfer"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |connection, _sender, _path, _interface, _signal, parameters| {
            let Some((session_path, mime_type, serial)) =
                parameters.get::<(ObjectPath, String, u32)>()
            else {
                tracing::warn!("Wayland clipboard transfer signal had invalid parameters");
                return;
            };
            let session = session_path.as_str();
            let snapshot = transfer_snapshot(offer.borrow().as_ref(), session, &mime_type);
            let Some(snapshot) = snapshot else {
                let _ = selection_write_done(connection, session, serial, false);
                return;
            };
            let fd = match selection_write(connection, session, serial) {
                Ok(fd) => fd,
                Err(error) => {
                    tracing::warn!("Wayland clipboard SelectionWrite failed: {error}");
                    let _ = selection_write_done(connection, session, serial, false);
                    return;
                }
            };
            let cancel = gio::Cancellable::new();
            let completion_offer = Rc::clone(&offer);
            let completion_connection = connection.clone();
            let completion_session = session.to_owned();
            clipboard_transfer::write_async(
                fd,
                snapshot,
                CLIPBOARD_TRANSFER_TIMEOUT,
                &cancel,
                move |result| {
                    let transfer_succeeded = result.is_ok()
                        && selection_write_done(
                            &completion_connection,
                            &completion_session,
                            serial,
                            result.is_ok(),
                        )
                        .map(|()| true)
                        .unwrap_or_else(|error| {
                            tracing::warn!("Wayland clipboard SelectionWriteDone failed: {error}");
                            false
                        });
                    if transfer_succeeded {
                        mark_offer_transferred(
                            &mut completion_offer.borrow_mut(),
                            &completion_session,
                        );
                    }
                },
            );
        },
    )
}

fn selection_write(
    connection: &gio::DBusConnection,
    session: &str,
    serial: u32,
) -> Result<std::os::fd::OwnedFd, String> {
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| format!("invalid RemoteDesktop session path: {error}"))?
        .to_variant();
    let parameters = glib::Variant::tuple_from_iter([session_path, serial.to_variant()]);
    let (reply, fds) = connection
        .call_with_unix_fd_list_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            CLIPBOARD_INTERFACE,
            "SelectionWrite",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            None::<&gio::UnixFDList>,
            gio::Cancellable::NONE,
        )
        .map_err(|error| format!("clipboard SelectionWrite failed: {error}"))?;
    clipboard_transfer::take_reply_fd(&reply, &fds)
        .map_err(|error| format!("clipboard SelectionWrite returned invalid descriptor: {error:?}"))
}

fn selection_write_done(
    connection: &gio::DBusConnection,
    session: &str,
    serial: u32,
    success: bool,
) -> Result<(), String> {
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| format!("invalid RemoteDesktop session path: {error}"))?
        .to_variant();
    let parameters =
        glib::Variant::tuple_from_iter([session_path, serial.to_variant(), success.to_variant()]);
    connection
        .call_sync(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            CLIPBOARD_INTERFACE,
            "SelectionWriteDone",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            PORTAL_CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        )
        .map_err(|error| format!("clipboard SelectionWriteDone failed: {error}"))?;
    Ok(())
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
    fn start_requires_clipboard_authorization_before_paste() {
        let results = glib::VariantDict::new(None);
        results.insert("devices", DEVICE_TYPE_KEYBOARD);
        results.insert("clipboard_enabled", false);
        let error = parse_start_response(&results.end())
            .expect_err("keyboard permission alone must not authorize clipboard paste");
        assert!(error.contains("clipboard"), "{error}");
    }

    #[test]
    fn start_rejects_missing_clipboard_authorization() {
        let results = glib::VariantDict::new(None);
        results.insert("devices", DEVICE_TYPE_KEYBOARD);
        assert!(parse_start_response(&results.end()).is_err());
    }

    #[test]
    fn start_accepts_keyboard_and_clipboard_with_rotated_token() {
        let results = glib::VariantDict::new(None);
        results.insert("devices", DEVICE_TYPE_KEYBOARD);
        results.insert("clipboard_enabled", true);
        results.insert("restore_token", "rotated-token");
        assert_eq!(
            parse_start_response(&results.end()).unwrap(),
            Some("rotated-token".into())
        );
    }

    #[test]
    fn start_rejects_clipboard_without_keyboard() {
        let results = glib::VariantDict::new(None);
        results.insert("devices", 2u32);
        results.insert("clipboard_enabled", true);
        let error = parse_start_response(&results.end()).unwrap_err();
        assert!(error.contains("keyboard"), "{error}");
    }

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

    #[test]
    fn permission_denied_flag_can_be_queried_and_updated() {
        set_permission_denied(true);
        assert!(is_permission_denied());
        set_permission_denied(false);
        assert!(!is_permission_denied());
    }

    #[test]
    fn clipboard_selection_advertises_plain_utf8_and_plain_text() {
        assert_eq!(CLIPBOARD_MIME_UTF8, "text/plain;charset=utf-8");
        assert_eq!(CLIPBOARD_MIME_TEXT, "text/plain");
    }

    #[test]
    fn paste_command_preserves_unicode_snapshot() {
        let expected = Arc::<[u8]>::from("árbol 🦀\n第二行".as_bytes());
        match WorkerCommand::Paste(expected.clone()) {
            WorkerCommand::Paste(snapshot) => assert_eq!(snapshot.as_ref(), expected.as_ref()),
            WorkerCommand::Prewarm => panic!("paste content was discarded"),
        }
    }

    #[test]
    fn selection_transfer_accepts_only_current_session_and_advertised_mime() {
        let session = "/org/freedesktop/portal/desktop/session/test/session";
        let expected = Arc::<[u8]>::from(&b"exact snapshot"[..]);
        assert_eq!(
            transfer_snapshot(
                Some(&ClipboardOffer {
                    session: session.into(),
                    bytes: expected.clone(),
                    generation: 1,
                    transferred: false,
                }),
                session,
                CLIPBOARD_MIME_UTF8,
            ),
            Some(expected),
        );
        assert_eq!(
            transfer_snapshot(
                Some(&ClipboardOffer {
                    session: session.into(),
                    bytes: Arc::from(&b"other"[..]),
                    generation: 1,
                    transferred: false,
                }),
                "/org/freedesktop/portal/desktop/session/test/other",
                CLIPBOARD_MIME_UTF8,
            ),
            None,
        );
        assert_eq!(
            transfer_snapshot(
                Some(&ClipboardOffer {
                    session: session.into(),
                    bytes: Arc::from(&b"other"[..]),
                    generation: 1,
                    transferred: false,
                }),
                session,
                "application/octet-stream",
            ),
            None,
        );
    }

    #[test]
    fn selection_transfer_timeout_is_bounded() {
        assert_eq!(CLIPBOARD_TRANSFER_TIMEOUT, Duration::from_secs(5));
        assert_eq!(CLIPBOARD_OFFER_TIMEOUT, Duration::from_secs(5));
    }

    #[test]
    fn valid_transfers_reuse_offer_until_expiry() {
        let offer = Some(ClipboardOffer {
            session: "session".into(),
            bytes: Arc::from(&b"snapshot"[..]),
            generation: 1,
            transferred: false,
        });
        assert_eq!(
            transfer_snapshot(offer.as_ref(), "session", CLIPBOARD_MIME_TEXT),
            Some(Arc::from(&b"snapshot"[..]))
        );
        assert_eq!(
            transfer_snapshot(offer.as_ref(), "session", CLIPBOARD_MIME_TEXT),
            Some(Arc::from(&b"snapshot"[..]))
        );
    }

    #[test]
    fn invalid_transfer_does_not_consume_offer() {
        let offer = Some(ClipboardOffer {
            session: "session".into(),
            bytes: Arc::from(&b"snapshot"[..]),
            generation: 1,
            transferred: false,
        });
        assert_eq!(
            transfer_snapshot(offer.as_ref(), "other", CLIPBOARD_MIME_TEXT),
            None
        );
        assert_eq!(
            transfer_snapshot(offer.as_ref(), "session", "application/octet-stream"),
            None
        );
        assert!(offer.is_some());
    }

    #[test]
    fn take_expired_offer_only_consumes_matching_generation() {
        let mut offer = Some(ClipboardOffer {
            session: "session".into(),
            bytes: Arc::from(&b"snapshot"[..]),
            generation: 2,
            transferred: false,
        });
        assert!(take_expired_offer(&mut offer, "session", 1).is_none());
        assert!(offer.is_some());
        let expired = take_expired_offer(&mut offer, "session", 2).expect("expired offer");
        assert!(!expired.transferred);
        assert!(offer.is_none());
    }

    #[test]
    fn successful_transfer_marks_offer_without_consuming_it() {
        let mut offer = Some(ClipboardOffer {
            session: "session".into(),
            bytes: Arc::from(&b"snapshot"[..]),
            generation: 1,
            transferred: false,
        });
        mark_offer_transferred(&mut offer, "session");
        assert!(offer.as_ref().is_some_and(|offer| offer.transferred));
        let mut wrong = Some(ClipboardOffer {
            session: "session".into(),
            bytes: Arc::from(&b"snapshot"[..]),
            generation: 1,
            transferred: false,
        });
        mark_offer_transferred(&mut wrong, "other");
        assert!(!wrong.as_ref().unwrap().transferred);
    }

    #[test]
    fn paste_outcomes_keep_latest_eight_in_fifo_order() {
        let mut queue = VecDeque::new();
        for _ in 0..9 {
            enqueue_paste_outcome(&mut queue, PasteOutcome::Failed);
        }
        assert_eq!(queue.len(), 8);
        assert!(queue.iter().all(|outcome| *outcome == PasteOutcome::Failed));
    }

    #[test]
    fn finish_paste_with_publishes_only_for_pending() {
        paste_outcomes().lock().unwrap().clear();
        PASTE_PENDING.store(false, Ordering::Release);
        finish_paste_with(PasteOutcome::Failed);
        assert!(poll_paste_outcome().is_none());
        PASTE_PENDING.store(true, Ordering::Release);
        finish_paste_with(PasteOutcome::ClipboardTransferred);
        assert_eq!(
            poll_paste_outcome(),
            Some(PasteOutcome::ClipboardTransferred)
        );
    }

    #[test]
    fn offer_timeout_notifies_worker_context() {
        let context = glib::MainContext::new();
        let (timeout_tx, timeout_rx) = mpsc::channel();
        context
            .with_thread_default(|| {
                schedule_offer_timeout(&context, timeout_tx, "session".into(), 1, Duration::ZERO);
                for _ in 0..100 {
                    while context.pending() {
                        context.iteration(false);
                    }
                    if let Ok((session, generation)) = timeout_rx.try_recv() {
                        assert_eq!(session, "session");
                        assert_eq!(generation, 1);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                panic!("offer timeout did not notify the worker context");
            })
            .unwrap();
    }

    #[test]
    fn only_one_pending_paste_is_accepted() {
        PASTE_PENDING.store(false, Ordering::Relaxed);
        assert!(try_begin_paste());
        assert!(!try_begin_paste());
        finish_paste();
        assert!(try_begin_paste());
        finish_paste();
    }
}
