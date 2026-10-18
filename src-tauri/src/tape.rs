//! Recording live reports to files and replaying them in place of the controller.
//!
//! `forward_events` hands every live packet to [`Tape::record`] and asks [`Tape::replay`] for
//! the packets that are due; while a recording plays, the UI sees those instead of live input.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use sc_core::protocol;
use sc_core::recording::{self, Packet, Writer};
use serde::{Deserialize, Serialize};

/// Recordings stop on their own after this long (~2 MB per minute).
const MAX_RECORDING: Duration = Duration::from_secs(10 * 60);

/// A recording in the recordings folder.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingInfo {
    /// File name without the extension.
    name: String,
    bytes: u64,
    seconds: f64,
    /// Last modified, in ms since the Unix epoch.
    modified: u64,
}

/// Recorder and player state, sent with every snapshot.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TapeStatus {
    recording: Option<RecordingStatus>,
    playback: Option<PlaybackStatus>,
    /// Why the last recording stopped on its own, if it did.
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RecordingStatus {
    name: String,
    seconds: f64,
    packets: u64,
}

#[derive(Debug, Clone, Serialize)]
struct PlaybackStatus {
    name: String,
    position: f64,
    duration: f64,
    paused: bool,
    looping: bool,
}

/// What the player's controls ask for (`control_playback`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaybackAction {
    Pause,
    Resume,
    Stop,
    /// Pause on the next state report.
    Step,
    Seek { seconds: f64 },
    Loop { on: bool },
}

struct Recorder {
    name: String,
    path: PathBuf,
    writer: Writer<BufWriter<File>>,
    started: Instant,
    packets: u64,
}

struct Player {
    name: String,
    packets: Vec<Packet>,
    /// Next packet to deliver.
    pos: usize,
    /// When position 0 was (or would have been) played.
    origin: Instant,
    paused_at: Option<Duration>,
    looping: bool,
    /// The replayed state must start over: the player restarted or jumped back.
    reset: bool,
}

impl Player {
    fn duration(&self) -> Duration {
        self.packets.last().map_or(Duration::ZERO, |p| p.at)
    }

    fn position(&self) -> Duration {
        self.paused_at.unwrap_or_else(|| self.origin.elapsed()).min(self.duration())
    }

    fn seek(&mut self, to: Duration) {
        let to = to.min(self.duration());
        if to < self.position() {
            self.pos = 0;
            self.reset = true;
        }
        self.origin = Instant::now() - to;
        if self.paused_at.is_some() {
            self.paused_at = Some(to);
        }
    }
}

pub struct Tape {
    dir: PathBuf,
    recorder: Option<Recorder>,
    player: Option<Player>,
    message: Option<String>,
}

fn is_state(data: &[u8]) -> bool {
    matches!(
        data.first(),
        Some(&(protocol::REPORT_STATE | protocol::REPORT_STATE_NO_QUAT | protocol::REPORT_STATE_TIMESTAMP))
    )
}

/// `name` as a file name, or an error if it could escape the recordings folder.
fn checked_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.starts_with('.')
        || name.chars().any(|c| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control());
    if bad { Err(format!("\"{name}\" can't be used as a file name")) } else { Ok(name) }
}

