use crate::desktop::{DesktopConfig, DesktopErrorCode, DesktopRuntime};
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_channel::oneshot;
use futures_util::{FutureExt, StreamExt};
use gtk::glib::variant::{ObjectPath, ToVariant};
use gtk::{gio, glib};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
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

#[derive(Debug, Clone)]
struct PortalBinding {
    shortcut_id: String,
    preferred_trigger: String,
    description: String,
}

/// Commands from the facade to the async portal coordinator.
///
/// The coordinator owns the single in-flight request, so control, cancel
/// and shutdown stay responsive while a consent dialog is open.
pub(super) enum WorkerCommand {
    Refresh,
    Reconcile {
        revision: u64,
        specs: Vec<DesiredBinding>,
    },
    SetParent {
        parent: Option<PortalParent>,
    },
    CancelInteraction {
        generation: u64,
        reply: oneshot::Sender<CancelOutcome>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

/// One desired binding at a worker revision: stable prompt identity with
/// the runtime id the UI maps activations through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DesiredBinding {
    pub runtime_id: u32,
    pub prompt_id: String,
    pub shortcut_id: String,
    pub preferred_trigger: String,
    pub description: String,
}

/// Validated parent-window metadata for user-initiated consent. The owned
/// lease stays on the main thread; the worker receives only the handle
/// string plus its generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PortalParent {
    pub generation: u64,
    pub handle: String,
}

use crate::desktop::CancelOutcome;

/// Backend lifecycle observed by the worker. The service reports Pending
/// immediately; Ready/Unavailable arrive through the status channel once
/// the private connection handshake completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BackendState {
    Pending,
    Ready,
    Unavailable(String),
}

pub(super) struct PortalHotkeyService {
    runtime_ids: Mutex<HashMap<String, u32>>,
    command_tx: UnboundedSender<WorkerCommand>,
    next_id: AtomicU32,
}

fn spawn_worker(
    command_rx: UnboundedReceiver<WorkerCommand>,
    status_tx: UnboundedSender<BackendState>,
    event_tx: UnboundedSender<crate::desktop::HotkeyEvent>,
    runtime: Arc<DesktopRuntime>,
) -> Option<()> {
    thread::Builder::new()
        .name("jameprompt-wayland-shortcuts".into())
        .spawn(move || portal_worker(command_rx, status_tx, event_tx, runtime))
        .ok()?;
    Some(())
}

impl PortalHotkeyService {
    /// Constructor sharing the facade-owned event channel, so portal and
    /// native backends expose one uniform subscription shape.
    pub(super) fn new_with_event(
        event_tx: UnboundedSender<crate::desktop::HotkeyEvent>,
        runtime: Arc<DesktopRuntime>,
    ) -> Option<Self> {
        let (command_tx, command_rx) = futures_channel::mpsc::unbounded();
        let (status_tx, status_rx) = futures_channel::mpsc::unbounded();

        // The worker owns its GLib MainContext/MainLoop; construction
        // returns immediately with a Pending backend instead of blocking
        // GUI startup on bus initialization.
        spawn_worker(command_rx, status_tx, event_tx, runtime)?;
        // Drain the initial status asynchronously: the first real state is
        // observed through events, never by blocking here.
        drop(status_rx);

        Some(Self {
            runtime_ids: Mutex::new(HashMap::new()),
            command_tx,
            next_id: AtomicU32::new(1),
        })
    }

    /// Queues one versioned desired snapshot for reconciliation.
    ///
    /// Stable runtime ids are assigned per prompt; unparseable triggers
    /// and empty identities are rejected before any backend contact and
    /// reported so the UI marks them Failed, never Pending. A queued
    /// command is never displayed as ready: Active arrives only through
    /// [`crate::desktop::HotkeyEvent::Acknowledged`].
    pub(super) fn reconcile(
        &self,
        revision: u64,
        specs: &[crate::desktop::ShortcutSpec],
    ) -> Result<crate::desktop::ReconcileReceipt, DesktopErrorCode> {
        let mut desired = Vec::with_capacity(specs.len());
        let mut rejected = Vec::new();
        let mut runtime_ids = self
            .runtime_ids
            .lock()
            .map_err(|_| DesktopErrorCode::Disconnected)?;
        for spec in specs {
            let prompt_id = spec.prompt_id.trim();
            let Some(preferred_trigger) = to_portal_trigger(&spec.preferred_trigger) else {
                rejected.push(spec.prompt_id.clone());
                continue;
            };
            let Some(shortcut_id) = portal_shortcut_id(prompt_id) else {
                rejected.push(spec.prompt_id.clone());
                continue;
            };
            let Some(runtime_id) =
                runtime_id_for_prompt(prompt_id, &mut runtime_ids, &self.next_id)
            else {
                rejected.push(spec.prompt_id.clone());
                continue;
            };
            desired.push(DesiredBinding {
                runtime_id,
                prompt_id: prompt_id.to_string(),
                shortcut_id,
                preferred_trigger,
                description: portal_description(&spec.description),
            });
        }
        drop(runtime_ids);
        desired.sort_by(|a, b| a.prompt_id.cmp(&b.prompt_id));
        self.command_tx
            .unbounded_send(WorkerCommand::Reconcile {
                revision,
                specs: desired,
            })
            .map_err(|_| DesktopErrorCode::Disconnected)?;
        Ok(crate::desktop::ReconcileReceipt {
            rejected_prompt_ids: rejected,
        })
    }

    /// Updates the parent context for user-initiated consent. `None`
    /// clears a lost parent; in-flight parent-dependent requests are
    /// cancelled by the coordinator.
    pub(super) fn set_parent(
        &self,
        parent: Option<crate::desktop::ParentContext>,
    ) -> Result<(), DesktopErrorCode> {
        let parent = parent.map(|context| PortalParent {
            generation: context.generation,
            handle: context.handle,
        });
        self.command_tx
            .unbounded_send(WorkerCommand::SetParent { parent })
            .map_err(|_| DesktopErrorCode::Disconnected)?;
        Ok(())
    }

    /// Re-drives reconciliation of the last desired snapshot, for the
    /// explicit UI retry action. Exactly one request per call; the worker
    /// debounces and serializes.
    pub(super) fn retry_pending(&self) -> Result<(), DesktopErrorCode> {
        self.command_tx
            .unbounded_send(WorkerCommand::Refresh)
            .map_err(|_| DesktopErrorCode::Disconnected)?;
        Ok(())
    }

    /// Requests cancellation of any in-flight portal interaction for
    /// `generation`. The returned receiver resolves after the remote
    /// Request was closed and its subscription released — not when the
    /// command is merely queued.
    pub(super) fn cancel_interaction(&self, generation: u64) -> oneshot::Receiver<CancelOutcome> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let command = WorkerCommand::CancelInteraction {
            generation,
            reply: reply_tx,
        };
        if self.command_tx.unbounded_send(command).is_err() {
            let (fallback_tx, fallback_rx) = oneshot::channel();
            let _ = fallback_tx.send(CancelOutcome::NoPendingRequest);
            return fallback_rx;
        }
        reply_rx
    }

    /// Ordered shutdown: cancels the in-flight request, closes the
    /// session, releases subscriptions, then resolves the receiver.
    pub(super) fn shutdown(&self) -> oneshot::Receiver<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .command_tx
            .unbounded_send(WorkerCommand::Shutdown { reply: reply_tx })
            .is_err()
        {
            let (fallback_tx, fallback_rx) = oneshot::channel();
            let _ = fallback_tx.send(());
            return fallback_rx;
        }
        reply_rx
    }
}

/// One portal Response signal routed by object path.
#[derive(Debug)]
struct ResponseEvent {
    path: String,
    code: u32,
    results: glib::Variant,
}

/// Progress shared between the in-flight interaction future and the
/// coordinator, so cancellation closes the right remote objects even when
/// the method reply (carrying the actual handle) never arrives.
#[derive(Debug, Default)]
struct InteractionProgress {
    predicted_path: String,
    actual_path: Option<String>,
    session: Option<String>,
    /// Window generation the parent (if any) was exported for. A close
    /// for generation N never cancels a request parented to N+1.
    parent_generation: Option<u64>,
}

fn portal_worker(
    command_rx: UnboundedReceiver<WorkerCommand>,
    status_tx: UnboundedSender<BackendState>,
    event_tx: UnboundedSender<crate::desktop::HotkeyEvent>,
    runtime: Arc<DesktopRuntime>,
) {
    let context = glib::MainContext::new();
    let main_loop = glib::MainLoop::new(Some(&context), false);

    let config = runtime.config();
    let app_id = runtime.app_id().to_string();
    let acquire_status_tx = status_tx.clone();
    // Only this thread constructs and uses the shortcut connection; the
    // interaction futures are MainContext-bound and never Send. A fresh
    // context cannot be owned by another thread; failure is reported as
    // unavailable instead of panicking the worker.
    if context
        .with_thread_default(|| {
            let quit_loop = main_loop.clone();
            let _coordinator = context.spawn_local(async move {
                let _ = status_tx.unbounded_send(BackendState::Pending);
                match super::connection::session_connection() {
                    Ok(connection) => {
                        if let Err(error) = register_host_application(&connection, &app_id)
                            .and_then(|_| ensure_global_shortcuts_available(&connection))
                        {
                            let _ = status_tx.unbounded_send(BackendState::Unavailable(error));
                            let _ = event_tx.unbounded_send(
                                crate::desktop::HotkeyEvent::BackendUnavailable(
                                    DesktopErrorCode::BackendUnavailable,
                                ),
                            );
                            quit_loop.quit();
                            return;
                        }
                        let _ = status_tx.unbounded_send(BackendState::Ready);
                        run_coordinator(&connection, config, command_rx, event_tx).await;
                    }
                    Err(error) => {
                        let _ = status_tx.unbounded_send(BackendState::Unavailable(error));
                        let _ = event_tx.unbounded_send(
                            crate::desktop::HotkeyEvent::BackendUnavailable(
                                DesktopErrorCode::BusUnavailable,
                            ),
                        );
                    }
                }
                quit_loop.quit();
            });

            main_loop.run();
        })
        .is_err()
    {
        let _ = acquire_status_tx.unbounded_send(BackendState::Unavailable(
            "shortcut worker could not own its GLib main context".to_string(),
        ));
    }
}

