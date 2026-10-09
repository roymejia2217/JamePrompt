//! Linux event-driven StatusNotifierItem backend (ksni 0.3.6).
//!
//! Callbacks send typed events over channels; they never touch
//! `JamePromptApp` or iced window handles. Show requests are coalesced on
//! a bounded channel with an explicit counter; Quit travels on a dedicated
//! control channel that is always polled first so a Show flood can never
//! drop termination. The service lives for the subscription lifetime:
//! dropping the stream releases the shutdown trigger and the owned service
//! task awaits `Handle::shutdown()` before exiting.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};

use futures_util::Stream;
use ksni::{self, TrayMethods};

#[cfg(test)]
use crate::config::APP_ID;
use crate::config::APP_NAME;
use crate::desktop::{DesktopErrorCode, DesktopRuntime};
use crate::tray::TraySignal;

const SHOW_CHANNEL_CAPACITY: usize = 64;

/// Counts Show callbacks coalesced while one Show is already queued.
static COALESCED_SHOWS: AtomicU64 = AtomicU64::new(0);

/// Converts compile-time RGBA bytes to ksni ARGB32 network byte order.
///
/// Input is `[r, g, b, a]` per pixel; output is `[a, r, g, b]` per pixel.
/// Returns an error instead of panicking on a truncated buffer.
pub fn rgba_to_argb32(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let expected = width as usize * height as usize * 4;
    if rgba.len() < expected {
        return Err(format!(
            "tray icon buffer too small: got {} bytes, need {expected}",
            rgba.len()
        ));
    }
    let mut argb = Vec::with_capacity(expected);
    for pixel in rgba.chunks_exact(4).take(expected / 4) {
        argb.push(pixel[3]);
        argb.push(pixel[0]);
        argb.push(pixel[1]);
        argb.push(pixel[2]);
    }
    Ok(argb)
}

/// Number of Show callbacks coalesced since process start (diagnostics).
#[allow(dead_code)]
pub fn coalesced_show_count() -> u64 {
    COALESCED_SHOWS.load(Ordering::Relaxed)
}

struct JamePromptTray {
    app_id: String,
    show_tx: tokio::sync::mpsc::Sender<()>,
    control_tx: tokio::sync::mpsc::UnboundedSender<TraySignal>,
    show_pending: Arc<AtomicBool>,
    icon_argb: Vec<u8>,
    icon_width: i32,
    icon_height: i32,
}

impl JamePromptTray {
    fn emit_show(&self) {
        // Coalesce: at most one Show outstanding; the canonical window
        // lifecycle coalesces repeated shows as well.
        if self
            .show_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            if self.show_tx.try_send(()).is_err() {
                COALESCED_SHOWS.fetch_add(1, Ordering::Relaxed);
                self.show_pending.store(false, Ordering::Release);
            }
        } else {
            COALESCED_SHOWS.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn emit_control(&self, signal: TraySignal) {
        let _ = self.control_tx.send(signal);
    }
}

impl ksni::Tray for JamePromptTray {
    fn id(&self) -> String {
        format!("{}.tray", self.app_id)
    }

    fn title(&self) -> String {
        APP_NAME.to_string()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ksni::Icon {
            width: self.icon_width,
            height: self.icon_height,
            data: self.icon_argb.clone(),
        }]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: APP_NAME.to_string(),
            description: "Show JamePrompt".to_string(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.emit_show();
    }

