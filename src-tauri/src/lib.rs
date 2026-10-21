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
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tauri_plugin_opener::OpenerExt;

mod devices;
#[cfg(windows)]
mod inject_win;
mod layout;
mod midi;
#[cfg(windows)]
#[allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, clippy::all)]
mod midi2_bindings;
mod midi_out;
#[cfg(windows)]
mod midi_win;
mod osc;
mod osc_out;
mod outputs;
mod play_out;
mod shaping;
mod tape;
mod tray;
#[cfg(target_os = "linux")]
mod uinput;
#[cfg(windows)]
mod vigem;
use midi::Preset;
use midi_out::{MidiOut, MidiPorts, MidiStatus, OutputChoice, PresetInfo};
use osc_out::{Destination, OscOut, OscStatus};
use outputs::{Outputs, SharedMidi, SharedOsc, SharedPlay};
use play_out::{GamepadConfig, KeyMouseConfig, Mode, PlayOut, PlayStatus, PresetKind, Slot};
use tape::{PlaybackAction, RecordingInfo, Tape, TapeStatus};
use tray::{SharedSettings, SharedView, Tray, TrayLines};

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
    /// OSC output state; like MIDI, it follows whatever this snapshot shows.
    osc: OscStatus,
    /// Keyboard & mouse and Gamepad output state; they follow this snapshot too.
    play: PlayStatus,
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
    // Read the file without holding the tape: the emit loop needs it for live input meanwhile.
    let file = tape.lock().unwrap().file(&name)?;
    let packets = sc_core::recording::read_file(&file)?;
    tape.lock().unwrap().play(&name, packets)
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
    paths.iter().try_for_each(|p| tape::check_import(p))?;
    let mut tape = tape.lock().unwrap();
    paths.iter().map(|p| tape.import(p)).collect()
}

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

/// Where OSC goes: every destination with its preset, for the OSC tab to start from.
#[tauri::command]
fn osc_destinations(osc: State<'_, SharedOsc>) -> Vec<Destination> {
    osc.lock().unwrap().destinations()
}

/// Replace the destinations (the OSC tab sends the whole list after every edit).
#[tauri::command]
fn osc_set_destinations(destinations: Vec<Destination>, osc: State<'_, SharedOsc>) {
    osc.lock().unwrap().set_destinations(destinations);
}

/// The OSC tab's on/off switch.
#[tauri::command]
fn osc_set_enabled(on: bool, osc: State<'_, SharedOsc>) {
    osc.lock().unwrap().set_enabled(on);
}

/// Start (or with `on` false, stop) waiting for an input to move; the snapshot reports it.
#[tauri::command]
fn osc_learn(on: bool, osc: State<'_, SharedOsc>) {
    let mut osc = osc.lock().unwrap();
    if on { osc.start_learn() } else { osc.cancel_learn() }
}

#[tauri::command]
async fn osc_list_presets(osc: State<'_, SharedOsc>) -> Result<Vec<osc_out::PresetInfo>, String> {
    Ok(osc.lock().unwrap().list_presets())
}

/// A preset for the editor to put in a destination (it applies once the tab sends the list).
#[tauri::command]
async fn osc_load_preset(name: String, builtin: bool, osc: State<'_, SharedOsc>) -> Result<osc::Preset, String> {
    osc.lock().unwrap().load_preset(&name, builtin)
}

#[tauri::command]
async fn osc_save_preset(name: String, preset: osc::Preset, osc: State<'_, SharedOsc>) -> Result<(), String> {
    osc.lock().unwrap().save_preset(&name, preset)
}

#[tauri::command]
async fn osc_delete_preset(name: String, osc: State<'_, SharedOsc>) -> Result<(), String> {
    osc.lock().unwrap().delete_preset(&name)
}