/// Async reconciliation coordinator. A single state owner multiplexes the
/// next command, the current request completion, session/owner events and
/// a GLib timeout — no sleep loop, so control/cancel/shutdown stay
/// responsive while a consent dialog is open.
///
/// Reconciliation is versioned: one synchronization owner covers
/// startup/create/edit/delete/import/toggle. A queued command is never
/// reported Active; only accepted current-revision routes publish Active
/// with the backend-assigned trigger description.
async fn run_coordinator(
    connection: &gio::DBusConnection,
    config: DesktopConfig,
    mut commands: UnboundedReceiver<WorkerCommand>,
    event_tx: UnboundedSender<crate::desktop::HotkeyEvent>,
) {
    let request_prefix = match request_prefix(connection) {
        Ok(prefix) => prefix,
        Err(error) => {
            tracing::warn!("portal request prefix unavailable: {error}");
            return;
        }
    };

    let activation_routes = Arc::new(Mutex::new(ActivationRoutes::default()));
    let (closed_tx, mut closed_rx) = futures_channel::mpsc::unbounded::<String>();
    let _closed_subscription = subscribe_session_closed(connection, closed_tx);
    subscribe_activated(connection, Arc::clone(&activation_routes), event_tx.clone());
    let (changed_tx, mut changed_rx) = futures_channel::mpsc::unbounded::<String>();
    let _changed_subscription = subscribe_shortcuts_changed(connection, changed_tx);
    let (owner_tx, mut owner_rx) = futures_channel::mpsc::unbounded::<bool>();
    let _owner_subscription = subscribe_portal_owner(connection, owner_tx);

    let mut state = ReconcileState::default();
    let mut debounce_at: Option<Instant> = None;
    let debounce = Duration::from_millis(config.reconcile_debounce_ms);
    let mut pending: Option<PendingInteraction> = None;
    let mut active_session: Option<String> = None;

    loop {
        if let Some(mut interaction) = pending.take() {
            // One request in flight: commands, completion and session
            // events stay serviced during consent. Ownership is taken for
            // the select and restored unless the interaction terminates.
            enum PendingFate {
                Keep,
                Done,
                Exit,
            }
            let fate = futures_util::select! {
                command = commands.next().fuse() => {
                    match command {
                        Some(WorkerCommand::Refresh) => {
                            state.refresh(&mut debounce_at);
                            PendingFate::Keep
                        }
                        Some(WorkerCommand::Reconcile { revision, specs }) => {
                            state.store_desired(revision, specs);
                            // Re-evaluate after the in-flight request:
                            // serial replacement, never two binds at
                            // once for one session slot.
                            debounce_at = Some(Instant::now());
                            PendingFate::Keep
                        }
                        Some(WorkerCommand::SetParent { parent }) => {
                            state.set_parent(parent, &mut debounce_at);
                            PendingFate::Keep
                        }
                        Some(WorkerCommand::CancelInteraction { generation, reply }) => {
                            let outcome = cancel_guarded(
                                connection,
                                &config,
                                &interaction,
                                generation,
                            )
                            .await;
                            if outcome == CancelOutcome::Released {
                                // Re-drive under the new conditions (a
                                // cleared parent defers to AwaitingWindow
                                // instead of reopening consent).
                                debounce_at = Some(Instant::now());
                            }
                            let _ = reply.send(outcome);
                            PendingFate::Done
                        }
                        Some(WorkerCommand::Shutdown { reply }) => {
                            shutdown_interaction(connection, &config, &interaction).await;
                            shutdown_session(connection, &config, &mut active_session).await;
                            let _ = reply.send(());
                            PendingFate::Exit
                        }
                        None => {
                            shutdown_interaction(connection, &config, &interaction).await;
                            PendingFate::Exit
                        }
                    }
                }
                outcome = interaction.future.as_mut().fuse() => {
                    apply_interaction_outcome(
                        &interaction,
                        outcome,
                        &activation_routes,
                        &event_tx,
                        &mut state,
                        &mut active_session,
                    )
                    .await;
                    PendingFate::Done
                }
                closed = closed_rx.next().fuse() => {
                    if let Some(session) = closed {
                        if session_closed(&activation_routes, &mut active_session, &session) {
                            publish_session_loss(&event_tx, &mut state);
                            debounce_at = Some(Instant::now());
                        }
                    }
                    PendingFate::Keep
                }
                changed = changed_rx.next().fuse() => {
                    if let Some(session) = changed {
                        if Some(session.as_str()) == active_session.as_deref() {
                            state.refresh_triggers = true;
                            debounce_at = Some(Instant::now());
                        }
                    }
                    PendingFate::Keep
                }
                owner_present = owner_rx.next().fuse() => {
                    if let Some(present) = owner_present {
                        on_portal_owner(
                            connection,
                            &config,
                            present,
                            &activation_routes,
                            &event_tx,
                            &mut state,
                            &mut active_session,
                            &mut debounce_at,
                        )
                        .await;
                    }
                    PendingFate::Keep
                }
            };
            match fate {
                PendingFate::Keep => {
                    pending = Some(interaction);
                }
                PendingFate::Done => {}
                PendingFate::Exit => break,
            }
            continue;
        }

        // Idle: no request in flight. Fire the debounced reconciliation.
        if let Some(started) = debounce_at {
            if started.elapsed() >= debounce {
                debounce_at = None;
                fire_reconciliation(
                    connection,
                    &config,
                    &request_prefix,
                    &activation_routes,
                    &event_tx,
                    &mut state,
                    &mut active_session,
                    &mut pending,
                )
                .await;
            }
        }

        futures_util::select! {
            command = commands.next().fuse() => {
                match command {
                    Some(WorkerCommand::Refresh) => {
                        state.refresh(&mut debounce_at);
                    }
                    Some(WorkerCommand::Reconcile { revision, specs }) => {
                        state.store_desired(revision, specs);
                        state.refresh(&mut debounce_at);
                    }
                    Some(WorkerCommand::SetParent { parent }) => {
                        state.set_parent(parent, &mut debounce_at);
                    }
                    Some(WorkerCommand::CancelInteraction { generation: _, reply }) => {
                        let _ = reply.send(CancelOutcome::NoPendingRequest);
                    }
                    Some(WorkerCommand::Shutdown { reply }) => {
                        shutdown_session(connection, &config, &mut active_session).await;
                        let _ = reply.send(());
                        break;
                    }
                    None => break,
                }
            }
            closed = closed_rx.next().fuse() => {
                if let Some(session) = closed {
                    if session_closed(&activation_routes, &mut active_session, &session) {
                        publish_session_loss(&event_tx, &mut state);
                        debounce_at = Some(Instant::now());
                    }
                }
            }
            changed = changed_rx.next().fuse() => {
                if let Some(session) = changed {
                    if Some(session.as_str()) == active_session.as_deref() {
                        state.refresh_triggers = true;
                        debounce_at = Some(Instant::now());
                    }
                }
            }
            owner_present = owner_rx.next().fuse() => {
                if let Some(present) = owner_present {
                    on_portal_owner(
                        connection,
                        &config,
                        present,
                        &activation_routes,
                        &event_tx,
                        &mut state,
                        &mut active_session,
                        &mut debounce_at,
                    )
                    .await;
                }
            }
            _ = glib::timeout_future(Duration::from_millis(20)).fuse() => {}
        }
    }
}

/// Current routes for Activated validation, updated only by the
/// coordinator alongside `active_routes`.
#[derive(Debug, Default)]
struct ActivationRoutes {
    session: Option<String>,
    revision: u64,
    routes: HashMap<String, u32>,
}

/// Versioned desired-shortcut state owned by the coordinator.
#[derive(Debug, Default)]
struct ReconcileState {
    revision: u64,
    specs: Vec<DesiredBinding>,
    last_revision: u64,
    last_specs: Vec<DesiredBinding>,
    /// Shortcut ids ever accepted by the backend: rebinding them needs
    /// no new consent dialog.
    accepted_ever: HashSet<String>,
    parent: Option<PortalParent>,
    /// Revisions blocked from automatic consent after user
    /// cancellation/denial; explicit retry unblocks the current one.
    blocked_revisions: HashSet<u64>,
    /// Bounded transport recovery attempts for the current outage.
    transport_retries: u8,
    /// Next allowed automatic transport retry (bounded backoff).
    retry_at: Option<Instant>,
    /// ShortcutsChanged requested a trigger refresh.
    refresh_triggers: bool,
    /// Last published acknowledgement per prompt, for deduplication.
    published: HashMap<String, crate::desktop::ShortcutAcknowledgement>,
}

impl ReconcileState {
    /// Bounded backoff for transport recovery: 100, 250, 500, 1000, 2000
    /// ms, at most five attempts. Human consent is never retried.
    fn transport_backoff(&self) -> Option<Duration> {
        const BACKOFF_MS: [u64; 5] = [100, 250, 500, 1000, 2000];
        BACKOFF_MS
            .get(self.transport_retries as usize)
            .map(|ms| Duration::from_millis(*ms))
    }

    /// True when only cosmetic metadata (descriptions) changed: the
    /// accelerator set is identical, so no new session or consent dialog
    /// is required.
    fn same_accelerators(&self) -> bool {
        let current: Vec<(&str, &str)> = self
            .specs
            .iter()
            .map(|spec| (spec.shortcut_id.as_str(), spec.preferred_trigger.as_str()))
            .collect();
        let last: Vec<(&str, &str)> = self
            .last_specs
            .iter()
            .map(|spec| (spec.shortcut_id.as_str(), spec.preferred_trigger.as_str()))
            .collect();
        current == last
    }
}

impl ReconcileState {
    fn store_desired(&mut self, revision: u64, mut specs: Vec<DesiredBinding>) {
        specs.sort_by(|a, b| a.prompt_id.cmp(&b.prompt_id));
        self.revision = revision;
        self.specs = specs;
    }

    fn set_parent(&mut self, parent: Option<PortalParent>, debounce_at: &mut Option<Instant>) {
        let changed = self.parent != parent;
        self.parent = parent;
        if changed {
            *debounce_at = Some(Instant::now());
        }
    }

    fn refresh(&mut self, debounce_at: &mut Option<Instant>) {
        // Explicit retry unblocks the current revision once.
        self.blocked_revisions.remove(&self.revision);
        self.transport_retries = 0;
        *debounce_at = Some(Instant::now());
    }

    fn effective_changed(&self) -> bool {
        self.revision != self.last_revision || self.specs != self.last_specs
    }

    fn unaccepted_new(&self) -> Vec<DesiredBinding> {
        self.specs
            .iter()
            .filter(|spec| !self.accepted_ever.contains(&spec.shortcut_id))
            .cloned()
            .collect()
    }
}

use crate::desktop::{HotkeyEvent, ShortcutAcknowledgement, ShortcutStatus};

fn publish_ack(
    event_tx: &UnboundedSender<HotkeyEvent>,
    published: &mut HashMap<String, ShortcutAcknowledgement>,
    ack: ShortcutAcknowledgement,
) {
    let changed = published.get(&ack.prompt_id) != Some(&ack);
    if changed {
        published.insert(ack.prompt_id.clone(), ack.clone());
        let _ = event_tx.unbounded_send(HotkeyEvent::Acknowledged(ack));
    }
}

/// Records an externally closed session: invalidates active routes at
/// once. Returns true when the closed session was the current one.
fn session_closed(
    activation_routes: &Arc<Mutex<ActivationRoutes>>,
    active_session: &mut Option<String>,
    session: &str,
) -> bool {
    if active_session.as_deref() != Some(session) {
        return false;
    }
    *active_session = None;
    if let Ok(mut activation) = activation_routes.lock() {
        *activation = ActivationRoutes::default();
    }
    true
}

/// Publishes Unavailable for every last-known shortcut when the live
/// session dies: disconnect can never leave Active badges behind.
fn publish_session_loss(event_tx: &UnboundedSender<HotkeyEvent>, state: &mut ReconcileState) {
    let revision = state.revision;
    for spec in state.last_specs.clone() {
        publish_ack(
            event_tx,
            &mut state.published,
            ShortcutAcknowledgement {
                revision,
                prompt_id: spec.prompt_id,
                runtime_id: Some(spec.runtime_id),
                status: ShortcutStatus::Unavailable,
                trigger_description: None,
                error_code: Some(DesktopErrorCode::Disconnected),
            },
        );
    }
    // The desired set is unchanged, but its transport session is gone. Make
    // the next owner-return pass perform a real bind instead of reaffirming
    // stale routes.
    state.last_revision = revision.saturating_sub(1);
}

async fn shutdown_session(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    active_session: &mut Option<String>,
) {
    if let Some(session) = active_session.take() {
        close_session_async(connection, config, &session).await;
    }
}

/// Portal bus owner changes: on loss, invalidate active routes and
/// publish Unavailable immediately; on return, reconcile once when a
/// real owner is present. Blocked (user-cancelled/denied) revisions are
/// never rebound automatically.
async fn on_portal_owner(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    present: bool,
    activation_routes: &Arc<Mutex<ActivationRoutes>>,
    event_tx: &UnboundedSender<HotkeyEvent>,
    state: &mut ReconcileState,
    active_session: &mut Option<String>,
    debounce_at: &mut Option<Instant>,
) {
    if present {
        if active_session.is_none() && !state.specs.is_empty() {
            *debounce_at = Some(Instant::now());
        }
        return;
    }
    if active_session.take().is_some() {
        if let Ok(mut activation) = activation_routes.lock() {
            *activation = ActivationRoutes::default();
        }
        publish_session_loss(event_tx, state);
        let _ = event_tx.unbounded_send(HotkeyEvent::BackendUnavailable(
            DesktopErrorCode::Disconnected,
        ));
    }
    let _ = (connection, config);
}

/// Guarded cancellation: a close for window generation N never cancels a
/// request already parented to a newer generation. Returns the outcome
/// for the acknowledgement reply.
async fn cancel_guarded(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    interaction: &PendingInteraction,
    generation: u64,
) -> CancelOutcome {
    let parent_generation = interaction
        .progress
        .lock()
        .ok()
        .and_then(|state| state.parent_generation);
    if let Some(parent_generation) = parent_generation {
        if parent_generation != generation {
            return CancelOutcome::NoPendingRequest;
        }
    }
    cancel_interaction_remote(connection, config, interaction, generation).await;
    CancelOutcome::Released
}

/// One in-flight create+bind interaction: a single cancellable future plus
/// shared progress so cancellation closes the right remote objects. The
/// future is MainContext-bound (GIO/GLib values are not Send) and lives
/// only on the worker thread.
struct PendingInteraction {
    revision: u64,
    specs: Vec<DesiredBinding>,
    progress: Arc<Mutex<InteractionProgress>>,
    future: std::pin::Pin<Box<dyn futures_util::Future<Output = InteractionOutcome>>>,
}

#[derive(Debug)]
enum InteractionOutcome {
    Bound {
        session: String,
        accepted: HashMap<String, Option<String>>,
    },
    Failed {
        error: DesktopErrorCode,
        detail: String,
    },
}

/// Starts the debounced bind interaction: closes the old session first
/// (frees identical accelerator grabs), then creates exactly one
/// replacement session and issues one BindShortcuts call — all inside a
/// single cancellable future. The replacement gap is observable and never
/// presented as Active.
fn start_bind_interaction(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    request_prefix: &str,
    revision: u64,
    specs: &[DesiredBinding],
    old_session: Option<String>,
    parent: Option<PortalParent>,
) -> Result<PendingInteraction, String> {
    let snapshot: HashMap<u32, PortalBinding> = specs
        .iter()
        .map(|spec| {
            (
                spec.runtime_id,
                PortalBinding {
                    shortcut_id: spec.shortcut_id.clone(),
                    preferred_trigger: spec.preferred_trigger.clone(),
                    description: spec.description.clone(),
                },
            )
        })
        .collect();
    // Fail fast on an unrepresentable snapshot instead of opening a
    // consent dialog that can never succeed.
    shortcuts_variant(&snapshot)?;
    let progress = Arc::new(Mutex::new(InteractionProgress {
        predicted_path: String::new(),
        actual_path: None,
        session: None,
        parent_generation: parent.as_ref().map(|parent| parent.generation),
    }));
    let task_progress = Arc::clone(&progress);
    let connection = connection.clone();
    let config = *config;
    let request_prefix = request_prefix.to_string();
    let task_snapshot = snapshot;
    let parent_handle = parent.map(|parent| parent.handle);
    let future = async move {
        run_bind_interaction(
            &connection,
            &config,
            &request_prefix,
            &task_snapshot,
            old_session,
            parent_handle.as_deref(),
            &task_progress,
        )
        .await
    };
    Ok(PendingInteraction {
        revision,
        specs: specs.to_vec(),
        progress,
        future: Box::pin(future),
    })
}

