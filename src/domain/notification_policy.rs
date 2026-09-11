//! Pure mapping from application outcomes to notification intent.

use super::notification::{
    Field, MessageKey, Notification, NotificationAction, NotificationContext,
    NotificationInvariantError, Operation, Persistence, Presentation, Severity,
};

/// Events recognized by the notification policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationEvent {
    PromptCopied,
    SearchCompleted { result_count: usize },
    NameRequired,
    ContentRequired,
    InvalidShortcut,
    PasteRequested,
    PasteBusy,
    PasteCompleted,
    PastePermissionDenied,
    PasteFailed,
    SaveFailed,
    ExportFailed,
    ImportFailed,
    DeleteFailed,
    WindowHiddenToTray,
    WindowRestored,
    WindowRestoreFailed,
    DestructiveActionRequested,
}

/// Deterministic policy for classifying domain/application outcomes.
pub struct NotificationPolicy;

impl NotificationPolicy {
    /// Maps one event to a valid, presenter-independent notification.
    pub const fn classify(
        event: NotificationEvent,
    ) -> Result<Notification, NotificationInvariantError> {
        match event {
            NotificationEvent::PromptCopied => Notification::new(
                Severity::Success,
                Presentation::Status,
                Persistence::Transient,
                None,
                MessageKey::PromptCopied,
                NotificationContext::None,
            ),
            NotificationEvent::SearchCompleted { result_count } => Notification::new(
                Severity::Info,
                Presentation::Status,
                Persistence::Transient,
                None,
                MessageKey::SearchCompleted,
                NotificationContext::SearchResultCount(result_count),
            ),
            NotificationEvent::NameRequired => Self::required_field(Field::Name),
            NotificationEvent::ContentRequired => Self::required_field(Field::Content),
            NotificationEvent::InvalidShortcut => Notification::new(
                Severity::Error,
                Presentation::InlineField,
                Persistence::UntilCorrected,
                Some(NotificationAction::FocusField(Field::Hotkey)),
                MessageKey::InvalidShortcut,
                NotificationContext::Field(Field::Hotkey),
            ),
            NotificationEvent::PasteRequested => Notification::new(
                Severity::Info,
                Presentation::Status,
                Persistence::Transient,
                None,
                MessageKey::PasteRequested,
                NotificationContext::Operation(Operation::Paste),
            ),
            NotificationEvent::PasteBusy => Notification::new(
                Severity::Info,
                Presentation::Status,
                Persistence::Transient,
                None,
                MessageKey::PasteBusy,
                NotificationContext::Operation(Operation::Paste),
            ),
            NotificationEvent::PasteCompleted => Notification::new(
                Severity::Success,
                Presentation::Status,
                Persistence::Transient,
                None,
                MessageKey::PasteCompleted,
                NotificationContext::Operation(Operation::Paste),
            ),
            NotificationEvent::PastePermissionDenied => Notification::new(
                Severity::Warning,
                Presentation::MessageBar,
                Persistence::UntilResolved,
                Some(NotificationAction::Dismiss),
                MessageKey::PastePermissionDenied,
                NotificationContext::Operation(Operation::Paste),
            ),
            NotificationEvent::PasteFailed => {
                Self::operation_failed(MessageKey::PasteFailed, Operation::Paste)
            }
            NotificationEvent::SaveFailed => {
                Self::operation_failed(MessageKey::SaveFailed, Operation::Save)
            }
            NotificationEvent::ExportFailed => {
                Self::operation_failed(MessageKey::ExportFailed, Operation::Export)
            }
            NotificationEvent::ImportFailed => {
                Self::operation_failed(MessageKey::ImportFailed, Operation::Import)
            }
            NotificationEvent::DeleteFailed => {
                Self::operation_failed(MessageKey::DeleteFailed, Operation::Delete)
            }
            NotificationEvent::DestructiveActionRequested => Notification::new(
                Severity::Warning,
                Presentation::Dialog,
                Persistence::UntilDecided,
                Some(NotificationAction::Confirm),
                MessageKey::DestructiveActionRequested,
                NotificationContext::None,
            ),
            NotificationEvent::WindowHiddenToTray => Notification::new(
                Severity::Info,
                Presentation::Status,
                Persistence::Transient,
                None,
                MessageKey::WindowHiddenToTray,
                NotificationContext::None,
            ),
            NotificationEvent::WindowRestored => Notification::new(
                Severity::Success,
                Presentation::Status,
                Persistence::Transient,
                None,
                MessageKey::WindowRestored,
                NotificationContext::None,
            ),
            NotificationEvent::WindowRestoreFailed => Notification::new(
                Severity::Error,
                Presentation::MessageBar,
                Persistence::UntilResolved,
                Some(NotificationAction::Dismiss),
                MessageKey::WindowRestoreFailed,
                NotificationContext::None,
            ),
        }
    }

    const fn required_field(field: Field) -> Result<Notification, NotificationInvariantError> {
        Notification::new(
            Severity::Error,
            Presentation::InlineField,
            Persistence::UntilCorrected,
            Some(NotificationAction::FocusField(field)),
            MessageKey::FieldRequired,
            NotificationContext::Field(field),
        )
    }

