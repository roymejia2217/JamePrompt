const BORDER_RADIUS: f32 = 8.0;
const MAIN_PANEL_PADDING: u16 = 15;
const DETAIL_PANEL_INNER_PADDING: u16 = 20;
const DETAIL_SECTION_SPACING: u16 = 15;
const HEADER_LOGO_SIZE: u16 = 28;
const HEADER_BRAND_SPACING: u16 = 10;
const MODAL_SECTION_SPACING: u16 = 10;
const PROMPT_PRIMARY_TOOLBAR_SPACING: u16 = 10;
const PROMPT_SECONDARY_TOOLBAR_SPACING: u16 = 8;
const PROMPT_FILTER_PICKER_WIDTH: f32 = 120.0;
const PROMPT_SORT_PICKER_WIDTH: f32 = 170.0;
const PROMPT_LIST_FAVORITE_INDICATOR_WIDTH: f32 = 24.0;
const PROMPT_LIST_FAVORITE_ICON_SIZE: u16 = 16;
const COMPACT_LAYOUT_WIDTH_THRESHOLD: f32 = 840.0;
const LIST_PANEL_SPACING: u16 = 10;
const PROMPT_LIST_SPACING: u16 = 5;
const PROMPT_CARD_SPACING: u16 = 8;
const PROMPT_CARD_PADDING: u16 = 10;
const PROMPT_PREVIEW_TEXT_SIZE: u16 = 14;
const PROMPT_DETAIL_TITLE_SIZE: u16 = 24;
const PROMPT_CONTENT_TEXT_SIZE: u16 = 14;
const PROMPT_METADATA_TEXT_SIZE: u16 = 12;
const EMPTY_STATE_TEXT_SIZE: u16 = 16;
const MODAL_TITLE_TEXT_SIZE: u16 = 20;
const MODAL_LABEL_TEXT_SIZE: u16 = 12;
const MODAL_BODY_TEXT_SIZE: u16 = 14;
const MODAL_WIDTH_STANDARD: f32 = 350.0;
const MODAL_WIDTH_WIDE: f32 = 400.0;
const PROMPT_EDITOR_HEIGHT: f32 = 160.0;
const CONTROL_PADDING: u16 = 8;
const COMPACT_BACK_BUTTON_PADDING: u16 = 8;
const COMPACT_BODY_SPACING: u16 = 10;

const APP_LOGO_DARK_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/images/app_logo_dark.png"
));
const APP_LOGO_LIGHT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/images/app_logo_light.png"
));

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use iced::{
    alignment,
    widget::{
        button, checkbox, column, container, image, opaque, pick_list, row, scrollable, stack,
        text, text_editor, text_input, tooltip, Space,
    },
    Border, Color, ContentFit, Element, Font, Length, Subscription, Task, Theme,
};

#[cfg(not(test))]
use crate::autostart;
use crate::config::{
    get_current_year, get_db_path, get_settings_path, Settings, APP_DESCRIPTION, APP_NAME,
    APP_VERSION, WINDOW_INITIAL_WIDTH,
};
use crate::db::Database;
use crate::hotkeys::HotkeyService;
use crate::icon;
use crate::launch::should_show_window_after_hidden_start;
use crate::models::{Prompt, PromptFilter, PromptId, PromptQuery, PromptSort};
use crate::perf;
use crate::tray::{pump_platform_events, TrayEvent, TrayHandle};

#[cfg(not(test))]
fn sync_autostart_enabled(enabled: bool) -> Option<String> {
    autostart::sync(enabled)
        .err()
        .map(|error| format!("Autostart sync failed: {}", error))
}

#[cfg(test)]
fn sync_autostart_enabled(_enabled: bool) -> Option<String> {
    None
}

#[derive(Debug, Clone)]
pub enum Message {
    SearchChanged(String),
    PromptSelected(PromptId),
    PromptListRequested,
    CopyPressed(PromptId, String),
    NewPressed,
    DeletePressed(PromptId),
    DeleteConfirmPressed(PromptId),
    DeleteCancelPressed,
    EditPressed(PromptId),
    FavoriteToggled(PromptId, bool),
    HotkeyTick,
    // New prompt form
    FormNameChanged(String),
    FormContentEdited(iced::widget::text_editor::Action),
    FormSave,
    FormCancel,
    FormHotkeyRecordPressed,
    FormHotkeyCaptured(String),
    FormHotkeyClearPressed,
    FormHotkeyListeningCancelled,
    FormHotkeyEnabledToggled(bool),
    // Settings
    SettingsPressed,
    SettingsCancel,
    SettingsHotkeyToggled(bool),
    SettingsAutostartToggled(bool),
    SettingsThemeChanged(String),
    SettingsSave,
    // Info modal
    InfoPressed,
    InfoDismissed,
    // Window management
    CloseRequested(iced::window::Id),
    ShowWindow(Option<iced::window::Id>),
    WindowResized(iced::Size),
    SmokeTick,
    SmokeExitRequested,
    // Tray
    TrayTick,
    PromptFilterChanged(crate::models::PromptFilter),
    PromptSortChanged(crate::models::PromptSort),
}

pub struct NewPromptForm {
    pub name: String,
    pub content: String,
    pub content_editor: text_editor::Content,
    pub hotkey: String,
    pub hotkey_enabled: bool,
    pub editing_id: Option<PromptId>,
}

impl Default for NewPromptForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            content: String::new(),
            content_editor: text_editor::Content::new(),
            hotkey: String::new(),
            hotkey_enabled: true,
            editing_id: None,
        }
    }
}

