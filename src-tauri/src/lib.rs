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

mod midi;
mod midi_out;
mod tape;
use midi::Preset;
use midi_out::{MidiOut, MidiPorts, MidiStatus, OutputChoice, PresetInfo};
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
    /// MIDI output state; MIDI follows whatever this snapshot shows (live input or a replay).
    midi: MidiStatus,
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

type SharedMidi = Arc<Mutex<MidiOut>>;

/// The preset being played and where MIDI goes, for the MIDI tab to start from.
#[derive(Serialize)]
struct MidiSetup {
    preset: Preset,
    output: OutputChoice,
}

#[tauri::command]
fn midi_setup(midi: State<'_, SharedMidi>) -> MidiSetup {
    let midi = midi.lock().unwrap();
    MidiSetup { preset: midi.preset().clone(), output: midi.output().clone() }
}

/// MIDI output ports on this computer.
#[tauri::command]
async fn midi_ports() -> Result<MidiPorts, String> {
    MidiOut::ports()
}

#[tauri::command]
async fn midi_set_output(output: OutputChoice, midi: State<'_, SharedMidi>) -> Result<(), String> {
    midi.lock().unwrap().set_output(output);
    Ok(())
}

/// Play `preset` from now on (the MIDI tab sends it after every edit).
#[tauri::command]
fn midi_set_preset(preset: Preset, midi: State<'_, SharedMidi>) {
    midi.lock().unwrap().set_preset(preset);
}

#[tauri::command]
fn midi_panic(midi: State<'_, SharedMidi>) {
    midi.lock().unwrap().panic();
}

/// Start (or with `on` false, stop) waiting for an input to move; the snapshot reports it.
#[tauri::command]
fn midi_learn(on: bool, midi: State<'_, SharedMidi>) {
    let mut midi = midi.lock().unwrap();
    if on { midi.start_learn() } else { midi.cancel_learn() }
}

#[tauri::command]
async fn midi_list_presets(midi: State<'_, SharedMidi>) -> Result<Vec<PresetInfo>, String> {
    Ok(midi.lock().unwrap().list_presets())
}

/// Load a preset and start playing it; returns it for the editor.
#[tauri::command]
async fn midi_load_preset(name: String, builtin: bool, midi: State<'_, SharedMidi>) -> Result<Preset, String> {
    let mut midi = midi.lock().unwrap();
    let preset = midi.load_preset(&name, builtin)?;
    midi.set_preset(preset.clone());
    Ok(preset)
}

/// Save the preset being played under `name`.
#[tauri::command]
async fn midi_save_preset(name: String, midi: State<'_, SharedMidi>) -> Result<(), String> {
    midi.lock().unwrap().save_preset(&name)
}

#[tauri::command]
async fn midi_delete_preset(name: String, midi: State<'_, SharedMidi>) -> Result<(), String> {
    midi.lock().unwrap().delete_preset(&name)
}

/// Open the saved presets folder in the file manager.
#[tauri::command]
async fn midi_reveal_presets(app: AppHandle, midi: State<'_, SharedMidi>) -> Result<(), String> {
    let dir = midi.lock().unwrap().presets_folder()?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

/// A [`Snapshot`] built up from reports, one for live input and one for a replayed recording.
#[derive(Default)]
struct Fold {
    snap: Snapshot,
    /// State reports since the report rate was last updated.
    rate_count: u32,
}

impl Fold {
    /// Fold in one report; returns the state if it was a state report.
    fn packet(&mut self, slot: &str, data: &[u8]) -> Option<ControllerState> {
        let snap = &mut self.snap;
        match protocol::parse(data) {
            Some(Report::State(s)) => {
                snap.slot = Some(slot.to_string());
                snap.state = Some(s);
                snap.raw = data.to_vec();
                self.rate_count += 1;
                return Some(s);
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
        None
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

fn forward_events(app: AppHandle, rx: mpsc::Receiver<HubEvent>, tape: SharedTape, midi: SharedMidi) {
    let steam_running = Arc::new(AtomicBool::new(false));
    let flag = steam_running.clone();
    thread::spawn(move || watch_steam(flag));

    let mut live = Fold::default();
    let mut replay = Fold::default();
    let mut last_emit = Instant::now();
    let mut rate_window = Instant::now();
    // MIDI follows what the UI shows: live input, or the recording while one plays.
    let mut playing = false;
    loop {
        // Wake up in time for the next replayed report.
        let wait = tape.lock().unwrap().next_due().map_or(EMIT_INTERVAL, |d| d.min(EMIT_INTERVAL));
        match rx.recv_timeout(wait) {
            Ok(HubEvent::Packet { slot, data }) => {
                let state = live.packet(&slot, &data);
                tape.lock().unwrap().record(&slot, &data);
                if let (Some(s), false) = (state, playing) {
                    midi.lock().unwrap().process(&s);
                }
                if live.snap.state.is_none() && !playing {
                    midi.lock().unwrap().release_all();
                }
            }
            Ok(HubEvent::Closed { slot, .. }) => {
                live.closed(&slot);
                if live.snap.state.is_none() && !playing {
                    midi.lock().unwrap().release_all();
                }
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let mut deck = tape.lock().unwrap();
        if deck.take_reset() {
            replay = Fold::default();
            // A new recording or a seek: notes held at the old position would hang.
            midi.lock().unwrap().release_all();
            if let Some((slot, data)) = deck.first_battery() {
                replay.packet(slot, data);
            }
        }
        let now_playing = deck.playing();
        if now_playing != playing {
            playing = now_playing;
            midi.lock().unwrap().release_all();
        }
        deck.replay(|slot, data| {
            if let Some(s) = replay.packet(slot, data)
                && playing
            {
                midi.lock().unwrap().process(&s);
            }
        });
        replay.snap.report_rate = deck.replay_rate();
        let status = (last_emit.elapsed() >= EMIT_INTERVAL).then(|| deck.status());
        drop(deck);
        // Outside the tape lock: reconnecting to a port can take a moment.
        midi.lock().unwrap().tick();
        if rate_window.elapsed() >= Duration::from_secs(1) {
            live.update_rate();
            rate_window = Instant::now();
        }
        if let Some(status) = status {
            last_emit = Instant::now();
            let snap = if playing { &mut replay.snap } else { &mut live.snap };
            snap.steam_running = steam_running.load(Ordering::Relaxed);
            snap.tape = status;
            snap.midi = midi.lock().unwrap().status();
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
            midi_setup,
            midi_ports,
            midi_set_output,
            midi_set_preset,
            midi_panic,
            midi_learn,
            midi_list_presets,
            midi_load_preset,
            midi_save_preset,
            midi_delete_preset,
            midi_reveal_presets,
        ])
        .setup(|app| {
            let (tx, rx) = mpsc::channel();
            app.manage(hub::spawn(tx, HubOptions::default()));
            let tape: SharedTape = Arc::new(Mutex::new(Tape::new(app.path().app_data_dir()?.join("recordings"))));
            app.manage(tape.clone());
            let midi: SharedMidi = Arc::new(Mutex::new(MidiOut::new(app.path().app_data_dir()?.join("midi"))));
            app.manage(midi.clone());
            let handle = app.handle().clone();
            thread::spawn(move || forward_events(handle, rx, tape, midi));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