/// Open the saved OSC presets folder in the file manager.
#[tauri::command]
async fn osc_reveal_presets(app: AppHandle, osc: State<'_, SharedOsc>) -> Result<(), String> {
    let dir = osc.lock().unwrap().presets_folder()?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

/// The layouts and settings the Keyboard & mouse and Gamepad tabs start from.
#[derive(Serialize)]
struct PlaySetup {
    #[serde(flatten)]
    config: play_out::Config,
    mode: Mode,
}

#[tauri::command]
fn play_setup(play: State<'_, SharedPlay>) -> PlaySetup {
    let play = play.lock().unwrap();
    PlaySetup { config: play.config().clone(), mode: play.mode() }
}

/// Keyboard & mouse on/off and its two layouts (the tab sends them after every edit).
#[tauri::command]
fn play_set_keymouse(config: KeyMouseConfig, play: State<'_, SharedPlay>) {
    play.lock().unwrap().set_keymouse(config);
}

/// Gamepad on/off and its layout (the tab sends them after every edit).
#[tauri::command]
fn play_set_gamepad(config: GamepadConfig, play: State<'_, SharedPlay>) {
    play.lock().unwrap().set_gamepad(config);
}

#[tauri::command]
fn play_set_auto_switch(on: bool, play: State<'_, SharedPlay>) {
    play.lock().unwrap().set_auto_switch(on);
}

#[tauri::command]
fn play_set_mode(mode: Mode, play: State<'_, SharedPlay>) {
    play.lock().unwrap().set_mode(mode);
}

/// Which layout the tab in view shows, for its live view.
#[tauri::command]
fn play_set_preview(slot: Option<Slot>, play: State<'_, SharedPlay>) {
    play.lock().unwrap().set_preview(slot);
}

#[tauri::command]
async fn play_list_presets(kind: PresetKind, play: State<'_, SharedPlay>) -> Result<Vec<play_out::PresetInfo>, String> {
    Ok(play.lock().unwrap().presets(kind).list())
}

/// A layout for the editor (it plays once the tab sends it with the rest).
#[tauri::command]
async fn play_load_preset(kind: PresetKind, name: String, builtin: bool, play: State<'_, SharedPlay>) -> Result<layout::Layout, String> {
    play.lock().unwrap().presets(kind).load(&name, builtin)
}

#[tauri::command]
async fn play_save_preset(kind: PresetKind, name: String, layout: layout::Layout, play: State<'_, SharedPlay>) -> Result<(), String> {
    play.lock().unwrap().presets(kind).save(&name, layout)
}

#[tauri::command]
async fn play_delete_preset(kind: PresetKind, name: String, play: State<'_, SharedPlay>) -> Result<(), String> {
    play.lock().unwrap().presets(kind).delete(&name)
}

/// Open the saved layouts folder of `kind` in the file manager.
#[tauri::command]
async fn play_reveal_presets(kind: PresetKind, app: AppHandle, play: State<'_, SharedPlay>) -> Result<(), String> {
    let dir = play.lock().unwrap().presets(kind).folder()?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

/// Run the ViGEmBus installer (Windows asks for administrator permission); the snapshot says
/// when it is done.
#[tauri::command]
fn play_install_driver(play: State<'_, SharedPlay>) -> Result<(), String> {
    play.lock().unwrap().install_driver()
}

/// Look again whether the gamepad driver is there (the Gamepad tab asks when it opens).
#[tauri::command]
fn play_check_driver(play: State<'_, SharedPlay>) {
    play.lock().unwrap().check_driver();
}

/// Whether the tray icon could be made; without one, closing the window quits.
struct TrayAvailable(bool);

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

fn forward_events(app: AppHandle, rx: mpsc::Receiver<HubEvent>, hub: Hub, tape: SharedTape, outputs: Outputs, view: SharedView, tray: Option<Tray>) {
    let steam_running = Arc::new(AtomicBool::new(false));
    let flag = steam_running.clone();
    thread::spawn(move || watch_steam(flag));

    let mut live = Fold::default();
    let mut replay = Fold::default();
    let mut next_emit = Instant::now();
    let mut rate_window = Instant::now();
    let mut tray_lines = TrayLines::default();
    // Outputs follow what the UI shows: live input, or the recording while one plays.
    let mut playing = false;
    // The rumble last sent to the live controller for the virtual pad (large, small motor).
    let mut rumble_sent = (0u8, 0u8);
    loop {
        // What the UI can see: output tabs in view, and whether a window shows at all. While it
        // is minimised or closed to the tray, nobody sees the UI: send it nothing.
        let (watched, visible) = {
            let v = view.lock().unwrap();
            (v.watched(), v.visible())
        };
        // Wake up in time for the next replayed report.
        let wait = tape.lock().unwrap().next_due().map_or(EMIT_INTERVAL, |d| d.min(EMIT_INTERVAL));
        match rx.recv_timeout(wait) {
            Ok(HubEvent::Packet { slot, data }) => {
                let state = live.packet(&slot, &data);
                tape.lock().unwrap().record(&slot, &data);
                if let (Some(s), false) = (state, playing) {
                    outputs.process(&s, live.snap.battery, watched);
                }
                if live.snap.state.is_none() && !playing {
                    outputs.release_all();
                }
            }
            Ok(HubEvent::Closed { slot, .. }) => {
                live.closed(&slot);
                if live.snap.state.is_none() && !playing {
                    outputs.release_all();
                }
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let mut deck = tape.lock().unwrap();
        if deck.take_reset() {
            replay = Fold::default();
            // A new recording or a seek: notes and buttons held at the old position would hang.
            outputs.release_all();
            if let Some((slot, data)) = deck.first_battery() {
                replay.packet(slot, data);
            }
        }
        let now_playing = deck.playing();
        if now_playing != playing {
            playing = now_playing;
            outputs.release_all();
        }
        deck.replay(|slot, data| {
            if let Some(s) = replay.packet(slot, data)
                && playing
            {
                outputs.process(&s, replay.snap.battery, watched);
            }
        });
        replay.snap.report_rate = deck.replay_rate();
        let now = Instant::now();
        let status = (visible && now >= next_emit).then(|| deck.status());
        drop(deck);
        // Outside the tape lock: reconnecting to a port can take a moment.
        outputs.tick(watched);
        // The mode switch click and the game's rumble go to the live controller, not during a replay.
        let (haptics, rumble) = {
            let mut play = outputs.play.lock().unwrap();
            (play.take_haptics(), if playing { (0, 0) } else { play.rumble() })
        };
        match live.snap.state.and(live.snap.slot.as_deref()) {
            Some(slot) => {
                let send = |haptic| {
                    let (reply, _) = mpsc::channel();
                    let _ = hub.send(slot, Command::Haptic { haptic, reply });
                };
                if !playing {
                    haptics.into_iter().for_each(send);
                }
                if rumble != rumble_sent {
                    send(Haptic::Rumble { left: rumble.0 as u16 * 257, right: rumble.1 as u16 * 257 });
                    rumble_sent = rumble;
                }
            }
            // The hub stops a rumble with its controller; send it again once one is back.
            None => rumble_sent = (0, 0),
        }
        if rate_window.elapsed() >= Duration::from_secs(1) {
            live.update_rate();
            rate_window = Instant::now();
            if let Some(tray) = &tray {
                let snap = &live.snap;
                let lines = TrayLines::new(
                    snap.state.and(snap.slot.as_deref()),
                    snap.battery.map(|b| b.level),
                    &outputs.midi.lock().unwrap().status(),
                    &outputs.osc.lock().unwrap().status(),
                    &outputs.play.lock().unwrap().status(),
                );
                if lines != tray_lines {
                    tray.update(&lines);
                    tray_lines = lines;
                }
            }
        }
        if let Some(status) = status {
            // Keep to the schedule instead of counting from now: an emit waits for the next report
            // (~3.8 ms apart), so restarting the clock at each one slowed the UI to ~56 per second
            // and left some 60 Hz frames without a fresh snapshot. After a stall, start over.
            next_emit = (next_emit + EMIT_INTERVAL).max(now);
            let snap = if playing { &mut replay.snap } else { &mut live.snap };
            snap.steam_running = steam_running.load(Ordering::Relaxed);
            snap.tape = status;
            snap.midi = outputs.midi.lock().unwrap().status();
            snap.osc = outputs.osc.lock().unwrap().status();
            snap.play = outputs.play.lock().unwrap().status();
            let _ = app.emit("controller", &*snap);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // First: a second launch (say from the Start menu while we sit in the tray) opens this
        // instance's window instead of starting another that would fight over the controller.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::open_window(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![tray::TRAY_ARG])))
        .plugin(tauri_plugin_opener::init())
        .on_window_event(tray::on_window_event)
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
            osc_destinations,
            osc_set_destinations,
            osc_set_enabled,
            osc_learn,
            osc_list_presets,
            osc_load_preset,
            osc_save_preset,
            osc_delete_preset,
            osc_reveal_presets,
            play_setup,
            play_set_keymouse,
            play_set_gamepad,
            play_set_auto_switch,
            play_set_mode,
            play_set_preview,
            play_list_presets,
            play_load_preset,
            play_save_preset,
            play_delete_preset,
            play_reveal_presets,
            play_install_driver,
            play_check_driver,
            tray::set_tab,
            tray::background_settings,
            tray::set_close_to_tray,
            tray::set_autostart,
        ])
        .setup(|app| {
            let (tx, rx) = mpsc::channel();
            let hub = hub::spawn(tx, HubOptions::default());
            app.manage(hub.clone());
            let tape: SharedTape = Arc::new(Mutex::new(Tape::new(app.path().app_data_dir()?.join("recordings"))));
            app.manage(tape.clone());
            let midi: SharedMidi = Arc::new(Mutex::new(MidiOut::new(app.path().app_data_dir()?.join("midi"))));
            app.manage(midi.clone());
            let osc: SharedOsc = Arc::new(Mutex::new(OscOut::new(app.path().app_data_dir()?.join("osc"))));
            app.manage(osc.clone());
            let play: SharedPlay = Arc::new(Mutex::new(PlayOut::new(app.path().app_data_dir()?.join("play"))));
            app.manage(play.clone());
            let outputs = Outputs { midi, osc, play };
            app.manage(outputs.clone());
            let settings: SharedSettings = Arc::new(Mutex::new(tray::Settings::load(app.path().app_data_dir()?.join("settings.json"))));
            app.manage(settings);
            let view = SharedView::default();
            app.manage(view.clone());
            // Without a tray (say a Linux desktop without one) the app still runs, it just can't
            // be reopened once closed; closing quits then (see the exit handler below).
            let tray = Tray::create(app.handle()).map_err(|e| eprintln!("no tray icon: {e}")).ok();
            app.manage(TrayAvailable(tray.is_some()));
            // Started with the system: stay in the tray.
            if !std::env::args().any(|a| a == tray::TRAY_ARG) || tray.is_none() {
                tray::open_window(app.handle());
            }
            let handle = app.handle().clone();
            thread::spawn(move || forward_events(handle, rx, hub, tape, outputs, view, tray));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| match event {
            // The last window closed: keep running in the tray if that's the setting.
            RunEvent::ExitRequested { code: None, api, .. }
                if app.state::<TrayAvailable>().0 && app.state::<SharedSettings>().lock().unwrap().close_to_tray() =>
            {
                api.prevent_exit()
            }
            // Quitting: let go of held notes and buttons so nothing hangs in the DAW or app.
            RunEvent::Exit => app.state::<Outputs>().release_all(),
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => tray::open_window(app),
            _ => {}
        });
}

#[cfg(test)]
mod screenshot_data {
    use super::*;
    use std::collections::VecDeque;

    /// Replays a recording through the same folds, MIDI, OSC and layout engines as the app and
    /// writes the snapshots the UI would get (60 per second), plus the MIDI preset, OSC destinations
    /// and play setup, as JSON for the README screenshots (tools/screenshots):
    /// `SC_RECORDING=<file.screc> SC_SNAPSHOTS=<out.json> cargo test -p steam-controller-app --lib snapshots -- --ignored`
    #[test]
    #[ignore]
    fn snapshots() {
        let input = std::env::var("SC_RECORDING").expect("SC_RECORDING");
        let output = std::env::var("SC_SNAPSHOTS").expect("SC_SNAPSHOTS");
        let packets = sc_core::recording::read_file(std::path::Path::new(&input)).unwrap();
        let mut fold = Fold::default();
        // The default preset, with notes on the right pad so the MIDI tab's grid lights up.
        let preset = midi::Preset { right_pad: midi::builtin_presets()[1].right_pad, ..Default::default() };
        let mut engine = midi::Engine::new(preset.clone());
        let (mut msgs, mut log, mut sent) = (Vec::new(), VecDeque::new(), 0u64);
        // OSC: the stream to TouchDesigner on this PC, Resolume on another, VRChat switched off.
        let osc_preset = |name: &str| osc::builtin_presets().into_iter().find(|p| p.name == name).unwrap();
        let dests = vec![
            Destination { enabled: true, host: "127.0.0.1".into(), port: 10000, preset: osc_preset("TouchDesigner") },
            Destination { enabled: true, host: "192.168.1.42".into(), port: 7000, preset: osc_preset("Resolume Arena / Avenue") },
            Destination { enabled: false, host: "127.0.0.1".into(), port: 9000, preset: osc_preset("VRChat") },
        ];
        let mut osc_engines: Vec<osc::Engine> = dests.iter().map(|d| osc::Engine::new(d.preset.clone())).collect();
        // When each destination sent each message, for its msg/s.
        let mut osc_sent: Vec<VecDeque<Duration>> = dests.iter().map(|_| VecDeque::new()).collect();
        let mut osc_msgs = Vec::new();
        // Keyboard & mouse and the gamepad both on, in Desktop mode; the Gamepad tab's live view
        // gets the pad layout's preview instead (mock.js swaps it in).
        let play_setup = play_out::Config {
            keymouse: play_out::KeyMouseConfig { enabled: true, ..Default::default() },
            gamepad: play_out::GamepadConfig { enabled: true, ..Default::default() },
            auto_switch: true,
        };
        let mut km_engine = layout::Engine::new(play_setup.keymouse.desktop.clone());
        let mut pad_engine = layout::Engine::new(play_setup.gamepad.layout.clone());
        let (mut km_speed, mut pad_speed) = (play_out::Speed::default(), play_out::Speed::default());
        let mut pad_previews = Vec::new();
        let mut states: VecDeque<Duration> = VecDeque::new();
        let mut frames = Vec::new();
        let mut next = Duration::ZERO;
        for p in &packets {
            while p.at >= next {
                while states.front().is_some_and(|t| next.saturating_sub(*t) > Duration::from_secs(1)) {
                    states.pop_front();
                }
                fold.snap.report_rate = states.len() as u32;
                fold.snap.midi = MidiStatus {
                    output: OutputChoice::Virtual,
                    connected: Some("Steam Controller".into()),
                    rate: 180,
                    levels: engine.levels(),
                    cells: engine.grid_cells(),
                    log: log.iter().cloned().collect(),
                    ..Default::default()
                };
                fold.snap.osc = OscStatus {
                    enabled: true,
                    destinations: dests
                        .iter()
                        .zip(&osc_engines)
                        .zip(&mut osc_sent)
                        .map(|((d, e), times)| {
                            while times.front().is_some_and(|t| next.saturating_sub(*t) > Duration::from_secs(1)) {
                                times.pop_front();
                            }
                            osc_out::DestinationStatus {
                                address: d.enabled.then(|| format!("{}:{}", d.host, d.port)),
                                rate: if d.enabled { times.len() as u32 } else { 0 },
                                levels: e.levels(),
                                values: e.values(),
                                ..Default::default()
                            }
                        })
                        .collect(),
                    ..Default::default()
                };
                fold.snap.play = PlayStatus {
                    mode: Mode::Desktop,
                    auto_switch: true,
                    active: Some(Slot::Desktop),
                    keymouse: play_out::KeyMouseStatus { enabled: true, supported: true, apps_supported: true, ..Default::default() },
                    gamepad: play_out::GamepadStatus {
                        enabled: true,
                        support: Some(devices::PadSupport::Ready),
                        connected: true,
                        player: Some(1),
                        ..Default::default()
                    },
                    preview: play_out::preview(Some(Slot::Desktop), Some(km_engine.frame()), &km_speed),
                    ..Default::default()
                };
                pad_previews.push(play_out::preview(Some(Slot::Pad), Some(pad_engine.frame()), &pad_speed));
                frames.push(fold.snap.clone());
                next += EMIT_INTERVAL;
            }
            if let Some(s) = fold.packet(&p.slot, &p.data) {
                states.push_back(p.at);
                km_speed.add(km_engine.process(&s, p.at), p.at);
                pad_speed.add(pad_engine.process(&s, p.at), p.at);
                engine.process(&s, &mut msgs);
                for (i, e) in osc_engines.iter_mut().enumerate() {
                    e.process(&s, fold.snap.battery.map(|b| b.level), p.at, &mut osc_msgs);
                    osc_sent[i].extend(osc_msgs.drain(..).map(|_| p.at));
                }
                for m in msgs.drain(..) {
                    sent += 1;
                    log.push_back((sent, m.as_slice().to_vec()));
                    if log.len() > 14 {
                        log.pop_front();
                    }
                }
            }
        }
        let osc_presets: Vec<_> = osc::builtin_presets()
            .into_iter()
            .map(|p| serde_json::json!({ "name": p.name, "builtin": true, "port": p.port }))
            .collect();
        let json = serde_json::json!({
            "preset": preset,
            "osc": { "destinations": dests, "presets": osc_presets, "builtin": osc::builtin_presets() },
            "play": {
                "setup": { "keymouse": play_setup.keymouse, "gamepad": play_setup.gamepad, "auto_switch": true, "mode": Mode::Desktop },
                "presets": {
                    "keymouse": layout::builtin_keymouse().iter().map(|l| serde_json::json!({ "name": l.name, "builtin": true })).collect::<Vec<_>>(),
                    "gamepad": layout::builtin_gamepad().iter().map(|l| serde_json::json!({ "name": l.name, "builtin": true })).collect::<Vec<_>>(),
                },
                "builtin": { "keymouse": layout::builtin_keymouse(), "gamepad": layout::builtin_gamepad() },
                "pad_previews": pad_previews,
            },
            "frames": frames,
        });
        std::fs::write(output, json.to_string()).unwrap();
        println!("{} frames", frames.len());
    }
}
