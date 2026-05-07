use crate::config::{APP_ID, APP_NAME};
use crate::perf;

use tray_icon::{
    menu::{Menu, MenuId, MenuItem, PredefinedMenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent, TrayIconId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    ShowRequested,
    QuitRequested,
}

pub struct TrayHandle {
    _icon: TrayIcon,
    tray_id: TrayIconId,
    show_id: MenuId,
    quit_id: MenuId,
}

impl TrayHandle {
    #[cfg_attr(test, allow(dead_code))]
    pub fn new() -> Result<Self, String> {
        perf::measure("tray.new", || {
            let ids = TrayIds::new();
            let menu = build_menu(&ids)?;
            let icon = tray_icon::Icon::from_rgba(
                crate::APP_TRAY_ICON.data.to_vec(),
                crate::APP_TRAY_ICON.width,
                crate::APP_TRAY_ICON.height,
            )
            .map_err(|err| format!("invalid tray icon data: {err}"))?;

            let tray = TrayIconBuilder::new()
                .with_id(ids.tray_id.clone())
                .with_menu(Box::new(menu))
                .with_tooltip(APP_NAME)
                .with_icon(icon)
                .build()
                .map_err(|err| format!("could not create system tray icon: {err}"))?;

            Ok(Self {
                _icon: tray,
                tray_id: ids.tray_id,
                show_id: ids.show_id,
                quit_id: ids.quit_id,
            })
        })
    }

    pub fn poll_event(&self) -> Option<TrayEvent> {
        perf::measure("tray.poll_event", || {
            while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                if event.id == self.show_id {
                    return Some(TrayEvent::ShowRequested);
                }

                if event.id == self.quit_id {
                    return Some(TrayEvent::QuitRequested);
                }
            }

            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                match event {
                    TrayIconEvent::Click {
                        id,
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                    | TrayIconEvent::DoubleClick {
                        id,
                        button: MouseButton::Left,
                        ..
                    } if id == self.tray_id => return Some(TrayEvent::ShowRequested),
                    _ => {}
                }
            }

            None
        })
    }
}

pub fn pump_platform_events() {
    perf::measure("tray.pump_platform_events", || {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    });
}

#[cfg_attr(test, allow(dead_code))]
struct TrayIds {
    tray_id: TrayIconId,
    show_id: MenuId,
    quit_id: MenuId,
}

impl TrayIds {
    #[cfg_attr(test, allow(dead_code))]
    fn new() -> Self {
        Self {
            tray_id: TrayIconId::new(format!("{APP_ID}.tray")),
            show_id: MenuId::new(format!("{APP_ID}.show")),
            quit_id: MenuId::new(format!("{APP_ID}.quit")),
        }
    }
}

#[cfg_attr(test, allow(dead_code))]
fn build_menu(ids: &TrayIds) -> Result<Menu, String> {
    let show = MenuItem::with_id(ids.show_id.clone(), format!("Show {APP_NAME}"), true, None);
    let separator = PredefinedMenuItem::separator();
    let quit = MenuItem::with_id(ids.quit_id.clone(), "Quit", true, None);
    let menu = Menu::new();

    menu.append_items(&[&show, &separator, &quit])
        .map_err(|err| format!("could not create tray menu: {err}"))?;

    Ok(menu)
}
