//! The MIDI output: which port the engine's messages go to, the preset being played, saved
//! presets, and what the MIDI tab shows. `forward_events` feeds it every state report.
//!
//! The app makes its own port named "Steam Controller" that DAWs list as an input: with midir
//! on macOS and Linux, with Windows MIDI Services on Windows 11 25H2+ (`midi_win.rs`). Elsewhere
//! the user picks an existing port, such as a loopback port made with loopMIDI.
//!
//! Opening a port can be slow (or hang, if the Windows MIDI service is stuck), so it happens on
//! a thread of its own and `tick` picks up the result; the event loop never waits for it.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use midir::{MidiOutput, MidiOutputConnection};
use sc_core::ControllerState;
use serde::{Deserialize, Serialize};

use crate::midi::{builtin_presets, Engine, Msg, Preset};

const CLIENT_NAME: &str = "Steam Controller Toolkit";
#[cfg(not(windows))]
const VIRTUAL_PORT_NAME: &str = "Steam Controller";
/// How often to look again for a chosen port that is missing (e.g. loopMIDI not started yet).
const RETRY: Duration = Duration::from_secs(2);
const LOG_LEN: usize = 14;
const PRESET_EXT: &str = "json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputChoice {
    #[default]
    Off,
    /// Our own virtual port (on Windows only where Windows MIDI Services can make one).
    Virtual,
    Port { name: String },
}

/// What the MIDI tab needs, sent in every snapshot.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MidiStatus {
    pub output: OutputChoice,
    /// Name of the port messages are going to, if connected.
    pub connected: Option<String>,
    /// A port is being opened.
    pub connecting: bool,
    pub error: Option<String>,
    /// Messages sent over the last second.
    pub rate: u32,
    /// Output level 0..1 of each mapping, in preset order.
    pub levels: Vec<Option<f32>>,
    /// Cell under the finger on the left and right pad grids, and whether it plays a note.
    pub cells: [Option<(u8, bool)>; 2],
    /// Latest messages, oldest first, each numbered so the UI can tell new ones apart.
    pub log: Vec<(u64, Vec<u8>)>,
    pub learning: bool,
    /// The input picked by the last learn, as a source name.
    pub learned: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresetInfo {
    pub name: String,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MidiPorts {
    pub ports: Vec<String>,
    /// This PC can have our own virtual port.
    pub virtual_port: bool,
    /// It's Windows' preview version, which works once per Windows restart: offer it as opt-in.
    /// Also true while that service is stuck (`virtual_port` is then false until a restart).
    pub virtual_experimental: bool,
    /// Why it can't, when it can't (Windows only).
    pub virtual_note: Option<String>,
}

/// An open output.
enum Sink {
    Midir(MidiOutputConnection),
    #[cfg(windows)]
    Virtual(crate::midi_win::VirtualPort),
}

impl Sink {
    fn send(&mut self, msg: &[u8]) -> Result<(), String> {
        match self {
            Sink::Midir(c) => c.send(msg).map_err(|e| e.to_string()),
            #[cfg(windows)]
            Sink::Virtual(p) => p.send(msg),
        }
    }
}

type Opened = Result<(Sink, String), String>;

/// Saved between runs in `config.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    output: OutputChoice,
    preset: Preset,
}

pub struct MidiOut {
    engine: Engine,
    output: OutputChoice,
    conn: Option<(Sink, String)>,
    /// A port being opened on another thread.
    pending: Option<mpsc::Receiver<Opened>>,
    /// First run: the output was set to our virtual port without asking; if this PC can't have
    /// one, quietly fall back to Off instead of showing an error.
    auto_virtual: bool,
    error: Option<String>,
    last_try: Option<Instant>,
    dir: PathBuf,
    log: VecDeque<(u64, Vec<u8>)>,
    sent: u64,
    rate: u32,
    rate_count: u32,
    rate_window: Instant,
    buf: Vec<Msg>,
    /// Writes `config.json` (see [`MidiOut::save_config`]).
    saver: mpsc::Sender<String>,
}