async fn run_bind_interaction(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    request_prefix: &str,
    snapshot: &HashMap<u32, PortalBinding>,
    old_session: Option<String>,
    parent_handle: Option<&str>,
    progress: &Arc<Mutex<InteractionProgress>>,
) -> InteractionOutcome {
    // Serial replacement: the old session (and its identical grabs) is
    // released before the new bind is attempted.
    if let Some(previous) = old_session {
        close_session_async(connection, config, &previous).await;
    }
    match create_session_async(connection, config, request_prefix, progress).await {
        Ok(session) => {
            {
                if let Ok(mut state) = progress.lock() {
                    state.session = Some(session.clone());
                }
            }
            match bind_shortcuts_async(
                connection,
                config,
                request_prefix,
                &session,
                snapshot,
                parent_handle,
                progress,
            )
            .await
            {
                Ok(accepted) => InteractionOutcome::Bound { session, accepted },
                Err((error, detail)) => {
                    close_session_async(connection, config, &session).await;
                    InteractionOutcome::Failed { error, detail }
                }
            }
        }
        Err((error, detail)) => InteractionOutcome::Failed { error, detail },
    }
}

/// Publishes the replacement gap before closing the previous session. No
/// route remains Active while the new desired set is being bound.
fn publish_replacement_gap(
    event_tx: &UnboundedSender<HotkeyEvent>,
    activation_routes: &Arc<Mutex<ActivationRoutes>>,
    state: &mut ReconcileState,
) {
    if let Ok(mut routes) = activation_routes.lock() {
        *routes = ActivationRoutes::default();
    }
    let current_ids: HashSet<&str> = state
        .specs
        .iter()
        .map(|spec| spec.prompt_id.as_str())
        .collect();
    for spec in state.last_specs.clone() {
        if !current_ids.contains(spec.prompt_id.as_str()) {
            publish_ack(
                event_tx,
                &mut state.published,
                ShortcutAcknowledgement {
                    revision: state.revision,
                    prompt_id: spec.prompt_id,
                    runtime_id: Some(spec.runtime_id),
                    status: ShortcutStatus::Inactive,
                    trigger_description: None,
                    error_code: None,
                },
            );
        }
    }
    for spec in state.specs.clone() {
        publish_ack(
            event_tx,
            &mut state.published,
            ShortcutAcknowledgement {
                revision: state.revision,
                prompt_id: spec.prompt_id,
                runtime_id: Some(spec.runtime_id),
                status: ShortcutStatus::AwaitingConsent,
                trigger_description: None,
                error_code: None,
            },
        );
    }
}

/// Fires one debounced reconciliation step: trigger-only refresh, empty
/// teardown, deferred consent, or exactly one serial replacement. The old
/// session was already closed inside the interaction future before the new
/// bind, so identical accelerator grabs never collide.
#[allow(clippy::too_many_arguments)]
async fn fire_reconciliation(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    request_prefix: &str,
    activation_routes: &Arc<Mutex<ActivationRoutes>>,
    event_tx: &UnboundedSender<HotkeyEvent>,
    state: &mut ReconcileState,
    active_session: &mut Option<String>,
    pending: &mut Option<PendingInteraction>,
) {
    if state.refresh_triggers {
        state.refresh_triggers = false;
        if let Some(session) = active_session.clone() {
            refresh_actual_triggers(
                connection,
                config,
                &session,
                activation_routes,
                event_tx,
                state,
            )
            .await;
        }
    }

    if !state.effective_changed() {
        reaffirm_published(event_tx, state);
        return;
    }
    if state.blocked_revisions.contains(&state.revision) {
        // User-cancelled/denied: no automatic consent replay. Explicit
        // retry (Refresh) unblocks this revision.
        return;
    }
    if let Some(retry_at) = state.retry_at {
        if Instant::now() < retry_at {
            return;
        }
        state.retry_at = None;
    }

    let revision = state.revision;
    if state.specs.is_empty() {
        // Empty desired set: close any session, clear maps, acknowledge
        // Inactive. No consent dialog is opened.
        if let Some(session) = active_session.take() {
            close_session_async(connection, config, &session).await;
        }
        if let Ok(mut activation) = activation_routes.lock() {
            *activation = ActivationRoutes::default();
        }
        for spec in state.last_specs.clone() {
            publish_ack(
                event_tx,
                &mut state.published,
                ShortcutAcknowledgement {
                    revision,
                    prompt_id: spec.prompt_id,
                    runtime_id: Some(spec.runtime_id),
                    status: ShortcutStatus::Inactive,
                    trigger_description: None,
                    error_code: None,
                },
            );
        }
        state.last_revision = revision;
        state.last_specs.clear();
        state.transport_retries = 0;
        return;
    }

    if state.same_accelerators() {
        // Cosmetic metadata only (descriptions): no new session, no
        // consent dialog. Bookkeeping advances without rebinding.
        state.last_revision = revision;
        state.last_specs.clone_from(&state.specs);
        reaffirm_published(event_tx, state);
        return;
    }

    let unaccepted = state.unaccepted_new();
    if !unaccepted.is_empty() && state.parent.is_none() {
        // New shortcuts need user consent, which needs a live parent
        // window. Defer without closing the existing session and without
        // substituting a blank parent. last_* is deliberately untouched
        // so the parent arrival fires the real bind.
        for spec in &unaccepted {
            publish_ack(
                event_tx,
                &mut state.published,
                ShortcutAcknowledgement {
                    revision,
                    prompt_id: spec.prompt_id.clone(),
                    runtime_id: Some(spec.runtime_id),
                    status: ShortcutStatus::AwaitingWindow,
                    trigger_description: None,
                    error_code: None,
                },
            );
        }
        return;
    }

    for spec in &state.specs {
        let already_active = state.published.get(&spec.prompt_id).is_some_and(|ack| {
            ack.status == ShortcutStatus::Active && ack.revision == state.last_revision
        });
        if !already_active {
            publish_ack(
                event_tx,
                &mut state.published,
                ShortcutAcknowledgement {
                    revision,
                    prompt_id: spec.prompt_id.clone(),
                    runtime_id: Some(spec.runtime_id),
                    status: ShortcutStatus::AwaitingConsent,
                    trigger_description: None,
                    error_code: None,
                },
            );
        }
    }
    publish_replacement_gap(event_tx, activation_routes, state);
    let old_session = active_session.take();
    match start_bind_interaction(
        connection,
        config,
        request_prefix,
        revision,
        &state.specs,
        old_session,
        state.parent.clone(),
    ) {
        Ok(interaction) => {
            *pending = Some(interaction);
        }
        Err(detail) => {
            tracing::warn!(
                "Wayland shortcut binding failed; keeping previous bindings active: {detail}"
            );
        }
    }
}

/// Re-publishes the last published states so a duplicated reconcile can
/// never leave a UI Pending without an acknowledgement.
fn reaffirm_published(event_tx: &UnboundedSender<HotkeyEvent>, state: &mut ReconcileState) {
    for spec in state.specs.clone() {
        let Some(mut ack) = state.published.get(&spec.prompt_id).cloned() else {
            continue;
        };
        ack.revision = state.revision;
        publish_ack(event_tx, &mut state.published, ack);
    }
}

async fn apply_interaction_outcome(
    interaction: &PendingInteraction,
    outcome: InteractionOutcome,
    activation_routes: &Arc<Mutex<ActivationRoutes>>,
    event_tx: &UnboundedSender<HotkeyEvent>,
    state: &mut ReconcileState,
    active_session: &mut Option<String>,
) {
    match outcome {
        InteractionOutcome::Bound { session, accepted } => {
            for shortcut_id in accepted.keys() {
                state.accepted_ever.insert(shortcut_id.clone());
            }
            let mut activation = HashMap::new();
            for spec in &interaction.specs {
                match accepted.get(&spec.shortcut_id) {
                    Some(trigger) => {
                        activation.insert(spec.shortcut_id.clone(), spec.runtime_id);
                        publish_ack(
                            event_tx,
                            &mut state.published,
                            ShortcutAcknowledgement {
                                revision: interaction.revision,
                                prompt_id: spec.prompt_id.clone(),
                                runtime_id: Some(spec.runtime_id),
                                status: ShortcutStatus::Active,
                                trigger_description: trigger.clone(),
                                error_code: None,
                            },
                        );
                    }
                    None => {
                        // Accepted subset excludes this id: the user did
                        // not confirm it. Not Active, never silently kept.
                        publish_ack(
                            event_tx,
                            &mut state.published,
                            ShortcutAcknowledgement {
                                revision: interaction.revision,
                                prompt_id: spec.prompt_id.clone(),
                                runtime_id: Some(spec.runtime_id),
                                status: ShortcutStatus::Cancelled,
                                trigger_description: None,
                                error_code: None,
                            },
                        );
                    }
                }
            }
            if let Ok(mut current) = activation_routes.lock() {
                *current = ActivationRoutes {
                    session: Some(session.clone()),
                    revision: interaction.revision,
                    routes: activation,
                };
            }
            *active_session = Some(session);
            state.last_revision = interaction.revision;
            state.last_specs = interaction.specs.clone();
            state.transport_retries = 0;
            state.retry_at = None;
        }
        InteractionOutcome::Failed { error, detail } => {
            match error {
                DesktopErrorCode::RequestCancelled => {
                    // User cancellation is expected: no retry storm, no
                    // automatic consent replay for this revision.
                    tracing::info!("portal shortcut request cancelled: {detail}");
                    state.blocked_revisions.insert(interaction.revision);
                    state.last_revision = interaction.revision;
                    state.last_specs.clone_from(&interaction.specs);
                    for spec in &interaction.specs {
                        publish_ack(
                            event_tx,
                            &mut state.published,
                            ShortcutAcknowledgement {
                                revision: interaction.revision,
                                prompt_id: spec.prompt_id.clone(),
                                runtime_id: Some(spec.runtime_id),
                                status: ShortcutStatus::Cancelled,
                                trigger_description: None,
                                error_code: None,
                            },
                        );
                    }
                }
                DesktopErrorCode::RequestDenied => {
                    tracing::warn!("portal shortcut request denied: {detail}");
                    state.blocked_revisions.insert(interaction.revision);
                    state.last_revision = interaction.revision;
                    state.last_specs.clone_from(&interaction.specs);
                    for spec in &interaction.specs {
                        publish_ack(
                            event_tx,
                            &mut state.published,
                            ShortcutAcknowledgement {
                                revision: interaction.revision,
                                prompt_id: spec.prompt_id.clone(),
                                runtime_id: Some(spec.runtime_id),
                                status: ShortcutStatus::Failed,
                                trigger_description: None,
                                error_code: Some(DesktopErrorCode::RequestDenied),
                            },
                        );
                    }
                }
                DesktopErrorCode::ParentUnavailable | DesktopErrorCode::ParentExportFailed => {
                    for spec in &interaction.specs {
                        publish_ack(
                            event_tx,
                            &mut state.published,
                            ShortcutAcknowledgement {
                                revision: interaction.revision,
                                prompt_id: spec.prompt_id.clone(),
                                runtime_id: Some(spec.runtime_id),
                                status: ShortcutStatus::AwaitingWindow,
                                trigger_description: None,
                                error_code: Some(error),
                            },
                        );
                    }
                }
                _ => {
                    // Transient transport/invalid responses: bounded
                    // backoff recovery, then stop until explicit retry.
                    tracing::warn!("Wayland shortcut binding failed ({error}): {detail}");
                    let status = if error == DesktopErrorCode::Disconnected
                        || error == DesktopErrorCode::BusUnavailable
                        || error == DesktopErrorCode::BackendUnavailable
                    {
                        ShortcutStatus::Unavailable
                    } else {
                        ShortcutStatus::Failed
                    };
                    for spec in &interaction.specs {
                        publish_ack(
                            event_tx,
                            &mut state.published,
                            ShortcutAcknowledgement {
                                revision: interaction.revision,
                                prompt_id: spec.prompt_id.clone(),
                                runtime_id: Some(spec.runtime_id),
                                status,
                                trigger_description: None,
                                error_code: Some(error),
                            },
                        );
                    }
                    if let Some(backoff) = state.transport_backoff() {
                        state.transport_retries += 1;
                        state.retry_at = Some(Instant::now() + backoff);
                    } else {
                        // Retries exhausted: bookkeeping advances so the
                        // failure is terminal until explicit retry, not a
                        // silent tight loop.
                        state.last_revision = interaction.revision;
                        state.last_specs.clone_from(&interaction.specs);
                    }
                }
            }
        }
    }
}

/// Refreshes backend-assigned trigger descriptions without a new session:
/// ShortcutsChanged carries the current session, so a read-only
/// ListShortcuts updates the displayed actual triggers.
async fn refresh_actual_triggers(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    session: &str,
    activation_routes: &Arc<Mutex<ActivationRoutes>>,
    event_tx: &UnboundedSender<HotkeyEvent>,
    state: &mut ReconcileState,
) {
    let triggers = match list_shortcuts_async(connection, config, session).await {
        Ok(triggers) => triggers,
        Err((error, detail)) => {
            tracing::warn!("shortcut trigger refresh failed ({error}): {detail}");
            return;
        }
    };
    let (current, revision) = activation_routes
        .lock()
        .map(|routes| (routes.routes.clone(), routes.revision))
        .unwrap_or_default();
    for spec in state.last_specs.clone() {
        if let Some(runtime) = current.get(&spec.shortcut_id) {
            let trigger = triggers.get(&spec.shortcut_id).cloned().unwrap_or_default();
            publish_ack(
                event_tx,
                &mut state.published,
                ShortcutAcknowledgement {
                    revision,
                    prompt_id: spec.prompt_id,
                    runtime_id: Some(*runtime),
                    status: ShortcutStatus::Active,
                    trigger_description: trigger,
                    error_code: None,
                },
            );
        }
    }
}