impl NewPromptForm {
    pub fn from_prompt(p: &Prompt) -> Self {
        Self {
            name: p.name.clone(),
            content: p.content.clone(),
            content_editor: text_editor::Content::with_text(&p.content),
            hotkey: p.hotkey.clone().unwrap_or_default(),
            hotkey_enabled: p.hotkey_enabled,
            editing_id: Some(p.id.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiDensity {
    Regular,
    Compact,
}

fn ui_density_for_width(width: f32) -> UiDensity {
    if width < COMPACT_LAYOUT_WIDTH_THRESHOLD {
        UiDensity::Compact
    } else {
        UiDensity::Regular
    }
}

pub struct JamePromptApp {
    db: Database,
    all_prompts: Vec<Prompt>,
    prompts: Vec<Prompt>,
    search_term: String,
    prompt_filter: PromptFilter,
    prompt_sort: PromptSort,
    selected_id: Option<PromptId>,
    status_message: String,
    settings: Settings,
    hotkey_service: Option<Arc<HotkeyService>>,
    hotkey_ids: HashMap<u32, PromptId>,
    prompt_by_id: HashMap<PromptId, Prompt>,
    new_form: Option<NewPromptForm>,
    show_settings: bool,
    show_info: bool,
    pending_delete_id: Option<PromptId>,
    listening_for_hotkey: bool,
    main_window_id: Option<iced::window::Id>,
    content_width: f32,
    smoke_mode: bool,
    smoke_deadline: Option<Instant>,
    // Tray fields
    tray: Option<TrayHandle>,
}

impl Default for JamePromptApp {
    fn default() -> Self {
        let (db, startup_warning) = perf::measure("startup.database_open", || {
            Self::open_database(&get_db_path())
        });
        let all_prompts =
            perf::measure("startup.prompt_load", || db.get_all("").unwrap_or_default());
        let settings = perf::measure("startup.settings_load", || {
            Settings::load(&get_settings_path())
        });
        let autostart_warning = perf::measure("startup.autostart_sync", || {
            sync_autostart_enabled(settings.autostart_enabled)
        });

        let hotkey_service = perf::measure("startup.hotkey_service_init", || {
            HotkeyService::new().map(Arc::new)
        });
        let mut hotkey_ids = HashMap::new();

        perf::measure("startup.hotkey_registration", || {
            if let Some(ref svc) = hotkey_service {
                for p in &all_prompts {
                    if let Some(ref hk) = p.hotkey {
                        if p.hotkey_enabled && settings.hotkeys_enabled {
                            if let Some(hotkey_id) = svc.register(hk) {
                                hotkey_ids.insert(hotkey_id, p.id.clone());
                            }
                        }
                    }
                }
            }
        });

        let prompt_count = all_prompts.len();
        let prompt_by_id = Self::build_prompt_index(&all_prompts);
        let (tray, tray_status) = perf::measure("startup.tray_init", initialize_tray);
        let status_message = match (startup_warning, tray_status) {
            (Some(db_warning), Some(tray_warning)) => format!("{db_warning} | {tray_warning}"),
            (Some(db_warning), None) => db_warning,
            (None, Some(tray_warning)) => tray_warning,
            (None, None) => format!("{} prompts loaded", prompt_count),
        };
        let status_message = match autostart_warning {
            Some(autostart_warning) => format!("{status_message} | {autostart_warning}"),
            None => status_message,
        };

        Self {
            db,
            all_prompts: all_prompts.clone(),
            prompts: all_prompts,
            search_term: String::new(),
            prompt_filter: PromptFilter::All,
            prompt_sort: PromptSort::NameAsc,
            selected_id: None,
            status_message,
            settings,
            hotkey_service,
            hotkey_ids,
            prompt_by_id,
            new_form: None,
            show_settings: false,
            show_info: false,
            pending_delete_id: None,
            listening_for_hotkey: false,
            main_window_id: None,
            content_width: WINDOW_INITIAL_WIDTH,
            smoke_mode: false,
            smoke_deadline: None,
            tray,
        }
    }
}

#[cfg(test)]
impl JamePromptApp {
    pub(crate) fn with_database(db: Database) -> Self {
        let all_prompts = perf::measure("startup.test_prompt_load", || {
            db.get_all("").unwrap_or_default()
        });
        let prompt_by_id = Self::build_prompt_index(&all_prompts);

        Self {
            db,
            all_prompts: all_prompts.clone(),
            prompts: all_prompts,
            search_term: String::new(),
            prompt_filter: PromptFilter::All,
            prompt_sort: PromptSort::NameAsc,
            selected_id: None,
            status_message: String::new(),
            settings: Settings::default(),
            hotkey_service: None,
            hotkey_ids: HashMap::new(),
            prompt_by_id,
            new_form: None,
            show_settings: false,
            show_info: false,
            pending_delete_id: None,
            listening_for_hotkey: false,
            main_window_id: None,
            content_width: WINDOW_INITIAL_WIDTH,
            smoke_mode: false,
            smoke_deadline: None,
            tray: None,
        }
    }
}

impl JamePromptApp {
    fn open_database(path: &Path) -> (Database, Option<String>) {
        match Database::new(path) {
            Ok(db) => (db, None),
            Err(error) => {
                let warning = format!(
                    "Database unavailable at {}: {}. Using in-memory storage.",
                    path.display(),
                    error
                );
                let db = Database::in_memory().expect("In-memory database initialization failed");
                (db, Some(warning))
            }
        }
    }

    fn current_query(&self) -> PromptQuery {
        PromptQuery {
            search: self.search_term.clone(),
            filter: self.prompt_filter,
            sort: self.prompt_sort,
        }
    }

    fn current_density(&self) -> UiDensity {
        ui_density_for_width(self.content_width)
    }

    fn refresh_prompts(&mut self, success_message: &str) {
        perf::measure("ui.refresh_prompts", || {
            self.prompts = self.visible_prompts_for_query(&self.current_query());
            if let Some(selected_id) = &self.selected_id {
                if !self.prompt_by_id.contains_key(selected_id) {
                    self.selected_id = None;
                } else if !self.prompts.iter().any(|p| &p.id == selected_id) {
                    self.selected_id = None;
                }
            }
            self.status_message = success_message.to_string();
        });
    }

    fn unregister_prompt_hotkey(&mut self, id: &str) {
        if let Some(ref svc) = self.hotkey_service {
            let old_hotkey_id = self
                .hotkey_ids
                .iter()
                .find(|(_, pid)| pid.as_str() == id)
                .map(|(&hid, _)| hid);
            if let Some(hid) = old_hotkey_id {
                svc.unregister(hid);
                self.hotkey_ids.remove(&hid);
            }
        }
    }

    fn build_prompt_index(prompts: &[Prompt]) -> HashMap<PromptId, Prompt> {
        prompts
            .iter()
            .cloned()
            .map(|prompt| (prompt.id.clone(), prompt))
            .collect()
    }

    fn visible_prompts_for_query(&self, query: &PromptQuery) -> Vec<Prompt> {
        let normalized_search = {
            let search = query.search.trim();
            if search.is_empty() {
                None
            } else {
                Some(search.to_lowercase())
            }
        };
        let mut prompts: Vec<Prompt> = self
            .all_prompts
            .iter()
            .filter(|prompt| {
                self.prompt_matches_query(prompt, query.filter, normalized_search.as_deref())
            })
            .cloned()
            .collect();
        self.sort_prompts(&mut prompts, query.sort);
        prompts
    }

    fn prompt_matches_query(
        &self,
        prompt: &Prompt,
        filter: PromptFilter,
        normalized_search: Option<&str>,
    ) -> bool {
        if matches!(filter, PromptFilter::Favorites) && !prompt.favorite {
            return false;
        }

        let Some(needle) = normalized_search else {
            return true;
        };
        let name = prompt.name.to_lowercase();
        let content = prompt.content.to_lowercase();
        name.contains(&needle) || content.contains(&needle)
    }

    fn sort_prompts(&self, prompts: &mut [Prompt], sort: PromptSort) {
        match sort {
            PromptSort::NameAsc => {
                prompts
                    .sort_by_cached_key(|prompt| (prompt.name.to_lowercase(), prompt.id.clone()));
            }
            PromptSort::RecentlyUsed => {
                prompts.sort_by_cached_key(|prompt| {
                    (
                        Reverse(prompt.last_used_at.clone()),
                        prompt.name.to_lowercase(),
                        prompt.id.clone(),
                    )
                });
            }
            PromptSort::RecentlyUpdated => {
                prompts.sort_by_cached_key(|prompt| {
                    (
                        Reverse(prompt.updated_at.clone()),
                        prompt.name.to_lowercase(),
                        prompt.id.clone(),
                    )
                });
            }
            PromptSort::MostUsed => {
                prompts.sort_by_cached_key(|prompt| {
                    (
                        Reverse(prompt.use_count),
                        prompt.name.to_lowercase(),
                        prompt.id.clone(),
                    )
                });
            }
        }
    }

    fn prompt_ref_by_id(&self, id: &str) -> Option<&Prompt> {
        self.prompt_by_id.get(id)
    }

    fn prompt_cloned_by_id(&self, id: &str) -> Option<Prompt> {
        self.prompt_by_id.get(id).cloned()
    }

    fn update_prompt_cache(&mut self, prompt: Prompt) {
        if let Some(existing) = self
            .all_prompts
            .iter_mut()
            .find(|existing| existing.id == prompt.id)
        {
            *existing = prompt.clone();
        } else {
            self.all_prompts.push(prompt.clone());
        }

        self.prompt_by_id.insert(prompt.id.clone(), prompt);
    }

    fn remove_prompt_cache(&mut self, id: &str) {
        self.all_prompts.retain(|prompt| prompt.id != id);
        self.prompt_by_id.remove(id);
    }

    fn sync_prompt_cache_from_db(&mut self, id: &str) {
        match self.db.get_by_id(id) {
            Ok(Some(prompt)) => self.update_prompt_cache(prompt),
            Ok(None) => self.remove_prompt_cache(id),
            Err(error) => {
                self.status_message = format!("Database error: {}", error);
            }
        }
    }
}

#[cfg(not(test))]
fn initialize_tray() -> (Option<TrayHandle>, Option<String>) {
    match TrayHandle::new() {
        Ok(tray) => (Some(tray), None),
        Err(error) => (None, Some(format!("Tray unavailable: {error}"))),
    }
}

#[cfg(test)]
fn initialize_tray() -> (Option<TrayHandle>, Option<String>) {
    (None, None)
}

/// Standalone function for `iced::keyboard::on_key_press` that captures hotkey combinations.
/// Returns `Message::FormHotkeyListeningCancelled` for Escape, `Message::FormHotkeyCaptured`
/// for valid hotkey combinations, or `None` for unrecognized input.
fn capture_hotkey(
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Option<Message> {
    // Handle Escape to cancel listening
    if let iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) = key {
        return Some(Message::FormHotkeyListeningCancelled);
    }
    // Format and capture
    if let Some(hotkey_str) = crate::hotkeys::format_hotkey(&key, modifiers) {
        if crate::hotkeys::validate_hotkey(&hotkey_str) {
            return Some(Message::FormHotkeyCaptured(hotkey_str));
        }
    }
    None
}

fn normalize_editor_text(text: &str) -> String {
    text.trim_end_matches('\n').to_string()
}

fn should_exit_on_close_request(smoke_mode: bool) -> bool {
    smoke_mode
}

fn smoke_duration_from_env() -> Duration {
    const DEFAULT_SMOKE_DURATION_MS: u64 = 15_000;

    let duration_ms = std::env::var("JAME_PROMPT_UI_SMOKE_DURATION_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SMOKE_DURATION_MS);

    Duration::from_millis(duration_ms)
}

impl JamePromptApp {
    #[cfg(test)]
    pub fn new() -> (Self, Task<Message>) {
        Self::new_with_hidden_start(false, false)
    }

    fn new_smoke_mode() -> Self {
        let db = Database::in_memory().expect("In-memory smoke database initialization failed");
        let all_prompts = Vec::new();
        let prompt_by_id = Self::build_prompt_index(&all_prompts);

        Self {
            db,
            all_prompts: all_prompts.clone(),
            prompts: all_prompts,
            search_term: String::new(),
            prompt_filter: PromptFilter::All,
            prompt_sort: PromptSort::NameAsc,
            selected_id: None,
            status_message: "Smoke mode ready".into(),
            settings: Settings::default(),
            hotkey_service: None,
            hotkey_ids: HashMap::new(),
            prompt_by_id,
            new_form: None,
            show_settings: false,
            show_info: false,
            pending_delete_id: None,
            listening_for_hotkey: false,
            main_window_id: None,
            content_width: WINDOW_INITIAL_WIDTH,
            smoke_mode: true,
            smoke_deadline: None,
            tray: None,
        }
    }

    pub fn new_with_hidden_start(start_minimized: bool, smoke_mode: bool) -> (Self, Task<Message>) {
        let mut app = if smoke_mode {
            Self::new_smoke_mode()
        } else {
            Self::default()
        };
        let smoke_exit_task = if smoke_mode {
            let smoke_duration = smoke_duration_from_env();
            Task::perform(
                async move {
                    std::thread::sleep(smoke_duration);
                },
                |_| Message::SmokeExitRequested,
            )
        } else {
            Task::none()
        };
        app.smoke_deadline = if smoke_mode {
            Some(Instant::now() + smoke_duration_from_env())
        } else {
            None
        };
        let tray_available = app.tray.is_some();
        let fallback_task =
            if should_show_window_after_hidden_start(start_minimized, tray_available) {
                iced::window::get_latest().map(Message::ShowWindow)
            } else {
                Task::none()
            };

        (app, Task::batch([fallback_task, smoke_exit_task]))
    }

    pub fn title(&self) -> String {
        APP_NAME.to_string()
    }

    pub fn theme(&self) -> Theme {
        match self.settings.theme.as_str() {
            "Light" => Theme::Light,
            _ => Theme::Dark,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let smoke_exit = if self.smoke_mode {
            iced::keyboard::on_key_press(|key, _modifiers| {
                if matches!(
                    key,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::F12)
                ) {
                    Some(Message::SmokeExitRequested)
                } else {
                    None
                }
            })
        } else {
            Subscription::none()
        };

        let smoke_tick = if self.smoke_mode {
            iced::time::every(Duration::from_millis(250)).map(|_| Message::SmokeTick)
        } else {
            Subscription::none()
        };

        let hotkey_tick = if self.hotkey_service.is_some()
            && self.settings.hotkeys_enabled
            && !self.hotkey_ids.is_empty()
        {
            iced::time::every(Duration::from_millis(50)).map(|_| Message::HotkeyTick)
        } else {
            Subscription::none()
        };

        let keyboard_capture = if self.listening_for_hotkey {
            iced::keyboard::on_key_press(capture_hotkey)
        } else {
            Subscription::none()
        };

        // Window close requests (X button) -> hide to tray
        let close_requests = iced::window::close_requests().map(Message::CloseRequested);

        let tray_events = if self.tray.is_some() {
            iced::time::every(Duration::from_millis(100)).map(|_| Message::TrayTick)
        } else {
            Subscription::none()
        };

        let resize_events =
            iced::window::resize_events().map(|(_id, size)| Message::WindowResized(size));

        Subscription::batch([
            smoke_exit,
            smoke_tick,
            hotkey_tick,
            keyboard_capture,
            close_requests,
            tray_events,
            resize_events,
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        perf::measure("ui.update", || {
            match message {
                Message::SearchChanged(term) => {
                    self.search_term = term;
                    self.refresh_prompts("");
                    self.status_message = format!("{} prompts found", self.prompts.len());
                }
                Message::PromptSelected(id) => {
                    self.selected_id = Some(id);
                    self.status_message = "Prompt selected".into();
                }
                Message::PromptListRequested => {
                    self.selected_id = None;
                    self.status_message = "Prompt list".into();
                }
                Message::CopyPressed(id, content) => {
                    if let Err(error) = self.db.record_use(&id) {
                        self.status_message = format!("Copied, but usage update failed: {}", error);
                    } else {
                        self.sync_prompt_cache_from_db(&id);
                        self.refresh_prompts("Copied to clipboard");
                    }
                    return iced::clipboard::write(content);
                }
                Message::NewPressed => {
                    self.new_form = Some(NewPromptForm::default());
                    self.status_message = "Creating new prompt...".into();
                }
                Message::DeletePressed(id) => {
                    self.pending_delete_id = Some(id);
                    self.status_message = "Confirm delete".into();
                }
                Message::DeleteCancelPressed => {
                    self.pending_delete_id = None;
                    self.status_message = "Delete cancelled".into();
                }
                Message::DeleteConfirmPressed(id) => {
                    self.unregister_prompt_hotkey(&id);
                    match self.db.delete(&id) {
                        Ok(()) => {
                            self.remove_prompt_cache(&id);
                            self.selected_id = None;
                            self.pending_delete_id = None;
                            self.refresh_prompts("Prompt deleted");
                        }
                        Err(e) => {
                            self.status_message = format!("Error deleting prompt: {}", e);
                        }
                    }
                }
                Message::EditPressed(id) => {
                    if let Some(p) = self.prompt_cloned_by_id(&id) {
                        self.new_form = Some(NewPromptForm::from_prompt(&p));
                        self.status_message = format!("Editing \"{}\"", p.name);
                    } else {
                        self.status_message = "Prompt not found".into();
                    }
                }
                Message::HotkeyTick => {
                    let triggered = HotkeyService::poll_events();
                    for hotkey_id in triggered {
                        if let Some(prompt_id) = self.hotkey_ids.get(&hotkey_id) {
                            if let Some(p) = self.prompt_cloned_by_id(prompt_id) {
                                self.status_message = format!("Hotkey: pasting \"{}\"", p.name);
                                crate::hotkeys::paste_to_active_window();
                                let _ = self.db.record_use(&p.id);
                                self.sync_prompt_cache_from_db(&p.id);
                                return iced::clipboard::write(p.content.clone());
                            }
                        }
                    }
                }
                Message::FormNameChanged(name) => {
                    if let Some(ref mut form) = self.new_form {
                        form.name = name;
                    }
                }
                Message::FormContentEdited(action) => {
                    if let Some(ref mut form) = self.new_form {
                        form.content_editor.perform(action);
                        form.content = normalize_editor_text(&form.content_editor.text());
                    }
                }
                Message::FormSave => {
                    if let Some(form) = self.new_form.take() {
                        if form.name.trim().is_empty() || form.content.trim().is_empty() {
                            self.status_message = "Name and content are required".into();
                            self.new_form = Some(form);
                        } else {
                            let hotkey = if form.hotkey.trim().is_empty() {
                                None
                            } else {
                                let hotkey = form.hotkey.trim().to_string();
                                if !crate::hotkeys::validate_hotkey(&hotkey) {
                                    self.status_message = format!("Invalid hotkey: {}", hotkey);
                                    self.new_form = Some(form);
                                    return Task::none();
                                }
                                Some(hotkey)
                            };

                            if let Some(editing_id) = form.editing_id {
                                // Edit mode: update existing prompt
                                let prompt = Prompt {
                                    id: editing_id.clone(),
                                    name: form.name.trim().to_string(),
                                    content: form.content.trim().to_string(),
                                    hotkey,
                                    hotkey_enabled: form.hotkey_enabled,
                                    ..Prompt::default()
                                };
                                match self.db.update(&prompt) {
                                    Ok(()) => {
                                        self.unregister_prompt_hotkey(&editing_id);
                                        // Register new hotkey if provided and enabled
                                        if let Some(ref hk) = prompt.hotkey {
                                            if prompt.hotkey_enabled
                                                && self.settings.hotkeys_enabled
                                            {
                                                if let Some(ref svc) = self.hotkey_service {
                                                    if let Some(hotkey_id) = svc.register(hk) {
                                                        self.hotkey_ids
                                                            .insert(hotkey_id, editing_id.clone());
                                                    }
                                                }
                                            }
                                        }
                                        self.selected_id = Some(editing_id.clone());
                                        self.sync_prompt_cache_from_db(&editing_id);
                                        self.refresh_prompts("Prompt updated");
                                    }
                                    Err(e) => {
                                        self.status_message =
                                            format!("Error updating prompt: {}", e);
                                        let content = prompt.content;
                                        let content_editor =
                                            text_editor::Content::with_text(&content);
                                        self.new_form = Some(NewPromptForm {
                                            name: prompt.name,
                                            content,
                                            content_editor,
                                            hotkey: prompt.hotkey.unwrap_or_default(),
                                            hotkey_enabled: prompt.hotkey_enabled,
                                            editing_id: Some(editing_id),
                                        });
                                    }
                                }
                            } else {
                                // Create mode: insert new prompt
                                let prompt = Prompt {
                                    id: String::new(),
                                    name: form.name.trim().to_string(),
                                    content: form.content.trim().to_string(),
                                    hotkey,
                                    hotkey_enabled: form.hotkey_enabled,
                                    ..Prompt::default()
                                };
                                match self.db.insert(&prompt) {
                                    Ok(new_id) => {
                                        if let Some(ref hk) = prompt.hotkey {
                                            if prompt.hotkey_enabled
                                                && self.settings.hotkeys_enabled
                                            {
                                                if let Some(ref svc) = self.hotkey_service {
                                                    if let Some(hotkey_id) = svc.register(hk) {
                                                        self.hotkey_ids
                                                            .insert(hotkey_id, new_id.clone());
                                                    }
                                                }
                                            }
                                        }
                                        self.selected_id = Some(new_id.clone());
                                        self.sync_prompt_cache_from_db(&new_id);
                                        self.refresh_prompts("Prompt created");
                                    }
                                    Err(e) => {
                                        self.status_message =
                                            format!("Error creating prompt: {}", e);
                                        let content = prompt.content;
                                        let content_editor =
                                            text_editor::Content::with_text(&content);
                                        self.new_form = Some(NewPromptForm {
                                            name: prompt.name,
                                            content,
                                            content_editor,
                                            hotkey: prompt.hotkey.unwrap_or_default(),
                                            hotkey_enabled: prompt.hotkey_enabled,
                                            editing_id: None,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                Message::FormCancel => {
                    self.new_form = None;
                    self.listening_for_hotkey = false;
                    self.status_message = "Cancelled".into();
                }
                Message::FormHotkeyRecordPressed => {
                    self.listening_for_hotkey = true;
                    self.status_message = "Press your hotkey combination...".into();
                }
                Message::FormHotkeyCaptured(hotkey_str) => {
                    self.listening_for_hotkey = false;
                    if let Some(ref mut form) = self.new_form {
                        form.hotkey = hotkey_str;
                        form.hotkey_enabled = true;
                    }
                    self.status_message = "Hotkey captured".into();
                }
                Message::FormHotkeyClearPressed => {
                    if let Some(ref mut form) = self.new_form {
                        form.hotkey = String::new();
                        form.hotkey_enabled = false;
                    }
                    self.status_message = "Hotkey cleared".into();
                }
                Message::FormHotkeyListeningCancelled => {
                    self.listening_for_hotkey = false;
                    self.status_message = "Hotkey capture cancelled".into();
                }
                Message::FormHotkeyEnabledToggled(enabled) => {
                    if let Some(ref mut form) = self.new_form {
                        form.hotkey_enabled = enabled;
                    }
                }
                Message::FavoriteToggled(id, favorite) => match self.db.set_favorite(&id, favorite)
                {
                    Ok(()) => {
                        self.sync_prompt_cache_from_db(&id);
                        self.selected_id = Some(id);
                        self.refresh_prompts(if favorite {
                            "Prompt added to favorites"
                        } else {
                            "Prompt removed from favorites"
                        });
                    }
                    Err(error) => {
                        self.status_message = format!("Error updating favorite: {}", error);
                    }
                },
                Message::PromptFilterChanged(filter) => {
                    self.prompt_filter = filter;
                    self.refresh_prompts("Filter changed");
                }
                Message::PromptSortChanged(sort) => {
                    self.prompt_sort = sort;
                    self.refresh_prompts("Sort changed");
                }
                Message::SettingsPressed => {
                    self.show_settings = true;
                    self.show_info = false;
                    self.status_message = "Settings".into();
                }
                Message::SettingsCancel => {
                    self.show_settings = false;
                    self.status_message = "Settings closed".into();
                }
                Message::SettingsHotkeyToggled(enabled) => {
                    self.settings.hotkeys_enabled = enabled;
                    if !enabled {
                        // Unregister ALL hotkeys
                        if let Some(ref svc) = self.hotkey_service {
                            for &hotkey_id in self.hotkey_ids.keys() {
                                svc.unregister(hotkey_id);
                            }
                        }
                        self.hotkey_ids.clear();
                        self.status_message = "All hotkeys disabled".into();
                    } else {
                        // Re-register all enabled prompts
                        if let Some(ref svc) = self.hotkey_service {
                            for p in &self.all_prompts {
                                if p.hotkey_enabled {
                                    if let Some(ref hk) = p.hotkey {
                                        if let Some(hotkey_id) = svc.register(hk) {
                                            self.hotkey_ids.insert(hotkey_id, p.id.clone());
                                        }
                                    }
                                }
                            }
                        }
                        self.status_message = "All hotkeys enabled".into();
                    }
                }
                Message::SettingsAutostartToggled(enabled) => {
                    self.settings.autostart_enabled = enabled;
                    self.status_message = if enabled {
                        "Autostart enabled".into()
                    } else {
                        "Autostart disabled".into()
                    };
                }
                Message::SettingsThemeChanged(theme) => {
                    self.settings.theme = theme;
                    self.status_message = format!("Theme changed to {}", self.settings.theme);
                }
                Message::SettingsSave => {
                    let path = get_settings_path();
                    match self.settings.save(&path) {
                        Ok(()) => {
                            if let Some(error) =
                                sync_autostart_enabled(self.settings.autostart_enabled)
                            {
                                self.status_message =
                                    format!("Settings saved, but autostart sync failed: {}", error);
                            } else {
                                self.status_message = "Settings saved".into();
                            }
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to save settings: {}", e);
                        }
                    }
                }
                Message::InfoPressed => {
                    self.show_info = true;
                    self.show_settings = false;
                    self.status_message = "About".into();
                }
                Message::InfoDismissed => {
                    self.show_info = false;
                    self.status_message = "About closed".into();
                }
                // Tray: Window close requested -> hide to tray
                Message::CloseRequested(id) => {
                    self.main_window_id = Some(id);
                    if should_exit_on_close_request(self.smoke_mode) {
                        self.status_message = format!("{APP_NAME} smoke run exiting");
                        return iced::exit();
                    }

                    self.status_message = format!("{APP_NAME} is still running in the system tray");
                    return iced::window::change_mode(id, iced::window::Mode::Hidden);
                }
                Message::ShowWindow(id) => {
                    if let Some(id) = id.or(self.main_window_id) {
                        self.main_window_id = Some(id);
                        self.status_message = format!("{APP_NAME} restored");
                        return Task::batch([
                            iced::window::change_mode(id, iced::window::Mode::Windowed),
                            iced::window::gain_focus(id),
                        ]);
                    }

                    self.status_message = "Unable to restore the window".into();
                }
                Message::WindowResized(size) => {
                    self.content_width = size.width;
                }
                Message::SmokeTick => {
                    if self
                        .smoke_deadline
                        .is_some_and(|deadline| Instant::now() >= deadline)
                    {
                        return iced::exit();
                    }
                }
                Message::SmokeExitRequested => {
                    return iced::exit();
                }
                Message::TrayTick => {
                    pump_platform_events();

                    if let Some(event) = self.tray.as_ref().and_then(TrayHandle::poll_event) {
                        return match event {
                            TrayEvent::ShowRequested => {
                                iced::window::get_latest().map(Message::ShowWindow)
                            }
                            TrayEvent::QuitRequested => iced::exit(),
                        };
                    }
                }
            }
            Task::none()
        })
    }

    pub fn view(&self) -> Element<'_, Message> {
        perf::measure("ui.view", || {
            let main_content = self.view_main();

            if self.new_form.is_some() {
                let modal = self.view_new_prompt_modal();
                stack!(main_content, modal).into()
            } else if self.pending_delete_id.is_some() {
                let modal = self.view_delete_confirmation_modal();
                stack!(main_content, modal).into()
            } else if self.show_settings {
                let modal = self.view_settings_modal();
                stack!(main_content, modal).into()
            } else if self.show_info {
                let modal = self.view_info_modal();
                stack!(main_content, modal).into()
            } else {
                main_content
            }
        })
    }

    fn view_main(&self) -> Element<'_, Message> {
        perf::measure("ui.view_main", || {
            let header = row![
                row![
                    image::Image::new(image_handle_for_theme(&self.theme()))
                        .width(HEADER_LOGO_SIZE)
                        .height(HEADER_LOGO_SIZE)
                        .content_fit(ContentFit::Contain),
                    text(format!(" {}", APP_NAME)).size(20)
                ]
                .spacing(HEADER_BRAND_SPACING)
                .align_y(alignment::Alignment::Center)
                .width(Length::Fill),
                tooltip(
                    button(icon::info())
                        .on_press(Message::InfoPressed)
                        .padding(8)
                        .style(button::text),
                    text("About"),
                    tooltip::Position::Top,
                ),
                tooltip(
                    button(icon::settings())
                        .on_press(Message::SettingsPressed)
                        .padding(8)
                        .style(button::text),
                    text("Settings"),
                    tooltip::Position::Top,
                ),
            ]
            .padding(MAIN_PANEL_PADDING)
            .align_y(alignment::Alignment::Center);

            let status_bar = container(text(&self.status_message).size(12))
                .width(Length::Fill)
                .padding([5, MAIN_PANEL_PADDING])
                .style(footer_container_style);

            let body = match self.current_density() {
                UiDensity::Regular => self.view_regular_body(),
                UiDensity::Compact => self.view_compact_body(),
            };

            column![header, body, status_bar].into()
        })
    }

    fn view_regular_body(&self) -> Element<'_, Message> {
        perf::measure("ui.view_regular_body", || {
            let left_panel = container(self.view_prompt_list(UiDensity::Regular))
                .width(Length::FillPortion(1))
                .height(Length::Fill);

            let right_panel = container(
                container(self.view_detail())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(DETAIL_PANEL_INNER_PADDING)
                    .style(detail_panel_style),
            )
            .width(Length::FillPortion(2))
            .height(Length::Fill)
            .padding(MAIN_PANEL_PADDING);

            row![left_panel, right_panel].height(Length::Fill).into()
        })
    }

    fn view_compact_body(&self) -> Element<'_, Message> {
        perf::measure("ui.view_compact_body", || {
            if self.selected_id.is_some() {
                let detail = container(self.view_detail())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(DETAIL_PANEL_INNER_PADDING)
                    .style(detail_panel_style);

                return container(
                    column![
                        tooltip(
                            button(icon::arrow_left())
                                .on_press(Message::PromptListRequested)
                                .padding(COMPACT_BACK_BUTTON_PADDING)
                                .style(button::secondary),
                            text("Back to list"),
                            tooltip::Position::Top,
                        ),
                        detail,
                    ]
                    .spacing(COMPACT_BODY_SPACING)
                    .padding(MAIN_PANEL_PADDING),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            }

            self.view_prompt_list(UiDensity::Compact)
        })
    }

    fn view_prompt_list(&self, density: UiDensity) -> Element<'_, Message> {
        perf::measure("ui.view_prompt_list", || {
            let mut list_col = column!().spacing(PROMPT_LIST_SPACING);
            for prompt in &self.prompts {
                list_col = list_col.push(
                    button(self.view_prompt_card(prompt, density))
                        .on_press(Message::PromptSelected(prompt.id.clone()))
                        .style(button::text)
                        .width(Length::Fill),
                );
            }

            container(
                column![
                    self.view_prompt_controls(),
                    Space::with_height(LIST_PANEL_SPACING),
                    scrollable(list_col).height(Length::Fill),
                ]
                .padding(MAIN_PANEL_PADDING),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        })
    }

    fn view_prompt_card<'a>(
        &'a self,
        prompt: &'a Prompt,
        density: UiDensity,
    ) -> Element<'a, Message> {
        let is_selected = self.selected_id.as_ref() == Some(&prompt.id);
        let title = text(&prompt.name).font(Font {
            weight: iced::font::Weight::Bold,
            ..Default::default()
        });
        let preview = text(prompt.preview(50))
            .size(PROMPT_PREVIEW_TEXT_SIZE)
            .color(prompt_preview_text_color(is_selected, &self.theme()));

        let content: Element<'_, Message> = match density {
            UiDensity::Regular => row![
                view_prompt_list_favorite_indicator(prompt.favorite),
                title.width(Length::FillPortion(2)),
                preview.width(Length::FillPortion(3)),
            ]
            .spacing(PROMPT_CARD_SPACING)
            .align_y(alignment::Alignment::Center)
            .padding(PROMPT_CARD_PADDING)
            .into(),
            UiDensity::Compact => row![
                view_prompt_list_favorite_indicator(prompt.favorite),
                column![title.width(Length::Fill), preview.width(Length::Fill),]
                    .spacing(PROMPT_CARD_SPACING)
                    .width(Length::Fill),
            ]
            .spacing(PROMPT_CARD_SPACING)
            .align_y(alignment::Alignment::Center)
            .padding(PROMPT_CARD_PADDING)
            .into(),
        };

        container(content)
            .width(Length::Fill)
            .style(move |theme| prompt_card_style(is_selected, theme))
            .into()
    }

    fn view_prompt_controls(&self) -> Element<'_, Message> {
        const FILTERS: [PromptFilter; 2] = [PromptFilter::All, PromptFilter::Favorites];
        const SORTS: [PromptSort; 4] = [
            PromptSort::NameAsc,
            PromptSort::RecentlyUsed,
            PromptSort::RecentlyUpdated,
            PromptSort::MostUsed,
        ];

        let primary_controls = row![
            text_input("Search prompts...", &self.search_term)
                .on_input(Message::SearchChanged)
                .padding(CONTROL_PADDING)
                .width(Length::Fill)
                .style(search_input_style),
            tooltip(
                button(icon::plus())
                    .on_press(Message::NewPressed)
                    .padding(CONTROL_PADDING)
                    .style(primary_button_style),
                text("New prompt"),
                tooltip::Position::Top,
            ),
        ]
        .spacing(PROMPT_PRIMARY_TOOLBAR_SPACING)
        .align_y(alignment::Alignment::Center)
        .width(Length::Fill);

        let secondary_controls = row![
            pick_list(
                FILTERS.as_slice(),
                Some(self.prompt_filter),
                Message::PromptFilterChanged
            )
            .padding(CONTROL_PADDING)
            .width(Length::Fixed(PROMPT_FILTER_PICKER_WIDTH))
            .style(pick_list_style),
            pick_list(
                SORTS.as_slice(),
                Some(self.prompt_sort),
                Message::PromptSortChanged
            )
            .padding(CONTROL_PADDING)
            .width(Length::Fixed(PROMPT_SORT_PICKER_WIDTH))
            .style(pick_list_style),
        ]
        .spacing(PROMPT_SECONDARY_TOOLBAR_SPACING)
        .align_y(alignment::Alignment::Center)
        .width(Length::Fill);

        column![primary_controls, secondary_controls]
            .spacing(PROMPT_SECONDARY_TOOLBAR_SPACING)
            .width(Length::Fill)
            .into()
    }

    fn view_detail(&self) -> Element<'_, Message> {
        perf::measure("ui.view_detail", || {
            if let Some(id) = &self.selected_id {
                if let Some(p) = self.prompt_ref_by_id(id) {
                    let hotkey_foreground = hotkey_badge_foreground_color(&self.theme());
                    let hotkey_row: Element<'_, Message> = if let Some(hk) = &p.hotkey {
                        container(
                            row![
                                icon::keyboard().color(hotkey_foreground),
                                text(hk)
                                    .size(PROMPT_METADATA_TEXT_SIZE)
                                    .color(hotkey_foreground),
                            ]
                            .spacing(6)
                            .align_y(alignment::Alignment::Center),
                        )
                        .padding(CONTROL_PADDING)
                        .width(Length::Shrink)
                        .style(hotkey_card_style)
                        .into()
                    } else {
                        Space::with_height(0).into()
                    };

                    return column![
                        text(&p.name).size(PROMPT_DETAIL_TITLE_SIZE).font(Font {
                            weight: iced::font::Weight::Bold,
                            ..Default::default()
                        }),
                        Space::with_height(DETAIL_SECTION_SPACING),
                        text(format!("Used {} times", p.use_count)).size(PROMPT_METADATA_TEXT_SIZE),
                        Space::with_height(DETAIL_SECTION_SPACING),
                        hotkey_row,
                        Space::with_height(DETAIL_SECTION_SPACING),
                        scrollable(
                            text(&p.content)
                                .font(iced::Font::MONOSPACE)
                                .size(PROMPT_CONTENT_TEXT_SIZE)
                        )
                        .height(Length::Fill),
                        Space::with_height(DETAIL_SECTION_SPACING),
                        row![
                            tooltip(
                                button(icon::star())
                                    .on_press(Message::FavoriteToggled(p.id.clone(), !p.favorite))
                                    .padding(CONTROL_PADDING)
                                    .style(if p.favorite {
                                        primary_button_style
                                    } else {
                                        button::secondary
                                    }),
                                text(if p.favorite {
                                    "Remove from favorites"
                                } else {
                                    "Add to favorites"
                                }),
                                tooltip::Position::Top,
                            ),
                            tooltip(
                                button(icon::copy())
                                    .on_press(Message::CopyPressed(p.id.clone(), p.content.clone()))
                                    .padding(CONTROL_PADDING)
                                    .style(primary_button_style),
                                text("Copy to clipboard"),
                                tooltip::Position::Top,
                            ),
                            tooltip(
                                button(icon::pencil())
                                    .on_press(Message::EditPressed(p.id.clone()))
                                    .padding(CONTROL_PADDING)
                                    .style(button::secondary),
                                text("Edit prompt"),
                                tooltip::Position::Top,
                            ),
                            tooltip(
                                button(icon::trash())
                                    .on_press(Message::DeletePressed(p.id.clone()))
                                    .padding(CONTROL_PADDING)
                                    .style(button::danger),
                                text("Delete prompt"),
                                tooltip::Position::Top,
                            ),
                        ]
                        .spacing(10),
                    ]
                    .into();
                } else {
                    return column![text("Prompt not found")].into();
                }
            }

            column![
                Space::with_height(Length::Fill),
                text("Select a prompt from the list")
                    .size(EMPTY_STATE_TEXT_SIZE)
                    .color(self.theme().extended_palette().secondary.strong.color),
                Space::with_height(Length::Fill),
            ]
            .align_x(alignment::Alignment::Center)
            .width(Length::Fill)
            .into()
        })
    }

    fn view_new_prompt_modal(&self) -> Element<'_, Message> {
        perf::measure("ui.view_new_prompt_modal", || {
            let form = self.new_form.as_ref().unwrap();

            let title = if form.editing_id.is_some() {
                "Edit Prompt"
            } else {
                "New Prompt"
            };

            let name_input = text_input("Prompt name...", &form.name)
                .on_input(Message::FormNameChanged)
                .padding(CONTROL_PADDING)
                .style(search_input_style);

            let content_input = text_editor(&form.content_editor)
                .placeholder("Prompt content...")
                .on_action(Message::FormContentEdited)
                .height(PROMPT_EDITOR_HEIGHT)
                .padding(CONTROL_PADDING);

            // Hotkey display and controls
            let hotkey_display_text = if self.listening_for_hotkey {
                "Press your hotkey combination...".to_string()
            } else if form.hotkey.is_empty() {
                "None".to_string()
            } else {
                form.hotkey.clone()
            };

            let hotkey_display = text_input("", &hotkey_display_text)
                .padding(CONTROL_PADDING)
                .style(search_input_style)
                .width(Length::Fill);

            let record_button = button(icon::keyboard())
                .on_press(Message::FormHotkeyRecordPressed)
                .padding(CONTROL_PADDING)
                .style(primary_button_style);

            let clear_button = button(icon::eraser())
                .on_press(Message::FormHotkeyClearPressed)
                .padding(CONTROL_PADDING)
                .style(primary_button_style);

            let hotkey_enabled_checkbox = checkbox("Enable hotkey", form.hotkey_enabled)
                .on_toggle(Message::FormHotkeyEnabledToggled)
                .size(16)
                .style(checkbox_style);

            let hotkey_row = row![
                hotkey_display,
                if self.listening_for_hotkey {
                    button(icon::keyboard())
                        .padding(CONTROL_PADDING)
                        .style(primary_button_style)
                } else {
                    record_button
                },
                if form.hotkey.is_empty() || self.listening_for_hotkey {
                    button(icon::eraser())
                        .padding(CONTROL_PADDING)
                        .style(primary_button_style)
                } else {
                    clear_button
                },
            ]
            .spacing(10)
            .align_y(alignment::Alignment::Center);

            let card = container(
                column![
                    row![
                        text(title).size(MODAL_TITLE_TEXT_SIZE).width(Length::Fill),
                        button(icon::x())
                            .on_press(Message::FormCancel)
                            .style(button::text)
                            .padding(4),
                    ],
                    Space::with_height(15),
                    text("Name").size(MODAL_LABEL_TEXT_SIZE),
                    name_input,
                    Space::with_height(10),
                    text("Content").size(MODAL_LABEL_TEXT_SIZE),
                    content_input,
                    Space::with_height(10),
                    text("Hotkey (optional)").size(MODAL_LABEL_TEXT_SIZE),
                    hotkey_row,
                    Space::with_height(MODAL_SECTION_SPACING),
                    hotkey_enabled_checkbox,
                    Space::with_height(20),
                    tooltip(
                        button(icon::save())
                            .on_press(Message::FormSave)
                            .padding(CONTROL_PADDING)
                            .style(primary_button_style),
                        text("Save prompt"),
                        tooltip::Position::Top,
                    ),
                ]
                .padding(20),
            )
            .width(modal_width_for_density(
                self.current_density(),
                MODAL_WIDTH_WIDE,
            ))
            .style(modal_card_style);

            // Semi-transparent backdrop with centered card
            let modal_overlay = container(
                container(card)
                    .padding(MAIN_PANEL_PADDING)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(modal_backdrop_style);

            opaque(modal_overlay)
        })
    }

    fn view_settings_modal(&self) -> Element<'_, Message> {
        perf::measure("ui.view_settings_modal", || {
            const THEMES: [&str; 2] = ["Dark", "Light"];

            let hotkey_checkbox = checkbox("Hotkeys enabled", self.settings.hotkeys_enabled)
                .on_toggle(Message::SettingsHotkeyToggled)
                .size(16)
                .style(checkbox_style);

            let autostart_checkbox = checkbox(
                "Start automatically on login",
                self.settings.autostart_enabled,
            )
            .on_toggle(Message::SettingsAutostartToggled)
            .size(16)
            .style(checkbox_style);

            let selected_theme = THEMES.iter().find(|&&t| t == self.settings.theme).copied();
            let theme_picker = pick_list(THEMES.as_slice(), selected_theme, |s| {
                Message::SettingsThemeChanged(s.to_string())
            })
            .padding(CONTROL_PADDING)
            .style(pick_list_style);

            let card = container(
                column![
                    row![
                        text("Settings")
                            .size(MODAL_TITLE_TEXT_SIZE)
                            .width(Length::Fill),
                        button(icon::x())
                            .on_press(Message::SettingsCancel)
                            .style(button::text)
                            .padding(4),
                    ],
                    Space::with_height(15),
                    hotkey_checkbox,
                    Space::with_height(MODAL_SECTION_SPACING),
                    autostart_checkbox,
                    Space::with_height(MODAL_SECTION_SPACING),
                    row![
                        text("Theme").size(MODAL_BODY_TEXT_SIZE).width(Length::Fill),
                        theme_picker,
                    ]
                    .align_y(alignment::Alignment::Center),
                    Space::with_height(20),
                    row![tooltip(
                        button(icon::save())
                            .on_press(Message::SettingsSave)
                            .padding(CONTROL_PADDING)
                            .style(primary_button_style),
                        text("Save settings"),
                        tooltip::Position::Top,
                    ),]
                    .spacing(10),
                ]
                .padding(20),
            )
            .width(modal_width_for_density(
                self.current_density(),
                MODAL_WIDTH_STANDARD,
            ))
            .style(modal_card_style);

            let modal_overlay = container(
                container(card)
                    .padding(MAIN_PANEL_PADDING)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(modal_backdrop_style);

            opaque(modal_overlay)
        })
    }

    fn view_delete_confirmation_modal(&self) -> Element<'_, Message> {
        perf::measure("ui.view_delete_confirmation_modal", || {
            let id = self.pending_delete_id.clone().unwrap_or_default();
            let name = self
                .prompt_ref_by_id(&id)
                .map(|prompt| prompt.name.as_str())
                .unwrap_or("selected prompt");

            let card = container(
                column![
                    text("Delete prompt").size(MODAL_TITLE_TEXT_SIZE),
                    Space::with_height(15),
                    text(format!("Delete \"{}\"?", name)).size(MODAL_BODY_TEXT_SIZE),
                    Space::with_height(20),
                    row![
                        button(text("Cancel"))
                            .on_press(Message::DeleteCancelPressed)
                            .padding(CONTROL_PADDING)
                            .style(button::secondary),
                        button(text("Delete"))
                            .on_press(Message::DeleteConfirmPressed(id))
                            .padding(CONTROL_PADDING)
                            .style(button::danger),
                    ]
                    .spacing(10),
                ]
                .padding(20),
            )
            .width(modal_width_for_density(
                self.current_density(),
                MODAL_WIDTH_STANDARD,
            ))
            .style(modal_card_style);

            let modal_overlay = container(
                container(card)
                    .padding(MAIN_PANEL_PADDING)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(modal_backdrop_style);

            opaque(modal_overlay)
        })
    }

    fn view_info_modal(&self) -> Element<'_, Message> {
        perf::measure("ui.view_info_modal", || {
            let year = get_current_year();

            let card = container(
                column![
                    row![
                        text("About")
                            .size(MODAL_TITLE_TEXT_SIZE)
                            .width(Length::Fill),
                        button(icon::x())
                            .on_press(Message::InfoDismissed)
                            .style(button::text)
                            .padding(4),
                    ],
                    Space::with_height(15),
                    text(APP_NAME).size(EMPTY_STATE_TEXT_SIZE).font(Font {
                        weight: iced::font::Weight::Bold,
                        ..Default::default()
                    }),
                    text(format!("v{}", APP_VERSION)).size(MODAL_BODY_TEXT_SIZE),
                    Space::with_height(10),
                    text(APP_DESCRIPTION).size(MODAL_BODY_TEXT_SIZE),
                    Space::with_height(10),
                    text(format!("(c) {} {}", year, "@roymejia2217")).size(MODAL_LABEL_TEXT_SIZE),
                ]
                .padding(20),
            )
            .width(modal_width_for_density(
                self.current_density(),
                MODAL_WIDTH_STANDARD,
            ))
            .style(modal_card_style);

            let modal_overlay = container(
                container(card)
                    .padding(MAIN_PANEL_PADDING)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(modal_backdrop_style);

            opaque(modal_overlay)
        })
    }
}

impl Drop for JamePromptApp {
    fn drop(&mut self) {
        let _ = perf::flush_global();
    }
}

fn hotkey_badge_foreground_color(theme: &Theme) -> Color {
    theme.extended_palette().background.base.text
}

fn view_prompt_list_favorite_indicator(favorite: bool) -> Element<'static, Message> {
    let width = Length::Fixed(PROMPT_LIST_FAVORITE_INDICATOR_WIDTH);

    if favorite {
        container(icon::star().size(PROMPT_LIST_FAVORITE_ICON_SIZE))
            .width(width)
            .into()
    } else {
        Space::with_width(width).into()
    }
}

fn image_handle_for_theme(theme: &Theme) -> iced::widget::image::Handle {
    let bytes = if matches!(theme, Theme::Light) {
        APP_LOGO_LIGHT_BYTES
    } else {
        APP_LOGO_DARK_BYTES
    };

    iced::widget::image::Handle::from_bytes(bytes)
}

fn prompt_card_style(is_selected: bool, theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(iced::Background::Color(if is_selected {
            palette.primary.weak.color
        } else {
            palette.background.base.color
        })),
        text_color: Some(if is_selected {
            palette.primary.weak.text
        } else {
            palette.background.base.text
        }),
        border: iced::Border {
            radius: BORDER_RADIUS.into(),
            width: 1.0,
            color: if is_selected {
                palette.primary.strong.color
            } else {
                palette.secondary.weak.color
            },
        },
        ..Default::default()
    }
}

fn prompt_preview_text_color(is_selected: bool, theme: &Theme) -> Color {
    let palette = theme.extended_palette();
    if is_selected {
        palette.primary.weak.text
    } else {
        palette.background.base.text
    }
}

fn modal_width_for_density(density: UiDensity, regular_width: f32) -> Length {
    match density {
        UiDensity::Regular => Length::Fixed(regular_width),
        UiDensity::Compact => Length::Fill,
    }
}

fn hotkey_card_style(theme: &Theme) -> container::Style {
    prompt_card_style(false, theme)
}

fn modal_card_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    surface_card_style(theme, palette.secondary.strong.color)
}

fn detail_panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    surface_card_style(theme, palette.secondary.weak.color)
}

fn surface_card_style(theme: &Theme, border_color: Color) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(iced::Background::Color(palette.background.base.color)),
        border: iced::Border {
            radius: BORDER_RADIUS.into(),
            width: 1.0,
            color: border_color,
        },
        ..Default::default()
    }
}

