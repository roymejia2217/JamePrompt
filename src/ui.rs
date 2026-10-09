const BORDER_RADIUS: f32 = 8.0;
const MAIN_PANEL_PADDING: u16 = 15;
const DETAIL_PANEL_INNER_PADDING: u16 = 20;
const DETAIL_SECTION_SPACING: u32 = 15;
const HEADER_LOGO_SIZE: u32 = 28;
const HEADER_BRAND_SPACING: u32 = 10;
const MODAL_SECTION_SPACING: u32 = 10;
const PROMPT_PRIMARY_TOOLBAR_SPACING: u32 = 10;
const PROMPT_SECONDARY_TOOLBAR_SPACING: u32 = 8;
const PROMPT_FILTER_PICKER_WIDTH: f32 = 120.0;
const PROMPT_SORT_PICKER_WIDTH: f32 = 170.0;
const PROMPT_LIST_FAVORITE_INDICATOR_WIDTH: f32 = 24.0;
const PROMPT_LIST_FAVORITE_ICON_SIZE: u32 = 16;
const COMPACT_LAYOUT_WIDTH_THRESHOLD: f32 = 840.0;
const LIST_PANEL_SPACING: u32 = 10;
const PROMPT_LIST_SPACING: u32 = 5;
const PROMPT_CARD_SPACING: u32 = 8;
const PROMPT_CARD_PADDING: u16 = 10;
const PROMPT_PREVIEW_TEXT_SIZE: u32 = 14;
const PROMPT_DETAIL_TITLE_SIZE: u32 = 24;
const PROMPT_CONTENT_TEXT_SIZE: u32 = 14;
const PROMPT_METADATA_TEXT_SIZE: u32 = 12;
const EMPTY_STATE_TEXT_SIZE: u32 = 16;
const MODAL_TITLE_TEXT_SIZE: u32 = 20;
const MODAL_LABEL_TEXT_SIZE: u32 = 12;
const MODAL_BODY_TEXT_SIZE: u32 = 14;
const MODAL_WIDTH_STANDARD: f32 = 350.0;
const MODAL_WIDTH_WIDE: f32 = 400.0;
const PROMPT_EDITOR_HEIGHT: f32 = 160.0;
const CONTROL_PADDING: u16 = 8;
const COMPACT_BACK_BUTTON_PADDING: u16 = 8;
const COMPACT_BODY_SPACING: u32 = 10;

fn prompt_editor_wrapping() -> iced::advanced::text::Wrapping {
    iced::advanced::text::Wrapping::WordOrGlyph
}

fn prompt_card_title_wrapping() -> iced::advanced::text::Wrapping {
    iced::advanced::text::Wrapping::WordOrGlyph
}

#[derive(Debug, Clone, Copy)]
enum ActionButtonTone {
    Neutral,
    Primary,
    Destructive,
}

fn action_button_style(tone: ActionButtonTone) -> fn(&Theme, button::Status) -> button::Style {
    match tone {
        ActionButtonTone::Neutral => neutral_action_button_style,
        ActionButtonTone::Primary => button::primary,
        ActionButtonTone::Destructive => button::danger,
    }
}

#[derive(Debug, Clone, Copy)]
enum DetailToolbarAction {
    Favorite { is_active: bool },
    Copy,
    Edit,
    Delete,
}

fn detail_toolbar_button_style(
    action: DetailToolbarAction,
) -> fn(&Theme, button::Status) -> button::Style {
    match action {
        DetailToolbarAction::Favorite { is_active: true } => {
            action_button_style(ActionButtonTone::Primary)
        }
        DetailToolbarAction::Favorite { is_active: false }
        | DetailToolbarAction::Copy
        | DetailToolbarAction::Edit => action_button_style(ActionButtonTone::Neutral),
        DetailToolbarAction::Delete => action_button_style(ActionButtonTone::Destructive),
    }
}

/// Styles non-destructive detail actions as neutral controls on the current
/// surface. The background `Pair` supplies its matching readable text color
/// for each native theme, so icons retain contrast without fixed colors.
fn neutral_action_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let (pair, is_disabled) = match status {
        button::Status::Active | button::Status::Pressed => (palette.background.weak, false),
        button::Status::Hovered => (palette.background.neutral, false),
        button::Status::Disabled => (palette.background.weak, true),
    };

    let mut style = button::secondary(theme, status);
    style.background = Some(iced::Background::Color(if is_disabled {
        pair.color.scale_alpha(0.5)
    } else {
        pair.color
    }));
    style.text_color = if is_disabled {
        pair.text.scale_alpha(0.5)
    } else {
        pair.text
    };
    style
}

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
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
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

use crate::application::notification_store::{NotificationId, NotificationStore};
#[cfg(not(test))]
use crate::autostart;
use crate::config::{
    get_current_year, Settings, APP_DESCRIPTION, APP_NAME, APP_VERSION, WINDOW_INITIAL_WIDTH,
};
use crate::db::Database;
#[cfg(target_os = "linux")]
use crate::desktop::ParentContext;
use crate::desktop::{
    CancelOutcome, DesktopRuntime, HotkeyEvent, RuntimeMode, ShortcutAcknowledgement,
    ShortcutSnapshot, ShortcutSpec, ShortcutStatus,
};
use crate::desktop_diagnostics::DesktopDiagnostics;
#[cfg(target_os = "linux")]
use crate::desktop_parent::SharedParentLease;
#[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
use crate::desktop_test_support::ControllerEvent as TestControllerEvent;
use crate::domain::notification::{
    Field, MessageKey, Notification, NotificationAction, NotificationContext, Presentation,
    Severity,
};
use crate::domain::notification_policy::{NotificationEvent, NotificationPolicy};
use crate::hotkeys::HotkeyService;
use crate::icon;
use crate::launch::should_show_window_after_hidden_start;
use crate::models::{Prompt, PromptFilter, PromptId, PromptQuery, PromptSort};
use crate::perf;
use crate::prompt_backup::{
    plan_import_operations, DuplicateMode, ImportMode, ImportSummary, PromptBackup,
    PromptImportPreview, PROMPT_BACKUP_DEFAULT_FILE_NAME, PROMPT_BACKUP_FILE_EXTENSION,
};
use crate::settings_service::SettingsService;
#[cfg(target_os = "windows")]
use crate::tray::{pump_platform_events, TrayEvent, TrayHandle};
use crate::tray::{TraySignal, TrayState};
use crate::window_lifecycle::{WindowEffect, WindowIntent, WindowPhase, WindowState};

#[cfg(not(test))]
fn sync_autostart_enabled(enabled: bool) -> Result<(), String> {
    autostart::sync(enabled).map_err(|error| error.to_string())
}

#[cfg(test)]
fn sync_autostart_enabled(_enabled: bool) -> Result<(), String> {
    Ok(())
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
    NativeClipboardPrepared(Option<String>),
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
    SettingsSaved(Result<(), String>),
    SettingsExportPressed,
    SettingsExportFinished(Result<PathBuf, String>),
    SettingsImportPressed,
    SettingsImportLoaded(Result<PromptBackup, String>),
    ImportModeSelected(ImportMode),
    DuplicateModeSelected(DuplicateMode),
    ImportConfirmPressed,
    ImportCancelPressed,
    // Info modal
    InfoPressed,
    InfoDismissed,
    // Window management
    CloseRequested(iced::window::Id),
    ShowWindow(Option<iced::window::Id>),
    /// Authoritative id assignment for an opening generation.
    WindowOpened {
        generation: u64,
        id: iced::window::Id,
    },
    /// Supplemental compositor observation (focus/redraw/close).
    DesktopWindowEvent {
        id: iced::window::Id,
        event: iced::window::Event,
    },
    /// Completed parent export for a window generation (Linux).
    #[cfg(target_os = "linux")]
    ParentExported {
        generation: u64,
        result: Result<SharedParentLease, crate::desktop::DesktopErrorCode>,
    },
    /// Portal cancellation acknowledgement before window destroy (Linux).
    InteractionCancelled {
        generation: u64,
        result: CancelOutcome,
    },
    WindowResized(iced::Size),
    SmokeTick,
    SmokeExitRequested,
    // Tray: polled on Windows; Linux uses the event-driven TraySignal
    // subscription, so this variant is never constructed there.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    TrayTick,
    #[cfg_attr(
        all(target_os = "linux", feature = "desktop-integration-tests"),
        allow(dead_code)
    )]
    TraySignal(TraySignal),
    /// Backend shortcut acknowledgement, activation or loss.
    HotkeyServiceEvent(HotkeyEvent),
    /// Explicit user retry of pending/failed bindings.
    RetryShortcutBinding,
    /// Feature-only test controller command (never constructed in
    /// production builds).
    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    ControllerPoll,
    /// Ordered quit pipeline entry (flush, cancel, close session).
    BeginQuit,
    /// Shutdown deadline fallback: exit and report.
    QuitForced,
    /// Open the desktop keyboard settings for manual rebinding (v1).
    ConfigureShortcutBinding,
    DesktopShutdownFinished(crate::desktop::ShutdownOutcome),
    PromptFilterChanged(crate::models::PromptFilter),
    PromptSortChanged(crate::models::PromptSort),
    NotificationDismissed(NotificationId),
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
    runtime: Arc<DesktopRuntime>,
    all_prompts: Vec<Prompt>,
    prompts: Vec<Prompt>,
    search_term: String,
    prompt_filter: PromptFilter,
    prompt_sort: PromptSort,
    selected_id: Option<PromptId>,
    status_message: String,
    notifications: NotificationStore,
    /// Current shortcut readiness warning, replaced (never stacked) on
    /// retry and resolved on valid success.
    shortcut_notice: Option<NotificationId>,
    /// Bounded structured desktop lifecycle trace (sanitized, no content).
    diagnostics: DesktopDiagnostics,
    /// Feature-only test controller identity and event queue.
    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    test_run_id: Option<String>,
    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    controller_events:
        Option<std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<TestControllerEvent>>>>,
    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    controller_clock_origin: Instant,
    form_errors: Vec<Notification>,
    settings: Settings,
    hotkey_service: Option<Arc<HotkeyService>>,
    /// Versioned desired-shortcut bookkeeping. `hotkey_ids` is derived
    /// from acknowledged Active routes only — a queued command is never
    /// shown or routed as ready.
    desired_revision: u64,
    last_desired_specs: Vec<ShortcutSpec>,
    shortcut_states: HashMap<PromptId, ShortcutAcknowledgement>,
    pending_hotkey_name: Option<String>,
    pending_native_paste_content: Option<String>,
    prompt_by_id: HashMap<PromptId, Prompt>,
    new_form: Option<NewPromptForm>,
    show_settings: bool,
    show_info: bool,
    pending_delete_id: Option<PromptId>,
    pending_import_backup: Option<PromptBackup>,
    pending_import_preview: Option<PromptImportPreview>,
    pending_import_mode: ImportMode,
    pending_duplicate_mode: DuplicateMode,
    listening_for_hotkey: bool,
    main_window_id: Option<iced::window::Id>,
    window_open_pending: bool,
    /// Authoritative lifecycle state; legacy id/pending fields mirror it.
    window_state: WindowState,
    /// Owned portal parent lease for the current window (Linux).
    #[cfg(target_os = "linux")]
    parent_lease: Option<SharedParentLease>,
    /// Process-owned AT-SPI root for the current window generation.
    #[cfg(target_os = "linux")]
    accessibility: Option<crate::desktop_accessibility::DesktopAccessibility>,
    /// Last compositor-reported focus state (observation, not a promise).
    window_focused: bool,
    /// A window destroy is waiting for the portal cancellation ack: the
    /// lease is dropped only after the worker confirms release.
    close_awaiting_cancel: bool,
    content_width: f32,
    smoke_mode: bool,
    smoke_deadline: Option<Instant>,
    // Tray fields
    #[cfg(target_os = "windows")]
    tray: Option<TrayHandle>,
    tray_state: TrayState,
    #[cfg_attr(
        all(target_os = "linux", feature = "desktop-integration-tests"),
        allow(dead_code)
    )]
    tray_subscription_instance: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImportReviewControls {
    show_import_mode_controls: bool,
    show_duplicate_summary: bool,
    show_duplicate_controls: bool,
    show_replace_warning: bool,
}