    fn menu(&self) -> Vec<ksni::menu::MenuItem<Self>> {
        let show_tx = self.show_tx.clone();
        let show_pending = Arc::clone(&self.show_pending);
        let control_tx = self.control_tx.clone();
        vec![
            ksni::menu::StandardItem {
                label: format!("Show {APP_NAME}"),
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray;
                    if show_pending
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                        && show_tx.try_send(()).is_err()
                    {
                        COALESCED_SHOWS.fetch_add(1, Ordering::Relaxed);
                        show_pending.store(false, Ordering::Release);
                    }
                }),
                ..Default::default()
            }
            .into(),
            ksni::menu::MenuItem::Separator,
            ksni::menu::StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray;
                    let _ = control_tx.send(TraySignal::QuitRequested);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_online(&self) {
        self.emit_control(TraySignal::HostOnline);
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        // Keep observing: the UI opens a visible fallback window while the
        // host is away instead of trapping the app windowless.
        self.emit_control(TraySignal::HostOffline);
        true
    }
}

struct Slot {
    runtime: Arc<DesktopRuntime>,
    show_rx: Option<tokio::sync::mpsc::Receiver<()>>,
    control_tx: tokio::sync::mpsc::UnboundedSender<TraySignal>,
    control_rx: Option<tokio::sync::mpsc::UnboundedReceiver<TraySignal>>,
    show_pending: Arc<AtomicBool>,
    tray: Option<JamePromptTray>,
}

static SLOTS: OnceLock<Mutex<HashMap<u64, Slot>>> = OnceLock::new();

fn slots() -> &'static Mutex<HashMap<u64, Slot>> {
    SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Persistent iced subscription: creates the SNI service once per instance
/// id and forwards typed signals. Cancellation drops the shutdown trigger
/// so the owned service task shuts down exactly once.
pub fn subscription(instance: u64, runtime: Arc<DesktopRuntime>) -> iced::Subscription<TraySignal> {
    {
        let mut slots = slots().lock().expect("tray slot lock");
        slots.entry(instance).or_insert_with(|| {
            let (show_tx, show_rx) = tokio::sync::mpsc::channel(SHOW_CHANNEL_CAPACITY);
            let (control_tx, control_rx) = tokio::sync::mpsc::unbounded_channel();
            let show_pending = Arc::new(AtomicBool::new(false));
            let icon_argb = rgba_to_argb32(
                &crate::APP_TRAY_ICON.data,
                crate::APP_TRAY_ICON.width,
                crate::APP_TRAY_ICON.height,
            )
            .unwrap_or_default();
            let tray = JamePromptTray {
                app_id: runtime.app_id().to_string(),
                show_tx: show_tx.clone(),
                control_tx: control_tx.clone(),
                show_pending: Arc::clone(&show_pending),
                icon_argb,
                icon_width: crate::APP_TRAY_ICON.width as i32,
                icon_height: crate::APP_TRAY_ICON.height as i32,
            };
            Slot {
                runtime: Arc::clone(&runtime),
                show_rx: Some(show_rx),
                control_tx,
                control_rx: Some(control_rx),
                show_pending,
                tray: Some(tray),
            }
        });
    }
    iced::Subscription::run_with(instance, stream_for_instance)
}

fn stream_for_instance(instance: &u64) -> impl Stream<Item = TraySignal> {
    let (mut show_rx, mut control_rx, shutdown_tx, show_pending) = {
        let mut slots = slots().lock().expect("tray slot lock");
        let slot = slots.get_mut(instance).expect("tray slot registered");
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let tray = slot.tray.take().expect("tray taken once per instance");
        let show_rx = slot.show_rx.take().expect("show receiver taken once");
        let control_rx = slot.control_rx.take().expect("control receiver taken once");
        let show_pending = Arc::clone(&slot.show_pending);
        // Spawn the owned service task now, while a Tokio runtime context
        // (iced's) is available on this subscription thread.
        let control_tx = slot.control_tx.clone();
        let runtime = Arc::clone(&slot.runtime);
        let timeout = std::time::Duration::from_millis(runtime.config().method_timeout_ms);
        tokio::spawn(async move {
            let spawn_result =
                tokio::time::timeout(timeout, tray.assume_sni_available(true).spawn()).await;
            match spawn_result {
                Ok(Ok(handle)) => {
                    let _ = control_tx.send(TraySignal::Ready);
                    // Ends when the stream is dropped (trigger sender
                    // dropped) or the process quits; then shut down the
                    // service and await its confirmation.
                    let _ = shutdown_rx.await;
                    handle.shutdown().await;
                }
                Ok(Err(error)) => {
                    let _ = control_tx.send(TraySignal::Unavailable(map_spawn_error(&error)));
                }
                Err(_) => {
                    let _ =
                        control_tx.send(TraySignal::Unavailable(DesktopErrorCode::TrayHostMissing));
                }
            }
        });
        (show_rx, control_rx, shutdown_tx, show_pending)
    };
    futures_util::stream::poll_fn(move |cx| {
        // Keep the shutdown trigger alive for the stream lifetime: dropping
        // the stream drops this sender, which releases the service task to
        // shut down exactly once.
        let _shutdown_alive = &shutdown_tx;
        // Control channel first: Quit/state is never stuck behind Shows.
        match control_rx.poll_recv(cx) {
            std::task::Poll::Ready(Some(signal)) => {
                return std::task::Poll::Ready(Some(signal));
            }
            std::task::Poll::Ready(None) => {
                // Service task ended (terminal Unavailable or shutdown).
                return std::task::Poll::Ready(None);
            }
            std::task::Poll::Pending => {}
        }
        match show_rx.poll_recv(cx) {
            std::task::Poll::Ready(Some(())) => {
                show_pending.store(false, Ordering::Release);
                std::task::Poll::Ready(Some(TraySignal::ShowRequested))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    })
}

fn map_spawn_error(error: &ksni::Error) -> DesktopErrorCode {
    use ksni::Error::*;
    match error {
        Dbus(_) | Watcher(_) => DesktopErrorCode::TrayHostMissing,
        WontShow => DesktopErrorCode::TrayHostMissing,
        #[allow(unreachable_patterns)]
        _ => DesktopErrorCode::TrayHostMissing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;

    #[test]
    fn rgba_converts_to_argb32_network_order() {
        // One red pixel, opaque: [r,g,b,a] -> [a,r,g,b].
        let argb = rgba_to_argb32(&[0x11, 0x22, 0x33, 0xFF], 1, 1).expect("convert");
        assert_eq!(argb, vec![0xFF, 0x11, 0x22, 0x33]);
    }

    #[test]
    fn rgba_rejects_truncated_buffer() {
        assert!(rgba_to_argb32(&[1, 2, 3], 1, 1).is_err());
    }

    #[test]
    fn show_flood_coalesces_and_quit_is_independent() {
        let (show_tx, mut show_rx) = tokio::sync::mpsc::channel::<()>(SHOW_CHANNEL_CAPACITY);
        let (control_tx, mut control_rx) = tokio::sync::mpsc::unbounded_channel::<TraySignal>();
        let tray = JamePromptTray {
            app_id: APP_ID.to_string(),
            show_tx,
            control_tx,
            show_pending: Arc::new(AtomicBool::new(false)),
            icon_argb: Vec::new(),
            icon_width: 0,
            icon_height: 0,
        };
        let mut tray = tray;
        for _ in 0..1000 {
            tray.activate(0, 0);
        }
        // Quit is emitted on the independent control channel.
        tray.emit_control(TraySignal::QuitRequested);
        assert_eq!(
            control_rx.try_recv(),
            Ok(TraySignal::QuitRequested),
            "a Show flood must never drop Quit"
        );
        let queued_shows = {
            let mut count = 0;
            while show_rx.try_recv().is_ok() {
                count += 1;
            }
            count
        };
        assert_eq!(queued_shows, 1, "flooded Shows coalesce to one pending");
    }

    #[test]
    fn icon_conversion_matches_embedded_asset_dimensions() {
        let argb = rgba_to_argb32(
            &crate::APP_TRAY_ICON.data,
            crate::APP_TRAY_ICON.width,
            crate::APP_TRAY_ICON.height,
        )
        .expect("embedded icon converts");
        assert_eq!(
            argb.len(),
            crate::APP_TRAY_ICON.width as usize * crate::APP_TRAY_ICON.height as usize * 4
        );
    }
}