impl MidiOut {
    /// Load the saved config from `dir` (presets go in `dir/presets`).
    pub fn new(dir: PathBuf) -> Self {
        #[cfg(windows)]
        crate::midi_win::init(dir.clone());
        let saved: Option<Config> = fs::read_to_string(dir.join("config.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
        // Out of the box, send to our own port: nothing to set up but picking it in the DAW.
        let auto_virtual = saved.is_none();
        let config = saved.unwrap_or_else(|| Config { output: OutputChoice::Virtual, ..Default::default() });
        let (saver, configs) = mpsc::channel::<String>();
        let config_dir = dir.clone();
        thread::spawn(move || {
            while let Ok(mut json) = configs.recv() {
                // Only the newest config matters.
                while let Ok(newer) = configs.try_recv() {
                    json = newer;
                }
                let _ = fs::create_dir_all(&config_dir);
                let _ = fs::write(config_dir.join("config.json"), json);
            }
        });
        MidiOut {
            engine: Engine::new(config.preset),
            output: config.output,
            conn: None,
            pending: None,
            auto_virtual,
            error: None,
            last_try: None,
            dir,
            log: VecDeque::new(),
            sent: 0,
            rate: 0,
            rate_count: 0,
            rate_window: Instant::now(),
            buf: Vec::new(),
            saver,
        }
    }

    fn presets_dir(&self) -> PathBuf {
        self.dir.join("presets")
    }

    /// Save the config on the saver thread. Every preset edit saves it, and writing it here would
    /// hold the lock the event loop needs for MIDI output (a small write can take milliseconds on
    /// Windows, e.g. while Defender scans the file).
    fn save_config(&self) {
        let config = Config { output: self.output.clone(), preset: self.engine.preset().clone() };
        if let Ok(json) = serde_json::to_string_pretty(&config) {
            let _ = self.saver.send(json);
        }
    }

    pub fn ports() -> Result<MidiPorts, String> {
        let out = MidiOutput::new(CLIENT_NAME).map_err(|e| e.to_string())?;
        let ports = out.ports().iter().filter_map(|p| out.port_name(p).ok()).collect();
        #[cfg(windows)]
        let support = crate::midi_win::available();
        #[cfg(windows)]
        // A stuck service is still the experimental port (it's out until the PC restarts), so the
        // UI keeps explaining that rather than treating the PC as unable to have one.
        let (virtual_experimental, virtual_note) =
            (support == Ok(crate::midi_win::Support::Preview) || crate::midi_win::stuck(), support.err());
        #[cfg(not(windows))]
        let (virtual_experimental, virtual_note) = (false, None);
        Ok(MidiPorts { ports, virtual_port: virtual_note.is_none(), virtual_experimental, virtual_note })
    }

    pub fn set_output(&mut self, output: OutputChoice) {
        self.release_all();
        self.conn = None;
        // A port still opening for the old choice closes as soon as it opens.
        self.pending = None;
        self.auto_virtual = false;
        self.error = None;
        self.last_try = None;
        self.output = output;
        self.save_config();
        self.connect();
    }

    fn connect(&mut self) {
        self.last_try = Some(Instant::now());
        let output = self.output.clone();
        if output == OutputChoice::Off {
            return;
        }
        let auto = self.auto_virtual;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let opened = match &output {
                // Never pick the experimental Windows port without the user asking for it.
                OutputChoice::Virtual if auto && !virtual_is_default() => Err(String::new()),
                OutputChoice::Virtual => open_virtual(),
                OutputChoice::Port { name } => open_port(name),
                OutputChoice::Off => return,
            };
            let _ = tx.send(opened);
        });
        self.pending = Some(rx);
    }

