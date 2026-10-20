//! Background mode: a tray icon, closing the window to the tray, starting with the system, and
//! what the UI can see. Closing the window destroys the webview (most of the app's memory and,
//! while it draws, most of its CPU), so in the tray only the device layer and the outputs run.
//! While no window is visible, `forward_events` sends the UI nothing and outputs that are off
//! skip their engines (see `outputs::Watched`).

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WebviewWindowBuilder, Window, WindowEvent, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::midi_out::{MidiStatus, OutputChoice};
use crate::osc_out::OscStatus;
use crate::outputs::Watched;
use crate::play_out::{PlayStatus, Slot};
use crate::devices::PadSupport;

/// Command-line flag for starting in the tray without a window; autostart passes it.
pub const TRAY_ARG: &str = "--tray";

const WINDOW: &str = "main";

/// What the UI can see: whether a window is open and not minimised, and its tab.
#[derive(Debug, Default)]
pub struct View {
    visible: bool,
    tab: String,
}

pub type SharedView = Arc<Mutex<View>>;

impl View {
    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn watched(&self) -> Watched {
        let on = |id: &str| self.visible && self.tab == id;
        Watched { midi: on("midi"), osc: on("osc"), keymouse: on("keymouse"), gamepad: on("gamepad") }
    }
}

/// Keeps `View::visible` up to date; registered with `Builder::on_window_event`.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let visible = match event {
        // Minimising and restoring both come as resizes.
        WindowEvent::Resized(_) => !window.is_minimized().unwrap_or(false),
        WindowEvent::Destroyed => false,
        _ => return,
    };
    window.state::<SharedView>().lock().unwrap().visible = visible;
}

/// The frontend reports the tab it shows.
#[tauri::command]
pub fn set_tab(tab: String, view: State<'_, SharedView>) {
    view.lock().unwrap().tab = tab;
}

/// Open the window, or bring it to the front if it is already open.
pub fn open_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(WINDOW) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let Some(config) = app.config().app.windows.iter().find(|w| w.label == WINDOW) else { return };
    match WebviewWindowBuilder::from_config(app, config).and_then(|b| b.build()) {
        Ok(w) => {
            let _ = w.set_focus();
            app.state::<SharedView>().lock().unwrap().visible = true;
        }
        Err(e) => eprintln!("couldn't open the window: {e}"),
    }
}

/// Saved between runs in `<app data>/settings.json`. Starting with the system is not in here:
/// the autostart plugin asks the OS (registry, LaunchAgent, autostart entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    close_to_tray: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Saved { close_to_tray: true }
    }
}

pub struct Settings {
    path: PathBuf,
    saved: Saved,
}

pub type SharedSettings = Arc<Mutex<Settings>>;

impl Settings {
    pub fn load(path: PathBuf) -> Self {
        let saved = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Settings { path, saved }
    }

    pub fn close_to_tray(&self) -> bool {
        self.saved.close_to_tray
    }

    fn save(&self) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(&self.saved).unwrap()).map_err(|e| e.to_string())
    }
}

/// What the Settings tab shows; mirrored in `src/tabs/settings.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundSettings {
    close_to_tray: bool,
    autostart: bool,
}

#[tauri::command]
pub fn background_settings(app: AppHandle, settings: State<'_, SharedSettings>) -> BackgroundSettings {
    BackgroundSettings {
        close_to_tray: settings.lock().unwrap().close_to_tray(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

#[tauri::command]
pub fn set_close_to_tray(on: bool, settings: State<'_, SharedSettings>) -> Result<(), String> {
    let mut s = settings.lock().unwrap();
    s.saved.close_to_tray = on;
    s.save()
}

#[tauri::command]
pub fn set_autostart(on: bool, app: AppHandle) -> Result<(), String> {
    let launcher = app.autolaunch();
    if on { launcher.enable() } else { launcher.disable() }.map_err(|e| e.to_string())
}

/// The tray icon's menu: five status lines (not clickable), Open and Quit.
#[derive(Clone)]
pub struct Tray {
    controller: MenuItem<Wry>,
    midi: MenuItem<Wry>,
    osc: MenuItem<Wry>,
    keymouse: MenuItem<Wry>,
    gamepad: MenuItem<Wry>,
}

impl Tray {
    pub fn create(app: &AppHandle) -> tauri::Result<Tray> {
        let status = |id: &str, text: &str| MenuItem::with_id(app, id, text, false, None::<&str>);
        let tray = Tray {
            controller: status("controller", "No controller")?,
            midi: status("midi", "MIDI: off")?,
            osc: status("osc", "OSC: off")?,
            keymouse: status("keymouse", "Keyboard & mouse: off")?,
            gamepad: status("gamepad", "Gamepad: off")?,
        };
        let menu = Menu::with_items(
            app,
            &[
                &tray.controller,
                &tray.midi,
                &tray.osc,
                &tray.keymouse,
                &tray.gamepad,
                &PredefinedMenuItem::separator(app)?,
                &MenuItem::with_id(app, "open", "Open Steam Controller Toolkit", true, None::<&str>)?,
                &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
            ],
        )?;
        let mut icon = TrayIconBuilder::with_id("main")
            .tooltip("Steam Controller Toolkit")
            .menu(&menu)
            // Left click opens the window; the menu is on the right button.
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id.as_ref() {
                "open" => open_window(app),
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                    open_window(tray.app_handle());
                }
            });
        if let Some(i) = app.default_window_icon() {
            icon = icon.icon(i.clone());
        }
        icon.build(app)?;
        Ok(tray)
    }

    /// Called about once a second from `forward_events`; the menu only changes when a line does.
    pub fn update(&self, lines: &TrayLines) {
        let items = [
            (&self.controller, &lines.controller),
            (&self.midi, &lines.midi),
            (&self.osc, &lines.osc),
            (&self.keymouse, &lines.keymouse),
            (&self.gamepad, &lines.gamepad),
        ];
        for (item, text) in items {
            if item.text().ok().as_ref() != Some(text) {
                let _ = item.set_text(text);
            }
        }
    }
}

/// The tray menu's status lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrayLines {
    pub controller: String,
    pub midi: String,
    pub osc: String,
    pub keymouse: String,
    pub gamepad: String,
}

