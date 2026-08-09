from __future__ import annotations

from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    target = Path(path)
    text = target.read_text()
    count = text.count(old)
    if count != expected:
        raise SystemExit(
            f"{path}: expected {expected} occurrence(s) of replacement block, found {count}"
        )
    target.write_text(text.replace(old, new, expected))


replace_exact(
    "src/hotkeys.rs",
    '''    pub fn register(&self, key_str: &str) -> Option<u32> {
        perf::measure("hotkeys.register", || match &self.backend {
            HotkeyBackend::Native(service) => service.register(key_str),
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => service.register(key_str),
        })
    }
''',
    '''    pub fn register(&self, prompt_id: &str, prompt_name: &str, key_str: &str) -> Option<u32> {
        perf::measure("hotkeys.register", || match &self.backend {
            HotkeyBackend::Native(service) => service.register(key_str),
            #[cfg(target_os = "linux")]
            HotkeyBackend::Portal(service) => service.register(prompt_id, prompt_name, key_str),
        })
    }
''',
)

replace_exact(
    "src/ui.rs",
    '''                            if let Some(hotkey_id) = svc.register(hk) {
                                hotkey_ids.insert(hotkey_id, p.id.clone());
                            }
''',
    '''                            if let Some(hotkey_id) = svc.register(&p.id, &p.name, hk) {
                                hotkey_ids.insert(hotkey_id, p.id.clone());
                            }
''',
)
replace_exact(
    "src/ui.rs",
    '''                        if let Some(hotkey_id) = svc.register(hotkey) {
                            self.hotkey_ids.insert(hotkey_id, prompt.id.clone());
                        }
''',
    '''                        if let Some(hotkey_id) =
                            svc.register(&prompt.id, &prompt.name, hotkey)
                        {
                            self.hotkey_ids.insert(hotkey_id, prompt.id.clone());
                        }
''',
)
replace_exact(
    "src/ui.rs",
    '''                                                    if let Some(hotkey_id) = svc.register(hk) {
                                                        self.hotkey_ids
                                                            .insert(hotkey_id, editing_id.clone());
                                                    }
''',
    '''                                                    if let Some(hotkey_id) =
                                                        svc.register(&prompt.id, &prompt.name, hk)
                                                    {
                                                        self.hotkey_ids
                                                            .insert(hotkey_id, editing_id.clone());
                                                    }
''',
)
replace_exact(
    "src/ui.rs",
    '''                                                    if let Some(hotkey_id) = svc.register(hk) {
                                                        self.hotkey_ids
                                                            .insert(hotkey_id, new_id.clone());
                                                    }
''',
    '''                                                    if let Some(hotkey_id) =
                                                        svc.register(&new_id, &prompt.name, hk)
                                                    {
                                                        self.hotkey_ids
                                                            .insert(hotkey_id, new_id.clone());
                                                    }
''',
)
replace_exact(
    "src/ui.rs",
    '''                                        if let Some(hotkey_id) = svc.register(hk) {
                                            self.hotkey_ids.insert(hotkey_id, p.id.clone());
                                        }
''',
    '''                                        if let Some(hotkey_id) =
                                            svc.register(&p.id, &p.name, hk)
                                        {
                                            self.hotkey_ids.insert(hotkey_id, p.id.clone());
                                        }
''',
)

