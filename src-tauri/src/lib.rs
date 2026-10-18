use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sc_core::hub::{self, HubOptions};
use sc_core::protocol::{self, BatteryStatus, Haptic, Report};
use sc_core::{Command, ControllerState, Hub, HubEvent};
use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

mod tape;
use tape::{PlaybackAction, RecordingInfo, Tape, TapeStatus};

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
    /// Recorder and player state. While a recording plays, the rest of the snapshot is replayed.
    tape: TapeStatus,
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

type SharedTape = Arc<Mutex<Tape>>;

/// Start recording every live report to a new file named `name`; returns the name used
/// (numbered if `name` was taken).
#[tauri::command]
fn start_recording(name: String, tape: State<'_, SharedTape>) -> Result<String, String> {
    tape.lock().unwrap().start_recording(&name)
}

#[tauri::command]
fn stop_recording(tape: State<'_, SharedTape>) -> Result<(), String> {
    tape.lock().unwrap().stop_recording()
}

/// Recordings in the recordings folder, newest first.
#[tauri::command]
async fn list_recordings(tape: State<'_, SharedTape>) -> Result<Vec<RecordingInfo>, String> {
    Ok(tape.lock().unwrap().list())
}

/// Replay a recording in place of live input.
#[tauri::command]
async fn play_recording(name: String, tape: State<'_, SharedTape>) -> Result<(), String> {
    tape.lock().unwrap().play(&name)
}

#[tauri::command]
fn control_playback(action: PlaybackAction, tape: State<'_, SharedTape>) -> Result<(), String> {
    tape.lock().unwrap().control(action)
}

#[tauri::command]
async fn delete_recording(name: String, tape: State<'_, SharedTape>) -> Result<(), String> {
    tape.lock().unwrap().delete(&name)
}

/// Show a recording in the file manager, or open the recordings folder if `name` is None.
#[tauri::command]
async fn reveal_recording(name: Option<String>, app: AppHandle, tape: State<'_, SharedTape>) -> Result<(), String> {
    // Look the path up, then let go of the tape: the file manager can take seconds to answer,
    // and the emit loop needs the lock meanwhile.
    let (file, dir) = {
        let tape = tape.lock().unwrap();
        (name.map(|name| tape.file(&name)).transpose()?, tape.dir().to_path_buf())
    };
    match file {
        Some(file) => app.opener().reveal_item_in_dir(file),
        None => {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            app.opener().open_path(dir.to_string_lossy(), None::<&str>)
        }
    }
    .map_err(|e| e.to_string())
}

/// Copy recordings dropped on the window into the recordings folder; returns their names.
#[tauri::command]
async fn import_recordings(paths: Vec<PathBuf>, tape: State<'_, SharedTape>) -> Result<Vec<String>, String> {
    let mut tape = tape.lock().unwrap();
    paths.iter().map(|p| tape.import(p)).collect()
}

/// A [`Snapshot`] built up from reports, one for live input and one for a replayed recording.
#[derive(Default)]
struct Fold {
    snap: Snapshot,
    /// State reports since the report rate was last updated.
    rate_count: u32,
}

impl Fold {
    fn packet(&mut self, slot: &str, data: &[u8]) {
        let snap = &mut self.snap;
        match protocol::parse(data) {
            Some(Report::State(s)) => {
                snap.slot = Some(slot.to_string());
                snap.state = Some(s);
                snap.raw = data.to_vec();
                self.rate_count += 1;
            }
            Some(Report::Battery(b)) => snap.battery = Some(b),
            Some(Report::Wireless(false)) => self.closed(slot),
            Some(Report::Other(id)) => match snap.unknown_reports.iter_mut().find(|u| u.id == id) {
                Some(u) => {
                    u.count += 1;
                    u.data = data.to_vec();
                }
                None => snap.unknown_reports.push(UnknownReport { id, count: 1, data: data.to_vec() }),
            },
            _ => {}
        }
    }

    fn closed(&mut self, slot: &str) {
        if self.snap.slot.as_deref() == Some(slot) {
            self.snap.slot = None;
            self.snap.state = None;
        }
    }

    fn update_rate(&mut self) {
        self.snap.report_rate = std::mem::take(&mut self.rate_count);
    }
}

fn forward_events(app: AppHandle, rx: mpsc::Receiver<HubEvent>, tape: SharedTape) {
    let steam_running = Arc::new(AtomicBool::new(false));
    let flag = steam_running.clone();
    thread::spawn(move || watch_steam(flag));

    let mut live = Fold::default();
    let mut replay = Fold::default();
    let mut last_emit = Instant::now();
    let mut rate_window = Instant::now();
    loop {
        // Wake up in time for the next replayed report.
        let wait = tape.lock().unwrap().next_due().map_or(EMIT_INTERVAL, |d| d.min(EMIT_INTERVAL));
        match rx.recv_timeout(wait) {
            Ok(HubEvent::Packet { slot, data }) => {
                live.packet(&slot, &data);
                tape.lock().unwrap().record(&slot, &data);
            }
            Ok(HubEvent::Closed { slot, .. }) => live.closed(&slot),
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let mut deck = tape.lock().unwrap();
        if deck.take_reset() {
            replay = Fold::default();
        }
        deck.replay(|slot, data| replay.packet(slot, data));
        let playing = deck.playing();
        let status = (last_emit.elapsed() >= EMIT_INTERVAL).then(|| deck.status());
        drop(deck);
        if rate_window.elapsed() >= Duration::from_secs(1) {
            live.update_rate();
            replay.update_rate();
            rate_window = Instant::now();
        }
        if let Some(status) = status {
            last_emit = Instant::now();
            let snap = if playing { &mut replay.snap } else { &mut live.snap };
            snap.steam_running = steam_running.load(Ordering::Relaxed);
            snap.tape = status;
            let _ = app.emit("controller", &*snap);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            turn_off_controller,
            play_haptic,
            start_recording,
            stop_recording,
            list_recordings,
            play_recording,
            control_playback,
            delete_recording,
            reveal_recording,
            import_recordings,
        ])
        .setup(|app| {
            let (tx, rx) = mpsc::channel();
            app.manage(hub::spawn(tx, HubOptions::default()));
            let tape: SharedTape = Arc::new(Mutex::new(Tape::new(app.path().app_data_dir()?.join("recordings"))));
            app.manage(tape.clone());
            let handle = app.handle().clone();
            thread::spawn(move || forward_events(handle, rx, tape));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
