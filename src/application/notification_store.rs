//! In-process notification hub.
//!
//! The store intentionally keeps notification intents separate.  It does not
//! render them and has no dependency on a UI toolkit, persistence, or a
//! platform adapter.

use crate::domain::notification::{Notification, Persistence, Severity};

/// Stable identifier returned when a notification is published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NotificationId(u64);

/// A notification together with its store identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationRecord {
    id: NotificationId,
    notification: Notification,
    sequence: u64,
}

impl NotificationRecord {
    pub const fn id(self) -> NotificationId {
        self.id
    }

    pub const fn notification(self) -> Notification {
        self.notification
    }

    #[cfg(test)]
    pub const fn is_persistent(self) -> bool {
        matches!(
            self.notification.persistence(),
            Persistence::UntilCorrected | Persistence::UntilResolved | Persistence::UntilDecided
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct StoredNotification {
    record: NotificationRecord,
}

/// Pure notification hub with deterministic priority and lifecycle.
#[derive(Debug, Default)]
pub struct NotificationStore {
    notifications: Vec<StoredNotification>,
    next_id: u64,
    next_sequence: u64,
}

impl NotificationStore {
    pub const fn new() -> Self {
        Self {
            notifications: Vec::new(),
            next_id: 0,
            next_sequence: 0,
        }
    }

    /// Publishes a notification and returns its explicit lifecycle identifier.
    ///
    /// Equivalent active semantic notifications are deduplicated by message
    /// key, context, and presentation.  Republishing one refreshes its age,
    /// so a same-severity notification remains the most recent one.
    pub fn publish(&mut self, notification: Notification) -> NotificationId {
        self.next_sequence = self.next_sequence.wrapping_add(1);

        if let Some(existing) = self
            .notifications
            .iter_mut()
            .find(|stored| same_semantic_notification(stored.record.notification, notification))
        {
            existing.record.sequence = self.next_sequence;
            return existing.record.id;
        }

        // Status/Transient is an ephemeral lane: a new status replaces the
        // previous one, while persistent notifications and other presenters
        // remain independently addressable.
        if is_ephemeral_status(notification) {
            self.notifications
                .retain(|stored| !is_ephemeral_status(stored.record.notification));
        }

        self.next_id = self.next_id.wrapping_add(1);
        let id = NotificationId(self.next_id);
        self.notifications.push(StoredNotification {
            record: NotificationRecord {
                id,
                notification,
                sequence: self.next_sequence,
            },
        });
        id
    }

    /// Returns the highest-priority active notification.
    ///
    /// Priority is `Error > Warning > Success > Info`; equal severities use
    /// publication sequence, with the newest notification winning.
    pub fn active(&self) -> Option<NotificationRecord> {
        self.notifications
            .iter()
            .map(|stored| stored.record)
            .max_by_key(|record| {
                (
                    severity_rank(record.notification.severity()),
                    record.sequence,
                )
            })
    }

    #[cfg(test)]
    pub fn get(&self, id: NotificationId) -> Option<NotificationRecord> {
        self.notifications
            .iter()
            .find(|stored| stored.record.id == id)
            .map(|stored| stored.record)
    }

    #[cfg(test)]
    pub const fn len(&self) -> usize {
        self.notifications.len()
    }

    #[cfg(test)]
    pub const fn is_empty(&self) -> bool {
        self.notifications.is_empty()
    }

    /// Resolves and removes a notification by its explicit identifier.
    #[cfg(test)]
    pub fn resolve(&mut self, id: NotificationId) -> bool {
        self.remove(id)
    }

    /// Dismisses and removes a notification by its explicit identifier.
    pub fn dismiss(&mut self, id: NotificationId) -> bool {
        self.remove(id)
    }

    fn remove(&mut self, id: NotificationId) -> bool {
        let Some(index) = self
            .notifications
            .iter()
            .position(|stored| stored.record.id == id)
        else {
            return false;
        };

        self.notifications.remove(index);
        true
    }
}

const fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Success => 1,
        Severity::Warning => 2,
        Severity::Error => 3,
    }
}

fn same_semantic_notification(left: Notification, right: Notification) -> bool {
    left.message() == right.message()
        && left.context() == right.context()
        && left.presentation() == right.presentation()
}

const fn is_ephemeral_status(notification: Notification) -> bool {
    matches!(
        notification.presentation(),
        crate::domain::notification::Presentation::Status
    ) && matches!(notification.persistence(), Persistence::Transient)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::notification::{
        Field, MessageKey, NotificationAction, NotificationContext, Operation, Presentation,
    };

    fn status(
        severity: Severity,
        message: MessageKey,
        context: NotificationContext,
    ) -> Notification {
        Notification::new(
            severity,
            Presentation::Status,
            Persistence::Transient,
            None,
            message,
            context,
        )
        .expect("valid status notification")
    }

    fn warning() -> Notification {
        Notification::new(
            Severity::Warning,
            Presentation::MessageBar,
            Persistence::UntilResolved,
            Some(NotificationAction::OpenSettings),
            MessageKey::PastePermissionDenied,
            NotificationContext::Operation(Operation::Paste),
        )
        .expect("valid warning notification")
    }

    fn error(field: Field) -> Notification {
        Notification::new(
            Severity::Error,
            Presentation::InlineField,
            Persistence::UntilCorrected,
            Some(NotificationAction::FocusField(field)),
            MessageKey::FieldRequired,
            NotificationContext::Field(field),
        )
        .expect("valid validation notification")
    }

    #[test]
    fn persistent_warning_survives_later_search_info() {
        let mut store = NotificationStore::new();
        let warning_id = store.publish(warning());
        let search_id = store.publish(status(
            Severity::Info,
            MessageKey::SearchCompleted,
            NotificationContext::SearchResultCount(3),
        ));

        assert_eq!(store.len(), 2);
        assert_eq!(store.active().map(|record| record.id()), Some(warning_id));
        assert!(store.get(search_id).is_some());
        assert!(store
            .get(warning_id)
            .expect("warning remains")
            .is_persistent());
    }

    #[test]
    fn repeated_distinct_search_counts_replace_only_ephemeral_status() {
        let mut store = NotificationStore::new();
        let warning_id = store.publish(warning());
        let first_search = store.publish(status(
            Severity::Info,
            MessageKey::SearchCompleted,
            NotificationContext::SearchResultCount(1),
        ));
        let second_search = store.publish(status(
            Severity::Info,
            MessageKey::SearchCompleted,
            NotificationContext::SearchResultCount(2),
        ));

        assert_ne!(first_search, second_search);
        assert_eq!(store.len(), 2);
        assert!(store.get(first_search).is_none());
        assert!(store.get(warning_id).is_some());
        assert_eq!(store.active().map(|record| record.id()), Some(warning_id));
    }

    #[test]
    fn errors_then_warnings_then_successes_then_info_are_prioritized() {
        let mut store = NotificationStore::new();
        let info_id = store.publish(status(
            Severity::Info,
            MessageKey::SearchCompleted,
            NotificationContext::SearchResultCount(1),
        ));
        let success_id = store.publish(status(
            Severity::Success,
            MessageKey::PromptCopied,
            NotificationContext::None,
        ));
        let warning_id = store.publish(warning());
        let error_id = store.publish(error(Field::Name));

        assert_eq!(store.active().map(|record| record.id()), Some(error_id));
        assert!(store.dismiss(error_id));
        assert_eq!(store.active().map(|record| record.id()), Some(warning_id));
        assert!(store.resolve(warning_id));
        assert_eq!(store.active().map(|record| record.id()), Some(success_id));
        assert!(store.dismiss(success_id));
        assert!(store.get(info_id).is_none());
        assert_eq!(store.active(), None);
    }

    #[test]
    fn newest_notification_wins_same_severity() {
        let mut store = NotificationStore::new();
        let first = store.publish(status(
            Severity::Info,
            MessageKey::SearchCompleted,
            NotificationContext::SearchResultCount(1),
        ));
        let second = store.publish(status(
            Severity::Info,
            MessageKey::SearchCompleted,
            NotificationContext::SearchResultCount(2),
        ));

        assert_eq!(store.active().map(|record| record.id()), Some(second));
        assert!(store.get(first).is_none());
    }

    #[test]
    fn equivalent_active_notifications_are_deduplicated() {
        let mut store = NotificationStore::new();
        let first = store.publish(warning());
        let repeated = store.publish(warning());

        assert_eq!(first, repeated);
        assert_eq!(store.len(), 1);
        assert_eq!(store.active().map(|record| record.id()), Some(first));
    }

    #[test]
    fn semantic_identity_ignores_action_and_persistence() {
        let mut store = NotificationStore::new();
        let first = store.publish(status(
            Severity::Info,
            MessageKey::SearchCompleted,
            NotificationContext::SearchResultCount(4),
        ));
        let equivalent = Notification::new(
            Severity::Success,
            Presentation::Status,
            Persistence::WhileProcess,
            Some(NotificationAction::Dismiss),
            MessageKey::SearchCompleted,
            NotificationContext::SearchResultCount(4),
        )
        .expect("valid equivalent semantic notification");

        assert_eq!(store.publish(equivalent), first);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn dismiss_and_resolve_are_explicit_and_idempotent() {
        let mut store = NotificationStore::new();
        let first = store.publish(warning());
        let second = store.publish(error(Field::Content));

        assert!(store.resolve(first));
        assert!(!store.resolve(first));
        assert!(store.dismiss(second));
        assert!(!store.dismiss(second));
        assert!(store.is_empty());
        assert_eq!(store.active(), None);
    }
}