    const fn operation_failed(
        message: MessageKey,
        operation: Operation,
    ) -> Result<Notification, NotificationInvariantError> {
        Notification::new(
            Severity::Error,
            Presentation::MessageBar,
            Persistence::UntilResolved,
            Some(NotificationAction::Dismiss),
            message,
            NotificationContext::Operation(operation),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_copy_is_transient_status_success() {
        let note = NotificationPolicy::classify(NotificationEvent::PromptCopied)
            .expect("copy outcome has a valid policy");

        assert_eq!(note.severity(), Severity::Success);
        assert_eq!(note.presentation(), Presentation::Status);
        assert_eq!(note.persistence(), Persistence::Transient);
        assert_eq!(note.action(), None);
    }

    #[test]
    fn search_is_informational_and_keeps_safe_count_context() {
        let note =
            NotificationPolicy::classify(NotificationEvent::SearchCompleted { result_count: 7 })
                .expect("search outcome has a valid policy");

        assert_eq!(note.severity(), Severity::Info);
        assert_eq!(note.context(), NotificationContext::SearchResultCount(7));
        assert!(!note.is_persistent());
    }

    #[test]
    fn required_name_and_content_are_inline_until_corrected() {
        for (event, field) in [
            (NotificationEvent::NameRequired, Field::Name),
            (NotificationEvent::ContentRequired, Field::Content),
        ] {
            let note = NotificationPolicy::classify(event)
                .expect("required-field outcome has a valid policy");
            assert_eq!(note.severity(), Severity::Error);
            assert_eq!(note.presentation(), Presentation::InlineField);
            assert_eq!(note.persistence(), Persistence::UntilCorrected);
            assert_eq!(note.action(), Some(NotificationAction::FocusField(field)));
            assert_eq!(note.context(), NotificationContext::Field(field));
        }
    }

    #[test]
    fn invalid_shortcut_is_inline_and_focuses_hotkey() {
        let note = NotificationPolicy::classify(NotificationEvent::InvalidShortcut)
            .expect("shortcut validation has a valid policy");

        assert_eq!(note.message(), MessageKey::InvalidShortcut);
        assert_eq!(
            note.action(),
            Some(NotificationAction::FocusField(Field::Hotkey))
        );
        assert_eq!(note.persistence(), Persistence::UntilCorrected);
    }

    #[test]
    fn denied_paste_is_recoverable_persistent_warning() {
        let note = NotificationPolicy::classify(NotificationEvent::PastePermissionDenied)
            .expect("permission outcome has a valid policy");

        assert_eq!(note.severity(), Severity::Warning);
        assert_eq!(note.presentation(), Presentation::MessageBar);
        assert_eq!(note.action(), Some(NotificationAction::Dismiss));
        assert!(note.is_persistent());
    }

    #[test]
    fn paste_progress_and_completion_use_replaceable_statuses() {
        let requested = NotificationPolicy::classify(NotificationEvent::PasteRequested)
            .expect("paste request has a valid policy");
        let completed = NotificationPolicy::classify(NotificationEvent::PasteCompleted)
            .expect("paste completion has a valid policy");

        assert_eq!(requested.presentation(), Presentation::Status);
        assert_eq!(requested.severity(), Severity::Info);
        assert_eq!(requested.persistence(), Persistence::Transient);
        assert_eq!(completed.presentation(), Presentation::Status);
        assert_eq!(completed.severity(), Severity::Success);
        assert_eq!(completed.persistence(), Persistence::Transient);
    }

    #[test]
    fn operation_failures_are_retryable_errors() {
        for (event, operation, message) in [
            (
                NotificationEvent::PasteFailed,
                Operation::Paste,
                MessageKey::PasteFailed,
            ),
            (
                NotificationEvent::SaveFailed,
                Operation::Save,
                MessageKey::SaveFailed,
            ),
            (
                NotificationEvent::ExportFailed,
                Operation::Export,
                MessageKey::ExportFailed,
            ),
            (
                NotificationEvent::ImportFailed,
                Operation::Import,
                MessageKey::ImportFailed,
            ),
            (
                NotificationEvent::DeleteFailed,
                Operation::Delete,
                MessageKey::DeleteFailed,
            ),
        ] {
            let note =
                NotificationPolicy::classify(event).expect("operation failure has a valid policy");
            assert_eq!(note.severity(), Severity::Error);
            assert_eq!(note.presentation(), Presentation::MessageBar);
            assert_eq!(note.persistence(), Persistence::UntilResolved);
            assert_eq!(note.action(), Some(NotificationAction::Dismiss));
            assert_eq!(note.message(), message);
            assert_eq!(note.context(), NotificationContext::Operation(operation));
        }
    }

    #[test]
    fn destructive_request_requires_a_dialog_decision() {
        let note = NotificationPolicy::classify(NotificationEvent::DestructiveActionRequested)
            .expect("destructive request has a valid policy");

        assert_eq!(note.severity(), Severity::Warning);
        assert_eq!(note.presentation(), Presentation::Dialog);
        assert_eq!(note.persistence(), Persistence::UntilDecided);
        assert_eq!(note.action(), Some(NotificationAction::Confirm));
    }
}