impl Default for JamePromptApp {
    fn default() -> Self {
        let runtime = Arc::new(
            DesktopRuntime::production()
                .unwrap_or_else(|error| panic!("invalid desktop runtime configuration: {error}")),
        );
        let (db, startup_warning) = perf::measure("startup.database_open", || {
            Self::open_database(&runtime.db_path())
        });
        let all_prompts =
            perf::measure("startup.prompt_load", || db.get_all("").unwrap_or_default());
        let settings = perf::measure("startup.settings_load", || {
            Settings::load(&runtime.settings_path())
        });
        let autostart_warning = perf::measure("startup.autostart_sync", || {
            sync_autostart_enabled(settings.autostart_enabled)
                .err()
                .map(|error| format!("Autostart sync failed: {}", error))
        });

        let hotkey_service = perf::measure("startup.hotkey_service_init", || {
            HotkeyService::new(Arc::clone(&runtime)).map(Arc::new)
        });
        if settings.hotkeys_enabled
            && (runtime.mode() == RuntimeMode::Production || runtime.native_consent_authorized())
        {
            crate::hotkeys::prewarm_permission(Arc::clone(&runtime));
        }

        let prompt_count = all_prompts.len();
        let prompt_by_id = Self::build_prompt_index(&all_prompts);
        let (tray_state, tray_status) = perf::measure("startup.tray_init", initialize_tray_state);
        #[cfg(target_os = "windows")]
        let tray = perf::measure("startup.tray_init", initialize_tray).0;
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
            runtime,
            all_prompts: all_prompts.clone(),
            prompts: all_prompts,
            search_term: String::new(),
            prompt_filter: PromptFilter::All,
            prompt_sort: PromptSort::NameAsc,
            selected_id: None,
            status_message,
            notifications: NotificationStore::new(),
            shortcut_notice: None,
            diagnostics: DesktopDiagnostics::new("main".to_string()),
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            test_run_id: None,
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            controller_events: None,
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            controller_clock_origin: Instant::now(),
            form_errors: Vec::new(),
            settings,
            hotkey_service,
            desired_revision: 0,
            last_desired_specs: Vec::new(),
            shortcut_states: HashMap::new(),
            pending_hotkey_name: None,
            pending_native_paste_content: None,
            prompt_by_id,
            new_form: None,
            show_settings: false,
            show_info: false,
            pending_delete_id: None,
            pending_import_backup: None,
            pending_import_preview: None,
            pending_import_mode: ImportMode::Merge,
            pending_duplicate_mode: DuplicateMode::Skip,
            listening_for_hotkey: false,
            main_window_id: None,
            window_open_pending: false,
            window_state: WindowState::absent(),
            #[cfg(target_os = "linux")]
            parent_lease: None,
            #[cfg(target_os = "linux")]
            accessibility: None,
            window_focused: false,
            close_awaiting_cancel: false,
            content_width: WINDOW_INITIAL_WIDTH,
            smoke_mode: false,
            smoke_deadline: None,
            #[cfg(target_os = "windows")]
            tray,
            tray_state,
            tray_subscription_instance: 1,
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
            runtime: test_runtime(),
            all_prompts: all_prompts.clone(),
            prompts: all_prompts,
            search_term: String::new(),
            prompt_filter: PromptFilter::All,
            prompt_sort: PromptSort::NameAsc,
            selected_id: None,
            status_message: String::new(),
            notifications: NotificationStore::new(),
            shortcut_notice: None,
            diagnostics: DesktopDiagnostics::new("main".to_string()),
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            test_run_id: None,
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            controller_events: None,
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            controller_clock_origin: Instant::now(),
            form_errors: Vec::new(),
            settings: Settings::default(),
            hotkey_service: None,
            desired_revision: 0,
            last_desired_specs: Vec::new(),
            shortcut_states: HashMap::new(),
            pending_hotkey_name: None,
            pending_native_paste_content: None,
            prompt_by_id,
            new_form: None,
            show_settings: false,
            show_info: false,
            pending_delete_id: None,
            pending_import_backup: None,
            pending_import_preview: None,
            pending_import_mode: ImportMode::Merge,
            pending_duplicate_mode: DuplicateMode::Skip,
            listening_for_hotkey: false,
            main_window_id: None,
            window_open_pending: false,
            window_state: WindowState::absent(),
            #[cfg(target_os = "linux")]
            parent_lease: None,
            #[cfg(target_os = "linux")]
            accessibility: None,
            window_focused: false,
            close_awaiting_cancel: false,
            content_width: WINDOW_INITIAL_WIDTH,
            smoke_mode: false,
            smoke_deadline: None,
            #[cfg(target_os = "windows")]
            tray: None,
            tray_state: TrayState::Starting,
            tray_subscription_instance: 1,
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
                if !self.prompt_by_id.contains_key(selected_id)
                    || !self.prompts.iter().any(|p| &p.id == selected_id)
                {
                    self.selected_id = None;
                }
            }
            self.status_message = success_message.to_string();
        });
    }

    /// Publishes a shortcut readiness warning, replacing the previous
    /// one so retries never stack duplicate banners.
    fn publish_shortcut_notice(&mut self, event: NotificationEvent) {
        if let Some(id) = self.shortcut_notice.take() {
            self.notifications.dismiss(id);
        }
        let notification = NotificationPolicy::classify(event)
            .expect("notification policy must only construct valid notifications");
        self.shortcut_notice = Some(self.notifications.publish(notification));
        if matches!(notification.presentation(), Presentation::MessageBar) {
            self.status_message.clear();
        }
    }

    fn show_notification(&mut self, event: NotificationEvent) {
        let notification = NotificationPolicy::classify(event)
            .expect("notification policy must only construct valid notifications");
        self.notifications.publish(notification);
        match notification.presentation() {
            Presentation::Status => self.status_message = self.notification_message(notification),
            Presentation::MessageBar => self.status_message.clear(),
            Presentation::InlineField | Presentation::Dialog => {}
        }
    }

    fn notification_message(&self, notification: Notification) -> String {
        match (notification.message(), notification.context()) {
            (MessageKey::PromptCopied, NotificationContext::None) => {
                "Copied to clipboard".to_string()
            }
            (MessageKey::SearchCompleted, NotificationContext::SearchResultCount(count)) => {
                format!("{count} prompts found")
            }
            (MessageKey::FieldRequired, NotificationContext::Field(Field::Name)) => {
                "Enter a prompt name".to_string()
            }
            (MessageKey::FieldRequired, NotificationContext::Field(Field::Content)) => {
                "Enter prompt content".to_string()
            }
            (MessageKey::InvalidShortcut, NotificationContext::Field(Field::Hotkey)) => {
                "Enter a valid hotkey combination".to_string()
            }
            (MessageKey::PasteRequested, NotificationContext::Operation(_)) => {
                "Automatic paste requested".to_string()
            }
            (MessageKey::PasteBusy, NotificationContext::Operation(_)) => {
                "Automatic paste is already in progress".to_string()
            }
            (MessageKey::PasteCompleted, NotificationContext::Operation(_)) => {
                "Automatic paste completed".to_string()
            }
            (MessageKey::PastePermissionDenied, NotificationContext::Operation(_)) => {
                "Automatic paste needs Remote Desktop and Clipboard permission. Grant it in your system settings, then try the shortcut again.".to_string()
            }
            (MessageKey::PasteFailed, NotificationContext::Operation(_)) => {
                "Automatic paste did not complete. Try the shortcut again.".to_string()
            }
            (MessageKey::SaveFailed, NotificationContext::Operation(_)) => {
                "Could not save your changes. Try again.".to_string()
            }
            (MessageKey::ExportFailed, NotificationContext::Operation(_)) => {
                "Could not export the prompts. Try again.".to_string()
            }
            (MessageKey::ImportFailed, NotificationContext::Operation(_)) => {
                "Could not import the backup. Check the file and try again.".to_string()
            }
            (MessageKey::DeleteFailed, NotificationContext::Operation(_)) => {
                "Could not delete the prompt. Try again.".to_string()
            }
            (MessageKey::DestructiveActionRequested, NotificationContext::None) => {
                "Confirm this action to continue.".to_string()
            }
            (MessageKey::WindowHiddenToTray, NotificationContext::None) => {
                "JamePrompt is still running in the system tray".to_string()
            }
            (MessageKey::WindowRestored, NotificationContext::None) => {
                "JamePrompt restored".to_string()
            }
            (MessageKey::WindowRestoreFailed, NotificationContext::None) => {
                "Could not restore the application window. Try again.".to_string()
            }
            (MessageKey::ShortcutAwaitingConsent, NotificationContext::None) => {
                "Waiting for shortcut approval in the system dialog".to_string()
            }
            (MessageKey::ShortcutBindingCancelled, NotificationContext::None) => {
                "Shortcut approval was cancelled. The prompt is kept; retry when ready.".to_string()
            }
            (MessageKey::ShortcutBindingUnavailable, NotificationContext::None) => {
                "Shortcut backend is unavailable. The prompt is kept; retry when ready.".to_string()
            }
            (MessageKey::ShortcutBindingFailed, NotificationContext::None) => {
                "Could not register the shortcut. Check the key and retry.".to_string()
            }
            _ => "Notification unavailable".to_string(),
        }
    }

    fn view_message_bar(&self) -> Element<'_, Message> {
        let Some(record) = self.notifications.active() else {
            return Space::new().height(0).into();
        };
        let notification = record.notification();
        if !matches!(notification.presentation(), Presentation::MessageBar) {
            return Space::new().height(0).into();
        }

        let theme = self.theme();
        let palette = theme.extended_palette();
        let color = match notification.severity() {
            Severity::Error => palette.danger.strong.color,
            Severity::Warning => palette.secondary.strong.color,
            Severity::Info | Severity::Success => palette.primary.strong.color,
        };
        let dismiss = matches!(notification.action(), Some(NotificationAction::Dismiss));
        let content = if dismiss {
            row![
                text(self.notification_message(notification)).width(Length::Fill),
                button(text("Dismiss"))
                    .on_press(Message::NotificationDismissed(record.id()))
                    .padding(CONTROL_PADDING)
                    .style(button::secondary),
            ]
            .spacing(10)
            .align_y(alignment::Alignment::Center)
        } else {
            row![text(self.notification_message(notification)).width(Length::Fill)]
        };

        container(content)
            .width(Length::Fill)
            .padding(MAIN_PANEL_PADDING)
            .style(move |theme: &Theme| container::Style {
                border: Border {
                    color,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                text_color: Some(theme.extended_palette().background.base.text),
                ..Default::default()
            })
            .into()
    }

    fn set_form_error(&mut self, event: NotificationEvent) {
        let notification = NotificationPolicy::classify(event)
            .expect("notification policy must only construct valid form errors");
        let field = match notification.context() {
            NotificationContext::Field(field) => field,
            _ => return,
        };
        self.form_errors
            .retain(|existing| existing.context() != NotificationContext::Field(field));
        self.form_errors.push(notification);
    }

    fn clear_form_error(&mut self, field: Field) {
        self.form_errors
            .retain(|error| error.context() != NotificationContext::Field(field));
    }

    fn form_error_message(&self, field: Field) -> Option<String> {
        self.form_errors
            .iter()
            .find(|error| error.context() == NotificationContext::Field(field))
            .copied()
            .map(|error| self.notification_message(error))
    }

    fn view_form_error(&self, field: Field) -> Element<'_, Message> {
        if let Some(message) = self.form_error_message(field) {
            text(message)
                .size(MODAL_LABEL_TEXT_SIZE)
                .color(self.theme().extended_palette().danger.strong.color)
                .into()
        } else {
            Space::new().height(0).into()
        }
    }

    /// Builds the effective sorted desired snapshot: globally enabled,
    /// per-prompt enabled, nonempty and parseable triggers only.
    fn desired_shortcut_specs(&self) -> Vec<ShortcutSpec> {
        if !self.settings.hotkeys_enabled {
            return Vec::new();
        }
        let mut specs: Vec<ShortcutSpec> = self
            .all_prompts
            .iter()
            .filter(|prompt| prompt.hotkey_enabled)
            .filter_map(|prompt| {
                prompt.hotkey.as_ref().and_then(|trigger| {
                    if trigger.trim().is_empty() || !crate::hotkeys::validate_hotkey(trigger) {
                        return None;
                    }
                    Some(ShortcutSpec {
                        prompt_id: prompt.id.clone(),
                        description: prompt.name.clone(),
                        preferred_trigger: trigger.clone(),
                    })
                })
            })
            .collect();
        specs.sort_by(|a, b| a.prompt_id.cmp(&b.prompt_id));
        specs
    }

    /// Single synchronization owner for every committed mutation. Builds
    /// the effective snapshot, bumps the revision only on effective
    /// change, marks affected prompts Pending, and queues exactly one
    /// reconcile. Content-only edits are no-ops: no new session, no new
    /// consent dialog. Must be called only after the database commit
    /// succeeded — never rolls back a committed prompt.
    fn reconcile_shortcuts_after_commit(&mut self) {
        let specs = self.desired_shortcut_specs();
        let last = (self.desired_revision > 0)
            .then(|| ShortcutSnapshot::new(self.desired_revision, self.last_desired_specs.clone()));
        let (snapshot, _) = crate::desktop::next_snapshot_revision(
            last.as_ref(),
            &specs,
            self.desired_revision.saturating_add(1),
        );
        if last.as_ref().is_some_and(|previous| {
            previous.revision == snapshot.revision && previous.same_effective_set(&snapshot)
        }) {
            return;
        }
        self.desired_revision = snapshot.revision;
        // Prompts that left the desired set lose their routes with the
        // serial replacement; drop their stale states now.
        let desired_ids: std::collections::HashSet<&str> =
            specs.iter().map(|spec| spec.prompt_id.as_str()).collect();
        self.shortcut_states
            .retain(|prompt_id, _| desired_ids.contains(prompt_id.as_str()));
        for spec in &specs {
            self.shortcut_states.insert(
                spec.prompt_id.clone(),
                ShortcutAcknowledgement {
                    revision: self.desired_revision,
                    prompt_id: spec.prompt_id.clone(),
                    runtime_id: None,
                    status: ShortcutStatus::Pending,
                    trigger_description: None,
                    error_code: None,
                },
            );
        }
        self.last_desired_specs = snapshot.shortcuts.clone();
        self.diagnostics.record(
            "save_committed",
            Some(self.desired_revision),
            Some(self.window_state.generation),
            Some(format!("desired={}", self.last_desired_specs.len())),
        );
        let Some(ref svc) = self.hotkey_service else {
            return;
        };
        match svc.reconcile(&snapshot) {
            Ok(receipt) => {
                for prompt_id in receipt.rejected_prompt_ids {
                    self.shortcut_states.insert(
                        prompt_id.clone(),
                        ShortcutAcknowledgement {
                            revision: self.desired_revision,
                            prompt_id,
                            runtime_id: None,
                            status: ShortcutStatus::Failed,
                            trigger_description: None,
                            error_code: None,
                        },
                    );
                }
            }
            Err(error) => {
                for spec in &self.last_desired_specs.clone() {
                    self.shortcut_states.insert(
                        spec.prompt_id.clone(),
                        ShortcutAcknowledgement {
                            revision: self.desired_revision,
                            prompt_id: spec.prompt_id.clone(),
                            runtime_id: None,
                            status: ShortcutStatus::Failed,
                            trigger_description: None,
                            error_code: Some(error),
                        },
                    );
                }
            }
        }
    }

    /// Maps an acknowledged runtime id back to its stable prompt identity.
    /// Only Active routes are routable; anything else is stale.
    fn runtime_to_prompt(&self, runtime_id: u32) -> Option<PromptId> {
        self.shortcut_states
            .iter()
            .find(|(_, ack)| {
                ack.runtime_id == Some(runtime_id) && ack.status == ShortcutStatus::Active
            })
            .map(|(prompt_id, _)| prompt_id.clone())
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
        name.contains(needle) || content.contains(needle)
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
            Err(_) => {
                self.show_notification(NotificationEvent::SaveFailed);
            }
        }
    }

    fn reload_prompt_cache_from_db(&mut self) -> Result<(), String> {
        let prompts = self
            .db
            .get_all("")
            .map_err(|error| format!("Database error: {}", error))?;
        self.all_prompts = prompts.clone();
        self.prompt_by_id = Self::build_prompt_index(&prompts);
        self.prompts = self.visible_prompts_for_query(&self.current_query());
        if let Some(selected_id) = &self.selected_id {
            if !self.prompt_by_id.contains_key(selected_id) {
                self.selected_id = None;
            }
        }
        Ok(())
    }

    /// Applies one backend acknowledgement to the readiness map.
    /// Accepts only the current desired revision for a still-desired
    /// prompt; stale or unknown acks are rejected without mutation so a
    /// late consent can never resurrect a deleted binding.
    fn apply_shortcut_acknowledgement(&mut self, ack: ShortcutAcknowledgement) -> bool {
        let desired_ids: std::collections::HashSet<String> = self
            .desired_shortcut_specs()
            .into_iter()
            .map(|spec| spec.prompt_id)
            .collect();
        crate::desktop::apply_acknowledgement(
            &mut self.shortcut_states,
            &desired_ids,
            self.desired_revision,
            ack,
        )
    }

    fn prompt_list_empty_message(&self) -> &'static str {
        if self.all_prompts.is_empty() {
            "Create or import a prompt to get started"
        } else {
            "No prompts match your filters"
        }
    }

    fn prompt_detail_empty_message(&self) -> &'static str {
        if self.all_prompts.is_empty() {
            "Create or import a prompt to get started"
        } else {
            "Select a prompt from the list"
        }
    }

    fn import_review_controls(&self, preview: &PromptImportPreview) -> ImportReviewControls {
        let has_existing_prompts = !self.all_prompts.is_empty();
        let has_duplicates = !preview.duplicate_names.is_empty();
        let merge_selected = matches!(self.pending_import_mode, ImportMode::Merge);
        let replace_selected = matches!(self.pending_import_mode, ImportMode::ReplaceAll);

        ImportReviewControls {
            show_import_mode_controls: has_existing_prompts,
            show_duplicate_summary: has_duplicates,
            show_duplicate_controls: has_existing_prompts && has_duplicates && merge_selected,
            show_replace_warning: has_existing_prompts && replace_selected,
        }
    }

    fn apply_pending_import(&mut self) -> Result<ImportSummary, String> {
        let backup = self
            .pending_import_backup
            .clone()
            .ok_or_else(|| "No import is waiting for confirmation".to_string())?;
        let operations = plan_import_operations(
            &backup,
            &self.all_prompts,
            self.pending_import_mode,
            self.pending_duplicate_mode,
        )
        .map_err(|error| error.to_string())?;

        let summary = if matches!(self.pending_import_mode, ImportMode::ReplaceAll) {
            let imported_ids = self
                .db
                .replace_all_imported(&operations.to_insert)
                .map_err(|error| format!("Import failed: {}", error))?;
            self.selected_id = imported_ids.first().cloned();
            ImportSummary {
                inserted: imported_ids.len(),
                overwritten: 0,
                skipped: 0,
            }
        } else {
            let overwrites: Vec<(String, Prompt)> = operations
                .to_overwrite
                .iter()
                .map(|overwrite| (overwrite.existing_id.clone(), overwrite.prompt.clone()))
                .collect();
            self.db
                .merge_imported(&operations.to_insert, &overwrites)
                .map_err(|error| format!("Import failed: {}", error))?;
            ImportSummary {
                inserted: operations.to_insert.len(),
                overwritten: operations.to_overwrite.len(),
                skipped: operations.skipped_names.len(),
            }
        };

        self.reload_prompt_cache_from_db()?;
        // One snapshot after the successful import transaction — never
        // one consent dialog per imported row.
        self.reconcile_shortcuts_after_commit();
        self.pending_import_backup = None;
        self.pending_import_preview = None;
        Ok(summary)
    }
}

fn normalized_json_path(mut path: PathBuf) -> PathBuf {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PROMPT_BACKUP_FILE_EXTENSION))
    {
        return path;
    }
    path.set_extension(PROMPT_BACKUP_FILE_EXTENSION);
    path
}

async fn export_prompts_to_json_file(prompts: Vec<Prompt>) -> Result<PathBuf, String> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .add_filter("JamePrompt backup", &[PROMPT_BACKUP_FILE_EXTENSION])
        .set_file_name(PROMPT_BACKUP_DEFAULT_FILE_NAME)
        .save_file()
        .await
    else {
        return Err("Export cancelled".to_string());
    };

    let path = normalized_json_path(handle.path().to_path_buf());
    let backup = PromptBackup::new(APP_VERSION, prompts);
    let json = backup.to_json().map_err(|error| error.to_string())?;
    std::fs::write(&path, json).map_err(|error| format!("Export failed: {}", error))?;
    Ok(path)
}

async fn import_prompts_from_json_file() -> Result<PromptBackup, String> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .add_filter("JamePrompt backup", &[PROMPT_BACKUP_FILE_EXTENSION])
        .pick_file()
        .await
    else {
        return Err("Import cancelled".to_string());
    };

    let path = handle.path().to_path_buf();
    let json =
        std::fs::read_to_string(&path).map_err(|error| format!("Import failed: {}", error))?;
    PromptBackup::from_json(&json).map_err(|error| error.to_string())
}

#[cfg(all(not(test), target_os = "windows"))]
fn initialize_tray() -> (Option<TrayHandle>, Option<String>) {
    match TrayHandle::new() {
        Ok(tray) => (Some(tray), None),
        Err(error) => (None, Some(format!("Tray unavailable: {error}"))),
    }
}

#[cfg(all(test, target_os = "windows"))]
fn initialize_tray() -> (Option<TrayHandle>, Option<String>) {
    (None, None)
}

/// Initial Linux tray state. The ksni service reports Ready/Unavailable
/// asynchronously through the subscription; `Starting` is explicitly not
/// ready, so hidden startup waits for the signal instead of assuming a
/// tray exists.
#[cfg(target_os = "linux")]
fn initialize_tray_state() -> (TrayState, Option<String>) {
    (TrayState::Starting, None)
}

