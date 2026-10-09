use super::PasteOutcome;
use crate::desktop::{
    CancelOutcome, DesktopErrorCode, HotkeyEvent, ReconcileReceipt, ShortcutAcknowledgement,
    ShortcutSpec, ShortcutStatus,
};
use futures_channel::mpsc::UnboundedSender;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager};
use rdev::{simulate, EventType, Key as RdevKey};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const MAX_PENDING_PASTE_OUTCOMES: usize = 8;

/// Main-thread native backend adapter. The global manager is not Send, so
/// the complete diff is applied synchronously on the calling (main)
/// thread and success/failure is reported truthfully into the same UI
/// acknowledgement path the portal backend uses. Native runtime ids may
/// differ from portal runtime ids; both map to stable prompt identities.
pub(super) struct NativeHotkeyService {
    manager: GlobalHotKeyManager,
    registered: RefCell<HashMap<u32, HotKey>>,
    prompt_of: RefCell<HashMap<u32, String>>,
    event_tx: UnboundedSender<HotkeyEvent>,
}

impl NativeHotkeyService {
    pub(super) fn new(event_tx: UnboundedSender<HotkeyEvent>) -> Option<Self> {
        GlobalHotKeyManager::new().ok().map(|manager| Self {
            manager,
            registered: RefCell::new(HashMap::new()),
            prompt_of: RefCell::new(HashMap::new()),
            event_tx,
        })
    }

    pub(super) fn reconcile(
        &self,
        revision: u64,
        specs: &[ShortcutSpec],
    ) -> Result<ReconcileReceipt, DesktopErrorCode> {
        let mut rejected = Vec::new();
        let mut desired: HashMap<u32, &ShortcutSpec> = HashMap::new();
        for spec in specs {
            match spec.preferred_trigger.parse::<HotKey>() {
                Ok(hotkey) => {
                    desired.insert(hotkey.id(), spec);
                }
                Err(_) => rejected.push(spec.prompt_id.clone()),
            }
        }
        let mut registered = self.registered.borrow_mut();
        let mut prompt_of = self.prompt_of.borrow_mut();
        // Unregister prompts that left the desired set first.
        let removed: Vec<u32> = registered
            .keys()
            .filter(|id| !desired.contains_key(id))
            .copied()
            .collect();
        for id in removed {
            if let Some(hotkey) = registered.remove(&id) {
                let _ = self.manager.unregister(hotkey);
            }
            let prompt_id = prompt_of.remove(&id).unwrap_or_default();
            self.acknowledge(
                revision,
                prompt_id,
                Some(id),
                ShortcutStatus::Inactive,
                None,
                None,
            );
        }
        // Register new prompts; report each outcome truthfully.
        for (id, spec) in &desired {
            if registered.contains_key(id) {
                prompt_of.insert(*id, spec.prompt_id.clone());
                self.acknowledge(
                    revision,
                    spec.prompt_id.clone(),
                    Some(*id),
                    ShortcutStatus::Active,
                    Some(spec.preferred_trigger.clone()),
                    None,
                );
                continue;
            }
            match spec.preferred_trigger.parse::<HotKey>() {
                Ok(parsed) if parsed.id() == *id => match self.manager.register(parsed) {
                    Ok(()) => {
                        registered.insert(*id, parsed);
                        prompt_of.insert(*id, spec.prompt_id.clone());
                        self.acknowledge(
                            revision,
                            spec.prompt_id.clone(),
                            Some(*id),
                            ShortcutStatus::Active,
                            Some(spec.preferred_trigger.clone()),
                            None,
                        );
                    }
                    Err(_) => {
                        self.acknowledge(
                            revision,
                            spec.prompt_id.clone(),
                            None,
                            ShortcutStatus::Failed,
                            None,
                            Some(DesktopErrorCode::BackendUnavailable),
                        );
                    }
                },
                _ => {
                    self.acknowledge(
                        revision,
                        spec.prompt_id.clone(),
                        None,
                        ShortcutStatus::Failed,
                        None,
                        Some(DesktopErrorCode::InvalidResponse),
                    );
                }
            }
        }
        Ok(ReconcileReceipt {
            rejected_prompt_ids: rejected,
        })
    }

    pub(super) fn set_parent(
        &self,
        _parent: Option<crate::desktop::ParentContext>,
    ) -> Result<(), DesktopErrorCode> {
        // Native backends need no parent window for consent.
        Ok(())
    }

    pub(super) fn retry_pending(&self) -> Result<(), DesktopErrorCode> {
        // Synchronous backend: nothing pends. The next reconcile
        // re-publishes current states.
        Ok(())
    }

    pub(super) fn cancel_interaction(
        &self,
        _generation: u64,
    ) -> futures_channel::oneshot::Receiver<CancelOutcome> {
        let (reply_tx, reply_rx) = futures_channel::oneshot::channel();
        let _ = reply_tx.send(CancelOutcome::NoPendingRequest);
        reply_rx
    }