/// Cancels the in-flight interaction against the real remote objects:
/// closes the actual request handle once known (and the predicted handle
/// when cancellation precedes the reply — plus any other buffered handle
/// under our prefix), then an already-created session. Dropping the
/// future cancels the underlying GIO call. The reply is sent after
/// release, never for merely enqueueing the command.
async fn cancel_interaction_remote(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    interaction: &PendingInteraction,
    _generation: u64,
) {
    let (predicted, actual, session) = match interaction.progress.lock() {
        Ok(state) => (
            state.predicted_path.clone(),
            state.actual_path.clone(),
            state.session.clone(),
        ),
        Err(_) => (String::new(), None, None),
    };
    match actual {
        Some(actual) => {
            close_request_async(connection, config, &actual).await;
            if actual != predicted && !predicted.is_empty() {
                close_request_async(connection, config, &predicted).await;
            }
        }
        None => {
            if !predicted.is_empty() {
                close_request_async(connection, config, &predicted).await;
            }
        }
    }
    if let Some(session) = session {
        close_session_async(connection, config, &session).await;
    }
}

/// Ordered terminal shutdown of one in-flight interaction.
async fn shutdown_interaction(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    interaction: &PendingInteraction,
) {
    cancel_interaction_remote(connection, config, interaction, 0).await;
}

/// Extracts a well-formed D-Bus error name token from a GDBus error
/// message. Matches exact `org.freedesktop.*` tokens instead of arbitrary
/// substrings, so unrelated log text can never classify a failure.
fn remote_dbus_error_name(message: &str) -> Option<&str> {
    message
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_'))
        .find(|token| {
            token.starts_with("org.freedesktop.DBus.Error.")
                || token.starts_with("org.freedesktop.portal.Error.")
        })
}

/// Classifies a failed portal method call into a stable error code.
/// Transport problems come from the typed `IOErrorEnum`; remote D-Bus
/// failures come from the exact error-name token.
fn classify_method_error(error: &glib::Error) -> (DesktopErrorCode, String) {
    if error.kind::<gio::IOErrorEnum>() == Some(gio::IOErrorEnum::TimedOut) {
        return (DesktopErrorCode::RequestTimeout, error.to_string());
    }
    if matches!(
        error.kind::<gio::IOErrorEnum>(),
        Some(
            gio::IOErrorEnum::Closed
                | gio::IOErrorEnum::BrokenPipe
                | gio::IOErrorEnum::NotConnected
                | gio::IOErrorEnum::ConnectionRefused
                | gio::IOErrorEnum::HostUnreachable
                | gio::IOErrorEnum::NetworkUnreachable
        )
    ) {
        return (DesktopErrorCode::Disconnected, error.to_string());
    }
    match remote_dbus_error_name(&error.to_string()) {
        Some("org.freedesktop.DBus.Error.ServiceUnknown")
        | Some("org.freedesktop.DBus.Error.NameHasNoOwner")
        | Some("org.freedesktop.DBus.Error.NoReply")
        | Some("org.freedesktop.DBus.Error.SpawnFailed") => {
            (DesktopErrorCode::BackendUnavailable, error.to_string())
        }
        Some("org.freedesktop.DBus.Error.AccessDenied") => {
            (DesktopErrorCode::RequestDenied, error.to_string())
        }
        Some("org.freedesktop.portal.Error.NotAllowed") => {
            (DesktopErrorCode::RequestDenied, error.to_string())
        }
        _ => (DesktopErrorCode::BackendUnavailable, error.to_string()),
    }
}