/// Windows keeps the synchronous tray assumption for hidden-start fallback.
#[cfg(not(target_os = "linux"))]
fn initialize_tray_state() -> (TrayState, Option<String>) {
    (TrayState::Starting, None)
}

/// Converts pressed keyboard events into hotkey capture messages.
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

/// User-facing label for a shortcut readiness state. The requested key
/// and the backend-assigned trigger are shown separately wherever both
/// are known; this label never rewrites one as the other.
fn status_label(status: ShortcutStatus) -> &'static str {
    match status {
        ShortcutStatus::Pending => "pending",
        ShortcutStatus::AwaitingWindow => "waiting for window",
        ShortcutStatus::AwaitingConsent => "waiting for approval",
        ShortcutStatus::Active => "active",
        ShortcutStatus::Inactive => "inactive",
        ShortcutStatus::Cancelled => "cancelled",
        ShortcutStatus::Unavailable => "unavailable",
        ShortcutStatus::Failed => "failed",
    }
}

fn clipboard_matches_expected_prompt(expected: &str, actual: Option<&str>) -> bool {
    actual == Some(expected)
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

const NATIVE_HOTKEY_SMOKE_ID: &str = "11111111-1111-4111-8111-111111111111";
const NATIVE_HOTKEY_SMOKE_NAME: &str = "Native hotkey smoke";
const NATIVE_HOTKEY_SMOKE_CONTENT: &str = "JamePrompt native smoke ñ 123";
const NATIVE_HOTKEY_SMOKE_HOTKEY: &str = "Ctrl+Shift+P";

fn memory_runtime() -> Arc<DesktopRuntime> {
    memory_runtime_with_consent(false)
}

fn memory_runtime_with_consent(native_consent_authorized: bool) -> Arc<DesktopRuntime> {
    Arc::new(
        DesktopRuntime::from_parts(
            crate::desktop::DesktopConfig::default(),
            "io.github.roymejia2217.JamePrompt.Test",
            std::env::temp_dir(),
            None,
            RuntimeMode::Synthetic,
            native_consent_authorized,
        )
        .expect("memory runtime must validate"),
    )
}

#[cfg(test)]
fn test_runtime() -> Arc<DesktopRuntime> {
    memory_runtime()
}

#[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
fn static_controller_event_sequence() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    SEQUENCE.fetch_add(1, Ordering::Relaxed)
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
            runtime: memory_runtime(),
            all_prompts: all_prompts.clone(),
            prompts: all_prompts,
            search_term: String::new(),
            prompt_filter: PromptFilter::All,
            prompt_sort: PromptSort::NameAsc,
            selected_id: None,
            status_message: "Smoke mode ready".into(),
            notifications: NotificationStore::new(),
            shortcut_notice: None,
            diagnostics: DesktopDiagnostics::new("main".to_string()),
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            test_run_id: None,
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            controller_events: None,
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            controller_clock_origin: Instant::now(),
            form_errors: Vec::new(),
            settings: Settings::default(),
            hotkey_service: None,
            desired_revision: 0,
            last_desired_specs: Vec::new(),
            shortcut_states: HashMap::new(),
            pending_hotkey_name: None,
            pending_native_paste_content: None,
            prompt_by_id,
            new_form: None,
            show_settings: false,
            show_info: false,
            pending_delete_id: None,
            pending_import_backup: None,
            pending_import_preview: None,
            pending_import_mode: ImportMode::Merge,
            pending_duplicate_mode: DuplicateMode::Skip,
            listening_for_hotkey: false,
            main_window_id: None,
            window_open_pending: false,
            window_state: WindowState::absent(),
            #[cfg(target_os = "linux")]
            parent_lease: None,
            #[cfg(target_os = "linux")]
            accessibility: None,
            window_focused: false,
            close_awaiting_cancel: false,
            content_width: WINDOW_INITIAL_WIDTH,
            smoke_mode: true,
            smoke_deadline: None,
            #[cfg(target_os = "windows")]
            tray: None,
            tray_state: TrayState::Starting,
            tray_subscription_instance: 1,
        }
    }

    fn new_native_hotkey_smoke_mode() -> Self {
        assert_eq!(
            crate::platform::hotkey_backend_kind(),
            crate::platform::HotkeyBackendKind::Native,
            "native hotkey smoke requires the native X11 or Windows backend"
        );

        let db = Database::in_memory()
            .expect("In-memory native hotkey smoke database initialization failed");
        let prompt = Prompt {
            id: NATIVE_HOTKEY_SMOKE_ID.to_string(),
            name: NATIVE_HOTKEY_SMOKE_NAME.to_string(),
            content: NATIVE_HOTKEY_SMOKE_CONTENT.to_string(),
            hotkey: Some(NATIVE_HOTKEY_SMOKE_HOTKEY.to_string()),
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        };
        let all_prompts = vec![prompt.clone()];
        let prompt_by_id = Self::build_prompt_index(&all_prompts);
        let runtime = memory_runtime_with_consent(true);
        let hotkey_service = Arc::new(
            HotkeyService::new(Arc::clone(&runtime))
                .expect("native hotkey smoke backend should initialize"),
        );
        let smoke_specs = vec![crate::desktop::ShortcutSpec {
            prompt_id: prompt.id.clone(),
            description: prompt.name.clone(),
            preferred_trigger: NATIVE_HOTKEY_SMOKE_HOTKEY.to_string(),
        }];
        hotkey_service
            .reconcile(&crate::desktop::ShortcutSnapshot::new(
                1,
                smoke_specs.clone(),
            ))
            .expect("native hotkey smoke shortcut should reconcile");

        Self {
            db,
            runtime,
            all_prompts: all_prompts.clone(),
            prompts: all_prompts,
            search_term: String::new(),
            prompt_filter: PromptFilter::All,
            prompt_sort: PromptSort::NameAsc,
            selected_id: Some(prompt.id),
            status_message: "Native hotkey smoke ready".into(),
            notifications: NotificationStore::new(),
            shortcut_notice: None,
            diagnostics: DesktopDiagnostics::new("main".to_string()),
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            test_run_id: None,
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            controller_events: None,
            #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
            controller_clock_origin: Instant::now(),
            form_errors: Vec::new(),
            settings: Settings::default(),
            hotkey_service: Some(hotkey_service),
            pending_hotkey_name: None,
            pending_native_paste_content: None,
            prompt_by_id,
            new_form: None,
            show_settings: false,
            show_info: false,
            pending_delete_id: None,
            pending_import_backup: None,
            pending_import_preview: None,
            pending_import_mode: ImportMode::Merge,
            pending_duplicate_mode: DuplicateMode::Skip,
            listening_for_hotkey: false,
            main_window_id: None,
            window_open_pending: false,
            window_state: WindowState::absent(),
            #[cfg(target_os = "linux")]
            parent_lease: None,
            #[cfg(target_os = "linux")]
            accessibility: None,
            window_focused: false,
            close_awaiting_cancel: false,
            content_width: WINDOW_INITIAL_WIDTH,
            smoke_mode: true,
            smoke_deadline: None,
            #[cfg(target_os = "windows")]
            tray: None,
            tray_state: TrayState::Starting,
            tray_subscription_instance: 1,
            desired_revision: 1,
            last_desired_specs: smoke_specs,
            shortcut_states: HashMap::new(),
        }
    }

    /// Queues the startup desired snapshot once (Pending, never Active)
    /// and subscribes to backend state even with zero active ids.
    /// Never blocks GUI startup on backend initialization.
    fn queue_startup_shortcuts(&mut self) {
        self.reconcile_shortcuts_after_commit();
    }

    fn with_startup_tasks(mut app: Self, start_minimized: bool) -> (Self, Task<Message>) {
        let smoke_exit_task = if app.smoke_mode {
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
        app.smoke_deadline = if app.smoke_mode {
            Some(Instant::now() + smoke_duration_from_env())
        } else {
            None
        };
        // Windows keeps the synchronous tray assumption. Other platforms
        // receive tray readiness asynchronously via TraySignal: a hidden
        // start waits for Ready or the bounded initial failure, then the
        // signal handler opens the fallback window if still windowless.
        #[cfg(target_os = "windows")]
        let tray_available = app.tray.is_some();
        #[cfg(not(target_os = "windows"))]
        let tray_available = true;
        let fallback_task =
            if should_show_window_after_hidden_start(start_minimized, tray_available) {
                Task::done(Message::ShowWindow(None))
            } else {
                Task::none()
            };
        // Defer user-interactive portal binds until the window (and its
        // parent export) is ready; the snapshot is queued once and the
        // backend state is subscribed even with zero active ids.
        app.queue_startup_shortcuts();

        (app, Task::batch([fallback_task, smoke_exit_task]))
    }

    pub fn new_with_hidden_start(start_minimized: bool, smoke_mode: bool) -> (Self, Task<Message>) {
        let app = if smoke_mode {
            Self::new_smoke_mode()
        } else {
            Self::default()
        };

        Self::with_startup_tasks(app, start_minimized)
    }

    pub fn new_native_hotkey_smoke(start_minimized: bool) -> (Self, Task<Message>) {
        Self::with_startup_tasks(Self::new_native_hotkey_smoke_mode(), start_minimized)
    }

    /// Feature-only test constructor: synthetic database in the private
    /// profile dir (never production paths), default settings, no
    /// autostart sync, no permission prewarm — but the real platform
    /// hotkey and tray backends.
    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    pub fn new_for_desktop_test(
        runtime: Arc<DesktopRuntime>,
        run_id: String,
        controller_events: std::sync::Arc<
            std::sync::Mutex<std::collections::VecDeque<TestControllerEvent>>,
        >,
    ) -> (Self, Task<Message>) {
        let db_path = runtime.db_path();
        Self::with_startup_tasks(
            Self::new_for_desktop_test_inner(&db_path, runtime, run_id, controller_events),
            false,
        )
    }

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    fn new_for_desktop_test_inner(
        db_path: &std::path::Path,
        runtime: Arc<DesktopRuntime>,
        run_id: String,
        controller_events: std::sync::Arc<
            std::sync::Mutex<std::collections::VecDeque<TestControllerEvent>>,
        >,
    ) -> Self {
        let (db, warning) = Self::open_database(db_path);
        let all_prompts = db.get_all("").unwrap_or_default();
        let prompt_by_id = Self::build_prompt_index(&all_prompts);
        let hotkey_service = HotkeyService::new(Arc::clone(&runtime)).map(Arc::new);
        Self {
            db,
            runtime,
            all_prompts: all_prompts.clone(),
            prompts: all_prompts,
            search_term: String::new(),
            prompt_filter: PromptFilter::All,
            prompt_sort: PromptSort::NameAsc,
            selected_id: None,
            status_message: warning.unwrap_or_default(),
            notifications: NotificationStore::new(),
            shortcut_notice: None,
            form_errors: Vec::new(),
            diagnostics: DesktopDiagnostics::new(run_id.clone()),
            test_run_id: Some(run_id),
            controller_events: Some(controller_events),
            controller_clock_origin: Instant::now(),
            settings: Settings::default(),
            hotkey_service,
            desired_revision: 0,
            last_desired_specs: Vec::new(),
            shortcut_states: HashMap::new(),
            pending_hotkey_name: None,
            pending_native_paste_content: None,
            prompt_by_id,
            new_form: None,
            show_settings: false,
            show_info: false,
            pending_delete_id: None,
            pending_import_backup: None,
            pending_import_preview: None,
            pending_import_mode: ImportMode::Merge,
            pending_duplicate_mode: DuplicateMode::Skip,
            listening_for_hotkey: false,
            main_window_id: None,
            window_open_pending: false,
            window_state: WindowState::absent(),
            parent_lease: None,
            accessibility: None,
            window_focused: false,
            close_awaiting_cancel: false,
            content_width: WINDOW_INITIAL_WIDTH,
            smoke_mode: false,
            smoke_deadline: None,
            tray_state: TrayState::Starting,
            tray_subscription_instance: 1,
        }
    }

    pub fn title(&self) -> String {
        APP_NAME.to_string()
    }

    /// Canonical user show intent: Absent opens one window for a new
    /// generation; repeated shows coalesce; show during Closing latches a
    /// single reopen on Closed.
    pub(crate) fn request_show(&mut self) -> Task<Message> {
        self.diagnostics.record(
            "show_requested",
            Some(self.desired_revision),
            Some(self.window_state.generation),
            None,
        );
        let effects =
            crate::window_lifecycle::transition(&mut self.window_state, WindowIntent::Show);
        self.mirror_window_state();
        self.apply_window_effects(effects)
    }

    /// Canonical close intent for a window id. Marks Closing and submits
    /// the surface destroy; Absent is published only on the Closed event.
    pub(crate) fn request_close(&mut self, id: iced::window::Id) -> Task<Message> {
        // Portal close handshake (Linux, production): request
        // cancellation and await the worker acknowledgement before
        // dropping the lease and destroying the window. The ack resolves
        // immediately when nothing is pending, so this never stalls the
        // common path.
        #[cfg(target_os = "linux")]
        let defer_for_cancel = !self.smoke_mode
            && self.hotkey_service.is_some()
            && self.window_state.id == Some(id)
            && self.window_state.phase == WindowPhase::Open;
        #[cfg(not(target_os = "linux"))]
        let defer_for_cancel = false;
        let effects =
            crate::window_lifecycle::transition(&mut self.window_state, WindowIntent::Close(id));
        self.diagnostics.record(
            "close_requested",
            Some(self.desired_revision),
            Some(self.window_state.generation),
            None,
        );
        self.mirror_window_state();
        if defer_for_cancel {
            #[cfg(target_os = "linux")]
            if let Some(ref svc) = self.hotkey_service {
                let generation = self.window_state.generation;
                self.close_awaiting_cancel = true;
                let pending = svc.cancel_interaction(generation);
                return Task::perform(async move { pending.await }, move |result| {
                    Message::InteractionCancelled {
                        generation,
                        result: result.unwrap_or(CancelOutcome::NoPendingRequest),
                    }
                });
            }
        }
        self.apply_window_effects(effects)
    }

    /// Canonical present intent for a known window id: presents when it is
    /// the current open window, otherwise falls back to a fresh show.
    pub(crate) fn present_window(&mut self, id: iced::window::Id) -> Task<Message> {
        if self.window_state.id == Some(id)
            && self.window_state.phase == crate::window_lifecycle::WindowPhase::Open
        {
            self.show_notification(NotificationEvent::WindowRestored);
            return crate::window_lifecycle::restore_for_platform(id);
        }
        self.request_show()
    }

    /// Authoritative open completion for a generation. Assigns the id once,
    /// disposes stale generations safely, and exports the Linux portal
    /// parent for the new window.
    fn note_window_opened(&mut self, generation: u64, id: iced::window::Id) -> Task<Message> {
        #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
        self.record_controller_event(0, "window_opened_dispatched");
        self.diagnostics.record(
            "window_opened",
            Some(self.desired_revision),
            Some(generation),
            None,
        );
        let effects = crate::window_lifecycle::transition(
            &mut self.window_state,
            WindowIntent::Opened { generation, id },
        );
        self.mirror_window_state();
        #[cfg(target_os = "linux")]
        if self.window_state.id == Some(id) && self.accessibility.is_none() {
            self.accessibility = Some(crate::desktop_accessibility::DesktopAccessibility::new());
        }
        let mut task = self.apply_window_effects(effects);
        #[cfg(target_os = "linux")]
        if self.window_state.id == Some(id) && self.parent_lease_for(id, generation).is_none() {
            task = Task::batch([
                task,
                crate::desktop_parent::export_parent(id, generation)
                    .map(move |result| Message::ParentExported { generation, result }),
            ]);
        }
        task
    }

    /// Authoritative destruction event. Publishes Absent, or the single
    /// latched reopen when a show arrived during Closing.
    fn note_window_closed(&mut self, id: iced::window::Id) -> Task<Message> {
        #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
        self.record_controller_event(0, "window_closed_dispatched");
        self.diagnostics.record(
            "window_closed",
            Some(self.desired_revision),
            Some(self.window_state.generation),
            None,
        );
        let effects =
            crate::window_lifecycle::transition(&mut self.window_state, WindowIntent::Closed(id));
        self.close_awaiting_cancel = false;
        self.mirror_window_state();
        #[cfg(target_os = "linux")]
        if self.window_state.id.is_none() {
            self.parent_lease = None;
            self.accessibility = None;
        }
        self.apply_window_effects(effects)
    }

    /// Supplemental compositor observation. Opened is authoritative only
    /// through the open-task completion; a matching event for the current
    /// id is observed without duplicating export or effects. Closed is
    /// authoritative destruction. Focus is recorded as observed policy,
    /// never as a restore success claim.
    fn handle_desktop_window_event(
        &mut self,
        id: iced::window::Id,
        event: iced::window::Event,
    ) -> Task<Message> {
        match event {
            iced::window::Event::Closed => self.note_window_closed(id),
            iced::window::Event::Focused => {
                self.window_focused = true;
                Task::none()
            }
            iced::window::Event::Unfocused => {
                self.window_focused = false;
                Task::none()
            }
            iced::window::Event::Opened { .. } => Task::none(),
            _ => Task::none(),
        }
    }

    fn apply_window_effects(&self, effects: Vec<WindowEffect>) -> Task<Message> {
        let mut tasks = Vec::with_capacity(effects.len());
        for effect in effects {
            match effect {
                WindowEffect::OpenWindow { generation } => {
                    tasks.push(
                        crate::window_lifecycle::open()
                            .map(move |id| Message::WindowOpened { generation, id }),
                    );
                }
                WindowEffect::Present { id } => {
                    tasks.push(crate::window_lifecycle::restore_for_platform(id));
                }
                WindowEffect::CloseWindow { id } | WindowEffect::DropStaleOpened { id } => {
                    tasks.push(crate::window_lifecycle::close(id));
                }
                WindowEffect::None => {}
            }
        }
        Task::batch(tasks)
    }

    /// Mirrors the authoritative lifecycle state into the legacy id/pending
    /// fields consumed by addressing and fallback decisions.
    fn mirror_window_state(&mut self) {
        self.main_window_id = self.window_state.id;
        self.window_open_pending = self.window_state.id.is_none()
            && self.window_state.phase == crate::window_lifecycle::WindowPhase::Opening;
    }

    #[cfg(target_os = "linux")]
    fn parent_lease_for(&self, id: iced::window::Id, generation: u64) -> Option<SharedParentLease> {
        self.parent_lease
            .clone()
            .filter(|lease| lease.is_current_for(generation) && self.window_state.id == Some(id))
    }

    /// Executes one controller operation through the production message
    /// handlers on the iced main thread — never direct database writes,
    /// never a parallel save path. The reply reports the production
    /// outcome (database row identity, shortcut status snapshot); a
    /// controller acknowledgement is NOT portal consent acceptance.
    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    fn record_controller_event(&self, operation_id: u64, event: &'static str) {
        let Some(queue) = self.controller_events.as_ref() else {
            return;
        };
        let sequence = static_controller_event_sequence();
        let event = TestControllerEvent {
            version: crate::desktop_test_support::PROTOCOL_VERSION,
            run_id: self.test_run_id.clone().unwrap_or_default(),
            operation_id,
            sequence,
            monotonic_ns: self
                .controller_clock_origin
                .elapsed()
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64,
            revision: Some(self.desired_revision),
            window_generation: Some(self.window_state.generation),
            event: event.to_string(),
        };
        if let Ok(mut events) = queue.lock() {
            if events.len() >= crate::desktop_test_support::MAX_PENDING_COMMANDS {
                events.pop_front();
            }
            events.push_back(event);
        }
    }

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    fn apply_controller_command(
        &mut self,
        command: crate::desktop_test_support::ControllerCommand,
    ) -> Task<Message> {
        use crate::desktop_test_support::{ControllerError, ControllerOperation};

        let request_id = command.request_id;
        let reply = command.reply;
        self.record_controller_event(request_id, "controller_operation_dispatched");
        let respond =
            move |ok: bool, result: Option<serde_json::Value>, error: Option<ControllerError>| {
                if let Ok(mut slot) = reply.lock() {
                    if let Some(sender) = slot.take() {
                        let _ = sender.send(crate::desktop_test_support::ControllerResponse {
                            version: crate::desktop_test_support::PROTOCOL_VERSION,
                            request_id,
                            ok,
                            result,
                            error,
                        });
                    }
                }
            };
        let failed = |message: String| {
            respond(
                false,
                None,
                Some(ControllerError {
                    code: "operation_failed".to_string(),
                    message,
                }),
            );
            Task::none()
        };
        // Sequential translation: each production message runs through the
        // same update path as real input, forwarding produced Tasks into
        // the same dispatcher.
        match command.operation {
            ControllerOperation::Hello {} => {
                respond(
                    true,
                    Some(serde_json::json!({
                        "protocol_version": crate::desktop_test_support::PROTOCOL_VERSION,
                        "run_id": self.test_run_id,
                        "pid": std::process::id(),
                        "revision": self.desired_revision,
                        "window_generation": self.window_state.generation,
                    })),
                    None,
                );
                Task::none()
            }
            ControllerOperation::State {} => {
                let shortcuts: Vec<serde_json::Value> = self
                    .shortcut_states
                    .iter()
                    .map(|(prompt_id, ack)| {
                        serde_json::json!({
                            "prompt_id": prompt_id,
                            "revision": ack.revision,
                            "status": format!("{:?}", ack.status),
                            "runtime_id": ack.runtime_id,
                            "trigger_description": ack.trigger_description,
                        })
                    })
                    .collect();
                respond(
                    true,
                    Some(serde_json::json!({
                        "window_id_assigned": self.main_window_id.is_some(),
                        "window_generation": self.window_state.generation,
                        "desired_revision": self.desired_revision,
                        "shortcuts": shortcuts,
                    })),
                    None,
                );
                Task::none()
            }
            ControllerOperation::CreatePrompt {
                name,
                content,
                hotkey,
                hotkey_enabled,
            } => {
                let mut tasks = vec![self.update(Message::NewPressed)];
                tasks.push(self.update(Message::FormNameChanged(name)));
                tasks.push(
                    self.update(Message::FormContentEdited(text_editor::Action::Edit(
                        text_editor::Edit::Paste(Arc::new(content)),
                    ))),
                );
                if let Some(hotkey) = hotkey {
                    if !hotkey.trim().is_empty() {
                        tasks.push(self.update(Message::FormHotkeyCaptured(hotkey)));
                    }
                }
                if !hotkey_enabled.unwrap_or(true) {
                    tasks.push(self.update(Message::FormHotkeyEnabledToggled(false)));
                }
                tasks.push(self.update(Message::FormSave));
                if !self.form_errors.is_empty() || self.new_form.is_some() {
                    return failed("prompt form validation failed".to_string());
                }
                let created = self.selected_id.clone();
                match created {
                    Some(id) => {
                        respond(
                            true,
                            Some(serde_json::json!({
                                "prompt_id": id,
                                "revision": self.desired_revision,
                            })),
                            None,
                        );
                    }
                    None => {
                        return failed("form save did not create a prompt".to_string());
                    }
                }
                Task::batch(tasks)
            }
            ControllerOperation::EditPrompt {
                id,
                name,
                content,
                hotkey,
                hotkey_enabled,
            } => {
                let mut tasks = vec![self.update(Message::EditPressed(id.clone()))];
                if let Some(name) = name {
                    tasks.push(self.update(Message::FormNameChanged(name)));
                }
                if let Some(content) = content {
                    tasks.push(
                        self.update(Message::FormContentEdited(text_editor::Action::SelectAll)),
                    );
                    tasks.push(
                        self.update(Message::FormContentEdited(text_editor::Action::Edit(
                            text_editor::Edit::Paste(Arc::new(content)),
                        ))),
                    );
                }
                if let Some(hotkey) = hotkey {
                    if hotkey.trim().is_empty() {
                        tasks.push(self.update(Message::FormHotkeyClearPressed));
                    } else {
                        tasks.push(self.update(Message::FormHotkeyCaptured(hotkey)));
                    }
                }
                if let Some(enabled) = hotkey_enabled {
                    tasks.push(self.update(Message::FormHotkeyEnabledToggled(enabled)));
                }
                tasks.push(self.update(Message::FormSave));
                if self.form_errors.is_empty()
                    && self.new_form.is_none()
                    && self.prompt_cloned_by_id(&id).is_some()
                {
                    respond(
                        true,
                        Some(serde_json::json!({
                            "prompt_id": id,
                            "revision": self.desired_revision,
                        })),
                        None,
                    );
                } else {
                    return failed(format!("prompt {id} missing after edit"));
                }
                Task::batch(tasks)
            }
            ControllerOperation::DeletePrompt { id } => {
                if self.prompt_cloned_by_id(&id).is_none() {
                    return failed(format!("no such prompt {id}"));
                }
                let mut tasks = vec![self.update(Message::DeletePressed(id.clone()))];
                tasks.push(self.update(Message::DeleteConfirmPressed(id.clone())));
                if self.prompt_cloned_by_id(&id).is_none() {
                    respond(
                        true,
                        Some(serde_json::json!({
                            "prompt_id": id,
                            "revision": self.desired_revision,
                        })),
                        None,
                    );
                } else {
                    return failed(format!("prompt {id} still present after delete"));
                }
                Task::batch(tasks)
            }
            ControllerOperation::ToggleGlobalHotkeys { enabled } => {
                let task = self.update(Message::SettingsHotkeyToggled(enabled));
                respond(
                    true,
                    Some(serde_json::json!({ "hotkeys_enabled": enabled })),
                    None,
                );
                task
            }
            ControllerOperation::ImportFixture { backup, mode } => {
                let backup_json = backup.to_string();
                match crate::prompt_backup::PromptBackup::from_json(&backup_json) {
                    Ok(validated) => {
                        match mode.as_deref() {
                            None | Some("merge") | Some("replace") => {}
                            Some(other) => return failed(format!("unknown import mode: {other}")),
                        }
                        self.pending_import_backup = Some(validated);
                        self.pending_import_mode = match mode.as_deref() {
                            Some("replace") => ImportMode::ReplaceAll,
                            _ => ImportMode::Merge,
                        };
                        let task = self.update(Message::ImportConfirmPressed);
                        respond(
                            true,
                            Some(serde_json::json!({
                                "revision": self.desired_revision,
                            })),
                            None,
                        );
                        task
                    }
                    Err(error) => failed(format!("import fixture invalid: {error}")),
                }
            }
            ControllerOperation::OpenExportDialog {} => {
                // The native chooser opens through the existing rfd path;
                // the test then drives it via AT-SPI. The terminal result
                // arrives through SettingsExportFinished, observed through
                // events — this reply only confirms dispatch.
                let task = self.update(Message::SettingsExportPressed);
                respond(true, Some(serde_json::json!({ "dispatched": true })), None);
                task
            }
            ControllerOperation::OpenImportDialog {} => {
                let task = self.update(Message::SettingsImportPressed);
                respond(true, Some(serde_json::json!({ "dispatched": true })), None);
                task
            }
            ControllerOperation::CloseWindow {} => match self.main_window_id {
                Some(id) => {
                    let task = self.update(Message::CloseRequested(id));
                    respond(
                        true,
                        Some(serde_json::json!({
                            "dispatched": true,
                            "window_id_assigned": self.main_window_id.is_some(),
                            "window_generation": self.window_state.generation,
                        })),
                        None,
                    );
                    task
                }
                None => failed("no window to close".to_string()),
            },
            ControllerOperation::ShowWindow {} => {
                let task = self.request_show();
                respond(
                    true,
                    Some(serde_json::json!({
                        "dispatched": true,
                        "window_id_assigned": self.main_window_id.is_some(),
                        "window_generation": self.window_state.generation,
                    })),
                    None,
                );
                task
            }
            ControllerOperation::Quit {} => {
                respond(true, Some(serde_json::json!({ "dispatched": true })), None);
                Task::done(Message::BeginQuit)
            }
        }
    }

    /// Runs the existing paste orchestration for one activated prompt:
    /// same cache lookup, busy policy, clipboard/paste requests and use
    /// counting as the polled path. Shared by native polling and portal
    /// stream activations.
    fn activate_prompt_hotkey(&mut self, prompt_id: &PromptId) -> Task<Message> {
        if self.pending_hotkey_name.is_some() {
            self.show_notification(NotificationEvent::PasteBusy);
            return Task::none();
        }
        let Some(p) = self.prompt_cloned_by_id(prompt_id) else {
            return Task::none();
        };
        if self.runtime.mode() == RuntimeMode::Synthetic
            && !self.runtime.native_consent_authorized()
        {
            self.show_notification(NotificationEvent::PastePermissionDenied);
            return Task::none();
        }
        match crate::hotkeys::request_paste(Arc::clone(&self.runtime), p.content) {
            crate::hotkeys::PasteRequest::Started => {
                if crate::hotkeys::is_paste_permission_denied() {
                    self.show_notification(NotificationEvent::PastePermissionDenied);
                } else {
                    self.pending_hotkey_name = Some(p.name);
                    self.show_notification(NotificationEvent::PasteRequested);
                }
            }
            crate::hotkeys::PasteRequest::ClipboardRequired(content) => {
                self.pending_hotkey_name = Some(p.name);
                self.pending_native_paste_content = Some(content.clone());
                self.show_notification(NotificationEvent::PasteRequested);

                return iced::clipboard::write(content)
                    .chain(iced::clipboard::read().map(Message::NativeClipboardPrepared));
            }
            crate::hotkeys::PasteRequest::Busy => {
                self.show_notification(NotificationEvent::PasteBusy);
            }
            crate::hotkeys::PasteRequest::Unavailable => {
                self.show_notification(NotificationEvent::PasteFailed);
            }
        }
        Task::none()
    }

    /// Handles one backend shortcut event. Acknowledgements update the
    /// readiness map (current revision and still-desired prompts only);
    /// activations route immediately without polling dependence; backend
    /// loss clears Active badges at once.
    fn handle_hotkey_service_event(&mut self, event: HotkeyEvent) -> Task<Message> {
        match event {
            HotkeyEvent::Acknowledged(ack) => {
                let status = ack.status;
                self.diagnostics.record(
                    "shortcut_acknowledged",
                    Some(ack.revision),
                    Some(self.window_state.generation),
                    Some(format!("status={status:?}")),
                );
                if self.apply_shortcut_acknowledgement(ack) {
                    match status {
                        ShortcutStatus::Active => {
                            // Valid success resolves the previous shortcut
                            // warning, if any.
                            if let Some(id) = self.shortcut_notice.take() {
                                self.notifications.dismiss(id);
                            }
                            self.status_message.clear();
                        }
                        ShortcutStatus::AwaitingConsent => {
                            self.show_notification(NotificationEvent::ShortcutAwaitingConsent);
                        }
                        ShortcutStatus::Failed => {
                            self.publish_shortcut_notice(NotificationEvent::ShortcutBindingFailed);
                        }
                        ShortcutStatus::Cancelled => {
                            self.publish_shortcut_notice(
                                NotificationEvent::ShortcutBindingCancelled,
                            );
                        }
                        ShortcutStatus::Unavailable => {
                            self.publish_shortcut_notice(
                                NotificationEvent::ShortcutBindingUnavailable,
                            );
                        }
                        ShortcutStatus::Pending
                        | ShortcutStatus::AwaitingWindow
                        | ShortcutStatus::Inactive => {}
                    }
                }
                Task::none()
            }
            HotkeyEvent::Activated {
                revision,
                runtime_id,
            } => {
                self.diagnostics.record(
                    "shortcut_activated",
                    Some(revision),
                    Some(self.window_state.generation),
                    None,
                );
                // Stale activations (older revision, deleted prompt or
                // non-Active route) are rejected, never routed.
                if revision != self.desired_revision {
                    return Task::none();
                }
                match self.runtime_to_prompt(runtime_id) {
                    Some(prompt_id) => self.activate_prompt_hotkey(&prompt_id),
                    None => Task::none(),
                }
            }
            HotkeyEvent::BackendUnavailable(error) => {
                for ack in self.shortcut_states.values_mut() {
                    if ack.status != ShortcutStatus::Inactive {
                        ack.status = ShortcutStatus::Unavailable;
                        ack.error_code = Some(error);
                    }
                }
                self.status_message = format!("Shortcut backend unavailable: {error}");
                Task::none()
            }
        }
    }

    /// Ordered quit: flushes structured diagnostics (when configured),
    /// cancels the in-flight portal request, closes the session and shuts
    /// down backend services within a bounded deadline, then exits.
    /// Process exit releases remaining owned bus resources atomically;
    /// the deadline only bounds how long well-behaved cleanup may take.
    fn begin_ordered_quit(&mut self) -> Task<Message> {
        let shutdown_timeout = self.runtime.config();
        let shutdown_timeout =
            std::time::Duration::from_millis(shutdown_timeout.shutdown_timeout_ms);
        match self.hotkey_service {
            Some(ref svc) => Task::batch([
                svc.shutdown(),
                Task::perform(
                    async move {
                        tokio::time::sleep(shutdown_timeout).await;
                    },
                    |_| Message::QuitForced,
                ),
            ]),
            None => {
                self.diagnostics
                    .record_shutdown_outcome(crate::desktop::ShutdownOutcome::Released);
                self.flush_diagnostics();
                iced::exit()
            }
        }
    }

    /// Writes buffered desktop events to the configured private evidence
    /// file. Unset path disables file output; every failure is reported,
    /// never a silent fake report.
    fn flush_diagnostics(&mut self) {
        let Some(path) = self.runtime.diagnostics_path() else {
            return;
        };
        match self.diagnostics.flush_to_file(path) {
            Ok(count) => {
                tracing::debug!(
                    "flushed {count} desktop events for run {}",
                    self.diagnostics.run_id()
                );
            }
            Err(error) => {
                self.status_message = format!("Diagnostics export failed: {error}");
            }
        }
    }

    /// Routes an event-driven tray signal into the canonical window intents.
    ///
    /// Show/Quit follow the same routes as the legacy polled events. A
    /// failed or lost tray host while windowless requests one fallback
    /// window so the app can never be trapped invisible; the canonical
    /// open path coalesces duplicates.
    fn handle_tray_signal(&mut self, signal: TraySignal) -> Task<Message> {
        self.diagnostics.record(
            "tray_callback",
            Some(self.desired_revision),
            Some(self.window_state.generation),
            Some(format!("signal={signal:?}")),
        );
        match signal {
            TraySignal::ShowRequested => Task::done(Message::ShowWindow(self.main_window_id)),
            TraySignal::QuitRequested => Task::done(Message::BeginQuit),
            TraySignal::Ready | TraySignal::HostOnline => {
                self.tray_state = TrayState::Ready;
                Task::none()
            }
            TraySignal::Unavailable(_) => {
                self.tray_state = TrayState::Unavailable;
                self.tray_fallback_task()
            }
            TraySignal::HostOffline => {
                self.tray_state = TrayState::HostOffline;
                self.tray_fallback_task()
            }
        }
    }

    fn should_request_tray_fallback(&self) -> bool {
        self.main_window_id.is_none() && !self.window_open_pending
    }

    /// Whether the 50ms hotkey tick subscription is needed: native event
    /// source, any acknowledged-or-pending shortcut, or an unfinished
    /// paste outcome. Portal activations never depend on this tick.
    fn needs_hotkey_tick(&self) -> bool {
        if self.hotkey_service.is_none() || !self.settings.hotkeys_enabled {
            return false;
        }
        if self.pending_hotkey_name.is_some() || self.pending_native_paste_content.is_some() {
            return true;
        }
        if crate::platform::hotkey_backend_kind() == crate::platform::HotkeyBackendKind::Native {
            return true;
        }
        self.shortcut_states.values().any(|ack| {
            matches!(
                ack.status,
                ShortcutStatus::Active
                    | ShortcutStatus::Pending
                    | ShortcutStatus::AwaitingConsent
                    | ShortcutStatus::AwaitingWindow
            )
        })
    }

    fn tray_fallback_task(&self) -> Task<Message> {
        if self.should_request_tray_fallback() {
            Task::done(Message::ShowWindow(None))
        } else {
            Task::none()
        }
    }

    pub(crate) fn is_smoke_mode(&self) -> bool {
        self.smoke_mode
    }

    pub fn theme(&self) -> Theme {
        match self.settings.theme.as_str() {
            "Light" => Theme::Light,
            _ => Theme::Dark,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let smoke_exit = if self.smoke_mode {
            iced::keyboard::listen().filter_map(|event| match event {
                iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::F12),
                    ..
                } => Some(Message::SmokeExitRequested),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let smoke_tick = if self.smoke_mode {
            iced::time::every(Duration::from_millis(250)).map(|_| Message::SmokeTick)
        } else {
            Subscription::none()
        };

        // Native activations stay on the polled path (X11 contract) and
        // paste outcomes must be collected while a paste is pending even
        // after the last shortcut was deleted. Portal activations arrive
        // through the hotkey service subscription below, never this tick.
        let hotkey_tick = if self.needs_hotkey_tick() {
            iced::time::every(Duration::from_millis(50)).map(|_| Message::HotkeyTick)
        } else {
            Subscription::none()
        };

        // Portal acknowledgements, activations and backend loss are
        // stream-driven whenever the service exists — including with zero
        // active ids, so loss can never hide behind an empty map.
        let hotkey_service_events = match self.hotkey_service {
            Some(ref svc) => svc.subscription().map(Message::HotkeyServiceEvent),
            None => Subscription::none(),
        };

        let keyboard_capture = if self.listening_for_hotkey {
            iced::keyboard::listen().filter_map(|event| match event {
                iced::keyboard::Event::KeyPressed { key, modifiers, .. } => {
                    capture_hotkey(key, modifiers)
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // Window close requests (X button) -> hide to tray
        let close_requests = iced::window::close_requests().map(Message::CloseRequested);

        // Windows keeps the 100ms tray-icon polling path. Linux uses the
        // persistent event-driven ksni subscription; no tray timer exists.
        #[cfg(target_os = "windows")]
        let tray_events = if self.tray.is_some() {
            iced::time::every(Duration::from_millis(100)).map(|_| Message::TrayTick)
        } else {
            Subscription::none()
        };
        #[cfg(target_os = "windows")]
        let tray_signals = Subscription::none();
        #[cfg(not(target_os = "windows"))]
        let tray_events = Subscription::none();
        #[cfg(not(target_os = "windows"))]
        let tray_signals =
            crate::tray::subscription(self.tray_subscription_instance, Arc::clone(&self.runtime))
                .map(Message::TraySignal);

        let resize_events =
            iced::window::resize_events().map(|(_id, size)| Message::WindowResized(size));

        // Supplemental compositor observations. CloseRequested stays on
        // the dedicated close_requests subscription; it is filtered here
        // so one physical close cannot drive two intents.
        let desktop_window_events = iced::window::events().filter_map(|(id, event)| {
            if matches!(event, iced::window::Event::CloseRequested) {
                None
            } else {
                Some(Message::DesktopWindowEvent { id, event })
            }
        });

        #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
        let controller_commands =
            iced::time::every(Duration::from_millis(20)).map(|_| Message::ControllerPoll);
        #[cfg(not(all(target_os = "linux", feature = "desktop-integration-tests")))]
        let controller_commands = Subscription::none();

        Subscription::batch([
            smoke_exit,
            smoke_tick,
            hotkey_tick,
            hotkey_service_events,
            controller_commands,
            keyboard_capture,
            close_requests,
            tray_events,
            tray_signals,
            resize_events,
            desktop_window_events,
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        perf::measure("ui.update", || {
            match message {
                Message::SearchChanged(term) => {
                    self.search_term = term;
                    self.refresh_prompts("");
                    self.show_notification(NotificationEvent::SearchCompleted {
                        result_count: self.prompts.len(),
                    });
                }
                Message::PromptSelected(id) => {
                    self.selected_id = Some(id);
                    self.status_message.clear();
                }
                Message::PromptListRequested => {
                    self.selected_id = None;
                    self.status_message.clear();
                }
                Message::CopyPressed(id, content) => {
                    if self.db.record_use(&id).is_err() {
                        self.show_notification(NotificationEvent::SaveFailed);
                    } else {
                        self.sync_prompt_cache_from_db(&id);
                        self.refresh_prompts("");
                        self.show_notification(NotificationEvent::PromptCopied);
                    }
                    return iced::clipboard::write(content);
                }
                Message::NewPressed => {
                    self.new_form = Some(NewPromptForm::default());
                    self.form_errors.clear();
                    self.status_message.clear();
                }
                Message::DeletePressed(id) => {
                    self.pending_delete_id = Some(id);
                    self.status_message.clear();
                }
                Message::DeleteCancelPressed => {
                    self.pending_delete_id = None;
                    self.status_message.clear();
                }
                Message::DeleteConfirmPressed(id) => {
                    // The database governs: a failed delete leaves the
                    // previous desired set and active status untouched.
                    match self.db.delete(&id) {
                        Ok(()) => {
                            self.remove_prompt_cache(&id);
                            self.selected_id = None;
                            self.pending_delete_id = None;
                            self.refresh_prompts("");
                            self.reconcile_shortcuts_after_commit();
                        }
                        Err(_) => {
                            self.show_notification(NotificationEvent::DeleteFailed);
                        }
                    }
                }
                Message::EditPressed(id) => {
                    if let Some(p) = self.prompt_cloned_by_id(&id) {
                        self.new_form = Some(NewPromptForm::from_prompt(&p));
                        self.form_errors.clear();
                        self.status_message.clear();
                    } else {
                        self.status_message.clear();
                    }
                }
                Message::HotkeyTick => {
                    if let Some(outcome) = crate::hotkeys::poll_paste_outcome() {
                        self.pending_native_paste_content = None;
                        if self.pending_hotkey_name.take().is_some() {
                            match outcome {
                                crate::hotkeys::PasteOutcome::Completed => {
                                    self.show_notification(NotificationEvent::PasteCompleted);
                                }
                                crate::hotkeys::PasteOutcome::Failed => {
                                    self.show_notification(NotificationEvent::PasteFailed);
                                }
                            }
                        }
                    }

                    // Native polled activations only. Portal activations
                    // arrive through HotkeyServiceEvent without polling.
                    if crate::platform::hotkey_backend_kind()
                        == crate::platform::HotkeyBackendKind::Native
                    {
                        if let Some(hotkey_id) = HotkeyService::poll_native_event() {
                            if let Some(prompt_id) = self.runtime_to_prompt(hotkey_id) {
                                return self.activate_prompt_hotkey(&prompt_id);
                            }
                        }
                    }
                }
                Message::HotkeyServiceEvent(event) => {
                    return self.handle_hotkey_service_event(event);
                }
                Message::RetryShortcutBinding => {
                    if let Some(ref svc) = self.hotkey_service {
                        if svc.retry_pending().is_ok() {
                            // Re-mark pending so the UI reflects the retry
                            // until fresh acknowledgements arrive.
                            for ack in self.shortcut_states.values_mut() {
                                if matches!(
                                    ack.status,
                                    ShortcutStatus::Failed
                                        | ShortcutStatus::Cancelled
                                        | ShortcutStatus::Unavailable
                                ) {
                                    ack.status = ShortcutStatus::Pending;
                                    ack.error_code = None;
                                }
                            }
                        }
                    }
                }
                Message::ConfigureShortcutBinding => {
                    // GlobalShortcuts v1 has no ConfigureShortcuts: direct
                    // the user to the desktop keyboard settings instead of
                    // calling a private backend interface.
                    let _ = std::process::Command::new("gnome-control-center")
                        .arg("keyboard")
                        .spawn();
                    self.status_message =
                        "Open GNOME Settings → Keyboard to change an existing shortcut".to_string();
                }
                Message::DesktopShutdownFinished(result) => {
                    self.diagnostics.record_shutdown_outcome(result);
                    self.flush_diagnostics();
                    if let crate::desktop::ShutdownOutcome::Forced(error) = result {
                        tracing::warn!("desktop shutdown forced: {error}");
                    }
                    return iced::exit();
                }
                #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
                Message::ControllerPoll => {
                    if let Some(command) = crate::desktop_test_support::poll_command() {
                        return self.apply_controller_command(command);
                    }
                }
                Message::BeginQuit => {
                    return self.begin_ordered_quit();
                }
                Message::QuitForced => {
                    self.diagnostics.record_shutdown_outcome(
                        crate::desktop::ShutdownOutcome::Forced(
                            crate::desktop::DesktopErrorCode::ShutdownTimeout,
                        ),
                    );
                    self.flush_diagnostics();
                    tracing::warn!("desktop shutdown deadline elapsed; forcing exit");
                    return iced::exit();
                }
                Message::NativeClipboardPrepared(actual) => {
                    let Some(expected) = self.pending_native_paste_content.take() else {
                        return Task::none();
                    };

                    if !clipboard_matches_expected_prompt(&expected, actual.as_deref()) {
                        self.pending_hotkey_name = None;
                        self.show_notification(NotificationEvent::PasteFailed);
                        return Task::none();
                    }

                    if !crate::hotkeys::paste_from_prepared_clipboard() {
                        self.pending_hotkey_name = None;
                        self.show_notification(NotificationEvent::PasteFailed);
                    }

                    return Task::none();
                }
                Message::FormNameChanged(name) => {
                    if !name.trim().is_empty() {
                        self.clear_form_error(Field::Name);
                    }
                    if let Some(ref mut form) = self.new_form {
                        form.name = name;
                    }
                }
                Message::FormContentEdited(action) => {
                    let has_content = if let Some(ref mut form) = self.new_form {
                        form.content_editor.perform(action);
                        form.content = normalize_editor_text(&form.content_editor.text());
                        !form.content.trim().is_empty()
                    } else {
                        false
                    };
                    if has_content {
                        self.clear_form_error(Field::Content);
                    }
                }
                Message::FormSave => {
                    if let Some(form) = self.new_form.take() {
                        if form.name.trim().is_empty() || form.content.trim().is_empty() {
                            self.form_errors.clear();
                            if form.name.trim().is_empty() {
                                self.set_form_error(NotificationEvent::NameRequired);
                            }
                            if form.content.trim().is_empty() {
                                self.set_form_error(NotificationEvent::ContentRequired);
                            }
                            self.status_message.clear();
                            self.new_form = Some(form);
                        } else {
                            let hotkey = if form.hotkey.trim().is_empty() {
                                None
                            } else {
                                let hotkey = form.hotkey.trim().to_string();
                                if !crate::hotkeys::validate_hotkey(&hotkey) {
                                    self.set_form_error(NotificationEvent::InvalidShortcut);
                                    self.status_message.clear();
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
                                        self.selected_id = Some(editing_id.clone());
                                        self.sync_prompt_cache_from_db(&editing_id);
                                        self.refresh_prompts("");
                                        // One snapshot after the successful
                                        // commit. Content-only edits are
                                        // no-ops: no new session, no dialog.
                                        self.reconcile_shortcuts_after_commit();
                                    }
                                    Err(_) => {
                                        self.show_notification(NotificationEvent::SaveFailed);
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
                                        self.selected_id = Some(new_id.clone());
                                        self.sync_prompt_cache_from_db(&new_id);
                                        self.refresh_prompts("");
                                        // Immediately Pending after the
                                        // successful insert — never Active
                                        // before the backend acknowledges.
                                        self.reconcile_shortcuts_after_commit();
                                    }
                                    Err(_) => {
                                        self.show_notification(NotificationEvent::SaveFailed);
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
                    self.form_errors.clear();
                    self.listening_for_hotkey = false;
                    self.status_message.clear();
                }
                Message::FormHotkeyRecordPressed => {
                    self.listening_for_hotkey = true;
                    self.status_message.clear();
                }
                Message::FormHotkeyCaptured(hotkey_str) => {
                    self.listening_for_hotkey = false;
                    self.clear_form_error(Field::Hotkey);
                    if let Some(ref mut form) = self.new_form {
                        form.hotkey = hotkey_str;
                        form.hotkey_enabled = true;
                    }
                    self.status_message.clear();
                }
                Message::FormHotkeyClearPressed => {
                    self.clear_form_error(Field::Hotkey);
                    if let Some(ref mut form) = self.new_form {
                        form.hotkey = String::new();
                        form.hotkey_enabled = false;
                    }
                    self.status_message.clear();
                }
                Message::FormHotkeyListeningCancelled => {
                    self.listening_for_hotkey = false;
                    self.status_message.clear();
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
                        self.refresh_prompts("");
                    }
                    Err(_) => {
                        self.show_notification(NotificationEvent::SaveFailed);
                    }
                },
                Message::PromptFilterChanged(filter) => {
                    self.prompt_filter = filter;
                    self.refresh_prompts("");
                }
                Message::PromptSortChanged(sort) => {
                    self.prompt_sort = sort;
                    self.refresh_prompts("");
                }
                Message::NotificationDismissed(id) => {
                    self.notifications.dismiss(id);
                }
                Message::SettingsPressed => {
                    self.show_settings = true;
                    self.show_info = false;
                    self.status_message.clear();
                }
                Message::SettingsCancel => {
                    self.show_settings = false;
                    self.status_message.clear();
                }
                Message::SettingsHotkeyToggled(enabled) => {
                    self.settings.hotkeys_enabled = enabled;
                    if enabled
                        && (self.runtime.mode() == RuntimeMode::Production
                            || self.runtime.native_consent_authorized())
                    {
                        crate::hotkeys::prewarm_permission(Arc::clone(&self.runtime));
                    }
                    // Disabled reconciles to the empty snapshot: the
                    // worker cancels, closes the session and acknowledges
                    // Inactive. Re-enabled reconciles one full snapshot.
                    self.reconcile_shortcuts_after_commit();
                    self.status_message.clear();
                }
                Message::SettingsAutostartToggled(enabled) => {
                    self.settings.autostart_enabled = enabled;
                    self.status_message.clear();
                }
                Message::SettingsThemeChanged(theme) => {
                    self.settings.theme = theme;
                    self.status_message.clear();
                }
                Message::SettingsSave => {
                    let path = self.runtime.settings_path();
                    let settings = self.settings.clone();
                    let allow_autostart = self.runtime.mode() == RuntimeMode::Production;
                    self.status_message.clear();
                    return Task::perform(
                        async move {
                            SettingsService::save_and_apply(&settings, &path, |enabled| {
                                if allow_autostart {
                                    sync_autostart_enabled(enabled)
                                } else {
                                    Ok(())
                                }
                            })
                            .map_err(|error| error.to_string())
                        },
                        Message::SettingsSaved,
                    );
                }
                Message::SettingsSaved(result) => match result {
                    Ok(()) => self.status_message.clear(),
                    Err(_) => self.show_notification(NotificationEvent::SaveFailed),
                },
                Message::SettingsExportPressed => {
                    self.status_message.clear();
                    return Task::perform(
                        export_prompts_to_json_file(self.all_prompts.clone()),
                        Message::SettingsExportFinished,
                    );
                }
                Message::SettingsExportFinished(result) => match result {
                    Ok(_) => self.status_message.clear(),
                    Err(_) => self.show_notification(NotificationEvent::ExportFailed),
                },
                Message::SettingsImportPressed => {
                    self.status_message.clear();
                    return Task::perform(
                        import_prompts_from_json_file(),
                        Message::SettingsImportLoaded,
                    );
                }
                Message::SettingsImportLoaded(result) => match result {
                    Ok(backup) => {
                        match PromptImportPreview::from_backup(&backup, &self.all_prompts) {
                            Ok(preview) => {
                                self.pending_import_backup = Some(backup);
                                self.pending_import_preview = Some(preview);
                                self.pending_import_mode = ImportMode::Merge;
                                self.pending_duplicate_mode = DuplicateMode::Skip;
                                self.show_settings = false;
                                self.status_message.clear();
                            }
                            Err(_) => {
                                self.show_notification(NotificationEvent::ImportFailed);
                            }
                        }
                    }
                    Err(_) => {
                        self.show_notification(NotificationEvent::ImportFailed);
                    }
                },
                Message::ImportModeSelected(mode) => {
                    if self.all_prompts.is_empty() && matches!(mode, ImportMode::ReplaceAll) {
                        self.pending_import_mode = ImportMode::Merge;
                        self.status_message.clear();
                        return Task::none();
                    }
                    self.pending_import_mode = mode;
                    self.status_message.clear();
                }
                Message::DuplicateModeSelected(mode) => {
                    self.pending_duplicate_mode = mode;
                    self.status_message.clear();
                }
                Message::ImportConfirmPressed => match self.apply_pending_import() {
                    Ok(summary) => {
                        let _ = summary;
                        self.status_message.clear();
                    }
                    Err(_) => {
                        self.show_notification(NotificationEvent::ImportFailed);
                    }
                },
                Message::ImportCancelPressed => {
                    self.pending_import_backup = None;
                    self.pending_import_preview = None;
                    self.status_message.clear();
                }
                Message::InfoPressed => {
                    self.show_info = true;
                    self.show_settings = false;
                    self.status_message.clear();
                }
                Message::InfoDismissed => {
                    self.show_info = false;
                    self.status_message.clear();
                }
                // Tray: Window close requested -> hide to tray.
                // Production close follows the canonical lifecycle in
                // main.update_application; this arm serves smoke mode and
                // direct intent routing with the same single owner.
                Message::CloseRequested(id) => {
                    if should_exit_on_close_request(self.smoke_mode) {
                        let _ = self.note_window_closed(id);
                        self.status_message.clear();
                        return iced::exit();
                    }

                    let closing_current = self.window_state.id == Some(id)
                        && self.window_state.phase == WindowPhase::Open;
                    let task = self.request_close(id);
                    if closing_current {
                        self.show_notification(NotificationEvent::WindowHiddenToTray);
                    }
                    return task;
                }
                Message::ShowWindow(id) => {
                    return match id {
                        Some(id) => self.present_window(id),
                        None => self.request_show(),
                    };
                }
                Message::WindowOpened { generation, id } => {
                    return self.note_window_opened(generation, id);
                }
                Message::DesktopWindowEvent { id, event } => {
                    return self.handle_desktop_window_event(id, event);
                }
                #[cfg(target_os = "linux")]
                Message::ParentExported { generation, result } => {
                    match result {
                        Ok(lease) => {
                            self.diagnostics.record(
                                "parent_export_finished",
                                Some(self.desired_revision),
                                Some(generation),
                                Some(format!("elapsed_ms={}", lease.export_elapsed_ms())),
                            );
                            if lease.is_current_for(generation)
                                && self.window_state.generation == generation
                            {
                                let context = ParentContext {
                                    generation: lease.generation(),
                                    handle: lease.handle(),
                                };
                                self.parent_lease = Some(lease);
                                // User-initiated consent parents to the
                                // live window from here on.
                                if let Some(ref svc) = self.hotkey_service {
                                    if let Err(error) = svc.set_parent(Some(context)) {
                                        self.status_message =
                                            format!("Shortcut parent unavailable: {error}");
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            self.status_message = format!("Window parent unavailable: {error}");
                        }
                    }
                }
                Message::InteractionCancelled { generation, result } => {
                    if !self.close_awaiting_cancel || self.window_state.generation != generation {
                        return Task::none();
                    }
                    self.close_awaiting_cancel = false;
                    if let CancelOutcome::Failed(error) = result {
                        self.status_message = format!("Shortcut cleanup failed: {error}");
                        let Some(id) = self.window_state.id else {
                            return Task::none();
                        };
                        let effects = crate::window_lifecycle::transition(
                            &mut self.window_state,
                            WindowIntent::AbortClose(id),
                        );
                        return self.apply_window_effects(effects);
                    }
                    // The worker confirmed release: drop the lease (and
                    // clear the worker parent) before destroying the
                    // surface. A stale Response can no longer reinstate
                    // routes for a destroyed window.
                    #[cfg(target_os = "linux")]
                    {
                        self.parent_lease = None;
                        if let Some(ref svc) = self.hotkey_service {
                            let _ = svc.set_parent(None);
                        }
                    }
                    return match self.window_state.id {
                        Some(id) => crate::window_lifecycle::close(id),
                        None => Task::none(),
                    };
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
                #[cfg(target_os = "windows")]
                Message::TrayTick => {
                    pump_platform_events();

                    if let Some(event) = self.tray.as_ref().and_then(TrayHandle::poll_event) {
                        return match event {
                            TrayEvent::ShowRequested => {
                                Task::done(Message::ShowWindow(self.main_window_id))
                            }
                            TrayEvent::QuitRequested => iced::exit(),
                        };
                    }
                }
                #[cfg(not(target_os = "windows"))]
                Message::TrayTick => {}
                Message::TraySignal(signal) => {
                    return self.handle_tray_signal(signal);
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
            } else if self.pending_import_backup.is_some() {
                let modal = self.view_import_confirmation_modal();
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

            column![header, self.view_message_bar(), body, status_bar].into()
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
            if self.prompts.is_empty() {
                list_col = list_col.push(
                    container(
                        text(self.prompt_list_empty_message())
                            .size(EMPTY_STATE_TEXT_SIZE)
                            .color(self.theme().extended_palette().secondary.strong.color),
                    )
                    .padding(40)
                    .center_x(Length::Fill),
                );
            }

            container(
                column![
                    self.view_prompt_controls(),
                    Space::new().height(LIST_PANEL_SPACING),
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
        let title = text(&prompt.name)
            .font(Font {
                weight: iced::font::Weight::Bold,
                ..Default::default()
            })
            .wrapping(prompt_card_title_wrapping());
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
                    .style(button::primary),
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
                    let readiness_row = self.shortcut_readiness_row(&p.id, p.hotkey.as_deref());
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
                        Space::new().height(0).into()
                    };

                    return column![
                        text(&p.name).size(PROMPT_DETAIL_TITLE_SIZE).font(Font {
                            weight: iced::font::Weight::Bold,
                            ..Default::default()
                        }),
                        Space::new().height(DETAIL_SECTION_SPACING),
                        text(format!("Used {} times", p.use_count)).size(PROMPT_METADATA_TEXT_SIZE),
                        Space::new().height(DETAIL_SECTION_SPACING),
                        hotkey_row,
                        readiness_row,
                        Space::new().height(DETAIL_SECTION_SPACING),
                        scrollable(
                            text(&p.content)
                                .font(iced::Font::MONOSPACE)
                                .size(PROMPT_CONTENT_TEXT_SIZE)
                        )
                        .height(Length::Fill),
                        Space::new().height(DETAIL_SECTION_SPACING),
                        row![
                            tooltip(
                                button(icon::star())
                                    .on_press(Message::FavoriteToggled(p.id.clone(), !p.favorite))
                                    .padding(CONTROL_PADDING)
                                    .style(detail_toolbar_button_style(
                                        DetailToolbarAction::Favorite {
                                            is_active: p.favorite,
                                        },
                                    )),
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
                                    .style(detail_toolbar_button_style(DetailToolbarAction::Copy)),
                                text("Copy to clipboard"),
                                tooltip::Position::Top,
                            ),
                            tooltip(
                                button(icon::pencil())
                                    .on_press(Message::EditPressed(p.id.clone()))
                                    .padding(CONTROL_PADDING)
                                    .style(detail_toolbar_button_style(DetailToolbarAction::Edit)),
                                text("Edit prompt"),
                                tooltip::Position::Top,
                            ),
                            tooltip(
                                button(icon::trash())
                                    .on_press(Message::DeletePressed(p.id.clone()))
                                    .padding(CONTROL_PADDING)
                                    .style(detail_toolbar_button_style(
                                        DetailToolbarAction::Delete
                                    )),
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
                Space::new().height(Length::Fill),
                text(self.prompt_detail_empty_message())
                    .size(EMPTY_STATE_TEXT_SIZE)
                    .color(self.theme().extended_palette().secondary.strong.color),
                Space::new().height(Length::Fill),
            ]
            .align_x(alignment::Alignment::Center)
            .width(Length::Fill)
            .into()
        })
    }

    /// Readiness badge for a prompt: requested key plus the backend
    /// state, and the backend-assigned trigger when Active and different
    /// from the request. Retry/Configure stay keyboard-focusable with
    /// descriptive text; the requested input is never overwritten with
    /// the assigned description.
    fn shortcut_readiness_row(
        &self,
        prompt_id: &PromptId,
        requested: Option<&str>,
    ) -> Element<'_, Message> {
        let Some(requested) = requested.filter(|key| !key.trim().is_empty()) else {
            return Space::new().height(0).into();
        };
        let _ = requested;
        let Some(ack) = self.shortcut_states.get(prompt_id) else {
            return Space::new().height(0).into();
        };
        let mut items: Vec<Element<'_, Message>> =
            vec![text(format!("Shortcut {}", status_label(ack.status)))
                .size(PROMPT_METADATA_TEXT_SIZE)
                .into()];
        if ack.status == ShortcutStatus::Active {
            if let Some(actual) = ack.trigger_description.as_deref() {
                let desired = self
                    .last_desired_specs
                    .iter()
                    .find(|spec| spec.prompt_id == *prompt_id)
                    .map(|spec| spec.preferred_trigger.as_str())
                    .unwrap_or("");
                if !actual.is_empty() && actual != desired {
                    items.push(
                        text(format!("assigned by system: {actual}"))
                            .size(PROMPT_METADATA_TEXT_SIZE)
                            .into(),
                    );
                }
            }
        }
        if matches!(
            ack.status,
            ShortcutStatus::Failed | ShortcutStatus::Cancelled | ShortcutStatus::Unavailable
        ) {
            items.push(
                button(text("Retry shortcut"))
                    .on_press(Message::RetryShortcutBinding)
                    .padding(CONTROL_PADDING)
                    .style(button::secondary)
                    .into(),
            );
            items.push(
                button(text("Change in system settings"))
                    .on_press(Message::ConfigureShortcutBinding)
                    .padding(CONTROL_PADDING)
                    .style(button::secondary)
                    .into(),
            );
        }
        row(items).spacing(8).into()
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
                .wrapping(prompt_editor_wrapping())
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
                .style(button::primary);

            let clear_button = button(icon::eraser())
                .on_press(Message::FormHotkeyClearPressed)
                .padding(CONTROL_PADDING)
                .style(button::primary);

            let hotkey_enabled_checkbox = checkbox(form.hotkey_enabled)
                .label("Enable hotkey")
                .on_toggle(Message::FormHotkeyEnabledToggled)
                .size(16)
                .style(checkbox_style);

            let hotkey_row = row![
                hotkey_display,
                if self.listening_for_hotkey {
                    button(icon::keyboard())
                        .padding(CONTROL_PADDING)
                        .style(button::primary)
                } else {
                    record_button
                },
                if form.hotkey.is_empty() || self.listening_for_hotkey {
                    button(icon::eraser())
                        .padding(CONTROL_PADDING)
                        .style(button::primary)
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
                    Space::new().height(15),
                    text("Name").size(MODAL_LABEL_TEXT_SIZE),
                    name_input,
                    self.view_form_error(Field::Name),
                    Space::new().height(10),
                    text("Content").size(MODAL_LABEL_TEXT_SIZE),
                    content_input,
                    self.view_form_error(Field::Content),
                    Space::new().height(10),
                    text("Hotkey (optional)").size(MODAL_LABEL_TEXT_SIZE),
                    hotkey_row,
                    self.view_form_error(Field::Hotkey),
                    Space::new().height(MODAL_SECTION_SPACING),
                    hotkey_enabled_checkbox,
                    Space::new().height(20),
                    tooltip(
                        button(icon::save())
                            .on_press(Message::FormSave)
                            .padding(CONTROL_PADDING)
                            .style(button::primary),
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

            let hotkey_checkbox = checkbox(self.settings.hotkeys_enabled)
                .label("Hotkeys enabled")
                .on_toggle(Message::SettingsHotkeyToggled)
                .size(16)
                .style(checkbox_style);

            let autostart_checkbox = checkbox(self.settings.autostart_enabled)
                .label("Start automatically on login")
                .on_toggle(Message::SettingsAutostartToggled)
                .size(16)
                .style(checkbox_style);

            let selected_theme = THEMES.iter().find(|&&t| t == self.settings.theme).copied();
            let theme_picker = pick_list(THEMES.as_slice(), selected_theme, |s| {
                Message::SettingsThemeChanged(s.to_string())
            })
            .padding(CONTROL_PADDING)
            .style(pick_list_style);

            let export_button = tooltip(
                button(icon::download())
                    .on_press(Message::SettingsExportPressed)
                    .padding(CONTROL_PADDING)
                    .style(action_button_style(ActionButtonTone::Neutral)),
                text("Export prompts"),
                tooltip::Position::Top,
            );

            let import_button = tooltip(
                button(icon::upload())
                    .on_press(Message::SettingsImportPressed)
                    .padding(CONTROL_PADDING)
                    .style(action_button_style(ActionButtonTone::Neutral)),
                text("Import prompts"),
                tooltip::Position::Top,
            );

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
                    Space::new().height(15),
                    hotkey_checkbox,
                    Space::new().height(MODAL_SECTION_SPACING),
                    autostart_checkbox,
                    Space::new().height(MODAL_SECTION_SPACING),
                    row![
                        text("Theme").size(MODAL_BODY_TEXT_SIZE).width(Length::Fill),
                        theme_picker,
                    ]
                    .align_y(alignment::Alignment::Center),
                    Space::new().height(MODAL_SECTION_SPACING),
                    row![import_button, export_button]
                        .spacing(10)
                        .align_y(alignment::Alignment::Center),
                    Space::new().height(20),
                    row![tooltip(
                        button(icon::save())
                            .on_press(Message::SettingsSave)
                            .padding(CONTROL_PADDING)
                            .style(action_button_style(ActionButtonTone::Primary)),
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
                    Space::new().height(15),
                    text(format!("Delete \"{}\"?", name)).size(MODAL_BODY_TEXT_SIZE),
                    Space::new().height(20),
                    row![
                        button(text("Cancel"))
                            .on_press(Message::DeleteCancelPressed)
                            .padding(CONTROL_PADDING)
                            .style(button::secondary),
                        button(text("Delete"))
                            .on_press(Message::DeleteConfirmPressed(id))
                            .padding(CONTROL_PADDING)
                            .style(action_button_style(ActionButtonTone::Destructive)),
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

    fn view_import_confirmation_modal(&self) -> Element<'_, Message> {
        perf::measure("ui.view_import_confirmation_modal", || {
            let preview = self
                .pending_import_preview
                .as_ref()
                .expect("Import preview should exist when import modal is shown");
            let controls = self.import_review_controls(preview);
            let duplicate_count = preview.duplicate_names.len();
            let duplicate_summary: Element<'_, Message> = if controls.show_duplicate_summary {
                text(format!(
                    "{} duplicate prompt names found: {}",
                    duplicate_count,
                    preview.duplicate_names.join(", ")
                ))
                .size(MODAL_BODY_TEXT_SIZE)
                .into()
            } else {
                Space::new().height(0).into()
            };
            let replace_warning: Element<'_, Message> = if controls.show_replace_warning {
                text("This will remove all current prompts before importing the backup.")
                    .size(MODAL_BODY_TEXT_SIZE)
                    .color(self.theme().extended_palette().danger.strong.color)
                    .into()
            } else {
                Space::new().height(0).into()
            };
            let duplicate_controls: Element<'_, Message> = if controls.show_duplicate_controls {
                row![
                    button(text("Skip duplicates"))
                        .on_press(Message::DuplicateModeSelected(DuplicateMode::Skip))
                        .padding(CONTROL_PADDING)
                        .style(
                            if matches!(self.pending_duplicate_mode, DuplicateMode::Skip) {
                                button::primary
                            } else {
                                button::secondary
                            }
                        ),
                    button(text("Overwrite duplicates"))
                        .on_press(Message::DuplicateModeSelected(DuplicateMode::Overwrite))
                        .padding(CONTROL_PADDING)
                        .style(
                            if matches!(self.pending_duplicate_mode, DuplicateMode::Overwrite) {
                                button::primary
                            } else {
                                button::secondary
                            }
                        ),
                ]
                .spacing(10)
                .into()
            } else {
                Space::new().height(0).into()
            };
            let import_mode_controls: Element<'_, Message> = if controls.show_import_mode_controls {
                row![
                    button(text("Merge"))
                        .on_press(Message::ImportModeSelected(ImportMode::Merge))
                        .padding(CONTROL_PADDING)
                        .style(if matches!(self.pending_import_mode, ImportMode::Merge) {
                            button::primary
                        } else {
                            button::secondary
                        }),
                    button(text("Replace all"))
                        .on_press(Message::ImportModeSelected(ImportMode::ReplaceAll))
                        .padding(CONTROL_PADDING)
                        .style(
                            if matches!(self.pending_import_mode, ImportMode::ReplaceAll) {
                                button::primary
                            } else {
                                button::secondary
                            }
                        ),
                ]
                .spacing(10)
                .into()
            } else {
                Space::new().height(0).into()
            };

            let confirm_label = if matches!(self.pending_import_mode, ImportMode::ReplaceAll) {
                "Replace prompts"
            } else {
                "Import prompts"
            };

            let card = container(
                column![
                    row![
                        text("Import prompts")
                            .size(MODAL_TITLE_TEXT_SIZE)
                            .width(Length::Fill),
                        button(icon::x())
                            .on_press(Message::ImportCancelPressed)
                            .style(button::text)
                            .padding(4),
                    ],
                    Space::new().height(15),
                    text(format!(
                        "{} prompts found in the backup",
                        preview.imported_count
                    ))
                    .size(MODAL_BODY_TEXT_SIZE),
                    duplicate_summary,
                    Space::new().height(MODAL_SECTION_SPACING),
                    import_mode_controls,
                    duplicate_controls,
                    replace_warning,
                    Space::new().height(20),
                    row![
                        button(text("Cancel"))
                            .on_press(Message::ImportCancelPressed)
                            .padding(CONTROL_PADDING)
                            .style(button::secondary),
                        button(text(confirm_label))
                            .on_press(Message::ImportConfirmPressed)
                            .padding(CONTROL_PADDING)
                            .style(button::primary),
                    ]
                    .spacing(10),
                ]
                .spacing(MODAL_SECTION_SPACING)
                .padding(20),
            )
            .width(modal_width_for_density(
                self.current_density(),
                MODAL_WIDTH_WIDE,
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
                    Space::new().height(15),
                    text(APP_NAME).size(EMPTY_STATE_TEXT_SIZE).font(Font {
                        weight: iced::font::Weight::Bold,
                        ..Default::default()
                    }),
                    text(format!("v{}", APP_VERSION)).size(MODAL_BODY_TEXT_SIZE),
                    Space::new().height(10),
                    text(APP_DESCRIPTION).size(MODAL_BODY_TEXT_SIZE),
                    Space::new().height(10),
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
        Space::new().width(width).into()
    }
}

fn image_handle_for_theme(theme: &Theme) -> iced::widget::image::Handle {
    if matches!(theme, Theme::Light) {
        (*APP_LOGO_LIGHT_HANDLE).clone()
    } else {
        (*APP_LOGO_DARK_HANDLE).clone()
    }
}

static APP_LOGO_DARK_HANDLE: LazyLock<iced::widget::image::Handle> =
    LazyLock::new(|| iced::widget::image::Handle::from_bytes(APP_LOGO_DARK_BYTES));
static APP_LOGO_LIGHT_HANDLE: LazyLock<iced::widget::image::Handle> =
    LazyLock::new(|| iced::widget::image::Handle::from_bytes(APP_LOGO_LIGHT_BYTES));

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
        text_input::Status::Focused { is_hovered: _ } => text_input::Style {
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
        pick_list::Status::Opened { is_hovered: _ } => pick_list::Style {
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

    #[test]
    fn native_clipboard_verification_requires_exact_prompt_content() {
        let expected = "JamePrompt test ñ 😀\nsecond line";

        assert!(clipboard_matches_expected_prompt(expected, Some(expected)));
        assert!(!clipboard_matches_expected_prompt(
            expected,
            Some("OLD_VALUE")
        ));
        assert!(!clipboard_matches_expected_prompt(expected, None));
    }

    #[test]
    fn prompt_editor_wraps_unbreakable_tokens_at_glyph_boundaries() {
        assert_eq!(
            prompt_editor_wrapping(),
            iced::advanced::text::Wrapping::WordOrGlyph
        );
    }

    #[test]
    fn prompt_card_title_wraps_long_segments_at_glyph_boundaries() {
        assert_eq!(
            prompt_card_title_wrapping(),
            iced::advanced::text::Wrapping::WordOrGlyph
        );
    }

    #[test]
    fn detail_toolbar_uses_semantic_action_styles() {
        let theme = Theme::Dark;
        let status = button::Status::Active;

        assert_eq!(
            detail_toolbar_button_style(DetailToolbarAction::Favorite { is_active: true })(
                &theme, status
            ),
            button::primary(&theme, status)
        );
        assert_eq!(
            detail_toolbar_button_style(DetailToolbarAction::Favorite { is_active: false })(
                &theme, status
            ),
            neutral_action_button_style(&theme, status)
        );
        assert_eq!(
            detail_toolbar_button_style(DetailToolbarAction::Copy)(&theme, status),
            neutral_action_button_style(&theme, status)
        );
        assert_eq!(
            detail_toolbar_button_style(DetailToolbarAction::Edit)(&theme, status),
            neutral_action_button_style(&theme, status)
        );
        assert_eq!(
            detail_toolbar_button_style(DetailToolbarAction::Delete)(&theme, status),
            button::danger(&theme, status)
        );
    }

    #[test]
    fn action_button_taxonomy_maps_each_role_to_its_theme_aware_style() {
        let theme = Theme::Dark;
        let status = button::Status::Active;

        assert_eq!(
            action_button_style(ActionButtonTone::Neutral)(&theme, status),
            neutral_action_button_style(&theme, status)
        );
        assert_eq!(
            action_button_style(ActionButtonTone::Primary)(&theme, status),
            button::primary(&theme, status)
        );
        assert_eq!(
            action_button_style(ActionButtonTone::Destructive)(&theme, status),
            button::danger(&theme, status)
        );
    }

    #[test]
    fn toolbar_neutral_actions_use_the_background_pair_for_each_theme() {
        for theme in [Theme::Dark, Theme::Light] {
            let pair = theme.extended_palette().background.weak;
            let style = neutral_action_button_style(&theme, button::Status::Active);

            assert_eq!(style.background, Some(iced::Background::Color(pair.color)));
            assert_eq!(style.text_color, pair.text);
        }
    }

    #[test]
    fn toolbar_neutral_actions_preserve_native_interaction_state_semantics() {
        let theme = Theme::Dark;
        let palette = theme.extended_palette();

        let hovered = neutral_action_button_style(&theme, button::Status::Hovered);
        assert_eq!(
            hovered.background,
            Some(iced::Background::Color(palette.background.neutral.color))
        );
        assert_eq!(hovered.text_color, palette.background.neutral.text);

        let pressed = neutral_action_button_style(&theme, button::Status::Pressed);
        assert_eq!(
            pressed.background,
            Some(iced::Background::Color(palette.background.weak.color))
        );
        assert_eq!(pressed.text_color, palette.background.weak.text);

        let disabled = neutral_action_button_style(&theme, button::Status::Disabled);
        assert_eq!(
            disabled.background,
            Some(iced::Background::Color(
                palette.background.weak.color.scale_alpha(0.5)
            ))
        );
        assert_eq!(
            disabled.text_color,
            palette.background.weak.text.scale_alpha(0.5)
        );
    }

    // Reproduction tests for bug: status ignored and theme mismatch
    #[test]
    fn repro_button_hover_state_is_ignored() {
        let active = button::primary(&Theme::Dark, button::Status::Active);
        let hovered = button::primary(&Theme::Dark, button::Status::Hovered);
        assert_ne!(
            active.background, hovered.background,
            "Bug: button hover state should change background color"
        );
    }

    #[test]
    fn primary_button_uses_official_palette_pairing() {
        let palette = Theme::Dark.extended_palette();
        let active = button::primary(&Theme::Dark, button::Status::Active);
        let hovered = button::primary(&Theme::Dark, button::Status::Hovered);

        assert_eq!(
            active.background,
            Some(iced::Background::Color(palette.primary.base.color))
        );
        assert_eq!(active.text_color, palette.primary.base.text);
        assert_eq!(
            hovered.background,
            Some(iced::Background::Color(palette.primary.strong.color))
        );
        assert_eq!(hovered.text_color, palette.primary.base.text);
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
    fn test_image_handle_identity_is_stable_per_theme() {
        let dark_first = image_handle_for_theme(&Theme::Dark);
        let dark_second = image_handle_for_theme(&Theme::Dark);
        let light_first = image_handle_for_theme(&Theme::Light);
        let light_second = image_handle_for_theme(&Theme::Light);

        assert_eq!(dark_first.id(), dark_second.id());
        assert_eq!(light_first.id(), light_second.id());
        assert_ne!(dark_first.id(), light_first.id());
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
        const {
            assert!(PROMPT_PRIMARY_TOOLBAR_SPACING > 0);
        }
        const {
            assert!(PROMPT_SECONDARY_TOOLBAR_SPACING > 0);
        }
        const {
            assert!(PROMPT_PRIMARY_TOOLBAR_SPACING >= PROMPT_SECONDARY_TOOLBAR_SPACING);
        }
    }

    #[test]
    fn test_prompt_toolbar_picker_widths_are_stable() {
        const {
            assert!(PROMPT_FILTER_PICKER_WIDTH >= 100.0);
        }
        const {
            assert!(PROMPT_SORT_PICKER_WIDTH >= 150.0);
        }
        const {
            assert!(PROMPT_SORT_PICKER_WIDTH > PROMPT_FILTER_PICKER_WIDTH);
        }
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
        const {
            assert!(PROMPT_LIST_FAVORITE_INDICATOR_WIDTH >= 20.0);
        }
        const {
            assert!(PROMPT_LIST_FAVORITE_INDICATOR_WIDTH <= 32.0);
        }
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

    // Group D: native primary button
    #[test]
    fn test_primary_button_style_has_background() {
        let style = button::primary(&Theme::Dark, button::Status::Active);
        let expected = Theme::Dark.extended_palette().primary.base.color;
        assert_eq!(style.background, Some(iced::Background::Color(expected)));
    }

    #[test]
    fn test_primary_button_style_has_text_color() {
        let style = button::primary(&Theme::Dark, button::Status::Active);
        let expected = Theme::Dark.extended_palette().primary.base.text;
        assert_eq!(style.text_color, expected);
    }

    #[test]
    fn test_primary_button_hover_uses_strong_background_and_base_text() {
        let style = button::primary(&Theme::Dark, button::Status::Hovered);
        let palette = Theme::Dark.extended_palette();
        assert_eq!(
            style.background,
            Some(iced::Background::Color(palette.primary.strong.color))
        );
        assert_eq!(style.text_color, palette.primary.base.text);
    }

    #[test]
    fn test_primary_button_disabled_scales_the_native_primary_pair() {
        let palette = Theme::Dark.extended_palette();
        let style = button::primary(&Theme::Dark, button::Status::Disabled);
        assert_eq!(
            style.background,
            Some(iced::Background::Color(
                palette.primary.base.color.scale_alpha(0.5)
            ))
        );
        assert_eq!(style.text_color, palette.primary.base.text.scale_alpha(0.5));
    }

    #[test]
    fn test_primary_button_style_hovered_differs_from_active() {
        let active = button::primary(&Theme::Dark, button::Status::Active);
        let hovered = button::primary(&Theme::Dark, button::Status::Hovered);
        assert_ne!(active.background, hovered.background);
    }

    #[test]
    fn test_primary_button_style_pressed_matches_active() {
        let active = button::primary(&Theme::Dark, button::Status::Active);
        let pressed = button::primary(&Theme::Dark, button::Status::Pressed);
        assert_eq!(active.background, pressed.background);
        assert_eq!(active.text_color, pressed.text_color);
    }

    #[test]
    fn test_primary_button_style_disabled_differs_from_active() {
        let active = button::primary(&Theme::Dark, button::Status::Active);
        let disabled = button::primary(&Theme::Dark, button::Status::Disabled);
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

        assert_eq!(
            app.form_error_message(Field::Hotkey).as_deref(),
            Some("Enter a valid hotkey combination"),
            "Invalid hotkey should be rejected next to the hotkey field"
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
    fn form_save_shows_missing_name_and_content_next_to_each_field() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        let mut app = JamePromptApp::with_database(db);
        app.new_form = Some(NewPromptForm::default());

        let _ = app.update(Message::FormSave);

        assert_eq!(
            app.form_error_message(Field::Name).as_deref(),
            Some("Enter a prompt name")
        );
        assert_eq!(
            app.form_error_message(Field::Content).as_deref(),
            Some("Enter prompt content")
        );
        assert!(app.status_message.is_empty());
        assert!(app.new_form.is_some());
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
        let focused = search_input_style(
            &Theme::Dark,
            text_input::Status::Focused { is_hovered: false },
        );
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
        let opened = pick_list_style(
            &Theme::Dark,
            pick_list::Status::Opened { is_hovered: false },
        );
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
        assert!(app.status_message.is_empty());
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
    fn window_lifecycle_coalesces_repeated_open_requests() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let _ = app.request_show();
        assert!(app.window_open_pending);
        // Repeated show while opening carries no new generation.
        let generation = app.window_state.generation;
        let _ = app.request_show();
        assert_eq!(app.window_state.generation, generation);

        let id = iced::window::Id::unique();
        let _ = app.note_window_opened(generation, id);
        assert!(!app.window_open_pending);

        let _ = app.note_window_closed(id);
        assert!(app.window_state.phase == WindowPhase::Absent);
    }

    #[test]
    fn repeated_show_while_opening_coalesces_to_one_generation() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());

        let _ = app.request_show();
        let generation = app.window_state.generation;
        assert_eq!(generation, 1);

        let _ = app.request_show();
        let _ = app.request_show();

        assert_eq!(app.window_state.generation, generation);
        assert_eq!(app.window_state.phase, WindowPhase::Opening);
    }

    #[test]
    fn stale_opened_completion_does_not_assign_id() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let _ = app.request_show();

        let stale = iced::window::Id::unique();
        let _ = app.note_window_opened(99, stale);

        assert_eq!(app.window_state.id, None);
        assert_eq!(app.main_window_id, None);
        assert!(app.window_open_pending);
    }

    #[test]
    fn show_during_closing_reopens_once_on_closed_event() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let _ = app.request_show();
        let id = iced::window::Id::unique();
        let _ = app.note_window_opened(1, id);
        let _ = app.request_close(id);
        assert_eq!(app.window_state.phase, WindowPhase::Closing);

        let _ = app.request_show();
        let _ = app.note_window_closed(id);

        assert_eq!(app.window_state.phase, WindowPhase::Opening);
        assert_eq!(app.window_state.generation, 2);
    }

    #[test]
    fn desktop_closed_event_is_authoritative_destruction() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let _ = app.request_show();
        let id = iced::window::Id::unique();
        let _ = app.note_window_opened(1, id);

        let _ = app.handle_desktop_window_event(id, iced::window::Event::Closed);

        assert_eq!(app.window_state.phase, WindowPhase::Absent);
        assert_eq!(app.main_window_id, None);
    }

    #[test]
    fn desktop_focus_events_track_observed_focus_only() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let id = iced::window::Id::unique();

        let _ = app.handle_desktop_window_event(id, iced::window::Event::Focused);
        assert!(app.window_focused);
        let _ = app.handle_desktop_window_event(id, iced::window::Event::Unfocused);
        assert!(!app.window_focused);
    }

    #[test]
    fn closing_stale_window_does_not_clear_current_window() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let current = iced::window::Id::unique();
        let stale = iced::window::Id::unique();
        let _ = app.request_show();
        let generation = app.window_state.generation;
        let _ = app.note_window_opened(generation, current);

        let _ = app.note_window_closed(stale);

        assert_eq!(app.main_window_id, Some(current));
        assert!(!app.window_open_pending);
    }

    #[test]
    fn reconcile_bumps_revision_once_for_same_effective_set() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());

        app.reconcile_shortcuts_after_commit();
        assert_eq!(app.desired_revision, 1);
        app.reconcile_shortcuts_after_commit();
        assert_eq!(
            app.desired_revision, 1,
            "identical snapshots must not bump the revision or storm consent"
        );
    }

    #[test]
    fn reconcile_marks_pending_and_clears_removed_prompts() {
        let db = Database::in_memory().expect("in-memory database");
        let id = db
            .insert(&Prompt {
                id: String::new(),
                name: "Pending".to_string(),
                content: "Content".to_string(),
                hotkey: Some("Ctrl+Alt+F8".to_string()),
                hotkey_enabled: true,
                favorite: false,
                created_at: String::new(),
                updated_at: String::new(),
                last_used_at: None,
                use_count: 0,
            })
            .expect("insert prompt with hotkey");
        let mut app = JamePromptApp::with_database(db);

        app.reconcile_shortcuts_after_commit();
        assert_eq!(app.desired_revision, 1);
        let ack = app.shortcut_states.get(&id).expect("pending state");
        assert_eq!(ack.status, ShortcutStatus::Pending);
        assert_eq!(ack.revision, 1);

        // Successful delete removes the state and bumps the revision;
        // the backend is reconciled only after the commit.
        let _ = app.update(Message::DeleteConfirmPressed(id.clone()));
        assert_eq!(app.desired_revision, 2);
        assert!(
            !app.shortcut_states.contains_key(&id),
            "deleted prompts keep no stale shortcut state"
        );
    }

    #[test]
    fn stale_and_unknown_acknowledgements_are_rejected() {
        let db = Database::in_memory().expect("in-memory database");
        let id = db
            .insert(&Prompt {
                id: String::new(),
                name: "Acked".to_string(),
                content: "Content".to_string(),
                hotkey: Some("Ctrl+Alt+F8".to_string()),
                hotkey_enabled: true,
                favorite: false,
                created_at: String::new(),
                updated_at: String::new(),
                last_used_at: None,
                use_count: 0,
            })
            .expect("insert prompt with hotkey");
        let mut app = JamePromptApp::with_database(db);
        app.reconcile_shortcuts_after_commit();

        // Stale revision: rejected without mutation.
        assert!(
            !app.apply_shortcut_acknowledgement(ShortcutAcknowledgement {
                revision: 99,
                prompt_id: id.clone(),
                runtime_id: Some(7),
                status: ShortcutStatus::Active,
                trigger_description: None,
                error_code: None,
            })
        );
        assert_eq!(app.shortcut_states[&id].status, ShortcutStatus::Pending);

        // Unknown prompt claiming Active: rejected.
        assert!(
            !app.apply_shortcut_acknowledgement(ShortcutAcknowledgement {
                revision: 1,
                prompt_id: "deleted-prompt".into(),
                runtime_id: Some(7),
                status: ShortcutStatus::Active,
                trigger_description: None,
                error_code: None,
            })
        );
        assert!(!app.shortcut_states.contains_key("deleted-prompt"));

        // Current revision for a desired prompt: applied.
        assert!(app.apply_shortcut_acknowledgement(ShortcutAcknowledgement {
            revision: 1,
            prompt_id: id.clone(),
            runtime_id: Some(7),
            status: ShortcutStatus::Active,
            trigger_description: Some("CTRL+ALT+F8".into()),
            error_code: None,
        }));
        let ack = &app.shortcut_states[&id];
        assert_eq!(ack.status, ShortcutStatus::Active);
        assert_eq!(ack.runtime_id, Some(7));
        assert_eq!(app.runtime_to_prompt(7), Some(id));
    }

    #[test]
    fn smoke_mode_disables_tray_integration() {
        let (app, _task) = JamePromptApp::new_with_hidden_start(false, true);

        assert!(app.smoke_mode);
        assert!(app.smoke_deadline.is_some());
        #[cfg(target_os = "windows")]
        assert!(app.tray.is_none());
        assert_eq!(app.tray_state, TrayState::Starting);
        assert!(app.hotkey_service.is_none());
        assert!(app.all_prompts.is_empty());
    }

    #[test]
    fn tray_ready_signal_marks_tray_ready() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());

        let _ = app.handle_tray_signal(TraySignal::Ready);

        assert_eq!(app.tray_state, TrayState::Ready);
    }

    #[test]
    fn tray_unavailable_while_windowless_requests_fallback() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        assert!(app.should_request_tray_fallback());

        let _ = app.handle_tray_signal(TraySignal::Unavailable(
            crate::desktop::DesktopErrorCode::TrayHostMissing,
        ));

        assert_eq!(app.tray_state, TrayState::Unavailable);
        assert!(app.should_request_tray_fallback());
    }

    #[test]
    fn tray_host_offline_with_open_window_requests_no_fallback() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let id = iced::window::Id::unique();
        app.main_window_id = Some(id);

        let _ = app.handle_tray_signal(TraySignal::HostOffline);

        assert_eq!(app.tray_state, TrayState::HostOffline);
        assert!(!app.should_request_tray_fallback());
    }

    #[test]
    fn tray_host_online_recovers_to_ready() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let _ = app.handle_tray_signal(TraySignal::HostOffline);
        assert_eq!(app.tray_state, TrayState::HostOffline);

        let _ = app.handle_tray_signal(TraySignal::HostOnline);

        assert_eq!(app.tray_state, TrayState::Ready);
    }

    #[test]
    fn left_prompt_list_empty_state_matches_detail_placeholder_on_initial_state() {
        let app = JamePromptApp::with_database(Database::in_memory().unwrap());

        assert_eq!(
            app.prompt_list_empty_message(),
            "Create or import a prompt to get started"
        );
    }

    #[test]
    fn detail_empty_state_invites_creation_when_no_prompts_exist() {
        let app = JamePromptApp::with_database(Database::in_memory().unwrap());

        assert_eq!(
            app.prompt_detail_empty_message(),
            "Create or import a prompt to get started"
        );
    }

    #[test]
    fn detail_empty_state_requests_selection_when_prompts_exist() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Alpha".to_string(),
            content: "Content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        })
        .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);
        app.selected_id = None;

        assert_eq!(
            app.prompt_detail_empty_message(),
            "Select a prompt from the list"
        );
    }

    #[test]
    fn left_prompt_list_empty_state_mentions_filters_when_prompts_exist() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Alpha".to_string(),
            content: "Content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        })
        .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);
        let _ = app.update(Message::SearchChanged("missing".to_string()));

        assert_eq!(
            app.prompt_list_empty_message(),
            "No prompts match your filters"
        );
    }

    #[test]
    fn import_review_hides_replace_and_duplicate_controls_for_empty_database() {
        let app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let controls = app.import_review_controls(&PromptImportPreview {
            imported_count: 1,
            duplicate_names: Vec::new(),
        });

        assert!(!controls.show_import_mode_controls);
        assert!(!controls.show_duplicate_summary);
        assert!(!controls.show_duplicate_controls);
        assert!(!controls.show_replace_warning);
    }

    #[test]
    fn import_review_shows_replace_choice_when_existing_prompts_exist() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Existing".to_string(),
            content: "Content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        })
        .expect("Insert should succeed");
        let app = JamePromptApp::with_database(db);
        let controls = app.import_review_controls(&PromptImportPreview {
            imported_count: 1,
            duplicate_names: Vec::new(),
        });

        assert!(controls.show_import_mode_controls);
        assert!(!controls.show_duplicate_summary);
        assert!(!controls.show_duplicate_controls);
        assert!(!controls.show_replace_warning);
    }

    #[test]
    fn import_review_shows_duplicate_controls_only_for_merge_with_duplicates() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Existing".to_string(),
            content: "Content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        })
        .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);
        let preview = PromptImportPreview {
            imported_count: 1,
            duplicate_names: vec!["Existing".to_string()],
        };

        app.pending_import_mode = ImportMode::Merge;
        let merge_controls = app.import_review_controls(&preview);
        assert!(merge_controls.show_duplicate_summary);
        assert!(merge_controls.show_duplicate_controls);
        assert!(!merge_controls.show_replace_warning);

        app.pending_import_mode = ImportMode::ReplaceAll;
        let replace_controls = app.import_review_controls(&preview);
        assert!(replace_controls.show_duplicate_summary);
        assert!(!replace_controls.show_duplicate_controls);
        assert!(replace_controls.show_replace_warning);
    }

    #[test]
    fn replace_all_import_mode_is_ignored_when_database_is_empty() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());

        let _ = app.update(Message::ImportModeSelected(ImportMode::ReplaceAll));

        assert!(matches!(app.pending_import_mode, ImportMode::Merge));
        assert!(app.status_message.is_empty());
    }

    #[test]
    fn import_cancel_preserves_existing_prompts() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Existing".to_string(),
            content: "Content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        })
        .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);
        app.pending_import_backup = Some(PromptBackup::new(
            "1.1.0",
            vec![Prompt {
                id: String::new(),
                name: "Imported".to_string(),
                content: "Imported content".to_string(),
                hotkey: None,
                hotkey_enabled: true,
                favorite: false,
                created_at: String::new(),
                updated_at: String::new(),
                last_used_at: None,
                use_count: 0,
            }],
        ));
        app.pending_import_preview = Some(PromptImportPreview {
            imported_count: 1,
            duplicate_names: Vec::new(),
        });

        let _ = app.update(Message::ImportCancelPressed);

        assert!(app.pending_import_backup.is_none());
        assert_eq!(app.all_prompts.len(), 1);
        assert_eq!(app.all_prompts[0].name, "Existing");
    }

    #[test]
    fn import_confirm_merge_refreshes_prompt_cache() {
        let db = Database::in_memory().expect("Failed to create in-memory database");
        db.insert(&Prompt {
            id: String::new(),
            name: "Existing".to_string(),
            content: "Content".to_string(),
            hotkey: None,
            hotkey_enabled: true,
            favorite: false,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            use_count: 0,
        })
        .expect("Insert should succeed");
        let mut app = JamePromptApp::with_database(db);
        app.pending_import_backup = Some(PromptBackup::new(
            "1.1.0",
            vec![Prompt {
                id: "99999999-9999-4999-8999-999999999999".to_string(),
                name: "Imported".to_string(),
                content: "Imported content".to_string(),
                hotkey: None,
                hotkey_enabled: true,
                favorite: true,
                created_at: "2026-05-11 10:00:00".to_string(),
                updated_at: "2026-05-11 10:30:00".to_string(),
                last_used_at: None,
                use_count: 3,
            }],
        ));
        app.pending_import_preview = Some(PromptImportPreview {
            imported_count: 1,
            duplicate_names: Vec::new(),
        });

        let _ = app.update(Message::ImportConfirmPressed);

        assert_eq!(app.all_prompts.len(), 2);
        let imported = app
            .prompt_cloned_by_id("99999999-9999-4999-8999-999999999999")
            .expect("Imported prompt should be cached");
        assert_eq!(imported.name, "Imported");
        assert!(imported.favorite);
        assert_eq!(imported.use_count, 3);
    }

    #[test]
    fn paste_permission_warning_survives_search_and_can_be_dismissed() {
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        app.show_notification(NotificationEvent::PastePermissionDenied);
        let warning = app
            .notifications
            .active()
            .expect("permission warning should be active");
        let warning_id = warning.id();
        assert_eq!(warning.notification().severity(), Severity::Warning);
        assert_eq!(
            warning.notification().presentation(),
            Presentation::MessageBar
        );
        assert!(warning.is_persistent());

        let warning_copy = app.notification_message(warning.notification());
        assert_eq!(
            warning_copy,
            "Automatic paste needs Remote Desktop and Clipboard permission. Grant it in your system settings, then try the shortcut again."
        );

        let _ = app.update(Message::SearchChanged("private search term".to_string()));
        assert_eq!(
            app.notifications.active().map(|record| record.id()),
            Some(warning_id)
        );

        let _ = app.update(Message::NotificationDismissed(warning_id));
        let search = app
            .notifications
            .active()
            .expect("search status should remain active after dismissal");
        assert_eq!(search.notification().message(), MessageKey::SearchCompleted);
        assert_eq!(search.notification().presentation(), Presentation::Status);
        assert_eq!(app.status_message, "0 prompts found");
    }

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    #[test]
    fn controller_create_validation_failure_does_not_report_previous_selection_as_success() {
        use crate::desktop_test_support::{
            ControllerCommand, ControllerOperation, ControllerResponse, PROTOCOL_VERSION,
        };
        use futures_channel::oneshot;
        use std::sync::{Arc, Mutex};

        let db = Database::in_memory().expect("in-memory database");
        let existing = db
            .insert(&Prompt {
                id: String::new(),
                name: "Existing".to_string(),
                content: "Existing content".to_string(),
                ..Prompt::default()
            })
            .expect("existing prompt");
        let mut app = JamePromptApp::with_database(db);
        app.selected_id = Some(existing.clone());

        let (reply_tx, mut reply_rx) = oneshot::channel::<ControllerResponse>();
        let command = ControllerCommand {
            request_id: 1,
            operation: ControllerOperation::CreatePrompt {
                name: String::new(),
                content: String::new(),
                hotkey: None,
                hotkey_enabled: None,
            },
            reply: Arc::new(Mutex::new(Some(reply_tx))),
        };
        let _ = app.apply_controller_command(command);
        let response = reply_rx
            .try_recv()
            .expect("controller must reply")
            .expect("reply sender must deliver");

        assert_eq!(response.version, PROTOCOL_VERSION);
        assert!(!response.ok, "invalid form must not reuse old selection");
        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("operation_failed")
        );
        assert_eq!(app.selected_id.as_deref(), Some(existing.as_str()));
    }

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    #[test]
    fn controller_edit_validation_failure_does_not_report_existing_row_as_success() {
        use crate::desktop_test_support::{
            ControllerCommand, ControllerOperation, ControllerResponse, PROTOCOL_VERSION,
        };
        use futures_channel::oneshot;
        use std::sync::{Arc, Mutex};

        let db = Database::in_memory().expect("in-memory database");
        let id = db
            .insert(&Prompt {
                id: String::new(),
                name: "Existing".to_string(),
                content: "Existing content".to_string(),
                ..Prompt::default()
            })
            .expect("existing prompt");
        let mut app = JamePromptApp::with_database(db);

        let (reply_tx, mut reply_rx) = oneshot::channel::<ControllerResponse>();
        let command = ControllerCommand {
            request_id: 2,
            operation: ControllerOperation::EditPrompt {
                id: id.clone(),
                name: Some(String::new()),
                content: None,
                hotkey: None,
                hotkey_enabled: None,
            },
            reply: Arc::new(Mutex::new(Some(reply_tx))),
        };
        let _ = app.apply_controller_command(command);
        let response = reply_rx
            .try_recv()
            .expect("controller must reply")
            .expect("reply sender must deliver");

        assert_eq!(response.version, PROTOCOL_VERSION);
        assert!(
            !response.ok,
            "invalid edit must not report row existence as success"
        );
        assert_eq!(
            app.prompt_cloned_by_id(&id).expect("row remains").name,
            "Existing"
        );
    }

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    #[test]
    fn controller_import_unknown_mode_is_rejected() {
        use crate::desktop_test_support::{
            ControllerCommand, ControllerOperation, ControllerResponse,
        };
        use futures_channel::oneshot;
        use std::sync::{Arc, Mutex};

        let backup = crate::prompt_backup::PromptBackup::new(
            "1.2.0-beta.10",
            vec![Prompt {
                id: "99999999-9999-4999-8999-999999999999".to_string(),
                name: "Imported".to_string(),
                content: "Imported content".to_string(),
                ..Prompt::default()
            }],
        );
        let backup = serde_json::to_value(backup).expect("backup serializes");
        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let (reply_tx, mut reply_rx) = oneshot::channel::<ControllerResponse>();
        let command = ControllerCommand {
            request_id: 3,
            operation: ControllerOperation::ImportFixture {
                backup,
                mode: Some("unknown".to_string()),
            },
            reply: Arc::new(Mutex::new(Some(reply_tx))),
        };
        let _ = app.apply_controller_command(command);
        let response = reply_rx
            .try_recv()
            .expect("controller must reply")
            .expect("reply sender must deliver");

        assert!(!response.ok, "unknown import mode must be rejected");
        assert!(app.pending_import_backup.is_none());
    }

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    #[test]
    fn controller_import_failure_returns_error_and_keeps_revision() {
        use crate::desktop_test_support::{
            ControllerCommand, ControllerOperation, ControllerResponse,
        };
        use futures_channel::oneshot;
        use std::sync::{Arc, Mutex};

        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        app.desired_revision = 7;
        let (reply_tx, mut reply_rx) = oneshot::channel::<ControllerResponse>();
        let command = ControllerCommand {
            request_id: 4,
            operation: ControllerOperation::ImportFixture {
                backup: serde_json::json!({"schema_version": 999}),
                mode: Some("merge".to_string()),
            },
            reply: Arc::new(Mutex::new(Some(reply_tx))),
        };

        let _ = app.apply_controller_command(command);
        let response = reply_rx
            .try_recv()
            .expect("controller must reply")
            .expect("reply sender must deliver");

        assert!(!response.ok, "invalid import must report failure");
        assert!(response.result.is_none());
        assert_eq!(app.desired_revision, 7);
        assert!(app.pending_import_backup.is_none());
    }

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    #[test]
    fn controller_dispatch_is_not_terminal_success() {
        use crate::desktop_test_support::{
            ControllerCommand, ControllerOperation, ControllerResponse,
        };
        use futures_channel::oneshot;
        use std::sync::{Arc, Mutex};

        let mut app = JamePromptApp::with_database(Database::in_memory().unwrap());
        let (reply_tx, mut reply_rx) = oneshot::channel::<ControllerResponse>();
        let command = ControllerCommand {
            request_id: 5,
            operation: ControllerOperation::OpenImportDialog {},
            reply: Arc::new(Mutex::new(Some(reply_tx))),
        };

        let _ = app.apply_controller_command(command);
        let response = reply_rx
            .try_recv()
            .expect("controller must reply")
            .expect("reply sender must deliver");

        assert!(response.ok);
        assert_eq!(
            response.result,
            Some(serde_json::json!({"dispatched": true}))
        );
        assert!(response.error.is_none());
    }
}