replace_exact(
    "src/ui.rs",
    '''    main_window_id: Option<iced::window::Id>,
    content_width: f32,
''',
    '''    main_window_id: Option<iced::window::Id>,
    window_open_pending: bool,
    content_width: f32,
''',
)
replace_exact(
    "src/ui.rs",
    '''            main_window_id: None,
            content_width: WINDOW_INITIAL_WIDTH,
''',
    '''            main_window_id: None,
            window_open_pending: false,
            content_width: WINDOW_INITIAL_WIDTH,
''',
    expected=3,
)
replace_exact(
    "src/ui.rs",
    '''                iced::window::get_latest().map(Message::ShowWindow)
''',
    '''                Task::done(Message::ShowWindow(None))
''',
)
replace_exact(
    "src/ui.rs",
    '''    pub fn title(&self) -> String {
        APP_NAME.to_string()
    }

    pub fn theme(&self) -> Theme {
''',
    '''    pub fn title(&self) -> String {
        APP_NAME.to_string()
    }

    pub(crate) fn begin_window_open(&mut self) -> bool {
        if self.main_window_id.is_some() || self.window_open_pending {
            return false;
        }
        self.window_open_pending = true;
        true
    }

    pub(crate) fn record_window_opened(&mut self, id: iced::window::Id) {
        self.main_window_id = Some(id);
        self.window_open_pending = false;
    }

    pub(crate) fn record_window_closed(&mut self, id: iced::window::Id) {
        if self.main_window_id == Some(id) {
            self.main_window_id = None;
        }
        self.window_open_pending = false;
    }

    pub(crate) fn is_smoke_mode(&self) -> bool {
        self.smoke_mode
    }

    pub fn theme(&self) -> Theme {
''',
)
replace_exact(
    "src/ui.rs",
    '''                            TrayEvent::ShowRequested => {
                                iced::window::get_latest().map(Message::ShowWindow)
                            }
''',
    '''                            TrayEvent::ShowRequested => {
                                Task::done(Message::ShowWindow(self.main_window_id))
                            }
''',
)
replace_exact(
    "src/ui.rs",
    '''    #[test]
    fn close_requests_exit_in_smoke_mode() {
        assert!(should_exit_on_close_request(true));
        assert!(!should_exit_on_close_request(false));
    }

''',
    '''    #[test]
    fn close_requests_exit_in_smoke_mode() {
        assert!(should_exit_on_close_request(true));
        assert!(!should_exit_on_close_request(false));
    }

    #[test]
    fn window_lifecycle_coalesces_repeated_open_requests() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        assert!(app.begin_window_open());
        assert!(!app.begin_window_open());

        let id = iced::window::Id::unique();
        app.record_window_opened(id);
        assert!(!app.begin_window_open());

        app.record_window_closed(id);
        assert!(app.begin_window_open());
    }

    #[test]
    fn closing_stale_window_does_not_clear_current_window() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let current = iced::window::Id::unique();
        let stale = iced::window::Id::unique();
        app.record_window_opened(current);

        app.record_window_closed(stale);

        assert_eq!(app.main_window_id, Some(current));
        assert!(!app.window_open_pending);
    }

''',
)

replace_exact(
    "src/main.rs",
    '''fn update_application(app: &mut JamePromptApp, message: Message) -> Task<Message> {
    match window_lifecycle::classify(&message) {
        WindowLifecycleAction::Delegate => app.update(message),
        WindowLifecycleAction::Close(id) => window_lifecycle::close(id),
        WindowLifecycleAction::Open => {
            window_lifecycle::open().map(|id| Message::ShowWindow(Some(id)))
        }
        WindowLifecycleAction::Restore(id) => window_lifecycle::restore(id),
    }
}
''',
    '''fn update_application(app: &mut JamePromptApp, message: Message) -> Task<Message> {
    match window_lifecycle::classify(&message) {
        WindowLifecycleAction::Delegate => app.update(message),
        WindowLifecycleAction::Close(_id) if app.is_smoke_mode() => app.update(message),
        WindowLifecycleAction::Close(id) => {
            app.record_window_closed(id);
            window_lifecycle::close(id)
        }
        WindowLifecycleAction::Open => {
            if app.begin_window_open() {
                window_lifecycle::open().map(|id| Message::ShowWindow(Some(id)))
            } else {
                Task::none()
            }
        }
        WindowLifecycleAction::Restore(id) => {
            app.record_window_opened(id);
            window_lifecycle::restore(id)
        }
    }
}
''',
)
replace_exact(
    "src/main.rs",
    '''fn initial_window_task(start_minimized: bool) -> Task<Message> {
    if start_minimized {
        Task::none()
    } else {
        window_lifecycle::open().map(|id| Message::ShowWindow(Some(id)))
    }
}
''',
    '''fn initial_window_task(app: &mut JamePromptApp, start_minimized: bool) -> Task<Message> {
    if start_minimized || !app.begin_window_open() {
        Task::none()
    } else {
        window_lifecycle::open().map(|id| Message::ShowWindow(Some(id)))
    }
}
''',
)
replace_exact(
    "src/main.rs",
    '''                let (app, startup_task) =
                    JamePromptApp::new_with_hidden_start(start_minimized, ui_smoke);
                let window_task = initial_window_task(start_minimized);
                (app, Task::batch([startup_task, window_task]))
''',
    '''                let (mut app, startup_task) =
                    JamePromptApp::new_with_hidden_start(start_minimized, ui_smoke);
                let window_task = initial_window_task(&mut app, start_minimized);
                (app, Task::batch([startup_task, window_task]))
''',
)