impl TrayLines {
    pub fn new(slot: Option<&str>, battery: Option<u8>, midi: &MidiStatus, osc: &OscStatus, play: &PlayStatus) -> Self {
        let controller = match slot {
            None => "No controller".to_string(),
            Some(slot) => {
                let battery = battery.map(|b| format!(" · {b}%")).unwrap_or_default();
                format!("Controller: {}{battery}", describe_slot(slot))
            }
        };
        let midi = match (&midi.output, &midi.connected) {
            (OutputChoice::Off, _) => "MIDI: off".to_string(),
            (_, Some(port)) => format!("MIDI: sending to {port}"),
            _ if midi.connecting => "MIDI: connecting…".to_string(),
            _ => "MIDI: not connected".to_string(),
        };
        let sending: Vec<&str> = osc.destinations.iter().filter_map(|d| d.address.as_deref()).collect();
        let osc = match sending.as_slice() {
            _ if !osc.enabled => "OSC: off".to_string(),
            [] if osc.destinations.iter().any(|d| d.resolving || d.error.is_some()) => "OSC: not connected".to_string(),
            [] => "OSC: off".to_string(),
            [one] => format!("OSC: sending to {one}"),
            many => format!("OSC: sending to {} destinations", many.len()),
        };
        let keymouse = match play.active {
            _ if !play.keymouse.enabled => "Keyboard & mouse: off",
            _ if play.keymouse.error.is_some() => "Keyboard & mouse: blocked",
            Some(Slot::Desktop) => "Keyboard & mouse: Desktop layout",
            Some(Slot::Game) => "Keyboard & mouse: Game layout",
            // Both on, in Game mode: the gamepad plays.
            _ => "Keyboard & mouse: on, gamepad in play",
        }
        .to_string();
        let pad = &play.gamepad;
        let gamepad = match (&pad.support, pad.player) {
            _ if !pad.enabled => "Gamepad: off".to_string(),
            (Some(PadSupport::Missing { .. }), _) => "Gamepad: driver missing".to_string(),
            (Some(PadSupport::Unsupported), _) => "Gamepad: not available".to_string(),
            _ if !pad.connected && pad.error.is_some() => "Gamepad: not connected".to_string(),
            _ if !pad.connected => "Gamepad: connecting…".to_string(),
            (_, Some(n)) => format!("Gamepad: on · player {n}"),
            _ => "Gamepad: on".to_string(),
        };
        TrayLines { controller, midi, osc, keymouse, gamepad }
    }
}

/// Readable form of a hub slot such as "1304:2"; mirrors `describeSlot` in `src/controller.ts`.
fn describe_slot(slot: &str) -> String {
    let (pid, iface) = slot.split_once(':').unwrap_or((slot, ""));
    let iface: u8 = iface.parse().unwrap_or(0);
    let (short, dongle) = match u16::from_str_radix(pid, 16) {
        Ok(0x1302) => ("USB", false),
        Ok(0x1303) => ("Bluetooth", false),
        Ok(0x1304) => ("Puck", true),
        Ok(0x1305) => ("Dongle", true),
        _ => return slot.to_string(),
    };
    // Dongle interfaces 2..=5 are its four controller slots.
    if dongle && iface >= 2 { format!("{short} · slot {}", iface - 1) } else { short.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots() {
        assert_eq!(describe_slot("1304:2"), "Puck · slot 1");
        assert_eq!(describe_slot("1302:0"), "USB");
        assert_eq!(describe_slot("abcd:1"), "abcd:1");
    }

    #[test]
    fn view_watches_only_the_visible_tab() {
        let mut v = View { visible: true, tab: "midi".into() };
        assert_eq!(v.watched(), Watched { midi: true, ..Default::default() });
        v.tab = "gamepad".into();
        assert_eq!(v.watched(), Watched { gamepad: true, ..Default::default() });
        v.visible = false;
        assert_eq!(v.watched(), Watched::default());
    }
}
