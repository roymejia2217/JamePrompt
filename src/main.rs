#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod autostart;
mod config;
pub mod db;
mod hotkeys;
mod icon;
mod launch;
pub mod migrations;
mod models;
mod perf;
mod perf_smoke;
mod platform;
mod prompt_backup;
pub mod prompt_repository;
pub mod prompt_service;
pub mod settings_service;
mod tray;
mod ui;
mod window_lifecycle;

// Include pre-decoded RGBA icon data (generated at compile time by buildtime_png)
include!(concat!(env!("OUT_DIR"), "/image.rs"));

use iced::{window, Element, Task, Theme};
use launch::{should_run_ui_smoke_from_args, should_start_minimized};
use perf::measure;
use ui::{JamePromptApp, Message};
use window_lifecycle::WindowLifecycleAction;

fn title_application(app: &JamePromptApp, _window_id: window::Id) -> String {
    app.title()
}

fn update_application(app: &mut JamePromptApp, message: Message) -> Task<Message> {
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

fn view_application(app: &JamePromptApp, _window_id: window::Id) -> Element<'_, Message> {
    app.view()
}

fn theme_application(app: &JamePromptApp, _window_id: window::Id) -> Theme {
    app.theme()
}

fn initial_window_task(app: &mut JamePromptApp, start_minimized: bool) -> Task<Message> {
    if start_minimized || !app.begin_window_open() {
        Task::none()
    } else {
        window_lifecycle::open().map(|id| Message::ShowWindow(Some(id)))
    }
}

fn main() -> iced::Result {
    if perf_smoke::should_run() {
        perf_smoke::run();
        return Ok(());
    }

    let ui_smoke = measure("startup.parse_ui_smoke", || {
        should_run_ui_smoke_from_args(std::env::args_os())
    });

    let _ = measure("startup.tracing_init", || {
        tracing_subscriber::fmt::try_init()
    });
    let start_minimized = measure("startup.parse_args", should_start_minimized);

    #[cfg(target_os = "linux")]
    measure("startup.gtk_init", || {
        if let Err(error) = gtk::init() {
            tracing::warn!("GTK initialization failed; system tray may be unavailable: {error}");
        }
    });

    iced::daemon(title_application, update_application, view_application)
        .theme(theme_application)
        .font(icon::FONT)
        .subscription(JamePromptApp::subscription)
        .run_with(move || {
            measure("startup.app_state", || {
                let (mut app, startup_task) =
                    JamePromptApp::new_with_hidden_start(start_minimized, ui_smoke);
                let window_task = initial_window_task(&mut app, start_minimized);
                (app, Task::batch([startup_task, window_task]))
            })
        })
}

#[cfg(test)]
mod tests {
    use crate::launch::{
        initial_window_visible, should_run_perf_smoke_from_args, should_run_ui_smoke_from_args,
        should_show_window_after_hidden_start, should_start_minimized_from_args,
    };
    use std::ffi::OsString;

    #[test]
    fn should_start_minimized_from_args_returns_true_for_start_minimized() {
        let args = [
            OsString::from("jame-prompt"),
            OsString::from("--start-minimized"),
        ];

        assert!(should_start_minimized_from_args(args));
    }

    #[test]
    fn should_start_minimized_from_args_returns_false_without_start_minimized() {
        let args = [OsString::from("jame-prompt")];

        assert!(!should_start_minimized_from_args(args));
    }

    #[test]
    fn should_start_minimized_from_args_ignores_unknown_args() {
        let args = [OsString::from("jame-prompt"), OsString::from("--unknown")];

        assert!(!should_start_minimized_from_args(args));
    }

    #[test]
    fn should_run_perf_smoke_from_args_returns_true_for_perf_smoke() {
        let args = [
            OsString::from("jame-prompt"),
            OsString::from("--perf-smoke"),
        ];

        assert!(should_run_perf_smoke_from_args(args));
    }

    #[test]
    fn should_run_perf_smoke_from_args_returns_false_without_perf_smoke() {
        let args = [OsString::from("jame-prompt")];

        assert!(!should_run_perf_smoke_from_args(args));
    }

    #[test]
    fn should_run_ui_smoke_from_args_returns_true_for_ui_smoke() {
        let args = [OsString::from("jame-prompt"), OsString::from("--ui-smoke")];

        assert!(should_run_ui_smoke_from_args(args));
    }

    #[test]
    fn should_run_ui_smoke_from_args_returns_false_without_ui_smoke() {
        let args = [OsString::from("jame-prompt")];

        assert!(!should_run_ui_smoke_from_args(args));
    }

    #[test]
    fn initial_window_visibility_is_hidden_for_start_minimized() {
        assert!(!initial_window_visible(true));
    }

    #[test]
    fn initial_window_visibility_is_visible_for_manual_launch() {
        assert!(initial_window_visible(false));
    }

    #[test]
    fn hidden_start_fallback_shows_window_when_tray_is_unavailable() {
        assert!(should_show_window_after_hidden_start(true, false));
    }

    #[test]
    fn hidden_start_fallback_keeps_window_hidden_when_tray_is_available() {
        assert!(!should_show_window_after_hidden_start(true, true));
    }

    #[test]
    fn hidden_start_fallback_does_not_affect_manual_launch() {
        assert!(!should_show_window_after_hidden_start(false, false));
    }
}