fn modal_backdrop_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(modal_backdrop_color(theme))),
        ..Default::default()
    }
}

fn modal_backdrop_color(theme: &Theme) -> Color {
    let palette = theme.extended_palette();
    let alpha = if palette.is_dark { 0.6 } else { 0.3 };
    Color::from_rgba(0.0, 0.0, 0.0, alpha)
}

fn footer_container_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(iced::Background::Color(palette.background.base.color)),
        text_color: Some(palette.background.base.text),
        ..Default::default()
    }
}

fn primary_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let base = button::Style {
        background: Some(iced::Background::Color(palette.primary.strong.color)),
        text_color: palette.primary.strong.text,
        border: iced::Border {
            radius: 4.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..Default::default()
    };
    match status {
        button::Status::Active => base,
        button::Status::Hovered => button::Style {
            background: Some(iced::Background::Color(palette.primary.base.color)),
            text_color: palette.primary.base.text,
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(iced::Background::Color(palette.primary.weak.color)),
            text_color: palette.primary.weak.text,
            ..base
        },
        button::Status::Disabled => button::Style {
            background: Some(iced::Background::Color(palette.background.weak.color)),
            text_color: palette.background.weak.text,
            ..base
        },
    }
}

fn search_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let base = text_input::Style {
        background: iced::Background::Color(palette.background.weak.color),
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: palette.secondary.strong.color,
        },
        icon: palette.secondary.strong.color,
        placeholder: palette.secondary.strong.color,
        value: palette.background.base.text,
        selection: palette.primary.strong.color,
    };
    match status {
        text_input::Status::Active => base,
        text_input::Status::Hovered => text_input::Style {
            border: iced::Border {
                width: 1.5,
                color: palette.primary.weak.color,
                ..base.border
            },
            ..base
        },
        text_input::Status::Focused => text_input::Style {
            border: iced::Border {
                width: 2.0,
                color: palette.primary.strong.color,
                ..base.border
            },
            ..base
        },
        text_input::Status::Disabled => text_input::Style {
            background: iced::Background::Color(palette.background.base.color),
            value: palette.background.weak.text,
            ..base
        },
    }
}