impl Tape {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, recorder: None, player: None, message: None }
    }

    fn path(&self, name: &str) -> Result<PathBuf, String> {
        Ok(self.dir.join(format!("{}.{}", checked_name(name)?, recording::EXTENSION)))
    }

    /// A path for a new recording named `name`, numbered if one with that name exists.
    fn new_path(&self, name: &str) -> Result<(String, PathBuf), String> {
        let name = checked_name(name)?;
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        (1..1000)
            .map(|n| if n == 1 { name.to_string() } else { format!("{name} ({n})") })
            .map(|n| self.path(&n).map(|p| (n, p)))
            .find(|r| r.as_ref().map_or(true, |(_, p)| !p.exists()))
            .unwrap_or_else(|| Err("Too many recordings with that name".into()))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn playing(&self) -> bool {
        self.player.is_some()
    }

    pub fn status(&self) -> TapeStatus {
        TapeStatus {
            recording: self.recorder.as_ref().map(|r| RecordingStatus {
                name: r.name.clone(),
                seconds: r.started.elapsed().as_secs_f64(),
                packets: r.packets,
            }),
            playback: self.player.as_ref().map(|p| PlaybackStatus {
                name: p.name.clone(),
                position: p.position().as_secs_f64(),
                duration: p.duration().as_secs_f64(),
                paused: p.paused_at.is_some(),
                looping: p.looping,
            }),
            message: self.message.clone(),
        }
    }

    pub fn start_recording(&mut self, name: &str) -> Result<String, String> {
        if self.recorder.is_some() {
            return Err("Already recording".into());
        }
        if self.player.is_some() {
            return Err("Stop the playback first".into());
        }
        let (name, path) = self.new_path(name)?;
        let file = File::create(&path).map_err(|e| e.to_string())?;
        let writer = Writer::new(BufWriter::new(file)).map_err(|e| e.to_string())?;
        self.recorder = Some(Recorder { name: name.clone(), path, writer, started: Instant::now(), packets: 0 });
        self.message = None;
        Ok(name)
    }

    pub fn stop_recording(&mut self) -> Result<(), String> {
        let r = self.recorder.take().ok_or("Not recording")?;
        r.writer.finish().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Add a live packet to the recording, if one is running.
    pub fn record(&mut self, slot: &str, data: &[u8]) {
        let Some(r) = &mut self.recorder else { return };
        let at = r.started.elapsed();
        let stop = if at > MAX_RECORDING {
            Some(format!("Recording stopped at the {} minute limit", MAX_RECORDING.as_secs() / 60))
        } else {
            r.packets += 1;
            r.writer.write(at, slot, data).err().map(|e| format!("Recording stopped: {e}"))
        };
        if let Some(message) = stop {
            let _ = self.stop_recording();
            self.message = Some(message);
        }
    }

    pub fn list(&self) -> Vec<RecordingInfo> {
        let Ok(entries) = fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out: Vec<RecordingInfo> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                if path.extension()? != recording::EXTENSION {
                    return None;
                }
                let meta = e.metadata().ok()?;
                // The file being written has no complete last line yet.
                let recording = self.recorder.as_ref().is_some_and(|r| r.path == path);
                Some(RecordingInfo {
                    name: path.file_stem()?.to_string_lossy().into_owned(),
                    bytes: meta.len(),
                    seconds: if recording { 0.0 } else { recording::duration(&path).map_or(0.0, |d| d.as_secs_f64()) },
                    modified: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_millis() as u64),
                })
            })
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.modified));
        out
    }

    pub fn play(&mut self, name: &str) -> Result<(), String> {
        if self.recorder.is_some() {
            return Err("Stop recording first".into());
        }
        let packets = recording::read_file(&self.path(name)?)?;
        if packets.is_empty() {
            return Err("The recording is empty".into());
        }
        let looping = self.player.as_ref().is_some_and(|p| p.looping);
        self.player = Some(Player {
            name: name.to_string(),
            packets,
            pos: 0,
            origin: Instant::now(),
            paused_at: None,
            looping,
            reset: true,
        });
        Ok(())
    }

    pub fn control(&mut self, action: PlaybackAction) -> Result<(), String> {
        if let PlaybackAction::Stop = action {
            self.player = None;
            return Ok(());
        }
        let p = self.player.as_mut().ok_or("Nothing is playing")?;
        match action {
            PlaybackAction::Pause => p.paused_at = Some(p.position()),
            PlaybackAction::Resume => {
                // Resuming at the end starts over.
                if p.paused_at.is_some_and(|at| at >= p.duration()) {
                    p.seek(Duration::ZERO);
                }
                if let Some(at) = p.paused_at.take() {
                    p.origin = Instant::now() - at;
                }
            }
            PlaybackAction::Step => {
                let next = p.packets[p.pos.min(p.packets.len() - 1)..].iter().find(|pk| is_state(&pk.data));
                let to = next.map_or(p.duration(), |pk| pk.at);
                p.paused_at = Some(to.max(p.position()));
            }
            PlaybackAction::Seek { seconds } => p.seek(Duration::from_secs_f64(seconds.max(0.0))),
            PlaybackAction::Loop { on } => p.looping = on,
            PlaybackAction::Stop => unreachable!(),
        }
        Ok(())
    }

    /// Whether the replayed state must be cleared before the next [`Tape::replay`]: the player
    /// started, jumped back or looped. Also pauses a finished, non-looping recording at its end.
    pub fn take_reset(&mut self) -> bool {
        let Some(p) = &mut self.player else { return false };
        if p.paused_at.is_none() && p.pos >= p.packets.len() {
            if p.looping {
                p.seek(Duration::ZERO);
            } else {
                p.paused_at = Some(p.duration());
            }
        }
        std::mem::take(&mut p.reset)
    }

    /// Hand every replayed packet that is due to `deliver`.
    pub fn replay(&mut self, mut deliver: impl FnMut(&str, &[u8])) {
        let Some(p) = &mut self.player else { return };
        let now = p.position();
        while let Some(pk) = p.packets.get(p.pos).filter(|pk| pk.at <= now) {
            deliver(&pk.slot, &pk.data);
            p.pos += 1;
        }
    }

    /// State reports per second in the second of recording around the play position. Taken from
    /// the recording rather than counted as packets are replayed, so it holds steady when playback
    /// starts, pauses or jumps.
    pub fn replay_rate(&self) -> u32 {
        const WINDOW: Duration = Duration::from_secs(1);
        let Some(p) = &self.player else { return 0 };
        let end = p.position().max(WINDOW).min(p.duration());
        let start = end.saturating_sub(WINDOW);
        let span = (end - start).as_secs_f64();
        if span <= 0.0 {
            return 0;
        }
        let first = p.packets.partition_point(|pk| pk.at < start);
        let last = p.packets.partition_point(|pk| pk.at <= end);
        let count = p.packets[first..last].iter().filter(|pk| is_state(&pk.data)).count();
        (count as f64 / span).round() as u32
    }

    /// The recording's first battery report. Battery reports are rare, so the replayed state
    /// starts with this one instead of showing no battery until the next comes along.
    pub fn first_battery(&self) -> Option<(&str, &[u8])> {
        let p = self.player.as_ref()?;
        let pk = p.packets.iter().find(|pk| pk.data.first() == Some(&protocol::REPORT_BATTERY))?;
        Some((&pk.slot, &pk.data))
    }

    /// How long until the next replayed packet is due, if one is.
    pub fn next_due(&self) -> Option<Duration> {
        let p = self.player.as_ref().filter(|p| p.paused_at.is_none())?;
        Some(p.packets.get(p.pos)?.at.saturating_sub(p.position()))
    }

    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        if self.player.as_ref().is_some_and(|p| p.name == name) {
            self.player = None;
        }
        if self.recorder.as_ref().is_some_and(|r| r.name == name) {
            return Err("Stop recording first".into());
        }
        fs::remove_file(self.path(name)?).map_err(|e| e.to_string())
    }

    pub fn file(&self, name: &str) -> Result<PathBuf, String> {
        let path = self.path(name)?;
        if path.exists() { Ok(path) } else { Err(format!("{name} no longer exists")) }
    }

    /// Copy a recording from elsewhere into the recordings folder; returns its name there.
    pub fn import(&mut self, from: &Path) -> Result<String, String> {
        recording::read_file(from).map_err(|e| format!("{}: {e}", from.display()))?;
        let stem = from.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let (name, path) = self.new_path(if stem.trim().is_empty() { "Imported" } else { &stem })?;
        fs::copy(from, path).map_err(|e| e.to_string())?;
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tape(test: &str) -> Tape {
        let dir = std::env::temp_dir().join(format!("sc-tape-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Tape::new(dir)
    }

    fn replayed(tape: &mut Tape) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        tape.replay(|_, data| out.push(data.to_vec()));
        out
    }

    #[test]
    fn records_and_steps_through_a_recording() {
        let mut tape = temp_tape("steps");
        assert_eq!(tape.start_recording("take").unwrap(), "take");
        // Recordings store microseconds; space the reports out so each gets its own timestamp,
        // as real ones (~3.8 ms apart) do. Reports at the same instant would play in one step.
        for data in [&[0x43, 0x01, 80][..], &[0x42, 0x01], &[0x42, 0x02]] {
            tape.record("1304:2", data);
            std::thread::sleep(Duration::from_millis(2));
        }
        tape.stop_recording().unwrap();
        assert_eq!(tape.start_recording("take").unwrap(), "take (2)");
        tape.stop_recording().unwrap();
        assert_eq!(tape.list().len(), 2);

        tape.play("take").unwrap();
        tape.control(PlaybackAction::Pause).unwrap();
        tape.control(PlaybackAction::Seek { seconds: 0.0 }).unwrap();
        assert!(tape.take_reset());
        tape.control(PlaybackAction::Step).unwrap();
        // Up to and including the first state report.
        assert_eq!(replayed(&mut tape), [vec![0x43, 0x01, 80], vec![0x42, 0x01]]);
        tape.control(PlaybackAction::Step).unwrap();
        assert_eq!(replayed(&mut tape), [vec![0x42, 0x02]]);

        tape.delete("take").unwrap();
        assert!(!tape.playing());
        let _ = fs::remove_dir_all(tape.dir());
    }

    #[test]
    fn names_stay_in_the_folder() {
        let mut tape = temp_tape("names");
        for name in ["", "../escape", "a/b", "a\\b", ".hidden", "c:"] {
            assert!(tape.start_recording(name).is_err(), "{name:?} was accepted");
        }
    }
}
