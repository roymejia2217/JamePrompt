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
mod prompt_backup;
pub mod prompt_repository;
pub mod prompt_service;
pub mod settings_service;
mod tray;
mod ui;

// Include pre-decoded RGBA icon data (generated at compile time by buildtime_png)
include!(concat!(env!("OUT_DIR"), "/image.rs"));

use config::{
    APP_ID, WINDOW_INITIAL_HEIGHT, WINDOW_INITIAL_WIDTH, WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH,
};
use iced::{window, Size};
use launch::{initial_window_visible, should_run_ui_smoke_from_args, should_start_minimized};
use perf::measure;
use ui::JamePromptApp;

// Embed the app icon directly in the binary (for window icon, not tray)
const APP_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/app_icon.png"
));

fn window_settings_for_start(
    start_minimized: bool,
    icon: Option<window::Icon>,
) -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(WINDOW_INITIAL_WIDTH, WINDOW_INITIAL_HEIGHT),
        min_size: Some(Size::new(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT)),
        icon,
        visible: initial_window_visible(start_minimized),
        exit_on_close_request: false,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = APP_ID.into();
    }

    settings
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

    // Initialize GTK for tray icon on Linux
    #[cfg(target_os = "linux")]
    measure("startup.gtk_init", || {
        if let Err(error) = gtk::init() {
            tracing::warn!("GTK initialization failed; system tray may be unavailable: {error}");
        }
    });

    // Load icon using the correct Iced 0.13.1 API
    let icon = measure("startup.window_icon_load", || {
        iced::window::icon::from_file_data(
            APP_ICON_PNG,
            None, // auto-detect format
        )
        .expect("Failed to load app icon from embedded PNG")
    });

    let window_settings = window_settings_for_start(start_minimized, Some(icon));

    iced::application(
        JamePromptApp::title,
        JamePromptApp::update,
        JamePromptApp::view,
    )
    .theme(JamePromptApp::theme)
    .font(icon::FONT)
    .subscription(JamePromptApp::subscription)
    .window(window_settings)
    .centered()
    .run_with(move || {
        measure("startup.app_state", || {
            JamePromptApp::new_with_hidden_start(start_minimized, ui_smoke)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::{
        should_run_perf_smoke_from_args, should_run_ui_smoke_from_args,
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
    fn window_settings_has_initial_size() {
        let settings = window_settings_for_start(false, None);

        assert_eq!(
            settings.size,
            Size::new(config::WINDOW_INITIAL_WIDTH, config::WINDOW_INITIAL_HEIGHT)
        );
    }

    #[test]
    fn window_settings_has_minimum_size() {
        let settings = window_settings_for_start(false, None);

        assert_eq!(
            settings.min_size,
            Some(Size::new(
                config::WINDOW_MIN_WIDTH,
                config::WINDOW_MIN_HEIGHT
            ))
        );
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
