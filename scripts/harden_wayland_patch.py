from pathlib import Path

path = Path("scripts/apply_wayland_shortcut_ux_fix.py")
text = path.read_text()
old = "replace_exact(\n    \"src/ui.rs\",\n    \'\'\'                iced::window::get_latest().map(Message::ShowWindow)\n\'\'\',\n    \'\'\'                Task::done(Message::ShowWindow(None))\n\'\'\',\n)\n"
new = "replace_exact(\n    \"src/ui.rs\",\n    \'\'\'        let fallback_task =\n            if should_show_window_after_hidden_start(start_minimized, tray_available) {\n                iced::window::get_latest().map(Message::ShowWindow)\n            } else {\n                Task::none()\n            };\n\'\'\',\n    \'\'\'        let fallback_task =\n            if should_show_window_after_hidden_start(start_minimized, tray_available) {\n                Task::done(Message::ShowWindow(None))\n            } else {\n                Task::none()\n            };\n\'\'\',\n)\n"
count = text.count(old)
if count != 1:
    raise SystemExit(f"Expected one ambiguous fallback replacement block, found {count}")
path.write_text(text.replace(old, new, 1))
print("Patch precondition hardened")