replace_exact(
    "src/hotkeys/portal.rs",
    '''struct PortalBinding {
    preferred_trigger: String,
    description: String,
}
''',
    '''struct PortalBinding {
    shortcut_id: String,
    preferred_trigger: String,
    description: String,
}
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''        let bindings = Arc::new(Mutex::new(HashMap::new()));
        let (refresh_tx, refresh_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::sync_channel(1);
        let worker_bindings = Arc::clone(&bindings);

        thread::Builder::new()
            .name("jameprompt-wayland-shortcuts".into())
            .spawn(move || portal_worker(worker_bindings, refresh_rx, init_tx))
            .ok()?;
''',
    '''        let bindings = Arc::new(Mutex::new(HashMap::new()));
        let active_routes = Arc::new(Mutex::new(HashMap::new()));
        let (refresh_tx, refresh_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::sync_channel(1);
        let worker_bindings = Arc::clone(&bindings);
        let worker_active_routes = Arc::clone(&active_routes);

        thread::Builder::new()
            .name("jameprompt-wayland-shortcuts".into())
            .spawn(move || {
                portal_worker(
                    worker_bindings,
                    worker_active_routes,
                    refresh_rx,
                    init_tx,
                )
            })
            .ok()?;
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''    pub(super) fn register(&self, key_str: &str) -> Option<u32> {
        let preferred_trigger = to_portal_trigger(key_str)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let binding = PortalBinding {
            preferred_trigger,
            description: format!("Paste JamePrompt prompt ({key_str})"),
        };
''',
    '''    pub(super) fn register(
        &self,
        prompt_id: &str,
        prompt_name: &str,
        key_str: &str,
    ) -> Option<u32> {
        let preferred_trigger = to_portal_trigger(key_str)?;
        let shortcut_id = portal_shortcut_id(prompt_id, prompt_name, key_str)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let binding = PortalBinding {
            shortcut_id,
            preferred_trigger,
            description: portal_description(prompt_name),
        };
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''fn portal_worker(
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    refresh_rx: mpsc::Receiver<()>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
) {
''',
    '''fn portal_worker(
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    active_routes: Arc<Mutex<HashMap<String, u32>>>,
    refresh_rx: mpsc::Receiver<()>,
    init_tx: mpsc::SyncSender<Result<(), String>>,
) {
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''        register_host_application(&connection)?;
        ensure_global_shortcuts_available(&connection)?;
        subscribe_activated(&connection);
        let _ = init_tx.send(Ok(()));

        run_worker_loop(&context, &connection, bindings, refresh_rx);
''',
    '''        register_host_application(&connection)?;
        ensure_global_shortcuts_available(&connection)?;
        subscribe_activated(&connection, Arc::clone(&active_routes));
        let _ = init_tx.send(Ok(()));

        run_worker_loop(
            &context,
            &connection,
            bindings,
            active_routes,
            refresh_rx,
        );
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''fn run_worker_loop(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    refresh_rx: mpsc::Receiver<()>,
) {
''',
    '''fn run_worker_loop(
    context: &glib::MainContext,
    connection: &gio::DBusConnection,
    bindings: Arc<Mutex<HashMap<u32, PortalBinding>>>,
    active_routes: Arc<Mutex<HashMap<String, u32>>>,
    refresh_rx: mpsc::Receiver<()>,
) {
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''        if pending_refresh.is_some_and(|started| started.elapsed() >= REBIND_DEBOUNCE) {
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
''',
    '''        if pending_refresh.is_some_and(|started| started.elapsed() >= REBIND_DEBOUNCE) {
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
                    routes.clear();
                }
                continue;
            }

            match create_and_bind_session(context, connection, &snapshot) {
                Ok(session) => {
                    let routes = snapshot
                        .iter()
                        .map(|(id, binding)| (binding.shortcut_id.clone(), *id))
                        .collect();
                    if let Ok(mut active) = active_routes.lock() {
                        *active = routes;
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
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''fn subscribe_activated(connection: &gio::DBusConnection) {
''',
    '''fn subscribe_activated(
    connection: &gio::DBusConnection,
    active_routes: Arc<Mutex<HashMap<String, u32>>>,
) {
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''            let Some(id) = shortcut_id
                .strip_prefix("jameprompt-")
                .and_then(|value| value.parse::<u32>().ok())
            else {
                return;
            };
            if let Ok(mut queue) = event_queue().lock() {
                queue.push_back(id);
            }
''',
    '''            let id = {
                let Ok(routes) = active_routes.lock() else {
                    return;
                };
                let Some(id) = routes.get(shortcut_id).copied() else {
                    return;
                };
                id
            };
            if let Ok(mut queue) = event_queue().lock() {
                queue.push_back(id);
            }
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''        entries.push(glib::Variant::tuple_from_iter([
            format!("jameprompt-{id}").to_variant(),
            properties.end(),
        ]));
''',
    '''        entries.push(glib::Variant::tuple_from_iter([
            binding.shortcut_id.to_variant(),
            properties.end(),
        ]));
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''fn portal_token(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

pub(super) fn to_portal_trigger(hotkey: &str) -> Option<String> {
''',
    '''fn portal_token(prefix: &str) -> String {
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

fn portal_shortcut_id(prompt_id: &str, prompt_name: &str, hotkey: &str) -> Option<String> {
    let prompt_id = prompt_id.trim();
    if prompt_id.is_empty() {
        return None;
    }
    let prompt_name = prompt_name.trim();
    let hotkey = hotkey.trim();
    let values = [prompt_id, prompt_name, hotkey];
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
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''            PortalBinding {
                preferred_trigger: "CTRL+SHIFT+p".into(),
                description: "Paste prompt".into(),
            },
''',
    '''            PortalBinding {
                shortcut_id: "prompt_test".into(),
                preferred_trigger: "CTRL+SHIFT+p".into(),
                description: "Paste prompt: Test".into(),
            },
''',
)
replace_exact(
    "src/hotkeys/portal.rs",
    '''    #[test]
    fn shortcut_array_has_portal_signature() {
''',
    '''    #[test]
    fn shortcut_id_is_stable_until_prompt_binding_changes() {
        let original = portal_shortcut_id("prompt-123", "continuar-proyecto", "Shift+Alt+C");
        assert_eq!(
            original,
            portal_shortcut_id("prompt-123", "continuar-proyecto", "Shift+Alt+C")
        );
        assert_ne!(
            original,
            portal_shortcut_id("prompt-123", "continuar-proyecto", "Shift+Alt+V")
        );
        assert_ne!(
            original,
            portal_shortcut_id("prompt-123", "continuar proyecto", "Shift+Alt+C")
        );
        assert_ne!(
            original,
            portal_shortcut_id("prompt-456", "continuar-proyecto", "Shift+Alt+C")
        );
    }

    #[test]
    fn portal_description_identifies_the_prompt_without_duplicating_the_trigger() {
        let description = portal_description("  continuar-proyecto  ");
        assert_eq!(description, "Paste prompt: continuar-proyecto");
        assert!(!description.contains("Shift+Alt+C"));
    }

    #[test]
    fn portal_shortcut_requires_persistent_prompt_identity() {
        assert_eq!(portal_shortcut_id("", "Prompt", "Ctrl+P"), None);
    }

    #[test]
    fn shortcut_array_has_portal_signature() {
''',
)

print("Wayland shortcut UX patch applied successfully")