fn checkbox_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let palette = theme.extended_palette();
    let base = checkbox::Style {
        background: palette.background.weak.color.into(),
        icon_color: palette.background.base.text,
        border: Border {
            color: palette.secondary.strong.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(palette.background.base.text),
    };
    match status {
        checkbox::Status::Active { is_checked: _ } => base,
        checkbox::Status::Hovered { is_checked: _ } => checkbox::Style {
            border: Border {
                color: palette.primary.strong.color,
                width: 1.5,
                ..base.border
            },
            ..base
        },
        checkbox::Status::Disabled { is_checked: _ } => checkbox::Style {
            background: palette.background.base.color.into(),
            text_color: Some(palette.background.weak.text),
            ..base
        },
    }
}

fn pick_list_style(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let palette = theme.extended_palette();
    let base = pick_list::Style {
        background: palette.background.weak.color.into(),
        border: Border {
            color: palette.secondary.strong.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: palette.background.base.text,
        placeholder_color: palette.background.base.text,
        handle_color: palette.background.base.text,
    };
    match status {
        pick_list::Status::Active => base,
        pick_list::Status::Hovered => pick_list::Style {
            border: Border {
                color: palette.primary.strong.color,
                width: 1.5,
                ..base.border
            },
            ..base
        },
        pick_list::Status::Opened => pick_list::Style {
            border: Border {
                color: palette.primary.strong.color,
                width: 2.0,
                ..base.border
            },
            ..base
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reproduction tests for bug: status ignored and theme mismatch
    #[test]
    fn repro_button_hover_state_is_ignored() {
        let active = primary_button_style(&Theme::Dark, button::Status::Active);
        let hovered = primary_button_style(&Theme::Dark, button::Status::Hovered);
        assert_ne!(
            active.background, hovered.background,
            "Bug: button hover state should change background color"
        );
    }

    #[test]
    fn repro_input_focus_state_is_ignored() {
        let active = search_input_style(&Theme::Dark, text_input::Status::Active);
        let hovered = search_input_style(&Theme::Dark, text_input::Status::Hovered);
        assert_ne!(
            active.border.color, hovered.border.color,
            "Bug: input hover state should change border color"
        );
    }

    #[test]
    fn repro_theme_dark_maps_to_builtin_dark() {
        let mut app = JamePromptApp::default();
        app.settings.theme = "Dark".to_string();
        assert_eq!(
            app.theme(),
            Theme::Dark,
            "Bug: 'Dark' setting should map to iced built-in Theme::Dark, not Moonfly"
        );
    }

    #[test]
    fn repro_theme_light_maps_to_builtin_light() {
        let mut app = JamePromptApp::default();
        app.settings.theme = "Light".to_string();
        assert_eq!(
            app.theme(),
            Theme::Light,
            "Bug: 'Light' setting should map to iced built-in Theme::Light, not GruvboxLight"
        );
    }

    #[test]
    fn test_title_omits_version_suffix() {
        let app = JamePromptApp::with_database(Database::in_memory().expect("In-memory DB failed"));

        assert_eq!(app.title(), APP_NAME);
    }

    #[test]
    fn test_image_handle_uses_theme_specific_logo() {
        let dark = image_handle_for_theme(&Theme::Dark);
        let light = image_handle_for_theme(&Theme::Light);

        match dark {
            iced::widget::image::Handle::Bytes(_, ref bytes) => {
                assert_eq!(bytes.as_ref(), APP_LOGO_DARK_BYTES);
            }
            _ => panic!("Dark theme should use embedded logo bytes"),
        }

        match light {
            iced::widget::image::Handle::Bytes(_, ref bytes) => {
                assert_eq!(bytes.as_ref(), APP_LOGO_LIGHT_BYTES);
            }
            _ => panic!("Light theme should use embedded logo bytes"),
        }
    }

    #[test]
    fn test_star_icon_is_registered() {
        assert!(
            icon::ALL_ICONS
                .iter()
                .any(|(name, _codepoint)| *name == "star"),
            "The Lucide star icon should be generated for favorite UI"
        );
    }

    #[test]
    fn test_arrow_left_icon_is_registered() {
        assert!(
            icon::ALL_ICONS
                .iter()
                .any(|(name, _codepoint)| *name == "arrow_left"),
            "The Lucide arrow-left icon should be generated for compact navigation"
        );
    }

    // Group A: modal_card_style
    #[test]
    fn test_modal_card_style_uses_theme_background() {
        let style = modal_card_style(&Theme::Dark);
        let expected = Theme::Dark.extended_palette().background.base.color;
        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_modal_card_style_has_border_radius() {
        let style = modal_card_style(&Theme::Dark);
        assert_eq!(style.border.radius, 8.0.into());
    }

    #[test]
    fn test_modal_card_style_has_theme_border_color() {
        let style = modal_card_style(&Theme::Dark);
        let expected = Theme::Dark.extended_palette().secondary.strong.color;
        assert_eq!(style.border.color, expected);
    }

    #[test]
    fn test_modal_card_style_light_differs_from_dark() {
        let dark = modal_card_style(&Theme::Dark);
        let light = modal_card_style(&Theme::Light);
        assert_ne!(dark.background, light.background);
    }

    // Group B: modal_backdrop_style
    #[test]
    fn test_modal_backdrop_style_dark_theme_uses_higher_alpha() {
        let style = modal_backdrop_style(&Theme::Dark);
        let expected = iced::Color::from_rgba(0.0, 0.0, 0.0, 0.6);
        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_modal_backdrop_style_light_theme_uses_lower_alpha() {
        let style = modal_backdrop_style(&Theme::Light);
        let expected = iced::Color::from_rgba(0.0, 0.0, 0.0, 0.3);
        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_modal_backdrop_style_no_border() {
        let style = modal_backdrop_style(&Theme::Dark);
        assert_eq!(style.border.width, 0.0);
    }

    #[test]
    fn test_modal_section_spacing_is_consistent() {
        assert_eq!(MODAL_SECTION_SPACING, 10);
    }

    #[test]
    fn test_detail_section_spacing_is_consistent() {
        assert_eq!(DETAIL_SECTION_SPACING, 15);
    }

    #[test]
    fn test_prompt_toolbar_spacing_separates_primary_and_secondary_controls() {
        assert!(PROMPT_PRIMARY_TOOLBAR_SPACING > 0);
        assert!(PROMPT_SECONDARY_TOOLBAR_SPACING > 0);
        assert!(PROMPT_PRIMARY_TOOLBAR_SPACING >= PROMPT_SECONDARY_TOOLBAR_SPACING);
    }

    #[test]
    fn test_prompt_toolbar_picker_widths_are_stable() {
        assert!(PROMPT_FILTER_PICKER_WIDTH >= 100.0);
        assert!(PROMPT_SORT_PICKER_WIDTH >= 150.0);
        assert!(PROMPT_SORT_PICKER_WIDTH > PROMPT_FILTER_PICKER_WIDTH);
    }

    #[test]
    fn test_ui_density_regular_above_threshold() {
        assert_eq!(
            ui_density_for_width(COMPACT_LAYOUT_WIDTH_THRESHOLD + 1.0),
            UiDensity::Regular
        );
    }

    #[test]
    fn test_ui_density_compact_below_threshold() {
        assert_eq!(
            ui_density_for_width(COMPACT_LAYOUT_WIDTH_THRESHOLD - 1.0),
            UiDensity::Compact
        );
    }

    #[test]
    fn test_ui_density_boundary_is_regular() {
        assert_eq!(
            ui_density_for_width(COMPACT_LAYOUT_WIDTH_THRESHOLD),
            UiDensity::Regular
        );
    }

    #[test]
    fn test_default_app_width_starts_regular() {
        let app = JamePromptApp::with_database(Database::in_memory().unwrap());

        assert_eq!(app.content_width, crate::config::WINDOW_INITIAL_WIDTH);
        assert_eq!(app.current_density(), UiDensity::Regular);
    }

    #[test]
    fn test_window_resized_updates_content_width() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());

        let _ = app.update(Message::WindowResized(iced::Size::new(
            COMPACT_LAYOUT_WIDTH_THRESHOLD - 1.0,
            500.0,
        )));

        assert_eq!(app.content_width, COMPACT_LAYOUT_WIDTH_THRESHOLD - 1.0);
        assert_eq!(app.current_density(), UiDensity::Compact);
    }

    #[test]
    fn test_modal_width_regular_uses_named_fixed_width() {
        assert_eq!(
            modal_width_for_density(UiDensity::Regular, MODAL_WIDTH_STANDARD),
            Length::Fixed(MODAL_WIDTH_STANDARD)
        );
    }

    #[test]
    fn test_modal_width_compact_uses_fill() {
        assert_eq!(
            modal_width_for_density(UiDensity::Compact, MODAL_WIDTH_STANDARD),
            Length::Fill
        );
    }

    #[test]
    fn test_prompt_list_favorite_indicator_width_is_stable() {
        assert!(PROMPT_LIST_FAVORITE_INDICATOR_WIDTH >= 20.0);
        assert!(PROMPT_LIST_FAVORITE_INDICATOR_WIDTH <= 32.0);
    }

    #[test]
    fn test_prompt_list_favorite_icon_size_fits_indicator() {
        assert!((PROMPT_LIST_FAVORITE_ICON_SIZE as f32) < PROMPT_LIST_FAVORITE_INDICATOR_WIDTH);
    }

    // Group C: container_style
    #[test]
    fn test_prompt_card_style_selected_uses_theme() {
        let style = prompt_card_style(true, &Theme::Dark);
        let expected = Theme::Dark.extended_palette().primary.weak.color;
        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_prompt_card_style_selected_has_text_color() {
        let style = prompt_card_style(true, &Theme::Dark);
        let expected = Theme::Dark.extended_palette().primary.weak.text;
        assert_eq!(style.text_color, Some(expected));
    }

    #[test]
    fn test_prompt_card_style_unselected_uses_palette_pairs() {
        let style = prompt_card_style(false, &Theme::Dark);
        let palette = Theme::Dark.extended_palette();
        assert_eq!(
            style.background,
            Some(iced::Background::Color(palette.background.base.color))
        );
        assert_eq!(style.text_color, Some(palette.background.base.text));
    }

    #[test]
    fn test_prompt_preview_text_color_unselected_uses_readable_theme_text() {
        let dark = prompt_preview_text_color(false, &Theme::Dark);
        let light = prompt_preview_text_color(false, &Theme::Light);

        assert_eq!(dark, Theme::Dark.extended_palette().background.base.text);
        assert_eq!(light, Theme::Light.extended_palette().background.base.text);
    }

    #[test]
    fn test_prompt_preview_text_color_selected_uses_selected_card_text() {
        let dark = prompt_preview_text_color(true, &Theme::Dark);
        let light = prompt_preview_text_color(true, &Theme::Light);

        assert_eq!(dark, Theme::Dark.extended_palette().primary.weak.text);
        assert_eq!(light, Theme::Light.extended_palette().primary.weak.text);
    }

    #[test]
    fn test_prompt_preview_text_color_light_avoids_secondary_strong() {
        let preview_color = prompt_preview_text_color(false, &Theme::Light);
        let low_contrast_color = Theme::Light.extended_palette().secondary.strong.color;

        assert_ne!(preview_color, low_contrast_color);
    }

    #[test]
    fn test_container_style_selected_light_differs_from_dark() {
        let dark = prompt_card_style(true, &Theme::Dark);
        let light = prompt_card_style(true, &Theme::Light);
        assert_ne!(dark.background, light.background);
    }

    #[test]
    fn test_hotkey_card_style_reuses_unselected_prompt_card_style() {
        let hotkey = hotkey_card_style(&Theme::Dark);
        let unselected_prompt = prompt_card_style(false, &Theme::Dark);

        assert_eq!(hotkey.background, unselected_prompt.background);
        assert_eq!(hotkey.text_color, unselected_prompt.text_color);
        assert_eq!(hotkey.border, unselected_prompt.border);
    }

    // Group D: primary_button_style
    #[test]
    fn test_primary_button_style_has_background() {
        let style = primary_button_style(&Theme::Dark, button::Status::Active);
        let expected = Theme::Dark.extended_palette().primary.strong.color;
        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_primary_button_style_has_text_color() {
        let style = primary_button_style(&Theme::Dark, button::Status::Active);
        let expected = Theme::Dark.extended_palette().primary.strong.text;
        assert_eq!(style.text_color, expected);
    }

    #[test]
    fn test_primary_button_style_has_border_radius() {
        let style = primary_button_style(&Theme::Dark, button::Status::Active);
        assert_eq!(style.border.radius, 4.0.into());
    }

    #[test]
    fn test_primary_button_style_light_differs_from_dark() {
        let dark = primary_button_style(&Theme::Dark, button::Status::Disabled);
        let light = primary_button_style(&Theme::Light, button::Status::Disabled);
        assert_ne!(
            dark.background, light.background,
            "Disabled button background should differ between themes"
        );
    }

    #[test]
    fn test_primary_button_style_hovered_differs_from_active() {
        let active = primary_button_style(&Theme::Dark, button::Status::Active);
        let hovered = primary_button_style(&Theme::Dark, button::Status::Hovered);
        assert_ne!(active.background, hovered.background);
    }

    #[test]
    fn test_primary_button_style_pressed_differs_from_active() {
        let active = primary_button_style(&Theme::Dark, button::Status::Active);
        let pressed = primary_button_style(&Theme::Dark, button::Status::Pressed);
        assert_ne!(active.background, pressed.background);
    }

    #[test]
    fn test_primary_button_style_disabled_differs_from_active() {
        let active = primary_button_style(&Theme::Dark, button::Status::Active);
        let disabled = primary_button_style(&Theme::Dark, button::Status::Disabled);
        assert_ne!(active.background, disabled.background);
    }

    #[test]
    fn test_danger_button_style_uses_theme_danger_palette() {
        let style = button::danger(&Theme::Dark, button::Status::Active);
        let expected = Theme::Dark.extended_palette().danger.base.color;

        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_secondary_button_style_uses_theme_secondary_palette() {
        let style = button::secondary(&Theme::Dark, button::Status::Active);
        let expected = Theme::Dark.extended_palette().secondary.base.color;

        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_detail_panel_style_uses_theme_surface() {
        let style = detail_panel_style(&Theme::Dark);
        let palette = Theme::Dark.extended_palette();

        assert_eq!(
            style.background,
            Some(iced::Background::Color(palette.background.base.color))
        );
        assert_eq!(style.border.color, palette.secondary.weak.color);
    }

    #[test]
    fn test_hotkey_badge_foreground_uses_theme_text_color() {
        let dark = hotkey_badge_foreground_color(&Theme::Dark);
        let light = hotkey_badge_foreground_color(&Theme::Light);

        assert_eq!(dark, Theme::Dark.extended_palette().background.base.text);
        assert_eq!(light, Theme::Light.extended_palette().background.base.text);
    }

    #[test]
    fn test_prompt_list_favorite_indicator_uses_card_text_color() {
        let selected = prompt_card_style(true, &Theme::Dark);
        let unselected = prompt_card_style(false, &Theme::Dark);

        assert_eq!(
            selected.text_color,
            Some(Theme::Dark.extended_palette().primary.weak.text)
        );
        assert_eq!(
            unselected.text_color,
            Some(Theme::Dark.extended_palette().background.base.text)
        );
    }

    #[test]
    fn test_form_save_rejects_invalid_hotkey() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        let mut app = JamePromptApp::with_database(db);

        app.new_form = Some(NewPromptForm {
            name: "Security".to_string(),
            content: "Invalid hotkey should not be saved".to_string(),
            content_editor: text_editor::Content::with_text("Invalid hotkey should not be saved"),
            hotkey: "Ctrl++".to_string(),
            hotkey_enabled: true,
            editing_id: None,
        });

        let _ = app.update(Message::FormSave);

        assert!(
            app.status_message.contains("Invalid hotkey"),
            "Invalid hotkey should be rejected with an explicit message"
        );
        assert!(
            app.prompts.is_empty(),
            "Invalid hotkey should not create a prompt"
        );
        let form = app.new_form.as_ref().expect("Form should be preserved");
        assert_eq!(form.name, "Security");
        assert_eq!(form.content, "Invalid hotkey should not be saved");
        assert_eq!(form.hotkey, "Ctrl++");
    }

    #[test]
    fn test_open_database_falls_back_to_memory_when_path_is_invalid() {
        let bad_path = Path::new("/this/path/should/not/exist/prompts.db");
        let (db, warning) = JamePromptApp::open_database(bad_path);

        assert!(
            warning.is_some(),
            "Invalid path should produce a startup warning"
        );
        assert!(
            db.get_all("")
                .expect("Fallback database should be usable")
                .is_empty(),
            "Fallback database should start empty"
        );
    }

    #[test]
    fn test_open_database_falls_back_from_corrupt_file() {
        use std::fs;

        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("corrupt.db");
        fs::write(&db_path, b"corrupt sqlite data").expect("Failed to write corrupt db");

        let (db, warning) = JamePromptApp::open_database(&db_path);

        assert!(
            warning.is_some(),
            "Corrupt database should trigger a warning"
        );
        assert!(
            db.get_all("")
                .expect("Fallback database should be usable")
                .is_empty(),
            "Fallback database should be empty after corrupt DB recovery"
        );
    }

    // Group E: search_input_style
    #[test]
    fn test_search_input_style_has_background() {
        let style = search_input_style(&Theme::Dark, text_input::Status::Active);
        let expected = Theme::Dark.extended_palette().background.weak.color;
        assert_eq!(style.background, iced::Background::Color(expected));
    }

    #[test]
    fn test_search_input_style_has_border() {
        let style = search_input_style(&Theme::Dark, text_input::Status::Active);
        assert_eq!(style.border.width, 1.0);
        assert_eq!(style.border.radius, 4.0.into());
    }

    #[test]
    fn test_search_input_style_light_differs_from_dark() {
        let dark = search_input_style(&Theme::Dark, text_input::Status::Active);
        let light = search_input_style(&Theme::Light, text_input::Status::Active);
        assert_ne!(dark.background, light.background);
    }

    #[test]
    fn test_search_input_style_hovered_differs_from_active() {
        let active = search_input_style(&Theme::Dark, text_input::Status::Active);
        let hovered = search_input_style(&Theme::Dark, text_input::Status::Hovered);
        assert_ne!(active.border.width, hovered.border.width);
    }

    #[test]
    fn test_search_input_style_focused_differs_from_active() {
        let active = search_input_style(&Theme::Dark, text_input::Status::Active);
        let focused = search_input_style(&Theme::Dark, text_input::Status::Focused);
        assert_ne!(active.border.color, focused.border.color);
    }

    // Group F: checkbox_style
    #[test]
    fn test_checkbox_style_has_background() {
        let style = checkbox_style(&Theme::Dark, checkbox::Status::Active { is_checked: false });
        let expected = Theme::Dark.extended_palette().background.weak.color.into();
        assert_eq!(style.background, expected);
    }

    #[test]
    fn test_checkbox_style_has_text_color() {
        let style = checkbox_style(&Theme::Dark, checkbox::Status::Active { is_checked: false });
        let expected = Some(Theme::Dark.extended_palette().background.base.text);
        assert_eq!(style.text_color, expected);
    }

    #[test]
    fn test_checkbox_style_uses_theme_foreground_for_icon() {
        let style = checkbox_style(&Theme::Dark, checkbox::Status::Active { is_checked: false });
        let expected = Theme::Dark.extended_palette().background.base.text;
        assert_eq!(style.icon_color, expected);
    }

    #[test]
    fn test_checkbox_style_light_differs_from_dark() {
        let dark = checkbox_style(&Theme::Dark, checkbox::Status::Active { is_checked: false });
        let light = checkbox_style(
            &Theme::Light,
            checkbox::Status::Active { is_checked: false },
        );
        assert_ne!(dark.background, light.background);
    }

    #[test]
    fn test_checkbox_style_hovered_differs_from_active() {
        let active = checkbox_style(&Theme::Dark, checkbox::Status::Active { is_checked: false });
        let hovered = checkbox_style(
            &Theme::Dark,
            checkbox::Status::Hovered { is_checked: false },
        );
        assert_ne!(active.border.color, hovered.border.color);
    }

    #[test]
    fn test_checkbox_style_disabled_differs_from_active() {
        let active = checkbox_style(&Theme::Dark, checkbox::Status::Active { is_checked: false });
        let disabled = checkbox_style(
            &Theme::Dark,
            checkbox::Status::Disabled { is_checked: false },
        );
        assert_ne!(active.background, disabled.background);
    }

    // Group G: pick_list_style
    #[test]
    fn test_pick_list_style_has_background() {
        let style = pick_list_style(&Theme::Dark, pick_list::Status::Active);
        let expected = Theme::Dark.extended_palette().background.weak.color.into();
        assert_eq!(style.background, expected);
    }

    #[test]
    fn test_pick_list_style_has_border() {
        let style = pick_list_style(&Theme::Dark, pick_list::Status::Active);
        let expected = Theme::Dark.extended_palette().secondary.strong.color;
        assert_eq!(style.border.color, expected);
    }

    #[test]
    fn test_pick_list_style_uses_theme_foreground_for_handle() {
        let style = pick_list_style(&Theme::Dark, pick_list::Status::Active);
        let expected = Theme::Dark.extended_palette().background.base.text;
        assert_eq!(style.handle_color, expected);
    }

    #[test]
    fn test_pick_list_style_light_differs_from_dark() {
        let dark = pick_list_style(&Theme::Dark, pick_list::Status::Active);
        let light = pick_list_style(&Theme::Light, pick_list::Status::Active);
        assert_ne!(dark.background, light.background);
    }

    #[test]
    fn test_pick_list_style_hovered_differs_from_active() {
        let active = pick_list_style(&Theme::Dark, pick_list::Status::Active);
        let hovered = pick_list_style(&Theme::Dark, pick_list::Status::Hovered);
        assert_ne!(active.border.color, hovered.border.color);
    }

    #[test]
    fn test_pick_list_style_opened_differs_from_active() {
        let active = pick_list_style(&Theme::Dark, pick_list::Status::Active);
        let opened = pick_list_style(&Theme::Dark, pick_list::Status::Opened);
        assert_ne!(active.border.width, opened.border.width);
    }

    // Group H: footer_container_style
    #[test]
    fn test_footer_container_style_uses_background_base() {
        let style = footer_container_style(&Theme::Dark);
        let expected = Theme::Dark.extended_palette().background.base.color;
        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_footer_container_style_light_differs_from_dark() {
        let dark = footer_container_style(&Theme::Dark);
        let light = footer_container_style(&Theme::Light);
        assert_ne!(dark.background, light.background);
    }

    // Group I: NewPromptForm
    #[test]
    fn test_new_prompt_form_default_hotkey_enabled() {
        let form = NewPromptForm::default();
        assert!(form.hotkey_enabled, "Default hotkey_enabled should be true");
        assert!(form.name.is_empty());
        assert!(form.content.is_empty());
        assert!(form.hotkey.is_empty());
        assert!(form.editing_id.is_none());
    }

    #[test]
    fn test_new_prompt_form_from_prompt_preserves_hotkey_enabled() {
        let prompt = Prompt {
            id: String::from("22222222-2222-4222-8222-222222222222"),
            name: "Test".to_string(),
            content: "Content".to_string(),
            hotkey: Some("Ctrl+G".to_string()),
            hotkey_enabled: false,
            ..Prompt::default()
        };
        let form = NewPromptForm::from_prompt(&prompt);
        assert!(
            !form.hotkey_enabled,
            "hotkey_enabled should be preserved from prompt"
        );
        assert_eq!(form.hotkey, "Ctrl+G");
        assert_eq!(
            form.editing_id,
            Some(String::from("22222222-2222-4222-8222-222222222222"))
        );
    }

    #[test]
    fn test_new_prompt_form_from_prompt_hotkey_enabled_true() {
        let prompt = Prompt {
            id: String::from("33333333-3333-4333-8333-333333333333"),
            name: "Another".to_string(),
            content: "Body".to_string(),
            hotkey: Some("Ctrl+Shift+P".to_string()),
            hotkey_enabled: true,
            ..Prompt::default()
        };
        let form = NewPromptForm::from_prompt(&prompt);
        assert!(
            form.hotkey_enabled,
            "hotkey_enabled should be true when prompt has it true"
        );
    }

    // Group J: Info modal state
    #[test]
    fn test_show_info_defaults_to_false() {
        let app = JamePromptApp::default();
        assert!(!app.show_info, "show_info should default to false");
    }

    #[test]
    fn test_info_pressed_opens_info_modal() {
        let (mut app, _) = JamePromptApp::new();
        let _ = app.update(Message::InfoPressed);
        assert!(app.show_info, "InfoPressed should set show_info to true");
        assert!(
            !app.show_settings,
            "InfoPressed should close settings modal"
        );
    }

    #[test]
    fn test_info_dismissed_closes_info_modal() {
        let (mut app, _) = JamePromptApp::new();
        app.show_info = true;
        let _ = app.update(Message::InfoDismissed);
        assert!(
            !app.show_info,
            "InfoDismissed should set show_info to false"
        );
    }

    #[test]
    fn test_settings_pressed_closes_info_modal() {
        let (mut app, _) = JamePromptApp::new();
        app.show_info = true;
        let _ = app.update(Message::SettingsPressed);
        assert!(!app.show_info, "SettingsPressed should close info modal");
        assert!(
            app.show_settings,
            "SettingsPressed should open settings modal"
        );
    }

    #[test]
    fn test_settings_autostart_toggled_updates_state() {
        let (mut app, _) = JamePromptApp::new();
        let _ = app.update(Message::SettingsAutostartToggled(true));
        assert!(
            app.settings.autostart_enabled,
            "SettingsAutostartToggled should update autostart_enabled to true"
        );

        let _ = app.update(Message::SettingsAutostartToggled(false));
        assert!(
            !app.settings.autostart_enabled,
            "SettingsAutostartToggled should update autostart_enabled to false"
        );
    }

    #[test]
    fn delete_pressed_opens_confirmation_without_deleting() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        let id = db
            .insert(&Prompt {
                id: String::new(),
                name: "Delete".to_string(),
                content: "Content".to_string(),
                hotkey: None,
                hotkey_enabled: true,
                favorite: false,
                created_at: String::new(),
                updated_at: String::new(),
                last_used_at: None,
                use_count: 0,
                ..Prompt::default()
            })
            .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);

        let _ = app.update(Message::DeletePressed(id.clone()));

        assert_eq!(app.pending_delete_id, Some(id));
        assert_eq!(app.prompts.len(), 1);
    }

    #[test]
    fn delete_cancel_preserves_prompt() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        let id = db
            .insert(&Prompt {
                id: String::new(),
                name: "Delete".to_string(),
                content: "Content".to_string(),
                hotkey: None,
                hotkey_enabled: true,
                favorite: false,
                created_at: String::new(),
                updated_at: String::new(),
                last_used_at: None,
                use_count: 0,
                ..Prompt::default()
            })
            .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);

        let _ = app.update(Message::DeletePressed(id));
        let _ = app.update(Message::DeleteCancelPressed);

        assert!(app.pending_delete_id.is_none());
        assert_eq!(app.prompts.len(), 1);
    }

    #[test]
    fn form_editor_text_is_normalized_on_save() {
        let text = normalize_editor_text("line one\nline two\n");

        assert_eq!(text, "line one\nline two");
    }

    #[test]
    fn test_favorite_toggled_updates_selected_prompt_state() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        let id = db
            .insert(&Prompt {
                id: String::new(),
                name: "Favorite target".to_string(),
                content: "Content".to_string(),
                hotkey: None,
                hotkey_enabled: true,
                favorite: false,
                created_at: String::new(),
                updated_at: String::new(),
                last_used_at: None,
                use_count: 0,
                ..Prompt::default()
            })
            .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);
        app.selected_id = Some(id.clone());

        let _ = app.update(Message::FavoriteToggled(id.clone(), true));

        let prompt = app
            .prompts
            .iter()
            .find(|prompt| prompt.id == id)
            .expect("Prompt should remain visible after favorite update");
        assert!(prompt.favorite);
        assert_eq!(app.selected_id, Some(id));
        assert_eq!(app.status_message, "Prompt added to favorites");
    }

    #[test]
    fn search_changed_filters_prompts_from_cached_data() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Alpha".to_string(),
            content: "First content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
            ..Prompt::default()
        })
        .expect("Insert should succeed");
        db.insert(&Prompt {
            id: String::new(),
            name: "Beta".to_string(),
            content: "Second content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
            ..Prompt::default()
        })
        .expect("Insert should succeed");

        let mut app = JamePromptApp::with_database(db);
        let _ = app.update(Message::SearchChanged("alpha".to_string()));

        assert_eq!(app.prompts.len(), 1);
        assert_eq!(app.prompts[0].name, "Alpha");
        assert_eq!(app.status_message, "1 prompts found");
    }

    #[test]
    fn search_changed_is_case_insensitive_for_cached_data() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Mixed Case".to_string(),
            content: "Searchable Content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
            ..Prompt::default()
        })
        .expect("Insert should succeed");

        let mut app = JamePromptApp::with_database(db);
        let _ = app.update(Message::SearchChanged("mixed".to_string()));

        assert_eq!(app.prompts.len(), 1);
        assert_eq!(app.prompts[0].name, "Mixed Case");
    }

    #[test]
    fn prompt_sort_changed_orders_cached_prompts_by_usage() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Low Usage".to_string(),
            content: "A".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 1,
            ..Prompt::default()
        })
        .expect("Insert should succeed");
        db.insert(&Prompt {
            id: String::new(),
            name: "High Usage".to_string(),
            content: "B".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 10,
            ..Prompt::default()
        })
        .expect("Insert should succeed");

        let mut app = JamePromptApp::with_database(db);
        let _ = app.update(Message::PromptSortChanged(PromptSort::MostUsed));

        assert_eq!(app.prompts.len(), 2);
        assert_eq!(app.prompts[0].name, "High Usage");
        assert_eq!(app.prompts[1].name, "Low Usage");
    }

    #[test]
    fn copy_pressed_updates_cached_usage_metrics() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        let id = db
            .insert(&Prompt {
                id: String::new(),
                name: "Usage".to_string(),
                content: "Copy me".to_string(),
                hotkey: None,
                hotkey_enabled: true,
                favorite: false,
                created_at: String::new(),
                updated_at: String::new(),
                last_used_at: None,
                use_count: 0,
                ..Prompt::default()
            })
            .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);

        let _ = app.update(Message::CopyPressed(id.clone(), "Copy me".to_string()));

        let prompt = app
            .prompt_cloned_by_id(&id)
            .expect("Prompt should remain cached after copy");
        assert_eq!(prompt.use_count, 1);
        assert!(prompt.last_used_at.is_some());
    }

    #[test]
    fn close_requests_exit_in_smoke_mode() {
        assert!(should_exit_on_close_request(true));
        assert!(!should_exit_on_close_request(false));
    }

    #[test]
    fn smoke_mode_disables_tray_integration() {
        let (app, _task) = JamePromptApp::new_with_hidden_start(false, true);

        assert!(app.smoke_mode);
        assert!(app.smoke_deadline.is_some());
        assert!(app.tray.is_none());
        assert!(app.hotkey_service.is_none());
        assert!(app.all_prompts.is_empty());
    }
}
