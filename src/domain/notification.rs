//! Pure notification values shared by application policy and presenters.
//!
//! This module deliberately has no dependency on Iced, persistence, the
//! clipboard, or an operating system.  User-visible copy is represented by a
//! closed [`MessageKey`] vocabulary and safe typed context; low-level errors,
//! paths, prompt contents, and prompt names cannot enter these values.

/// Semantic importance of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

/// The presenter that should render a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    Status,
    InlineField,
    MessageBar,
    Dialog,
}

/// Lifetime of a notification in the application model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persistence {
    Transient,
    WhileProcess,
    UntilCorrected,
    UntilResolved,
    UntilDecided,
}

/// Recovery or decision affordance exposed by a presenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationAction {
    Retry,
    OpenSettings,
    OpenHelp,
    FocusField(Field),
    Confirm,
    Dismiss,
}

/// Form fields that can own an inline validation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Content,
    Hotkey,
}

/// Operations whose outcome can be represented without leaking implementation
/// errors into user-visible copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Save,
    Export,
    Import,
    Delete,
    Paste,
}

/// Closed vocabulary for notification copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKey {
    PromptCopied,
    SearchCompleted,
    FieldRequired,
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

/// Safe, structured parameters for [`MessageKey`].
///
/// No prompt text, prompt name, filesystem path, database error, or platform
/// error is representable here.  A presenter may localize a key using these
/// bounded values without interpolating technical data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationContext {
    None,
    SearchResultCount(usize),
    Field(Field),
    Operation(Operation),
}

/// Error returned when a notification violates a domain invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationInvariantError {
    InlineFieldMustBeError,
    InlineFieldRequiresFieldContext,
    CorrectionMustBeInlineField,
    DecisionMustBeDialog,
    DialogRequiresConfirmAction,
    FieldContextRequiresInlineField,
}

/// A typed notification intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notification {
    severity: Severity,
    presentation: Presentation,
    persistence: Persistence,
    action: Option<NotificationAction>,
    message: MessageKey,
    context: NotificationContext,
}

impl Notification {
    /// Constructs a notification after checking cross-field invariants.
    pub const fn new(
        severity: Severity,
        presentation: Presentation,
        persistence: Persistence,
        action: Option<NotificationAction>,
        message: MessageKey,
        context: NotificationContext,
    ) -> Result<Self, NotificationInvariantError> {
        if matches!(presentation, Presentation::InlineField) && !matches!(severity, Severity::Error)
        {
            return Err(NotificationInvariantError::InlineFieldMustBeError);
        }

        if matches!(presentation, Presentation::InlineField)
            && !matches!(context, NotificationContext::Field(_))
        {
            return Err(NotificationInvariantError::InlineFieldRequiresFieldContext);
        }

        if matches!(persistence, Persistence::UntilCorrected)
            && !matches!(presentation, Presentation::InlineField)
        {
            return Err(NotificationInvariantError::CorrectionMustBeInlineField);
        }

        if matches!(persistence, Persistence::UntilDecided)
            && !matches!(presentation, Presentation::Dialog)
        {
            return Err(NotificationInvariantError::DecisionMustBeDialog);
        }

        if matches!(presentation, Presentation::Dialog)
            && !matches!(action, Some(NotificationAction::Confirm))
        {
            return Err(NotificationInvariantError::DialogRequiresConfirmAction);
        }

        if matches!(context, NotificationContext::Field(_))
            && !matches!(presentation, Presentation::InlineField)
        {
            return Err(NotificationInvariantError::FieldContextRequiresInlineField);
        }

        Ok(Self {
            severity,
            presentation,
            persistence,
            action,
            message,
            context,
        })
    }

    pub const fn severity(self) -> Severity {
        self.severity
    }

    pub const fn presentation(self) -> Presentation {
        self.presentation
    }

    pub const fn persistence(self) -> Persistence {
        self.persistence
    }

    pub const fn action(self) -> Option<NotificationAction> {
        self.action
    }

    pub const fn message(self) -> MessageKey {
        self.message
    }

    pub const fn context(self) -> NotificationContext {
        self.context
    }

    pub const fn is_persistent(self) -> bool {
        matches!(
            self.persistence,
            Persistence::UntilCorrected | Persistence::UntilResolved | Persistence::UntilDecided
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_validation_requires_error_and_field_context() {
        let note = Notification::new(
            Severity::Error,
            Presentation::InlineField,
            Persistence::UntilCorrected,
            Some(NotificationAction::FocusField(Field::Name)),
            MessageKey::FieldRequired,
            NotificationContext::Field(Field::Name),
        )
        .expect("valid field validation notification");

        assert_eq!(note.presentation(), Presentation::InlineField);
        assert_eq!(note.persistence(), Persistence::UntilCorrected);
        assert_eq!(
            note.action(),
            Some(NotificationAction::FocusField(Field::Name))
        );
        assert!(note.is_persistent());
    }

    #[test]
    fn dialog_requires_confirm_action_and_decision_persistence() {
        let note = Notification::new(
            Severity::Warning,
            Presentation::Dialog,
            Persistence::UntilDecided,
            Some(NotificationAction::Confirm),
            MessageKey::DestructiveActionRequested,
            NotificationContext::Operation(Operation::Delete),
        )
        .expect("valid destructive-action notification");

        assert_eq!(note.action(), Some(NotificationAction::Confirm));
        assert!(note.is_persistent());
    }

    #[test]
    fn technical_error_data_has_no_string_slot() {
        let note = Notification::new(
            Severity::Error,
            Presentation::MessageBar,
            Persistence::UntilResolved,
            Some(NotificationAction::Retry),
            MessageKey::SaveFailed,
            NotificationContext::Operation(Operation::Save),
        )
        .expect("valid save failure notification");

        assert_eq!(
            note.context(),
            NotificationContext::Operation(Operation::Save)
        );
        assert_eq!(note.message(), MessageKey::SaveFailed);
    }

    #[test]
    fn invalid_cross_field_combinations_are_rejected() {
        assert_eq!(
            Notification::new(
                Severity::Warning,
                Presentation::InlineField,
                Persistence::UntilCorrected,
                None,
                MessageKey::FieldRequired,
                NotificationContext::Field(Field::Content),
            ),
            Err(NotificationInvariantError::InlineFieldMustBeError)
        );
        assert_eq!(
            Notification::new(
                Severity::Error,
                Presentation::Status,
                Persistence::UntilCorrected,
                None,
                MessageKey::FieldRequired,
                NotificationContext::Field(Field::Content),
            ),
            Err(NotificationInvariantError::CorrectionMustBeInlineField)
        );
    }
}
