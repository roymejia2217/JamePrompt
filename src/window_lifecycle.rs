#[cfg(target_os = "linux")]
use crate::config::LINUX_DESKTOP_APP_ID;
use crate::config::{
    WINDOW_INITIAL_HEIGHT, WINDOW_INITIAL_WIDTH, WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH,
};
use crate::ui::Message;
use iced::{window, Size, Task};

const APP_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/icons/app_icon.png"
));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowLifecycleAction {
    Delegate,
    Close(window::Id),
    Open,
    Restore(window::Id),
}

pub(crate) fn classify(message: &Message) -> WindowLifecycleAction {
    match message {
        Message::CloseRequested(id) => WindowLifecycleAction::Close(*id),
        Message::ShowWindow(None) => WindowLifecycleAction::Open,
        Message::ShowWindow(Some(id)) => WindowLifecycleAction::Restore(*id),
        _ => WindowLifecycleAction::Delegate,
    }
}

pub(crate) fn settings() -> window::Settings {
    let icon = window::icon::from_file_data(APP_ICON_PNG, None)
        .expect("Failed to load app icon from embedded PNG");

    let settings = window::Settings {
        size: Size::new(WINDOW_INITIAL_WIDTH, WINDOW_INITIAL_HEIGHT),
        min_size: Some(Size::new(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT)),
        position: window::Position::Centered,
        icon: Some(icon),
        visible: true,
        exit_on_close_request: false,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    let settings = {
        let mut settings = settings;
        settings.platform_specific.application_id = LINUX_DESKTOP_APP_ID.into();
        settings
    };

    settings
}

pub(crate) fn open() -> Task<window::Id> {
    let (_, task) = window::open(settings());
    task
}

pub(crate) fn close(id: window::Id) -> Task<Message> {
    window::close(id)
}

pub(crate) fn restore(id: window::Id) -> Task<Message> {
    Task::batch([
        window::set_mode(id, window::Mode::Windowed),
        window::gain_focus(id),
    ])
}

/// Platform-aware restore for an existing window.
///
/// Supported `Windowed`/`gain_focus` semantics are kept where the backend
/// implements them. On Wayland `gain_focus` is a documented no-op for the
/// pinned winit, so a compositor attention request is used instead; focus
/// itself is observed separately and never promised.
pub(crate) fn restore_for_platform(id: window::Id) -> Task<Message> {
    #[cfg(target_os = "linux")]
    if crate::platform::display_server() == crate::platform::DisplayServer::Wayland {
        return restore_wayland(id);
    }
    restore(id)
}

/// Wayland/GNOME-safe restore: `request_user_attention` is a compositor
/// request, not a focus guarantee. Focused state is observed separately.
pub(crate) fn restore_wayland(id: window::Id) -> Task<Message> {
    Task::batch([
        window::set_mode(id, window::Mode::Windowed),
        window::request_user_attention(id, None),
    ])
}

/// Generation-tagged phase of the single managed toplevel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowPhase {
    Absent,
    Opening,
    Open,
    Closing,
}

/// Authoritative lifecycle state owned by the main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowState {
    pub generation: u64,
    pub id: Option<window::Id>,
    pub phase: WindowPhase,
    /// A Show arrived while Closing; reopen exactly once on Closed.
    pub pending_show: bool,
}

impl WindowState {
    pub(crate) fn absent() -> Self {
        Self {
            generation: 0,
            id: None,
            phase: WindowPhase::Absent,
            pending_show: false,
        }
    }
}

/// Intent routed into the canonical state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowIntent {
    Show,
    Close(window::Id),
    AbortClose(window::Id),
    Opened { generation: u64, id: window::Id },
    Closed(window::Id),
}

/// Effect chosen by the pure transition; executed by the main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowEffect {
    OpenWindow { generation: u64 },
    Present { id: window::Id },
    CloseWindow { id: window::Id },
    DropStaleOpened { id: window::Id },
    None,
}

