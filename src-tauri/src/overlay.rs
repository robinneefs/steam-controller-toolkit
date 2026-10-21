//! The on-screen Command Deck menu: a small transparent window (`overlay.html`) that shows the
//! wheel or grid of the stick or trackpad being touched, centred on the screen with the pointer.
//!
//! It must never take the keyboard from the program the shortcuts are for, so it is made once,
//! unfocused, not focusable and click-through, and never shown or hidden again (showing can
//! activate a window on Windows): with no menu open it waits off-screen, which also keeps it from
//! covering a full-screen game. It is made ahead, while the deck has a menu and the option is on,
//! so the first touch shows at once; that costs a webview (memory, no CPU while nothing changes).

use std::sync::mpsc;
use std::thread;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

use crate::play_out::OverlayMenu;

pub const LABEL: &str = "overlay";
const EVENT: &str = "deck-menu";
/// Logical size; menus are drawn to fit.
const SIZE: f64 = 460.0;
/// Where the window waits with no menu open.
const AWAY: PhysicalPosition<i32> = PhysicalPosition { x: -32000, y: -32000 };

pub struct Overlay {
    app: AppHandle,
    window: Option<WebviewWindow>,
    making: Option<mpsc::Receiver<Result<WebviewWindow, String>>>,
    shown: bool,
    /// The menu to show once the window is made.
    pending: Option<OverlayMenu>,
}

impl Overlay {
    pub fn new(app: AppHandle) -> Self {
        Overlay { app, window: None, making: None, shown: false, pending: None }
    }

    /// Make the window, or close it, to match whether the deck wants one.
    pub fn set_wanted(&mut self, wanted: bool) {
        if let Some(rx) = &self.making {
            match rx.try_recv() {
                Ok(Ok(w)) => {
                    self.making = None;
                    self.window = Some(w);
                    if let Some(menu) = self.pending.take() {
                        self.show(Some(menu));
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("couldn't make the on-screen menu: {e}");
                    self.making = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.making = None,
            }
        }
        if wanted && self.window.is_none() && self.making.is_none() {
            // Off the caller's thread: making a webview takes a moment.
            let (tx, rx) = mpsc::channel();
            let app = self.app.clone();
            thread::spawn(move || {
                let _ = tx.send(make(&app));
            });
            self.making = Some(rx);
        } else if !wanted {
            self.pending = None;
            self.shown = false;
            if let Some(w) = self.window.take() {
                let _ = w.destroy();
            }
        }
    }

    /// Show a menu, or put the window away (None).
    pub fn show(&mut self, menu: Option<OverlayMenu>) {
        let Some(w) = &self.window else {
            self.pending = menu;
            return;
        };
        // The page draws the menu (or nothing) before the window moves.
        let _ = self.app.emit_to(LABEL, EVENT, &menu);
        match menu {
            Some(_) if !self.shown => {
                if let Some(at) = centre(&self.app, w) {
                    let _ = w.set_position(at);
                    self.shown = true;
                }
            }
            None if self.shown => {
                let _ = w.set_position(AWAY);
                self.shown = false;
            }
            _ => {}
        }
    }
}

/// The window's top left for it to sit in the middle of the screen with the pointer.
fn centre(app: &AppHandle, w: &WebviewWindow) -> Option<PhysicalPosition<i32>> {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let size = w.outer_size().ok()?;
    let (pos, area) = (monitor.position(), monitor.size());
    Some(PhysicalPosition {
        x: pos.x + (area.width as i32 - size.width as i32) / 2,
        y: pos.y + (area.height as i32 - size.height as i32) / 2,
    })
}

#[cfg(not(target_os = "macos"))]
fn make(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Ok(w);
    }
    let w = tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("overlay.html".into()))
        .title("Command Deck menu")
        .inner_size(SIZE, SIZE)
        .position(AWAY.x as f64, AWAY.y as f64)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(true)
        .build()
        .map_err(|e| e.to_string())?;
    w.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
    Ok(w)
}

/// Keyboard and mouse output (and so the deck) isn't on macOS yet.
#[cfg(target_os = "macos")]
fn make(_app: &AppHandle) -> Result<WebviewWindow, String> {
    Err("The on-screen menu isn't available on macOS yet".into())
}
