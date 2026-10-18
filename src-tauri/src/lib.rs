use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use sc_core::hub::{self, HubOptions};
use sc_core::protocol::{self, BatteryStatus, Haptic, Report};
use sc_core::{Command, ControllerState, Hub, HubEvent};
use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State};

/// How often the UI gets a fresh snapshot (~60 fps). The controller itself reports at ~250 Hz.
const EMIT_INTERVAL: Duration = Duration::from_millis(16);

/// Everything the UI needs, sent as the `controller` event.
#[derive(Debug, Clone, Default, Serialize)]
struct Snapshot {
    /// HID interface the state came from, e.g. "1304:2" (dongle slot 2).
    slot: Option<String>,
    state: Option<ControllerState>,
    battery: Option<BatteryStatus>,
    /// Input reports received per second over the last second.
    report_rate: u32,
    /// Bytes of the latest state report, report id first.
    raw: Vec<u8>,
    /// Report ids the parser does not decode yet, with their latest bytes.
    unknown_reports: Vec<UnknownReport>,
    /// Steam is running. Its Desktop Layout turns controller input into mouse/keyboard input,
    /// which we cannot switch off from here (our lizard-mode-off only stops the firmware's own).
    steam_running: bool,
}

const STEAM_PROCESS: &str = if cfg!(windows) {
    "steam.exe"
} else if cfg!(target_os = "macos") {
    "steam_osx"
} else {
    "steam"
};

fn watch_steam(flag: Arc<AtomicBool>) {
    loop {
        // A fresh list each time: reusing one System kept exited processes listed on Windows.
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        let running = sys.processes_by_exact_name(OsStr::new(STEAM_PROCESS)).next().is_some();
        flag.store(running, Ordering::Relaxed);
        thread::sleep(Duration::from_secs(3));
    }
}

#[derive(Debug, Clone, Serialize)]
struct UnknownReport {
    id: u8,
    count: u64,
    data: Vec<u8>,
}

/// Ask the controller on `slot` (as in the snapshot) to power off.
#[tauri::command]
async fn turn_off_controller(slot: String, hub: State<'_, Hub>) -> Result<(), String> {
    let (reply, rx) = mpsc::channel();
    hub.send(&slot, Command::TurnOff { reply })?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "The controller did not respond".to_string())?
}

/// Play a haptic effect on the controller on `slot` and return the report's bytes. A rumble
/// keeps playing until a rumble with both speeds 0 is sent.
#[tauri::command]
async fn play_haptic(slot: String, haptic: Haptic, hub: State<'_, Hub>) -> Result<Vec<u8>, String> {
    let (reply, rx) = mpsc::channel();
    hub.send(&slot, Command::Haptic { haptic, reply })?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "The controller did not respond".to_string())??;
    Ok(protocol::haptic_report(&haptic))
}

fn forward_events(app: AppHandle, rx: mpsc::Receiver<HubEvent>) {
    let steam_running = Arc::new(AtomicBool::new(false));
    let flag = steam_running.clone();
    thread::spawn(move || watch_steam(flag));

    let mut snap = Snapshot::default();
    let mut last_emit = Instant::now();
    let mut rate_window = Instant::now();
    let mut rate_count = 0u32;
    loop {
        match rx.recv_timeout(EMIT_INTERVAL) {
            Ok(HubEvent::Packet { slot, data }) => match protocol::parse(&data) {
                Some(Report::State(s)) => {
                    snap.slot = Some(slot);
                    snap.state = Some(s);
                    snap.raw = data;
                    rate_count += 1;
                }
                Some(Report::Battery(b)) => snap.battery = Some(b),
                Some(Report::Wireless(false)) if snap.slot.as_deref() == Some(slot.as_str()) => {
                    snap.slot = None;
                    snap.state = None;
                }
                Some(Report::Other(id)) => match snap.unknown_reports.iter_mut().find(|u| u.id == id) {
                    Some(u) => {
                        u.count += 1;
                        u.data = data;
                    }
                    None => snap.unknown_reports.push(UnknownReport { id, count: 1, data }),
                },
                _ => {}
            },
            Ok(HubEvent::Closed { slot, .. }) if snap.slot.as_deref() == Some(slot.as_str()) => {
                snap.slot = None;
                snap.state = None;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if rate_window.elapsed() >= Duration::from_secs(1) {
            snap.report_rate = rate_count;
            rate_count = 0;
            rate_window = Instant::now();
        }
        if last_emit.elapsed() >= EMIT_INTERVAL {
            last_emit = Instant::now();
            snap.steam_running = steam_running.load(Ordering::Relaxed);
            let _ = app.emit("controller", &snap);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![turn_off_controller, play_haptic])
        .setup(|app| {
            let (tx, rx) = mpsc::channel();
            app.manage(hub::spawn(tx, HubOptions::default()));
            let handle = app.handle().clone();
            thread::spawn(move || forward_events(handle, rx));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
