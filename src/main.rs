#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod application;
mod autostart;
mod config;
pub mod db;
pub mod desktop;
#[cfg(target_os = "linux")]
mod desktop_accessibility;
mod desktop_diagnostics;
#[cfg(target_os = "linux")]
mod desktop_parent;
#[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
mod desktop_test_support;
pub mod domain;
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
use launch::{
    should_run_native_hotkey_smoke_from_args, should_run_ui_smoke_from_args, should_start_minimized,
};
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
        WindowLifecycleAction::Close(id) => app.request_close(id),
        WindowLifecycleAction::Open => app.request_show(),
        WindowLifecycleAction::Restore(id) => app.present_window(id),
    }
}

fn view_application(app: &JamePromptApp, _window_id: window::Id) -> Element<'_, Message> {
    app.view()
}

fn theme_application(app: &JamePromptApp, _window_id: window::Id) -> Theme {
    app.theme()
}

fn initial_window_task(app: &mut JamePromptApp, start_minimized: bool) -> Task<Message> {
    if start_minimized {
        Task::none()
    } else {
        // Canonical show intent: Absent opens one window for a new
        // generation; any other phase coalesces without duplicates.
        app.request_show()
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
    let native_hotkey_smoke = measure("startup.parse_native_hotkey_smoke", || {
        should_run_native_hotkey_smoke_from_args(std::env::args_os())
    });

    let _ = measure("startup.tracing_init", || {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(
                "jame_prompt=info,iced_winit=warn,iced_wgpu=warn,wgpu_hal=warn,sctk_adwaita=warn",
            )
        });
        tracing_subscriber::fmt().with_env_filter(filter).try_init()
    });
    let start_minimized = measure("startup.parse_args", should_start_minimized);

    // No main-thread GTK initialization: GIO/GLib portal work does not
    // require GTK widgets, and rfd owns its separate GTK thread. The Linux
    // tray no longer pumps GTK events (ksni service instead).

    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    let desktop_test =
        measure(
            "startup.parse_test_mode",
            || match launch::desktop_test_mode_from_args(std::env::args_os(), true) {
                Ok(mode) => mode,
                Err(error) => {
                    eprintln!("invalid desktop test invocation: {error:?}");
                    std::process::exit(2);
                }
            },
        );
    #[cfg(not(all(target_os = "linux", feature = "desktop-integration-tests")))]
    let desktop_test =
        measure(
            "startup.parse_test_mode",
            || match launch::desktop_test_mode_from_args(std::env::args_os(), false) {
                Ok(mode) => mode,
                Err(_) => {
                    eprintln!(
                        "desktop test switches require the desktop-integration-tests feature"
                    );
                    std::process::exit(2);
                }
            },
        );
    // Feature builds reject the test switches before any production state
    // when the feature is off; test mode below never touches production
    // paths, autostart or permission prewarm.
    #[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
    if let Some(mode) = desktop_test {
        return run_desktop_test(mode);
    }
    #[cfg(not(all(target_os = "linux", feature = "desktop-integration-tests")))]
    let _ = desktop_test;

    iced::daemon(
        move || {
            measure("startup.app_state", || {
                let (mut app, startup_task) = if native_hotkey_smoke {
                    JamePromptApp::new_native_hotkey_smoke(start_minimized)
                } else {
                    JamePromptApp::new_with_hidden_start(start_minimized, ui_smoke)
                };
                let window_task = initial_window_task(&mut app, start_minimized);
                (app, Task::batch([startup_task, window_task]))
            })
        },
        update_application,
        view_application,
    )
    .title(title_application)
    .theme(theme_application)
    .font(icon::FONT)
    .subscription(JamePromptApp::subscription)
    .run()
}

#[cfg(all(target_os = "linux", feature = "desktop-integration-tests"))]
fn run_desktop_test(mode: launch::DesktopTestMode) -> iced::Result {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    let desktop_config = match desktop::DesktopConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("invalid desktop configuration: {error}");
            std::process::exit(2);
        }
    };
    let production_data_dir = config::configured_data_dir();
    let profile = match desktop_test_support::SyntheticProfile::load(
        &mode.profile_path,
        &production_data_dir,
    ) {
        Ok(profile) => profile,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let runtime = Arc::new(
        desktop::DesktopRuntime::synthetic(profile, &production_data_dir, desktop_config)
            .unwrap_or_else(|error| {
                eprintln!("invalid synthetic desktop runtime: {error}");
                std::process::exit(2);
            }),
    );
    std::fs::create_dir_all(runtime.data_root()).unwrap_or_else(|error| {
        eprintln!("cannot create test data dir: {error}");
        std::process::exit(2);
    });
    let run_id = format!(
        "r{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    );
    let (inbox_tx, inbox_rx) = futures_channel::mpsc::unbounded();
    let events: Arc<Mutex<VecDeque<desktop_test_support::ControllerEvent>>> =
        Arc::new(Mutex::new(VecDeque::new()));
    desktop_test_support::register_inbox(1, inbox_rx);
    let _controller = desktop_test_support::TestController::start(
        &mode.controller_socket,
        run_id.clone(),
        inbox_tx,
        Arc::clone(&events),
        Instant::now(),
    )
    .unwrap_or_else(|error| {
        eprintln!("cannot start test controller: {error}");
        std::process::exit(2);
    });

    iced::daemon(
        move || {
            measure("startup.app_state", || {
                JamePromptApp::new_for_desktop_test(
                    Arc::clone(&runtime),
                    run_id.clone(),
                    Arc::clone(&events),
                )
            })
        },
        update_application,
        view_application,
    )
    .title(title_application)
    .theme(theme_application)
    .font(icon::FONT)
    .subscription(JamePromptApp::subscription)
    .run()
}

#[cfg(test)]
mod tests {
    use crate::launch::{
        initial_window_visible, should_run_native_hotkey_smoke_from_args,
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
    fn should_run_native_hotkey_smoke_from_args_returns_true_for_native_hotkey_smoke() {
        let args = [
            OsString::from("jame-prompt"),
            OsString::from("--native-hotkey-smoke"),
        ];

        assert!(should_run_native_hotkey_smoke_from_args(args));
    }

    #[test]
    fn should_run_native_hotkey_smoke_from_args_returns_false_without_native_hotkey_smoke() {
        let args = [OsString::from("jame-prompt")];

        assert!(!should_run_native_hotkey_smoke_from_args(args));
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