/// Pure lifecycle transition. No iced Tasks are created here so the full
/// close/show/flood matrix is unit-testable without a compositor.
pub(crate) fn transition(state: &mut WindowState, intent: WindowIntent) -> Vec<WindowEffect> {
    match intent {
        WindowIntent::Show => match state.phase {
            WindowPhase::Absent => {
                state.generation = state.generation.saturating_add(1);
                state.phase = WindowPhase::Opening;
                state.id = None;
                vec![WindowEffect::OpenWindow {
                    generation: state.generation,
                }]
            }
            WindowPhase::Opening | WindowPhase::Open => {
                if let Some(id) = state.id {
                    vec![WindowEffect::Present { id }]
                } else {
                    vec![WindowEffect::None]
                }
            }
            WindowPhase::Closing => {
                state.pending_show = true;
                vec![WindowEffect::None]
            }
        },
        WindowIntent::Close(id) => {
            if state.id == Some(id) && state.phase == WindowPhase::Open {
                state.phase = WindowPhase::Closing;
                state.pending_show = false;
                vec![WindowEffect::CloseWindow { id }]
            } else if state.phase == WindowPhase::Opening {
                state.phase = WindowPhase::Closing;
                state.pending_show = false;
                if let Some(current) = state.id {
                    vec![WindowEffect::CloseWindow { id: current }]
                } else {
                    state.phase = WindowPhase::Absent;
                    state.id = None;
                    vec![WindowEffect::None]
                }
            } else {
                vec![WindowEffect::None]
            }
        }
        WindowIntent::AbortClose(id) => {
            if state.id == Some(id) && state.phase == WindowPhase::Closing {
                state.phase = WindowPhase::Open;
                state.pending_show = false;
            }
            vec![WindowEffect::None]
        }
        WindowIntent::Opened { generation, id } => {
            if generation != state.generation {
                return vec![WindowEffect::DropStaleOpened { id }];
            }
            match state.phase {
                WindowPhase::Opening => {
                    state.id = Some(id);
                    state.phase = WindowPhase::Open;
                    vec![WindowEffect::Present { id }]
                }
                WindowPhase::Closing => vec![WindowEffect::DropStaleOpened { id }],
                WindowPhase::Open | WindowPhase::Absent => {
                    if state.id == Some(id) {
                        vec![WindowEffect::None]
                    } else {
                        vec![WindowEffect::DropStaleOpened { id }]
                    }
                }
            }
        }
        WindowIntent::Closed(id) => {
            if state.id != Some(id) {
                return vec![WindowEffect::None];
            }
            state.id = None;
            if state.pending_show {
                state.pending_show = false;
                state.generation = state.generation.saturating_add(1);
                state.phase = WindowPhase::Opening;
                vec![WindowEffect::OpenWindow {
                    generation: state.generation,
                }]
            } else {
                state.phase = WindowPhase::Absent;
                vec![WindowEffect::None]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_request_closes_the_window() {
        let id = window::Id::unique();

        assert_eq!(
            classify(&Message::CloseRequested(id)),
            WindowLifecycleAction::Close(id)
        );
    }

    #[test]
    fn missing_window_is_recreated_from_tray_request() {
        assert_eq!(
            classify(&Message::ShowWindow(None)),
            WindowLifecycleAction::Open
        );
    }

    #[test]
    fn existing_window_is_restored_from_tray_request() {
        let id = window::Id::unique();

        assert_eq!(
            classify(&Message::ShowWindow(Some(id))),
            WindowLifecycleAction::Restore(id)
        );
    }

    #[test]
    fn unrelated_messages_stay_owned_by_the_application() {
        assert_eq!(
            classify(&Message::SearchChanged("test".into())),
            WindowLifecycleAction::Delegate
        );
    }

    #[test]
    fn window_settings_preserve_product_dimensions_and_close_contract() {
        let settings = settings();

        assert_eq!(
            settings.size,
            Size::new(WINDOW_INITIAL_WIDTH, WINDOW_INITIAL_HEIGHT)
        );
        assert_eq!(
            settings.min_size,
            Some(Size::new(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT))
        );
        assert!(!settings.exit_on_close_request);
        assert!(settings.visible);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_window_uses_portal_compatible_application_id() {
        let settings = settings();
        assert_eq!(
            settings.platform_specific.application_id,
            LINUX_DESKTOP_APP_ID
        );
    }

    #[test]
    fn show_from_absent_opens_one_window_with_new_generation() {
        let mut state = WindowState::absent();
        let effects = transition(&mut state, WindowIntent::Show);
        assert_eq!(effects, vec![WindowEffect::OpenWindow { generation: 1 }]);
        assert_eq!(state.phase, WindowPhase::Opening);
        // Repeated show while opening without an id coalesces to nothing.
        let effects = transition(&mut state, WindowIntent::Show);
        assert_eq!(effects, vec![WindowEffect::None]);
    }

    #[test]
    fn stale_opened_completion_is_dropped_without_state_change() {
        let mut state = WindowState::absent();
        transition(&mut state, WindowIntent::Show);
        let stale_id = window::Id::unique();
        let effects = transition(
            &mut state,
            WindowIntent::Opened {
                generation: 99,
                id: stale_id,
            },
        );
        assert_eq!(
            effects,
            vec![WindowEffect::DropStaleOpened { id: stale_id }]
        );
        assert_eq!(state.phase, WindowPhase::Opening);
        assert_eq!(state.id, None);
    }

    #[test]
    fn opened_completion_assigns_id_once_and_ignores_repeats() {
        let mut state = WindowState::absent();
        transition(&mut state, WindowIntent::Show);
        let id = window::Id::unique();
        let effects = transition(&mut state, WindowIntent::Opened { generation: 1, id });
        assert_eq!(effects, vec![WindowEffect::Present { id }]);
        assert_eq!(state.phase, WindowPhase::Open);
        let repeat = transition(&mut state, WindowIntent::Opened { generation: 1, id });
        assert_eq!(repeat, vec![WindowEffect::None]);
    }

    #[test]
    fn show_during_closing_reopens_exactly_once_on_closed() {
        let mut state = WindowState::absent();
        transition(&mut state, WindowIntent::Show);
        let id = window::Id::unique();
        transition(&mut state, WindowIntent::Opened { generation: 1, id });
        transition(&mut state, WindowIntent::Close(id));
        assert_eq!(state.phase, WindowPhase::Closing);
        let latched = transition(&mut state, WindowIntent::Show);
        assert_eq!(latched, vec![WindowEffect::None]);
        let reopen = transition(&mut state, WindowIntent::Closed(id));
        assert_eq!(reopen, vec![WindowEffect::OpenWindow { generation: 2 }]);
        assert_eq!(state.phase, WindowPhase::Opening);
    }

    #[test]
    fn obsolete_closed_does_not_clear_newly_opened_window() {
        let mut state = WindowState::absent();
        transition(&mut state, WindowIntent::Show);
        let first = window::Id::unique();
        transition(
            &mut state,
            WindowIntent::Opened {
                generation: 1,
                id: first,
            },
        );
        transition(&mut state, WindowIntent::Close(first));
        // Show arrives while closing: Closed must reopen exactly once.
        transition(&mut state, WindowIntent::Show);
        let reopen = transition(&mut state, WindowIntent::Closed(first));
        assert_eq!(reopen, vec![WindowEffect::OpenWindow { generation: 2 }]);
        // A duplicate Closed for the obsolete id is ignored; generation 2 stays opening.
        let effects = transition(&mut state, WindowIntent::Closed(first));
        assert_eq!(effects, vec![WindowEffect::None]);
        assert_eq!(state.phase, WindowPhase::Opening);
        assert_eq!(state.generation, 2);
    }

    #[test]
    fn close_for_unknown_id_is_ignored() {
        let mut state = WindowState::absent();
        let unknown = window::Id::unique();
        let effects = transition(&mut state, WindowIntent::Close(unknown));
        assert_eq!(effects, vec![WindowEffect::None]);
        assert_eq!(state.phase, WindowPhase::Absent);
    }

    #[test]
    fn failed_close_cancellation_reopens_the_current_window() {
        let mut state = WindowState::absent();
        transition(&mut state, WindowIntent::Show);
        let id = window::Id::unique();
        transition(&mut state, WindowIntent::Opened { generation: 1, id });
        transition(&mut state, WindowIntent::Close(id));

        assert_eq!(state.phase, WindowPhase::Closing);
        assert_eq!(
            transition(&mut state, WindowIntent::AbortClose(id)),
            vec![WindowEffect::None]
        );
        assert_eq!(state.phase, WindowPhase::Open);
        assert_eq!(state.id, Some(id));
    }
}
