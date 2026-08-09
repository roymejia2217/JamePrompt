use crate::config::{
    APP_ID, WINDOW_INITIAL_HEIGHT, WINDOW_INITIAL_WIDTH, WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH,
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

    let mut settings = window::Settings {
        size: Size::new(WINDOW_INITIAL_WIDTH, WINDOW_INITIAL_HEIGHT),
        min_size: Some(Size::new(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT)),
        position: window::Position::Centered,
        icon: Some(icon),
        visible: true,
        exit_on_close_request: false,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = APP_ID.into();
    }

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
        window::change_mode(id, window::Mode::Windowed),
        window::gain_focus(id),
    ])
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
}
