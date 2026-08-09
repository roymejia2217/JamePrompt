use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager};
use rdev::{simulate, EventType, Key as RdevKey};
use std::cell::RefCell;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

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

pub(super) fn poll_events() -> Vec<u32> {
    let receiver = GlobalHotKeyEvent::receiver();
    let mut triggered = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        if event.state == global_hotkey::HotKeyState::Pressed {
            triggered.push(event.id());
        }
    }
    triggered
}

pub(super) fn paste_to_active_window() {
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(150));
        let _ = simulate(&EventType::KeyPress(RdevKey::ControlLeft));
        let _ = simulate(&EventType::KeyPress(RdevKey::KeyV));
        thread::sleep(Duration::from_millis(20));
        let _ = simulate(&EventType::KeyRelease(RdevKey::KeyV));
        let _ = simulate(&EventType::KeyRelease(RdevKey::ControlLeft));
    });
}