    /// Called often from the event loop: picks up a port that finished opening, and retries a
    /// chosen port that was missing.
    pub fn tick(&mut self) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Ok(conn)) => {
                    self.pending = None;
                    self.conn = Some(conn);
                    self.error = None;
                    if std::mem::take(&mut self.auto_virtual) {
                        self.save_config();
                    }
                }
                Ok(Err(e)) => {
                    self.pending = None;
                    if std::mem::take(&mut self.auto_virtual) {
                        self.output = OutputChoice::Off;
                    } else {
                        self.error = Some(e);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
            }
        } else if self.conn.is_none() && self.output != OutputChoice::Off && self.last_try.is_none_or(|t| t.elapsed() >= RETRY) {
            self.connect();
        }
        if self.rate_window.elapsed() >= Duration::from_secs(1) {
            self.rate = std::mem::take(&mut self.rate_count);
            self.rate_window = Instant::now();
        }
    }

    pub fn preset(&self) -> &Preset {
        self.engine.preset()
    }

    pub fn output(&self) -> &OutputChoice {
        &self.output
    }

    pub fn set_preset(&mut self, preset: Preset) {
        let mut buf = std::mem::take(&mut self.buf);
        self.engine.set_preset(preset, &mut buf);
        self.send(&mut buf);
        self.buf = buf;
        self.save_config();
    }

    /// Feed one state report from whatever the UI is showing (live input or a replay).
    pub fn process(&mut self, s: &ControllerState) {
        let mut buf = std::mem::take(&mut self.buf);
        self.engine.process(s, &mut buf);
        // Without a port the engine still runs, so the meters and learn work.
        self.send(&mut buf);
        self.buf = buf;
    }

    /// Turn off every sounding note, e.g. when switching between live input and a replay.
    pub fn release_all(&mut self) {
        let mut buf = std::mem::take(&mut self.buf);
        self.engine.release_all(&mut buf);
        self.send(&mut buf);
        self.buf = buf;
    }

    /// Release our notes and send All Notes Off on every channel, for stuck notes.
    pub fn panic(&mut self) {
        self.release_all();
        if let Some((conn, _)) = &mut self.conn {
            for ch in 0..16u8 {
                let _ = conn.send(&[0xB0 | ch, 123, 0]);
            }
        }
    }

    fn send(&mut self, buf: &mut Vec<Msg>) {
        for m in buf.drain(..) {
            let Some((conn, _)) = &mut self.conn else { continue };
            if let Err(e) = conn.send(m.as_slice()) {
                // The port went away (device unplugged, loopback port deleted); retry later.
                self.error = Some(e.to_string());
                self.conn = None;
                continue;
            }
            self.sent += 1;
            self.rate_count += 1;
            if self.log.len() == LOG_LEN {
                self.log.pop_front();
            }
            self.log.push_back((self.sent, m.as_slice().to_vec()));
        }
    }

    pub fn start_learn(&mut self) {
        self.engine.start_learn();
    }

    pub fn cancel_learn(&mut self) {
        self.engine.cancel_learn();
    }

    pub fn status(&self) -> MidiStatus {
        MidiStatus {
            output: self.output.clone(),
            connected: self.conn.as_ref().map(|(_, name)| name.clone()),
            connecting: self.pending.is_some(),
            error: self.error.clone(),
            rate: self.rate,
            levels: self.engine.levels(),
            cells: self.engine.grid_cells(),
            log: self.log.iter().cloned().collect(),
            learning: self.engine.learning(),
            learned: self.engine.learned().map(str::to_string),
        }
    }

    /// Built-in presets first, then saved ones by name.
    pub fn list_presets(&self) -> Vec<PresetInfo> {
        let mut saved: Vec<String> = fs::read_dir(self.presets_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.extension()? != PRESET_EXT {
                    return None;
                }
                Some(p.file_stem()?.to_string_lossy().into_owned())
            })
            .collect();
        saved.sort_by_key(|s| s.to_lowercase());
        let builtin = builtin_presets().into_iter().map(|p| PresetInfo { name: p.name, builtin: true });
        builtin.chain(saved.into_iter().map(|name| PresetInfo { name, builtin: false })).collect()
    }

    /// A saved preset by name, or a built-in one.
    pub fn load_preset(&self, name: &str, builtin: bool) -> Result<Preset, String> {
        if builtin {
            return builtin_presets().into_iter().find(|p| p.name == name).ok_or_else(|| format!("No built-in preset \"{name}\""));
        }
        let text = fs::read_to_string(self.preset_file(name)?).map_err(|e| format!("Couldn't read \"{name}\": {e}"))?;
        let mut preset: Preset = serde_json::from_str(&text).map_err(|e| format!("\"{name}\" is not a valid preset: {e}"))?;
        preset.name = name.to_string();
        Ok(preset)
    }

    /// Save the current preset as `name` (overwriting a saved one of that name).
    pub fn save_preset(&mut self, name: &str) -> Result<(), String> {
        let file = self.preset_file(name)?;
        let mut preset = self.engine.preset().clone();
        preset.name = name.to_string();
        fs::create_dir_all(self.presets_dir()).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
        fs::write(file, json).map_err(|e| format!("Couldn't save \"{name}\": {e}"))?;
        self.set_preset(preset);
        Ok(())
    }

    pub fn delete_preset(&self, name: &str) -> Result<(), String> {
        fs::remove_file(self.preset_file(name)?).map_err(|e| format!("Couldn't delete \"{name}\": {e}"))
    }

    pub fn presets_folder(&self) -> Result<PathBuf, String> {
        let dir = self.presets_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }

    fn preset_file(&self, name: &str) -> Result<PathBuf, String> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || name.starts_with('.') {
            return Err(format!("\"{name}\" can't be used as a preset name"));
        }
        if builtin_presets().iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
            return Err(format!("\"{name}\" is a built-in preset; pick another name"));
        }
        Ok(self.presets_dir().join(Path::new(name).with_extension(PRESET_EXT)))
    }
}