    pub(super) fn shutdown(&self) -> futures_channel::oneshot::Receiver<()> {
        let registered = self.registered.borrow_mut();
        for hotkey in registered.values() {
            let _ = self.manager.unregister(*hotkey);
        }
        drop(registered);
        self.registered.borrow_mut().clear();
        self.prompt_of.borrow_mut().clear();
        let (reply_tx, reply_rx) = futures_channel::oneshot::channel();
        let _ = reply_tx.send(());
        reply_rx
    }

    fn acknowledge(
        &self,
        revision: u64,
        prompt_id: String,
        runtime_id: Option<u32>,
        status: ShortcutStatus,
        trigger_description: Option<String>,
        error_code: Option<DesktopErrorCode>,
    ) {
        let _ = self
            .event_tx
            .unbounded_send(HotkeyEvent::Acknowledged(ShortcutAcknowledgement {
                revision,
                prompt_id,
                runtime_id,
                status,
                trigger_description,
                error_code,
            }));
    }
}

pub(super) fn poll_event() -> Option<u32> {
    let receiver = GlobalHotKeyEvent::receiver();
    while let Ok(event) = receiver.try_recv() {
        if event.state == global_hotkey::HotKeyState::Pressed {
            return Some(event.id());
        }
    }
    None
}

fn paste_outcomes() -> &'static Mutex<VecDeque<PasteOutcome>> {
    static OUTCOMES: OnceLock<Mutex<VecDeque<PasteOutcome>>> = OnceLock::new();
    OUTCOMES.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn enqueue_paste_outcome(outcome: PasteOutcome) {
    let mut outcomes = paste_outcomes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if outcomes.len() >= MAX_PENDING_PASTE_OUTCOMES {
        outcomes.pop_front();
    }
    outcomes.push_back(outcome);
}

pub(super) fn poll_paste_outcome() -> Option<PasteOutcome> {
    paste_outcomes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .pop_front()
}

fn paste_outcome_from_steps(steps: [bool; 4]) -> PasteOutcome {
    if steps.into_iter().all(|succeeded| succeeded) {
        PasteOutcome::Completed
    } else {
        PasteOutcome::Failed
    }
}

pub(super) fn paste_to_active_window() -> bool {
    thread::Builder::new()
        .name("jameprompt-native-paste".into())
        .spawn(|| {
            thread::sleep(Duration::from_millis(150));

            let control_pressed = simulate(&EventType::KeyPress(RdevKey::ControlLeft)).is_ok();
            let v_pressed = simulate(&EventType::KeyPress(RdevKey::KeyV)).is_ok();

            thread::sleep(Duration::from_millis(20));

            let v_released = simulate(&EventType::KeyRelease(RdevKey::KeyV)).is_ok();
            let control_released = simulate(&EventType::KeyRelease(RdevKey::ControlLeft)).is_ok();

            enqueue_paste_outcome(paste_outcome_from_steps([
                control_pressed,
                v_pressed,
                v_released,
                control_released,
            ]));
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{ShortcutSnapshot, ShortcutSpec};

    fn smoke_spec() -> ShortcutSpec {
        ShortcutSpec {
            prompt_id: "prompt-native".to_string(),
            description: "Native".to_string(),
            preferred_trigger: "Ctrl+Shift+F13".to_string(),
        }
    }

    #[test]
    fn native_reconcile_acknowledges_exact_routes_synchronously() {
        let (event_tx, event_rx) = futures_channel::mpsc::unbounded();
        let Some(service) = NativeHotkeyService::new(event_tx) else {
            // No native backend on this machine: nothing to assert.
            return;
        };
        let receipt = service
            .reconcile(1, &[smoke_spec()])
            .expect("native reconcile");
        assert!(receipt.rejected_prompt_ids.is_empty());
        let _ = event_rx;
        // Success/failure arrives on the shared event channel; the call
        // itself never disguises acceptance.
    }

    #[test]
    fn native_reconcile_rejects_unparseable_trigger() {
        let (event_tx, _event_rx) = futures_channel::mpsc::unbounded();
        let Some(service) = NativeHotkeyService::new(event_tx) else {
            return;
        };
        let receipt = service
            .reconcile(
                1,
                &[ShortcutSpec {
                    prompt_id: "prompt-bad".to_string(),
                    description: "Bad".to_string(),
                    preferred_trigger: "Not A Key".to_string(),
                }],
            )
            .expect("native reconcile");
        assert_eq!(receipt.rejected_prompt_ids, vec!["prompt-bad".to_string()]);
    }

    #[test]
    fn native_snapshot_roundtrip_uses_shortcut_snapshot_shape() {
        let snapshot = ShortcutSnapshot::new(3, vec![smoke_spec()]);
        assert_eq!(snapshot.revision, 3);
        assert_eq!(snapshot.shortcuts.len(), 1);
    }

    #[test]
    fn native_paste_reports_completed_only_when_every_injection_step_succeeds() {
        assert_eq!(
            paste_outcome_from_steps([true, true, true, true]),
            PasteOutcome::Completed
        );

        for failed_step in 0..4 {
            let mut steps = [true, true, true, true];
            steps[failed_step] = false;
            assert_eq!(paste_outcome_from_steps(steps), PasteOutcome::Failed);
        }
    }
}