fn register_host_application(connection: &gio::DBusConnection, app_id: &str) -> Result<(), String> {
    let options = empty_options();
    let parameters = glib::Variant::tuple_from_iter([app_id.to_variant(), options]);

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
            // A host without the Registry interface falls back to legacy
            // portal discovery; any other failure is a real error.
            match remote_dbus_error_name(&error.to_string()) {
                Some("org.freedesktop.DBus.Error.UnknownMethod")
                | Some("org.freedesktop.DBus.Error.UnknownInterface")
                | Some("org.freedesktop.DBus.Error.UnknownObject") => {
                    tracing::debug!(
                        "Host portal registry is unavailable; continuing with legacy portal discovery"
                    );
                    Ok(())
                }
                _ => Err(format!("host portal registration failed: {error}")),
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

/// Validates the full Activated tuple (session path, shortcut id,
/// timestamp, options) against the current revision and routes the
/// activation immediately as a [`HotkeyEvent`]. Stale sessions, unknown
/// ids and malformed tuples are rejected; tokens are never logged.
fn subscribe_activated(
    connection: &gio::DBusConnection,
    activation_routes: Arc<Mutex<ActivationRoutes>>,
    event_tx: UnboundedSender<HotkeyEvent>,
) {
    #[allow(deprecated)]
    connection.signal_subscribe(
        Some(PORTAL_BUS),
        Some(GLOBAL_SHORTCUTS_INTERFACE),
        Some("Activated"),
        Some(PORTAL_PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |_connection, _sender, _path, _interface, _signal, parameters| {
            if parameters.n_children() != 4 {
                return;
            }
            let Some(session) = parameters.child_value(0).get::<ObjectPath>() else {
                return;
            };
            let shortcut_id = parameters.child_value(1);
            let Some(shortcut_id) = shortcut_id.str() else {
                return;
            };
            if parameters.child_value(2).get::<u64>().is_none() {
                return;
            }
            if parameters.child_value(3).type_().as_str() != "a{sv}" {
                return;
            }
            let event = match match_activation(&activation_routes, session.as_str(), shortcut_id) {
                Some((revision, runtime_id)) => HotkeyEvent::Activated {
                    revision,
                    runtime_id,
                },
                None => return,
            };
            let _ = event_tx.unbounded_send(event);
        },
    );
}

/// Matches one Activated signal against the current revision and routes.
/// Rejects stale sessions, unknown ids and deleted prompts; the caller
/// validates tuple arity and field types before calling.
fn match_activation(
    activation_routes: &Arc<Mutex<ActivationRoutes>>,
    session: &str,
    shortcut_id: &str,
) -> Option<(u64, u32)> {
    let routes = activation_routes.lock().ok()?;
    if routes.session.as_deref() != Some(session) {
        return None;
    }
    routes
        .routes
        .get(shortcut_id)
        .copied()
        .map(|runtime_id| (routes.revision, runtime_id))
}

/// Forwards ShortcutsChanged for live sessions; the coordinator refreshes
/// backend-assigned triggers read-only instead of assuming the requested
/// preferred trigger was assigned.
fn subscribe_shortcuts_changed(
    connection: &gio::DBusConnection,
    changed_tx: UnboundedSender<String>,
) -> gio::SignalSubscriptionId {
    #[allow(deprecated)]
    connection.signal_subscribe(
        Some(PORTAL_BUS),
        Some(GLOBAL_SHORTCUTS_INTERFACE),
        Some("ShortcutsChanged"),
        Some(PORTAL_PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |_connection, _sender, _path, _interface, _signal, parameters| {
            if parameters.n_children() < 1 {
                return;
            }
            let Some(session) = parameters.child_value(0).get::<ObjectPath>() else {
                return;
            };
            let _ = changed_tx.unbounded_send(session.as_str().to_string());
        },
    )
}

/// Watches the portal bus owner: loss invalidates routes at once, return
/// reconciles once. Transport recovery stays bounded; user-denied
/// revisions are never rebound automatically.
fn subscribe_portal_owner(
    connection: &gio::DBusConnection,
    owner_tx: UnboundedSender<bool>,
) -> gio::SignalSubscriptionId {
    #[allow(deprecated)]
    connection.signal_subscribe(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        Some(PORTAL_BUS),
        gio::DBusSignalFlags::NONE,
        move |_connection, _sender, _path, _interface, _signal, parameters| {
            if parameters.n_children() != 3 {
                return;
            }
            let new_owner = parameters
                .child_value(2)
                .str()
                .unwrap_or_default()
                .to_string();
            let _ = owner_tx.unbounded_send(!new_owner.is_empty());
        },
    )
}

fn subscribe_session_closed(
    connection: &gio::DBusConnection,
    closed_tx: UnboundedSender<String>,
) -> gio::SignalSubscriptionId {
    #[allow(deprecated)]
    connection.signal_subscribe(
        Some(PORTAL_BUS),
        Some(SESSION_INTERFACE),
        Some("Closed"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |_connection, _sender, path, _interface, _signal, _parameters| {
            // Currency is decided by the coordinator against the live
            // session; every close is forwarded.
            let _ = closed_tx.unbounded_send(path.to_string());
        },
    )
}

/// Long-lived Response router for one interaction. Installed before the
/// method call so an immediate backend Response is buffered instead of
/// lost; every signal under our request prefix is validated and queued in
/// arrival order. Buffer overflow is reported through `overflowed` and
/// tears the interaction down as InvalidResponse — never an unbounded
/// signal cache.
fn subscribe_response_router(
    connection: &gio::DBusConnection,
    request_prefix: &str,
    response_tx: futures_channel::mpsc::Sender<ResponseEvent>,
    overflowed: Arc<std::sync::atomic::AtomicBool>,
) -> RouterGuard {
    let prefix = request_prefix.to_string();
    #[allow(deprecated)]
    let id = connection.signal_subscribe(
        Some(PORTAL_BUS),
        Some(REQUEST_INTERFACE),
        Some("Response"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |_connection, _sender, path, _interface, _signal, parameters| {
            if !path.starts_with(&prefix) {
                return;
            }
            if parameters.n_children() < 2 {
                return;
            }
            let Some(code) = parameters.child_value(0).get::<u32>() else {
                return;
            };
            let results = parameters.child_value(1);
            let mut response_tx = response_tx.clone();
            let event = ResponseEvent {
                path: path.to_string(),
                code,
                results,
            };
            if response_tx.try_send(event).is_err() {
                overflowed.store(true, std::sync::atomic::Ordering::Release);
            }
        },
    );
    RouterGuard {
        connection: connection.clone(),
        id: Some(id),
    }
}

/// Owns one Response router subscription; dropping releases it exactly
/// once on every terminal branch, including future cancellation.
struct RouterGuard {
    connection: gio::DBusConnection,
    id: Option<gio::SignalSubscriptionId>,
}

impl Drop for RouterGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            #[allow(deprecated)]
            self.connection.signal_unsubscribe(id);
        }
    }
}

/// Parses the accepted shortcut subset plus backend-assigned trigger
/// descriptions. GNOME owns the assigned triggers: an accepted id maps
/// to its `trigger_description` (which may differ from the requested
/// `preferred_trigger`), or `None` when the backend reports none.
fn parse_accepted(results: &glib::Variant) -> Result<HashMap<String, Option<String>>, String> {
    let results = glib::VariantDict::new(Some(results));
    let shortcuts = results
        .lookup_value("shortcuts", None)
        .ok_or_else(|| "BindShortcuts response did not include shortcuts".to_string())?;
    if shortcuts.type_().as_str() != "a(sa{sv})" {
        return Err("BindShortcuts response shortcuts had invalid type".to_string());
    }
    let mut accepted = HashMap::new();
    for index in 0..shortcuts.n_children() {
        let entry = shortcuts.child_value(index);
        if entry.n_children() != 2 {
            return Err("BindShortcuts response contained an invalid shortcut".to_string());
        }
        let id = entry
            .child_value(0)
            .get::<String>()
            .ok_or_else(|| "BindShortcuts response shortcut id was invalid".to_string())?;
        let properties = glib::VariantDict::new(Some(&entry.child_value(1)));
        let trigger = properties
            .lookup::<String>("trigger_description")
            .map_err(|error| format!("invalid trigger_description: {error}"))?;
        accepted.insert(id, trigger);
    }
    Ok(accepted)
}

/// Read-only trigger query for one live session. Never opens consent.
async fn list_shortcuts_async(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    session: &str,
) -> Result<HashMap<String, Option<String>>, (DesktopErrorCode, String)> {
    let handle_token = portal_token("list");
    let predicted = request_path(connection, &handle_token)
        .map_err(|detail| (DesktopErrorCode::InvalidResponse, detail))?;
    let prefix =
        request_prefix(connection).map_err(|detail| (DesktopErrorCode::InvalidResponse, detail))?;
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| {
            (
                DesktopErrorCode::InvalidResponse,
                format!("invalid portal session path: {error}"),
            )
        })?
        .to_variant();
    let options = glib::VariantDict::new(None);
    options.insert("handle_token", handle_token.as_str());
    let parameters = glib::Variant::tuple_from_iter([session_path, options.end()]);
    let progress = Arc::new(Mutex::new(InteractionProgress {
        predicted_path: predicted.clone(),
        actual_path: None,
        session: Some(session.to_string()),
        parent_generation: None,
    }));
    let results = portal_roundtrip(
        connection,
        config,
        &prefix,
        &progress,
        GLOBAL_SHORTCUTS_INTERFACE,
        "ListShortcuts",
        &parameters,
    )
    .await?;
    parse_accepted(&results).map_err(|detail| (DesktopErrorCode::InvalidResponse, detail))
}

/// Creates one portal session. The Response router is installed before
/// the method call; the method reply carries the actual request handle,
/// which is validated against our request prefix (a legacy backend may
/// emit a different handle before the reply — handled, never trusted
/// blindly).
async fn create_session_async(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    request_prefix: &str,
    progress: &Arc<Mutex<InteractionProgress>>,
) -> Result<String, (DesktopErrorCode, String)> {
    let handle_token = portal_token("create");
    let session_token = portal_token("session");
    let predicted = request_path(connection, &handle_token)
        .map_err(|detail| (DesktopErrorCode::InvalidResponse, detail))?;
    {
        if let Ok(mut state) = progress.lock() {
            state.predicted_path.clone_from(&predicted);
        }
    }
    let options = glib::VariantDict::new(None);
    options.insert("handle_token", handle_token.as_str());
    options.insert("session_handle_token", session_token.as_str());
    let parameters = glib::Variant::tuple_from_iter([options.end()]);

    let results = portal_roundtrip(
        connection,
        config,
        request_prefix,
        progress,
        GLOBAL_SHORTCUTS_INTERFACE,
        "CreateSession",
        &parameters,
    )
    .await?;

    let results = glib::VariantDict::new(Some(&results));
    results
        .lookup::<String>("session_handle")
        .map_err(|error| {
            (
                DesktopErrorCode::InvalidResponse,
                format!("invalid CreateSession response: {error}"),
            )
        })?
        .ok_or_else(|| {
            (
                DesktopErrorCode::InvalidResponse,
                "CreateSession response did not include session_handle".to_string(),
            )
        })
}

/// Binds the complete desired set with exactly one BindShortcuts call per
/// session. The parent is unparented in this stage; user-initiated consent
/// parenting lands with the P5 reconciliation wiring.
async fn bind_shortcuts_async(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    request_prefix: &str,
    session: &str,
    bindings: &HashMap<u32, PortalBinding>,
    parent_handle: Option<&str>,
    progress: &Arc<Mutex<InteractionProgress>>,
) -> Result<HashMap<String, Option<String>>, (DesktopErrorCode, String)> {
    let handle_token = portal_token("bind");
    let predicted = request_path(connection, &handle_token)
        .map_err(|detail| (DesktopErrorCode::InvalidResponse, detail))?;
    {
        // Cancellation from here on targets the bind request; the
        // completed create request needs no release.
        if let Ok(mut state) = progress.lock() {
            state.predicted_path.clone_from(&predicted);
            state.actual_path = None;
        }
    }
    let session_path = ObjectPath::try_from(session)
        .map_err(|error| {
            (
                DesktopErrorCode::InvalidResponse,
                format!("invalid portal session path: {error}"),
            )
        })?
        .to_variant();
    let shortcuts = shortcuts_variant(bindings)
        .map_err(|detail| (DesktopErrorCode::InvalidResponse, detail))?;
    let options = glib::VariantDict::new(None);
    options.insert("handle_token", handle_token.as_str());
    // A validated parent window parents user-initiated consent. No blank
    // parent is ever substituted for a failed live-window export: without
    // a lease the coordinator defers to AwaitingWindow instead.
    let parent = parent_handle.unwrap_or("").to_variant();
    let parameters =
        glib::Variant::tuple_from_iter([session_path, shortcuts, parent, options.end()]);

    let results = portal_roundtrip(
        connection,
        config,
        request_prefix,
        &progress,
        GLOBAL_SHORTCUTS_INTERFACE,
        "BindShortcuts",
        &parameters,
    )
    .await?;
    parse_accepted(&results).map_err(|detail| (DesktopErrorCode::InvalidResponse, detail))
}

/// One portal method call plus its Response, sharing the interaction
/// progress for cancellation. The router is installed before the call so
/// an immediate backend Response is buffered; the reply's actual handle
/// selects the buffered entry (or the predicted path when they match).
async fn portal_roundtrip(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    request_prefix: &str,
    progress: &Arc<Mutex<InteractionProgress>>,
    interface: &str,
    method: &str,
    parameters: &glib::Variant,
) -> Result<glib::Variant, (DesktopErrorCode, String)> {
    let (response_tx, mut response_rx) =
        futures_channel::mpsc::channel::<ResponseEvent>(config.queue_capacity);
    let overflowed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _router = subscribe_response_router(
        connection,
        request_prefix,
        response_tx,
        Arc::clone(&overflowed),
    );

    let reply = connection
        .call_future(
            Some(PORTAL_BUS),
            PORTAL_PATH,
            interface,
            method,
            Some(parameters),
            None,
            gio::DBusCallFlags::NONE,
            config.method_timeout_ms.min(i32::MAX as u64) as i32,
        )
        .await
        .map_err(|error| {
            let (code, detail) = classify_method_error(&error);
            (code, format!("{method} call failed: {detail}"))
        })?;

    let actual = parse_request_handle(&reply, request_prefix)
        .map_err(|detail| (DesktopErrorCode::InvalidResponse, detail))?;
    {
        if let Ok(mut state) = progress.lock() {
            state.actual_path = Some(actual.clone());
        }
    }

    let outcome = await_response_for(
        connection,
        config,
        &mut response_rx,
        &overflowed,
        &actual,
        Duration::from_millis(config.response_timeout_ms),
        config.queue_capacity,
    )
    .await;
    if outcome
        .as_ref()
        .is_err_and(|(code, _)| *code == DesktopErrorCode::RequestTimeout)
    {
        // A timed-out consent must not linger: release the remote Request
        // before reporting, mirroring the cancellation path.
        close_request_async(connection, config, &actual).await;
    }
    outcome
}

/// Extracts the actual request handle from a portal method reply and
/// validates it against our request prefix.
fn parse_request_handle(reply: &glib::Variant, request_prefix: &str) -> Result<String, String> {
    if reply.n_children() < 1 {
        return Err("portal method reply carried no request handle".to_string());
    }
    let actual = reply
        .child_value(0)
        .get::<ObjectPath>()
        .ok_or_else(|| "portal method reply request handle was invalid".to_string())?;
    let actual = actual.as_str().to_string();
    if !actual.starts_with(request_prefix) {
        return Err(format!(
            "portal request handle {actual:?} is outside our request namespace"
        ));
    }
    Ok(actual)
}

/// Waits for the Response on the actual request path. A buffered early
/// Response (emitted before the method reply) is consumed first; a late
/// Response after cancellation is never resurrected because the router is
/// dropped with this future.
async fn await_response_for(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    response_rx: &mut futures_channel::mpsc::Receiver<ResponseEvent>,
    overflowed: &Arc<std::sync::atomic::AtomicBool>,
    actual_path: &str,
    timeout: Duration,
    max_buffered: usize,
) -> Result<glib::Variant, (DesktopErrorCode, String)> {
    let _ = (connection, config);
    // Early responses buffered while the method reply was in flight.
    let mut buffered: Vec<ResponseEvent> = Vec::new();
    let mut timeout = glib::timeout_future(timeout).fuse();
    loop {
        if overflowed.load(std::sync::atomic::Ordering::Acquire) {
            return Err((
                DesktopErrorCode::QueueOverflow,
                "portal response router overflowed; tearing down interaction".to_string(),
            ));
        }
        while let Ok(event) = response_rx.try_recv() {
            if event.path == actual_path {
                return interpret_response(event);
            }
            if let Err(error) = buffer_unmatched_response(&mut buffered, event, max_buffered) {
                return Err((error, "portal response buffer overflowed".to_string()));
            }
        }
        if let Some(index) = buffered.iter().position(|event| event.path == actual_path) {
            let event = buffered.remove(index);
            return interpret_response(event);
        }
        futures_util::select! {
            event = response_rx.next().fuse() => {
                match event {
                    Some(event) if event.path == actual_path => {
                        return interpret_response(event);
                    }
                    Some(event) => {
                        if let Err(error) =
                            buffer_unmatched_response(&mut buffered, event, max_buffered)
                        {
                            return Err((error, "portal response buffer overflowed".to_string()));
                        }
                    }
                    None => {
                        return Err((
                            DesktopErrorCode::Disconnected,
                            "portal response stream ended before a response arrived".to_string(),
                        ));
                    }
                }
            }
            _ = timeout => {
                return Err((
                    DesktopErrorCode::RequestTimeout,
                    format!("portal request {actual_path} timed out"),
                ));
            }
        }
    }
}

fn buffer_unmatched_response(
    buffered: &mut Vec<ResponseEvent>,
    event: ResponseEvent,
    max_buffered: usize,
) -> Result<(), DesktopErrorCode> {
    if buffered.len() >= max_buffered {
        return Err(DesktopErrorCode::QueueOverflow);
    }
    buffered.push(event);
    Ok(())
}

fn interpret_response(event: ResponseEvent) -> Result<glib::Variant, (DesktopErrorCode, String)> {
    match event.code {
        0 => {
            if event.results.type_().as_str() == "a{sv}" {
                return Ok(event.results);
            }
            Err((
                DesktopErrorCode::InvalidResponse,
                "portal response results must have a{sv} type".to_string(),
            ))
        }
        1 => Err((
            DesktopErrorCode::RequestCancelled,
            "portal request was cancelled".to_string(),
        )),
        2 => Err((
            DesktopErrorCode::RequestDenied,
            "portal request was denied".to_string(),
        )),
        other => Err((
            DesktopErrorCode::InvalidResponse,
            format!("portal request failed with response code {other}"),
        )),
    }
}

/// Best-effort remote release of one Request object, awaited within the
/// method timeout so cancellation acknowledgement implies delivery.
async fn close_request_async(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    request_path: &str,
) {
    if ObjectPath::try_from(request_path).is_err() {
        return;
    }
    let _ = connection
        .call_future(
            Some(PORTAL_BUS),
            request_path,
            REQUEST_INTERFACE,
            "Close",
            None,
            None,
            gio::DBusCallFlags::NONE,
            config.method_timeout_ms.min(i32::MAX as u64) as i32,
        )
        .await;
}

async fn close_session_async(
    connection: &gio::DBusConnection,
    config: &DesktopConfig,
    session: &str,
) {
    if ObjectPath::try_from(session).is_err() {
        return;
    }
    let _ = connection
        .call_future(
            Some(PORTAL_BUS),
            session,
            SESSION_INTERFACE,
            "Close",
            None,
            None,
            gio::DBusCallFlags::NONE,
            config.method_timeout_ms.min(i32::MAX as u64) as i32,
        )
        .await;
}

/// Request namespace for this connection: every request path we predict
/// or accept must live under it.
fn request_prefix(connection: &gio::DBusConnection) -> Result<String, String> {
    let unique_name = connection
        .unique_name()
        .ok_or_else(|| "session bus connection has no unique name".to_string())?;
    let sender = unique_name.trim_start_matches(':').replace('.', "_");
    Ok(format!("/org/freedesktop/portal/desktop/request/{sender}/"))
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

        let (_command_tx, command_rx) = futures_channel::mpsc::unbounded();
        let (status_tx, mut status_rx) = futures_channel::mpsc::unbounded::<BackendState>();
        let (event_tx, _event_rx) =
            futures_channel::mpsc::unbounded::<crate::desktop::HotkeyEvent>();
        let runtime = Arc::new(
            DesktopRuntime::from_parts(
                DesktopConfig::default(),
                "io.github.roymejia2217.JamePrompt.Test",
                std::path::PathBuf::from("/tmp"),
                None,
                crate::desktop::RuntimeMode::Production,
                false,
            )
            .expect("test runtime"),
        );
        portal_worker(command_rx, status_tx, event_tx, runtime);
        let mut saw_unavailable = false;
        while let Ok(state) = status_rx.try_recv() {
            if let BackendState::Unavailable(error) = state {
                assert!(
                    error.contains("cannot connect to the session bus")
                        || error.contains("cannot resolve the session bus"),
                    "{error}"
                );
                saw_unavailable = true;
            }
        }
        assert!(
            saw_unavailable,
            "missing bus must report backend unavailable"
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
    fn parse_accepted_uses_only_portal_accepted_subset() {
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
        let accepted = parse_accepted(&response.end()).unwrap();
        assert_eq!(
            accepted
                .keys()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from(["prompt_a".to_string(), "prompt_b".to_string()])
        );
        assert!(accepted.values().all(|trigger| trigger.is_none()));
    }

    #[test]
    fn parse_accepted_accepts_empty_subset() {
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
        assert!(parse_accepted(&response.end()).unwrap().is_empty());
    }

    #[test]
    fn real_portal_response_results_are_accepted_without_boxing() {
        let results = glib::VariantDict::new(None).end();
        let interpreted = interpret_response(ResponseEvent {
            path: "/org/freedesktop/portal/desktop/request/test/request".into(),
            code: 0,
            results: results.clone(),
        })
        .expect("real portal a{sv} response");

        assert_eq!(interpreted.type_().as_str(), "a{sv}");
    }

    #[test]
    fn boxed_portal_response_results_are_rejected() {
        let results = glib::VariantDict::new(None).end();
        let boxed = results.to_variant();
        assert!(interpret_response(ResponseEvent {
            path: "/org/freedesktop/portal/desktop/request/test/request".into(),
            code: 0,
            results: boxed,
        })
        .is_err());
    }

    #[test]
    fn replacement_gap_publishes_nonactive_for_old_routes() {
        let (event_tx, mut event_rx) = futures_channel::mpsc::unbounded();
        let old = DesiredBinding {
            runtime_id: 1,
            prompt_id: "old".to_string(),
            shortcut_id: "old-shortcut".to_string(),
            preferred_trigger: "CTRL+ALT+1".to_string(),
            description: "Old".to_string(),
        };
        let current = DesiredBinding {
            runtime_id: 2,
            prompt_id: "new".to_string(),
            shortcut_id: "new-shortcut".to_string(),
            preferred_trigger: "CTRL+ALT+2".to_string(),
            description: "New".to_string(),
        };
        let mut state = ReconcileState {
            revision: 2,
            specs: vec![current.clone()],
            last_revision: 1,
            last_specs: vec![old.clone()],
            published: HashMap::from([(
                old.prompt_id.clone(),
                ShortcutAcknowledgement {
                    revision: 1,
                    prompt_id: old.prompt_id.clone(),
                    runtime_id: Some(old.runtime_id),
                    status: ShortcutStatus::Active,
                    trigger_description: Some("CTRL+ALT+1".to_string()),
                    error_code: None,
                },
            )]),
            ..ReconcileState::default()
        };
        let routes = Arc::new(Mutex::new(ActivationRoutes {
            session: Some("old-session".to_string()),
            revision: 1,
            routes: HashMap::from([(old.shortcut_id.clone(), old.runtime_id)]),
        }));

        publish_replacement_gap(&event_tx, &routes, &mut state);

        let events: Vec<HotkeyEvent> = std::iter::from_fn(|| event_rx.try_recv().ok()).collect();
        assert!(events.iter().any(|event| {
            matches!(
                event,
                HotkeyEvent::Acknowledged(ack)
                    if ack.prompt_id == "old" && ack.status == ShortcutStatus::Inactive
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                HotkeyEvent::Acknowledged(ack)
                    if ack.prompt_id == "new" && ack.status != ShortcutStatus::Active
            )
        }));
        assert!(routes.lock().expect("routes").routes.is_empty());
    }

    #[test]
    fn activation_match_rejects_stale_session_unknown_id_and_deleted_prompt() {
        let routes = Arc::new(Mutex::new(ActivationRoutes {
            session: Some("/session/current".into()),
            revision: 7,
            routes: HashMap::from([("prompt_a".into(), 42)]),
        }));
        assert_eq!(
            match_activation(&routes, "/session/current", "prompt_a"),
            Some((7, 42))
        );
        assert_eq!(match_activation(&routes, "/session/old", "prompt_a"), None);
        assert_eq!(
            match_activation(&routes, "/session/current", "prompt_missing"),
            None
        );
        // Deleted prompt: route removed, activation rejected.
        routes.lock().unwrap().routes.remove("prompt_a");
        assert_eq!(
            match_activation(&routes, "/session/current", "prompt_a"),
            None
        );
    }

    #[test]
    fn activation_match_rejects_missing_session() {
        let routes = Arc::new(Mutex::new(ActivationRoutes::default()));
        assert_eq!(
            match_activation(&routes, "/session/current", "prompt_a"),
            None
        );
    }

    #[test]
    fn fake_child_metadata_change_advances_revision_without_rebind_shape() {
        let base = DesiredBinding {
            runtime_id: 1,
            prompt_id: "prompt".to_string(),
            shortcut_id: "shortcut".to_string(),
            preferred_trigger: "CTRL+ALT+1".to_string(),
            description: "Before".to_string(),
        };
        let mut changed = base.clone();
        changed.description = "After".to_string();
        let mut state = ReconcileState::default();
        state.last_revision = 1;
        state.last_specs = vec![base];
        state.revision = 2;
        state.specs = vec![changed];

        assert!(state.effective_changed());
        assert!(state.same_accelerators());
    }

    #[test]
    fn fake_child_metadata_change_advances_published_ack_revision() {
        let (event_tx, mut event_rx) = futures_channel::mpsc::unbounded();
        let spec = DesiredBinding {
            runtime_id: 1,
            prompt_id: "prompt".to_string(),
            shortcut_id: "shortcut".to_string(),
            preferred_trigger: "CTRL+ALT+1".to_string(),
            description: "After".to_string(),
        };
        let ack = ShortcutAcknowledgement {
            revision: 1,
            prompt_id: spec.prompt_id.clone(),
            runtime_id: Some(spec.runtime_id),
            status: ShortcutStatus::Active,
            trigger_description: Some("CTRL+ALT+1".to_string()),
            error_code: None,
        };
        let mut state = ReconcileState {
            revision: 2,
            specs: vec![spec.clone()],
            published: HashMap::from([(spec.prompt_id.clone(), ack)]),
            ..ReconcileState::default()
        };

        reaffirm_published(&event_tx, &mut state);

        let HotkeyEvent::Acknowledged(updated) = event_rx.try_recv().expect("acknowledgement")
        else {
            panic!("expected acknowledgement");
        };
        assert_eq!(updated.revision, 2);
    }

    #[test]
    fn fake_child_owner_loss_forces_recovery_of_unchanged_desired_set() {
        let (event_tx, mut event_rx) = futures_channel::mpsc::unbounded();
        let spec = DesiredBinding {
            runtime_id: 1,
            prompt_id: "prompt".to_string(),
            shortcut_id: "shortcut".to_string(),
            preferred_trigger: "CTRL+ALT+1".to_string(),
            description: "Prompt".to_string(),
        };
        let mut state = ReconcileState {
            revision: 1,
            last_revision: 1,
            specs: vec![spec.clone()],
            last_specs: vec![spec],
            ..ReconcileState::default()
        };

        publish_session_loss(&event_tx, &mut state);

        assert_ne!(state.revision, state.last_revision);
        assert!(matches!(
            event_rx.try_recv().expect("session loss acknowledgement"),
            HotkeyEvent::Acknowledged(ShortcutAcknowledgement {
                status: ShortcutStatus::Unavailable,
                ..
            })
        ));
    }

    #[test]
    fn fake_child_explicit_retry_after_cancel_unblocks_once() {
        let mut state = ReconcileState {
            revision: 3,
            blocked_revisions: HashSet::from([3]),
            transport_retries: 4,
            ..ReconcileState::default()
        };
        let mut debounce = None;

        state.refresh(&mut debounce);

        assert!(!state.blocked_revisions.contains(&3));
        assert_eq!(state.transport_retries, 0);
        assert!(debounce.is_some());
    }

    #[test]
    fn fake_child_transport_retry_has_a_finite_backoff_budget() {
        let mut state = ReconcileState::default();
        let mut delays = Vec::new();
        while let Some(delay) = state.transport_backoff() {
            delays.push(delay);
            state.transport_retries += 1;
        }

        assert_eq!(delays.len(), 5);
        assert_eq!(state.transport_backoff(), None);
    }

    #[test]
    fn fake_child_session_closed_invalidates_current_routes_only() {
        let routes = Arc::new(Mutex::new(ActivationRoutes {
            session: Some("/session/current".to_string()),
            revision: 2,
            routes: HashMap::from([("shortcut".to_string(), 1)]),
        }));
        let mut active_session = Some("/session/current".to_string());

        assert!(session_closed(
            &routes,
            &mut active_session,
            "/session/current"
        ));
        assert!(active_session.is_none());
        assert!(routes.lock().expect("routes").routes.is_empty());
    }

    #[test]
    fn fake_child_session_closed_ignores_stale_session() {
        let routes = Arc::new(Mutex::new(ActivationRoutes {
            session: Some("/session/current".to_string()),
            revision: 2,
            routes: HashMap::from([("shortcut".to_string(), 1)]),
        }));
        let mut active_session = Some("/session/current".to_string());

        assert!(!session_closed(
            &routes,
            &mut active_session,
            "/session/old"
        ));
        assert_eq!(active_session.as_deref(), Some("/session/current"));
        assert_eq!(routes.lock().expect("routes").routes.len(), 1);
    }

    #[test]
    fn fake_child_unmatched_response_stream_has_total_bound() {
        let mut buffered = Vec::new();
        for _ in 0..4 {
            buffer_unmatched_response(
                &mut buffered,
                ResponseEvent {
                    path: "/other/request".to_string(),
                    code: 0,
                    results: glib::VariantDict::new(None).end(),
                },
                4,
            )
            .expect("buffer remains within the combined bound");
        }
        assert_eq!(
            buffer_unmatched_response(
                &mut buffered,
                ResponseEvent {
                    path: "/other/request-5".to_string(),
                    code: 0,
                    results: glib::VariantDict::new(None).end(),
                },
                4,
            ),
            Err(DesktopErrorCode::QueueOverflow)
        );
    }
}

/// Deterministic fake GlobalShortcuts backend on a private bus.
///
/// Implements only the public protocol surface the worker uses
/// (Registry.Register, Properties.Get version, CreateSession,
/// BindShortcuts, Request.Close/Response, Session.Close) with
/// controllable response timing/codes and an optional alternate request
/// handle. Never the production backend; assertions bind to the protocol
/// contract, not to implementation internals.
#[cfg(test)]
mod fake_portal {
    use super::*;
    use std::sync::{Arc, Mutex};

    const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum ResponseMode {
        ImmediateAccept,
        DelayedAccept(u64),
        AlternateHandleAccept,
    }

    impl ResponseMode {
        fn from_env() -> Self {
            match std::env::var("JAME_PROMPT_FAKE_PORTAL_MODE").as_deref() {
                Ok("delayed-accept") => {
                    let ms = std::env::var("JAME_PROMPT_FAKE_PORTAL_DELAY_MS")
                        .ok()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(4000);
                    Self::DelayedAccept(ms)
                }
                Ok("alternate-handle") => Self::AlternateHandleAccept,
                _ => Self::ImmediateAccept,
            }
        }
    }

    #[derive(Debug, Default)]
    pub(super) struct FakeCalls {
        pub(super) creates: Vec<String>,
        pub(super) binds: Vec<String>,
        pub(super) lists: Vec<String>,
        pub(super) request_closes: Vec<String>,
        pub(super) session_closes: Vec<String>,
        pub(super) responses: Vec<(String, u32)>,
    }

    pub(super) struct FakePortal {
        _connection: gio::DBusConnection,
        _registrations: Vec<gio::RegistrationId>,
        _owner: gio::OwnerId,
        _context_thread: Option<std::thread::JoinHandle<()>>,
        _quit_tx: Option<std::sync::mpsc::Sender<()>>,
        pub(super) calls: Arc<Mutex<FakeCalls>>,
    }

    fn portal_xml() -> gio::DBusNodeInfo {
        gio::DBusNodeInfo::for_xml(
            r#"<node>
              <interface name="org.freedesktop.host.portal.Registry">
                <method name="Register">
                  <arg type="s" name="app_id" direction="in"/>
                  <arg type="a{sv}" name="options" direction="in"/>
                </method>
              </interface>
              <interface name="org.freedesktop.DBus.Properties">
                <method name="Get">
                  <arg type="s" name="interface" direction="in"/>
                  <arg type="s" name="property" direction="in"/>
                  <arg type="v" name="value" direction="out"/>
                </method>
              </interface>
              <interface name="org.freedesktop.portal.GlobalShortcuts">
                <method name="CreateSession">
                  <arg type="a{sv}" name="options" direction="in"/>
                  <arg type="o" name="request" direction="out"/>
                </method>
                <method name="BindShortcuts">
                  <arg type="o" name="session" direction="in"/>
                  <arg type="a(sa{sv})" name="shortcuts" direction="in"/>
                  <arg type="s" name="parent_window" direction="in"/>
                  <arg type="a{sv}" name="options" direction="in"/>
                  <arg type="o" name="request" direction="out"/>
                </method>
                <method name="ListShortcuts">
                  <arg type="o" name="session_handle" direction="in"/>
                  <arg type="a{sv}" name="options" direction="in"/>
                  <arg type="o" name="request" direction="out"/>
                </method>
                <signal name="ShortcutsChanged">
                  <arg type="o" name="session_handle"/>
                  <arg type="a(sa{sv})" name="shortcuts"/>
                </signal>
              </interface>
            </node>"#,
        )
        .expect("portal test interface xml")
    }

    fn request_xml() -> gio::DBusNodeInfo {
        gio::DBusNodeInfo::for_xml(
            r#"<node>
              <interface name="org.freedesktop.portal.Request">
                <method name="Close"/>
                <signal name="Response">
                  <arg type="u" name="code"/>
                  <arg type="a{sv}" name="results"/>
                </signal>
              </interface>
            </node>"#,
        )
        .expect("request test interface xml")
    }

    fn session_xml() -> gio::DBusNodeInfo {
        gio::DBusNodeInfo::for_xml(
            r#"<node>
              <interface name="org.freedesktop.portal.Session">
                <method name="Close"/>
                <signal name="Closed">
                  <arg type="s" name="detail"/>
                </signal>
              </interface>
            </node>"#,
        )
        .expect("session test interface xml")
    }

    fn no_properties(_: gio::DBusConnection, _: &str, _: &str, _: &str, _: &str) -> glib::Variant {
        0u8.to_variant()
    }

    /// Empty method reply body for methods without out arguments.
    /// D-Bus requires an (empty) tuple here, never NULL.
    fn empty_response() -> glib::Variant {
        glib::Variant::tuple_from_iter(std::iter::empty::<glib::Variant>())
    }

    fn reject_property(
        _: gio::DBusConnection,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: glib::Variant,
    ) -> bool {
        false
    }

    fn sender_prefix(sender: &str) -> String {
        let sender = sender.trim_start_matches(':').replace('.', "_");
        format!("/org/freedesktop/portal/desktop/request/{sender}/")
    }

    fn handle_token(parameters: &glib::Variant, child: usize) -> Option<String> {
        let options = glib::VariantDict::new(Some(&parameters.child_value(child)));
        options.lookup::<String>("handle_token").ok().flatten()
    }

    fn requested_ids(shortcuts: &glib::Variant) -> Vec<String> {
        let mut ids = Vec::new();
        for index in 0..shortcuts.n_children() {
            let entry = shortcuts.child_value(index);
            if entry.n_children() > 0 {
                if let Some(id) = entry.child_value(0).get::<String>() {
                    ids.push(id);
                }
            }
        }
        ids
    }

    impl FakePortal {
        /// Builds the fake on a context-owning thread (name ownership
        /// callbacks need a running loop) and returns it for use from the
        /// scenario thread.
        pub(super) fn start() -> Self {
            let calls = Arc::new(Mutex::new(FakeCalls::default()));
            let mode = ResponseMode::from_env();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let (parts_tx, parts_rx) = std::sync::mpsc::channel();
            let (quit_tx, quit_rx) = std::sync::mpsc::channel::<()>();
            let setup_calls = Arc::clone(&calls);
            let context_thread = std::thread::Builder::new()
                .name("fake-portal-bus".into())
                .spawn(move || {
                    let context = glib::MainContext::new();
                    let main_loop = glib::MainLoop::new(Some(&context), false);
                    let quit_loop = main_loop.clone();
                    std::thread::spawn(move || {
                        let _ = quit_rx.recv();
                        quit_loop.quit();
                    });
                    let _ = context.with_thread_default(|| {
                        let connection =
                            gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
                                .expect("fake portal bus connection");
                        let mut registrations = Vec::new();
                        Self::register_desktop(&connection, &setup_calls, mode, &mut registrations);
                        // Readiness is reported from the acquired callback
                        // itself: blocking this thread here would stall the
                        // context the callback needs to be dispatched on.
                        // Registrations and the owner move out through a
                        // separate channel before the loop starts; dropping
                        // them would release the name/objects.
                        let ready_connection = connection.clone();
                        let owner = gio::bus_own_name_on_connection(
                            &connection,
                            PORTAL_BUS,
                            gio::BusNameOwnerFlags::NONE,
                            move |_, _| {
                                let _ = ready_tx.send((true, ready_connection.clone()));
                            },
                            |_, _| {},
                        );
                        let _ = parts_tx.send((registrations, owner));
                        main_loop.run();
                    });
                })
                .expect("fake portal context thread");
            let (registrations, owner) = parts_rx
                .recv_timeout(Duration::from_secs(15))
                .expect("fake portal registrations");
            let (acquired, connection) = ready_rx
                .recv_timeout(Duration::from_secs(15))
                .expect("fake portal name acquisition");
            assert!(acquired, "fake portal must own its bus name");
            Self {
                _connection: connection,
                _registrations: registrations,
                _owner: owner,
                _context_thread: Some(context_thread),
                _quit_tx: Some(quit_tx),
                calls,
            }
        }

        fn register_desktop(
            connection: &gio::DBusConnection,
            calls: &Arc<Mutex<FakeCalls>>,
            mode: ResponseMode,
            registrations: &mut Vec<gio::RegistrationId>,
        ) {
            let node = portal_xml();
            let desktop = node
                .lookup_interface("org.freedesktop.host.portal.Registry")
                .expect("registry interface");
            let properties = node
                .lookup_interface("org.freedesktop.DBus.Properties")
                .expect("properties interface");
            let shortcuts = node
                .lookup_interface("org.freedesktop.portal.GlobalShortcuts")
                .expect("shortcuts interface");

            registrations.push(
                connection
                    .register_object(
                        DESKTOP_PATH,
                        &desktop,
                        move |_, _, _, _, method, _, invocation| {
                            if method == "Register" {
                                invocation.return_value(Some(&empty_response()));
                            }
                        },
                        no_properties,
                        reject_property,
                    )
                    .expect("register fake registry"),
            );
            registrations.push(
                connection
                    .register_object(
                        DESKTOP_PATH,
                        &properties,
                        move |_, _, _, _, method, _, invocation| {
                            if method == "Get" {
                                let boxed = 1u32.to_variant().to_variant();
                                invocation
                                    .return_value(Some(&glib::Variant::tuple_from_iter([boxed])));
                            } else {
                                invocation.return_value(None);
                            }
                        },
                        no_properties,
                        reject_property,
                    )
                    .expect("register fake properties"),
            );
            let handler_calls = Arc::clone(calls);
            let handler_connection = connection.clone();
            registrations.push(
                connection
                    .register_object(
                        DESKTOP_PATH,
                        &shortcuts,
                        move |connection, sender, _, _, method, parameters, invocation| {
                            Self::handle_shortcut_method(
                                &connection,
                                sender,
                                method,
                                &parameters,
                                invocation,
                                &handler_calls,
                                &handler_connection,
                                mode,
                            );
                        },
                        no_properties,
                        reject_property,
                    )
                    .expect("register fake shortcuts"),
            );
        }

        #[allow(clippy::too_many_arguments)]
        fn handle_shortcut_method(
            _connection: &gio::DBusConnection,
            sender: &str,
            method: &str,
            parameters: &glib::Variant,
            invocation: gio::DBusMethodInvocation,
            calls: &Arc<Mutex<FakeCalls>>,
            service_connection: &gio::DBusConnection,
            mode: ResponseMode,
        ) {
            let prefix = sender_prefix(sender);
            match method {
                "CreateSession" => {
                    let token = handle_token(parameters, 0).unwrap_or_else(|| "no_token".into());
                    let actual = format!("{prefix}{token}");
                    if let Ok(mut recorded) = calls.lock() {
                        recorded.creates.push(actual.clone());
                    }
                    Self::register_request_object(service_connection, &actual, calls);
                    let session = format!(
                        "/org/freedesktop/portal/desktop/session/fake/{}",
                        sender.trim_start_matches(':').replace('.', "_")
                    );
                    Self::register_session_object(service_connection, &session, calls);
                    let results = {
                        let dict = glib::VariantDict::new(None);
                        dict.insert("session_handle", session.as_str());
                        dict.end()
                    };
                    // Session creation is answered immediately; the bind
                    // carries the timing under test.
                    Self::send_response(service_connection, &actual, 0, &results, calls);
                    let reply =
                        glib::Variant::tuple_from_iter([ObjectPath::try_from(actual.as_str())
                            .expect("fake request path")
                            .to_variant()]);
                    invocation.return_value(Some(&reply));
                }
                "BindShortcuts" => {
                    let token = handle_token(parameters, 3).unwrap_or_else(|| "no_token".into());
                    let predicted = format!("{prefix}{token}");
                    let actual = match mode {
                        ResponseMode::AlternateHandleAccept => format!("{prefix}alt_{token}"),
                        _ => predicted.clone(),
                    };
                    if let Ok(mut recorded) = calls.lock() {
                        recorded.binds.push(actual.clone());
                    }
                    Self::register_request_object(service_connection, &actual, calls);
                    let ids = requested_ids(&parameters.child_value(1));
                    let results = {
                        let dict = glib::VariantDict::new(None);
                        let template = glib::Variant::tuple_from_iter([
                            String::new().to_variant(),
                            glib::VariantDict::new(None).end(),
                        ]);
                        let entries: Vec<glib::Variant> = ids
                            .iter()
                            .map(|id| {
                                glib::Variant::tuple_from_iter([
                                    id.to_variant(),
                                    glib::VariantDict::new(None).end(),
                                ])
                            })
                            .collect();
                        let array = glib::Variant::array_from_iter_with_type(
                            template.type_(),
                            entries.iter(),
                        );
                        dict.insert_value("shortcuts", &array);
                        dict.end()
                    };
                    match mode {
                        ResponseMode::DelayedAccept(ms) => {
                            Self::emit_response_delayed(
                                service_connection,
                                &actual,
                                0,
                                &results,
                                calls,
                                Duration::from_millis(ms),
                            );
                        }
                        _ => {
                            // Emit before the method reply returns: the
                            // worker must buffer, not drop, this early
                            // Response.
                            Self::send_response(service_connection, &actual, 0, &results, calls);
                        }
                    }
                    let reply =
                        glib::Variant::tuple_from_iter([ObjectPath::try_from(actual.as_str())
                            .expect("fake request path")
                            .to_variant()]);
                    invocation.return_value(Some(&reply));
                    let _ = predicted;
                }
                "ListShortcuts" => {
                    let token = handle_token(parameters, 1).unwrap_or_else(|| "no_token".into());
                    let actual = format!("{prefix}{token}");
                    if let Ok(mut recorded) = calls.lock() {
                        recorded.lists.push(actual.clone());
                    }
                    Self::register_request_object(service_connection, &actual, calls);
                    let results = {
                        let dict = glib::VariantDict::new(None);
                        let template = glib::Variant::tuple_from_iter([
                            String::new().to_variant(),
                            glib::VariantDict::new(None).end(),
                        ]);
                        let shortcuts = glib::Variant::array_from_iter_with_type(
                            template.type_(),
                            std::iter::empty::<glib::Variant>(),
                        );
                        dict.insert_value("shortcuts", &shortcuts);
                        dict.end()
                    };
                    Self::send_response(service_connection, &actual, 0, &results, calls);
                    let reply =
                        glib::Variant::tuple_from_iter([ObjectPath::try_from(actual.as_str())
                            .expect("fake request path")
                            .to_variant()]);
                    invocation.return_value(Some(&reply));
                }
                // Unknown methods are left unanswered so GDBus reports a
                // structured UnknownMethod error to the caller.
                _ => {}
            }
        }

        fn register_request_object(
            connection: &gio::DBusConnection,
            path: &str,
            calls: &Arc<Mutex<FakeCalls>>,
        ) {
            let node = request_xml();
            let info = node
                .lookup_interface("org.freedesktop.portal.Request")
                .expect("request interface");
            let recorded = Arc::clone(calls);
            let path_owned = path.to_string();
            let _ = connection.register_object(
                path,
                &info,
                move |_, _, _, _, method, _, invocation| {
                    if method == "Close" {
                        if let Ok(mut recorded) = recorded.lock() {
                            recorded.request_closes.push(path_owned.clone());
                        }
                    }
                    invocation.return_value(Some(&empty_response()));
                },
                no_properties,
                reject_property,
            );
        }

        fn register_session_object(
            connection: &gio::DBusConnection,
            path: &str,
            calls: &Arc<Mutex<FakeCalls>>,
        ) {
            let node = session_xml();
            let info = node
                .lookup_interface("org.freedesktop.portal.Session")
                .expect("session interface");
            let recorded = Arc::clone(calls);
            let path_owned = path.to_string();
            let _ = connection.register_object(
                path,
                &info,
                move |_, _, _, _, method, _, invocation| {
                    if method == "Close" {
                        if let Ok(mut recorded) = recorded.lock() {
                            recorded.session_closes.push(path_owned.clone());
                        }
                    }
                    invocation.return_value(Some(&empty_response()));
                },
                no_properties,
                reject_property,
            );
        }

        fn emit_response_delayed(
            connection: &gio::DBusConnection,
            path: &str,
            code: u32,
            results: &glib::Variant,
            calls: &Arc<Mutex<FakeCalls>>,
            delay: Duration,
        ) {
            let connection = connection.clone();
            let path = path.to_string();
            let results = results.clone();
            let recorded = Arc::clone(calls);
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                Self::send_response(&connection, &path, code, &results, &recorded);
            });
        }

        fn send_response(
            connection: &gio::DBusConnection,
            path: &str,
            code: u32,
            results: &glib::Variant,
            calls: &Arc<Mutex<FakeCalls>>,
        ) {
            let parameters = glib::Variant::tuple_from_iter([code.to_variant(), results.clone()]);
            let _ = connection.emit_signal(
                None,
                path,
                REQUEST_INTERFACE,
                "Response",
                Some(&parameters),
            );
            if let Ok(mut recorded) = calls.lock() {
                recorded.responses.push((path.to_string(), code));
            }
        }
    }

    /// Runs `scenario` in a child on a private bus with the fake portal.
    /// The parent asserts the child succeeds; the child performs real
    /// D-Bus assertions with bounded deadlines, never skips.
    fn with_fake_portal(
        child_test: &str,
        mode: &str,
        delay_ms: Option<u64>,
        response_timeout_ms: Option<u64>,
        scenario: impl FnOnce() + Send + 'static,
    ) {
        const CHILD: &str = "JAME_PROMPT_TEST_FAKE_PORTAL";
        if std::env::var_os(CHILD).is_none() {
            let mut command = std::process::Command::new("dbus-run-session");
            command
                .arg("--")
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", child_test, "--nocapture"])
                .env(CHILD, "1")
                .env("JAME_PROMPT_FAKE_PORTAL_MODE", mode)
                .env_remove("DBUS_SESSION_BUS_ADDRESS")
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY");
            if let Some(ms) = delay_ms {
                command.env("JAME_PROMPT_FAKE_PORTAL_DELAY_MS", ms.to_string());
            }
            if let Some(ms) = response_timeout_ms {
                command.env("JAME_PROMPT_DESKTOP_RESPONSE_TIMEOUT_MS", ms.to_string());
            }
            let output = command.output().expect("dbus-run-session is required");
            assert!(
                output.status.success(),
                "fake portal test failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        scenario();
    }

    /// Bounded wait helper: polls `ready` until the deadline. A deadline
    /// expiry is an explicit test failure, never a skipped assertion.
    fn wait_until(deadline: Duration, ready: impl Fn() -> bool, what: &str) {
        let started = Instant::now();
        while !ready() {
            if started.elapsed() >= deadline {
                panic!("timed out waiting for fake portal condition: {what}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) mod tests {
        use super::super::fake_portal::*;
        use crate::desktop::ShortcutSpec;

        fn reconcile_prompts(
            service: &PortalHotkeyService,
            revision: u64,
            prompts: &[(&str, &str, &str)],
        ) {
            let specs: Vec<ShortcutSpec> = prompts
                .iter()
                .map(|(id, name, trigger)| ShortcutSpec {
                    prompt_id: id.to_string(),
                    description: name.to_string(),
                    preferred_trigger: trigger.to_string(),
                })
                .collect();
            let receipt = service
                .reconcile(revision, &specs)
                .expect("reconcile queues");
            assert!(
                receipt.rejected_prompt_ids.is_empty(),
                "test fixture must be valid"
            );
        }

        fn new_fake_service() -> (
            PortalHotkeyService,
            futures_channel::mpsc::UnboundedReceiver<crate::desktop::HotkeyEvent>,
        ) {
            let (event_tx, event_rx) = futures_channel::mpsc::unbounded();
            let runtime = Arc::new(
                DesktopRuntime::from_parts(
                    DesktopConfig::from_env().unwrap_or_default(),
                    "io.github.roymejia2217.JamePrompt.Test",
                    std::path::PathBuf::from("/tmp"),
                    None,
                    crate::desktop::RuntimeMode::Production,
                    false,
                )
                .expect("fake runtime validates"),
            );
            let service =
                PortalHotkeyService::new_with_event(event_tx, runtime).expect("service starts");
            (service, event_rx)
        }

        /// Drains backend events until `prompt_id` is acknowledged Active
        /// (or the deadline expires). Tests the real ack path, not an
        /// internal route table.
        fn wait_for_active(
            events: &mut futures_channel::mpsc::UnboundedReceiver<crate::desktop::HotkeyEvent>,
            prompt_id: &str,
            deadline: Duration,
        ) {
            use crate::desktop::{HotkeyEvent, ShortcutStatus};
            let started = Instant::now();
            loop {
                while let Ok(event) = events.try_recv() {
                    if let HotkeyEvent::Acknowledged(ack) = event {
                        if ack.prompt_id == prompt_id && ack.status == ShortcutStatus::Active {
                            return;
                        }
                    }
                }
                if started.elapsed() >= deadline {
                    panic!("timed out waiting for {prompt_id} to become active");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        /// Collects backend events for `window` and asserts `prompt_id`
        /// was never acknowledged Active in that span.
        fn assert_never_active(
            events: &mut futures_channel::mpsc::UnboundedReceiver<crate::desktop::HotkeyEvent>,
            prompt_id: &str,
            window: Duration,
        ) {
            use crate::desktop::{HotkeyEvent, ShortcutStatus};
            let started = Instant::now();
            while started.elapsed() < window {
                while let Ok(event) = events.try_recv() {
                    if let HotkeyEvent::Acknowledged(ack) = event {
                        assert!(
                            !(ack.prompt_id == prompt_id && ack.status == ShortcutStatus::Active),
                            "{prompt_id} must never become active"
                        );
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn wait_for_cancel(
            mut receiver: oneshot::Receiver<CancelOutcome>,
            deadline: Duration,
        ) -> CancelOutcome {
            let started = Instant::now();
            loop {
                if let Ok(Some(outcome)) = receiver.try_recv() {
                    return outcome;
                }
                if started.elapsed() >= deadline {
                    panic!("timed out waiting for cancel acknowledgement");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn wait_for_shutdown(mut receiver: oneshot::Receiver<()>, deadline: Duration) {
            let started = Instant::now();
            loop {
                if matches!(receiver.try_recv(), Ok(Some(()))) {
                    return;
                }
                if started.elapsed() >= deadline {
                    panic!("timed out waiting for ordered shutdown");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn fake_child_cancel_during_consent_closes_request() {
            with_fake_portal(
                "hotkeys::portal::fake_portal::tests::fake_child_cancel_during_consent_closes_request",
                "delayed-accept",
                Some(5000),
                Some(30000),
                || {
                    let fake = FakePortal::start();
                    let (service, mut events) =
                        new_fake_service();
                    // A live parent window is available in these scenarios;
                    // without one, new shortcuts correctly defer to
                    // AwaitingWindow and no bind is issued.
                    service
                        .set_parent(Some(crate::desktop::ParentContext {
                            generation: 1,
                            handle: "wayland:faketest".to_string(),
                        }))
                        .expect("parent accepted");
                    reconcile_prompts(&service, 1, &[("prompt-cancel", "Cancel me", "Ctrl+Alt+F8")]);
                    wait_until(
                        Duration::from_secs(10),
                        || {
                            fake.calls
                                .lock()
                                .map(|calls| !calls.binds.is_empty())
                                .unwrap_or(false)
                        },
                        "bind request to reach the fake backend",
                    );
                    let outcome = wait_for_cancel(
                        service.cancel_interaction(1),
                        Duration::from_secs(10),
                    );
                    assert_eq!(outcome, CancelOutcome::Released);
                    wait_until(
                        Duration::from_secs(10),
                        || {
                            fake.calls
                                .lock()
                                .map(|calls| !calls.request_closes.is_empty())
                                .unwrap_or(false)
                        },
                        "Request.Close for the cancelled bind",
                    );
                    let closes = fake
                        .calls
                        .lock()
                        .map(|calls| calls.session_closes.len())
                        .unwrap_or(usize::MAX);
                    assert_eq!(closes, 1, "candidate session closed exactly once");
                    assert_never_active(&mut events, "prompt-cancel", Duration::from_millis(500));
                },
            );
        }

        #[test]
        fn fake_child_timeout_closes_request_and_session() {
            with_fake_portal(
                "hotkeys::portal::fake_portal::tests::fake_child_timeout_closes_request_and_session",
                "delayed-accept",
                Some(5000),
                Some(1000),
                || {
                    let fake = FakePortal::start();
                    let (service, mut events) =
                        new_fake_service();
                    // A live parent window is available in these scenarios;
                    // without one, new shortcuts correctly defer to
                    // AwaitingWindow and no bind is issued.
                    service
                        .set_parent(Some(crate::desktop::ParentContext {
                            generation: 1,
                            handle: "wayland:faketest".to_string(),
                        }))
                        .expect("parent accepted");
                    reconcile_prompts(&service, 1, &[("prompt-timeout", "Time me out", "Ctrl+Alt+F9")]);
                    // Response timeout (1s) fires well before the 5s fake
                    // response; the worker must close both remote objects.
                    wait_until(
                        Duration::from_secs(15),
                        || {
                            fake.calls
                                .lock()
                                .map(|calls| !calls.request_closes.is_empty())
                                .unwrap_or(false)
                        },
                        "Request.Close after response timeout",
                    );
                    wait_until(
                        Duration::from_secs(15),
                        || {
                            fake.calls
                                .lock()
                                .map(|calls| !calls.session_closes.is_empty())
                                .unwrap_or(false)
                        },
                        "candidate Session.Close after response timeout",
                    );
                    assert_never_active(&mut events, "prompt-timeout", Duration::from_millis(500));
                },
            );
        }

        #[test]
        fn fake_child_update_during_consent_is_serviced() {
            with_fake_portal(
                "hotkeys::portal::fake_portal::tests::fake_child_update_during_consent_is_serviced",
                "delayed-accept",
                Some(2000),
                Some(30000),
                || {
                    let fake = FakePortal::start();
                    let (service, mut events) = new_fake_service();
                    // A live parent window is available in these scenarios;
                    // without one, new shortcuts correctly defer to
                    // AwaitingWindow and no bind is issued.
                    service
                        .set_parent(Some(crate::desktop::ParentContext {
                            generation: 1,
                            handle: "wayland:faketest".to_string(),
                        }))
                        .expect("parent accepted");
                    reconcile_prompts(&service, 1, &[("prompt-first", "First", "Ctrl+Alt+F8")]);
                    wait_until(
                        Duration::from_secs(10),
                        || {
                            fake.calls
                                .lock()
                                .map(|calls| !calls.binds.is_empty())
                                .unwrap_or(false)
                        },
                        "first bind request to reach the fake backend",
                    );
                    // Queue an edit while consent is pending: the command
                    // stream must stay responsive, not stall for the
                    // timeout.
                    reconcile_prompts(
                        &service,
                        2,
                        &[
                            ("prompt-first", "First", "Ctrl+Alt+F8"),
                            ("prompt-second", "Second", "Ctrl+Alt+F9"),
                        ],
                    );
                    wait_until(
                        Duration::from_secs(30),
                        || {
                            fake.calls
                                .lock()
                                .map(|calls| calls.binds.len() >= 2)
                                .unwrap_or(false)
                        },
                        "second bind after the first completes",
                    );
                    wait_for_active(&mut events, "prompt-first", Duration::from_secs(30));
                    wait_for_active(&mut events, "prompt-second", Duration::from_secs(30));
                },
            );
        }

        #[test]
        fn fake_child_shutdown_during_consent_is_bounded() {
            with_fake_portal(
                "hotkeys::portal::fake_portal::tests::fake_child_shutdown_during_consent_is_bounded",
                "delayed-accept",
                Some(8000),
                Some(30000),
                || {
                    let fake = FakePortal::start();
                    let (service, _events) = new_fake_service();
                    // A live parent window is available in these scenarios;
                    // without one, new shortcuts correctly defer to
                    // AwaitingWindow and no bind is issued.
                    service
                        .set_parent(Some(crate::desktop::ParentContext {
                            generation: 1,
                            handle: "wayland:faketest".to_string(),
                        }))
                        .expect("parent accepted");
                    reconcile_prompts(&service, 1, &[("prompt-shutdown", "Shut me down", "Ctrl+Alt+F8")]);
                    wait_until(
                        Duration::from_secs(10),
                        || {
                            fake.calls
                                .lock()
                                .map(|calls| !calls.binds.is_empty())
                                .unwrap_or(false)
                        },
                        "bind request to reach the fake backend",
                    );
                    let started = Instant::now();
                    wait_for_shutdown(service.shutdown(), Duration::from_secs(15));
                    assert!(
                        started.elapsed() < Duration::from_secs(15),
                        "shutdown must not wait out the consent timeout"
                    );
                    assert!(
                        fake.calls
                            .lock()
                            .map(|calls| !calls.request_closes.is_empty())
                            .unwrap_or(false),
                        "shutdown closes the pending request"
                    );
                },
            );
        }

        #[test]
        fn fake_child_early_response_before_reply_is_used() {
            with_fake_portal(
                "hotkeys::portal::fake_portal::tests::fake_child_early_response_before_reply_is_used",
                "immediate-accept",
                None,
                Some(30000),
                || {
                    let fake = FakePortal::start();
                    let (service, mut events) =
                        new_fake_service();
                    // A live parent window is available in these scenarios;
                    // without one, new shortcuts correctly defer to
                    // AwaitingWindow and no bind is issued.
                    service
                        .set_parent(Some(crate::desktop::ParentContext {
                            generation: 1,
                            handle: "wayland:faketest".to_string(),
                        }))
                        .expect("parent accepted");
                    reconcile_prompts(&service, 1, &[("prompt-early", "Early bird", "Ctrl+Alt+F8")]);
                    // The fake emits Response before returning the method
                    // reply; the router must buffer and match it.
                    wait_for_active(&mut events, "prompt-early", Duration::from_secs(15));
                    assert!(
                        fake.calls
                            .lock()
                            .map(|calls| calls.request_closes.is_empty())
                            .unwrap_or(false),
                        "successful early response must not close its request"
                    );
                },
            );
        }

        #[test]
        fn fake_child_serial_replacement_closes_old_before_new_bind() {
            with_fake_portal(
                "hotkeys::portal::fake_portal::tests::fake_child_serial_replacement_closes_old_before_new_bind",
                "immediate-accept",
                None,
                Some(30000),
                || {
                    let fake = FakePortal::start();
                    let (service, mut events) =
                        new_fake_service();
                    service
                        .set_parent(Some(crate::desktop::ParentContext {
                            generation: 1,
                            handle: "wayland:faketest".to_string(),
                        }))
                        .expect("parent accepted");
                    reconcile_prompts(&service, 1, &[("prompt-a", "A", "Ctrl+Alt+F8")]);
                    wait_for_active(&mut events, "prompt-a", Duration::from_secs(15));
                    let (creates, binds) = fake
                        .calls
                        .lock()
                        .map(|calls| (calls.creates.len(), calls.binds.len()))
                        .unwrap_or_default();
                    assert_eq!((creates, binds), (1, 1), "one session, one bind per revision");
                    // Changed set: serial replacement closes the old
                    // session before the new bind, exactly once.
                    reconcile_prompts(
                        &service,
                        2,
                        &[
                            ("prompt-a", "A", "Ctrl+Alt+F8"),
                            ("prompt-b", "B", "Ctrl+Alt+F9"),
                        ],
                    );
                    wait_for_active(&mut events, "prompt-a", Duration::from_secs(15));
                    wait_for_active(&mut events, "prompt-b", Duration::from_secs(15));
                    let calls = fake.calls.lock().expect("fake calls");
                    assert_eq!(calls.creates.len(), 2, "exactly one replacement session");
                    assert_eq!(calls.binds.len(), 2, "exactly one bind per session");
                    assert_eq!(
                        calls.session_closes.len(),
                        1,
                        "old session closed exactly once before replacement"
                    );
                },
            );
        }

        #[test]
        fn fake_child_alternate_handle_is_matched() {
            with_fake_portal(
                "hotkeys::portal::fake_portal::tests::fake_child_alternate_handle_is_matched",
                "alternate-handle",
                None,
                Some(30000),
                || {
                    let fake = FakePortal::start();
                    let (service, mut events) = new_fake_service();
                    // A live parent window is available in these scenarios;
                    // without one, new shortcuts correctly defer to
                    // AwaitingWindow and no bind is issued.
                    service
                        .set_parent(Some(crate::desktop::ParentContext {
                            generation: 1,
                            handle: "wayland:faketest".to_string(),
                        }))
                        .expect("parent accepted");
                    reconcile_prompts(&service, 1, &[("prompt-alt", "Alt handle", "Ctrl+Alt+F8")]);
                    wait_for_active(&mut events, "prompt-alt", Duration::from_secs(15));
                    let binds = fake
                        .calls
                        .lock()
                        .map(|calls| calls.binds.clone())
                        .unwrap_or_default();
                    assert_eq!(binds.len(), 1);
                    assert!(
                        binds[0].contains("alt_"),
                        "fake must use a non-predicted handle, got {}",
                        binds[0]
                    );
                },
            );
        }
    }
}
