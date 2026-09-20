use super::PasteOutcome;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager};
use rdev::{simulate, EventType, Key as RdevKey};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const MAX_PENDING_PASTE_OUTCOMES: usize = 8;

pub(super) struct NativeHotkeyService {
    manager: GlobalHotKeyManager,
    registered: RefCell<HashMap<u32, HotKey>>,
}

impl NativeHotkeyService {
    pub(super) fn new() -> Option<Self> {
        GlobalHotKeyManager::new().ok().map(|manager| Self {
            manager,
            registered: RefCell::new(HashMap::new()),
        })
    }

    pub(super) fn register(&self, key_str: &str) -> Option<u32> {
        let hotkey = key_str.parse::<HotKey>().ok()?;
        self.manager.register(hotkey).ok()?;
        self.registered.borrow_mut().insert(hotkey.id(), hotkey);
        Some(hotkey.id())
    }

    pub(super) fn unregister(&self, hotkey_id: u32) -> bool {
        if let Some(hotkey) = self.registered.borrow_mut().remove(&hotkey_id) {
            self.manager.unregister(hotkey).is_ok()
        } else {
            false
        }
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