fn open_port(name: &str) -> Opened {
    let out = MidiOutput::new(CLIENT_NAME).map_err(|e| e.to_string())?;
    let port = out
        .ports()
        .into_iter()
        .find(|p| out.port_name(p).is_ok_and(|n| n == name))
        .ok_or_else(|| format!("\"{name}\" is not available"))?;
    let conn = out.connect(&port, CLIENT_NAME).map_err(|e| format!("Couldn't open \"{name}\": {e}"))?;
    Ok((Sink::Midir(conn), name.to_string()))
}

#[cfg(not(windows))]
fn open_virtual() -> Opened {
    use midir::os::unix::VirtualOutput;
    let out = MidiOutput::new(CLIENT_NAME).map_err(|e| e.to_string())?;
    let conn = out.create_virtual(VIRTUAL_PORT_NAME).map_err(|e| format!("Couldn't create the virtual port: {e}"))?;
    Ok((Sink::Midir(conn), VIRTUAL_PORT_NAME.to_string()))
}

/// Our own port may be the out-of-the-box output: everywhere but on Windows' preview service.
fn virtual_is_default() -> bool {
    #[cfg(windows)]
    return crate::midi_win::available() == Ok(crate::midi_win::Support::System);
    #[cfg(not(windows))]
    true
}

#[cfg(windows)]
fn open_virtual() -> Opened {
    let port = crate::midi_win::VirtualPort::open()?;
    Ok((Sink::Virtual(port), crate::midi_win::PORT_NAME.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_saved_and_restored() {
        let dir = std::env::temp_dir().join(format!("sc-midi-config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut midi = MidiOut::new(dir.clone());
        // Quick edits, like a slider drag: the saver writes them in order, ending on the last.
        for name in ["a", "b", "last"] {
            midi.set_preset(Preset { name: name.into(), ..builtin_presets().remove(1) });
        }
        midi.set_output(OutputChoice::Off);
        let started = Instant::now();
        let saved = loop {
            let saved = fs::read_to_string(dir.join("config.json")).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok());
            match saved {
                Some(c) if c.output == OutputChoice::Off => break c,
                _ if started.elapsed() > Duration::from_secs(5) => panic!("config.json was not saved"),
                _ => thread::sleep(Duration::from_millis(10)),
            }
        };
        assert_eq!(saved.preset.name, "last");
        let restored = MidiOut::new(dir.clone());
        assert_eq!(restored.output(), &OutputChoice::Off);
        assert_eq!(restored.preset().name, "last");
        let _ = fs::remove_dir_all(&dir);
    }
}
