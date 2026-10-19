//! Turns controller state into MIDI messages: a preset of mappings (one input to one message)
//! plus a note grid on each trackpad. Pure logic; `midi_out.rs` owns the port.
//!
//! A mapping shapes its input into 0..1 (`shaping.rs`), then sends it as its target's message
//! whenever the value the message carries changes.

use sc_core::ControllerState;
use serde::{Deserialize, Serialize};

use crate::shaping::{Compiled, Learner, ShapeState, Shaping, FULL_PRESSURE};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// Control change; `hi_res` sends 14 bits as `cc` (MSB) and `cc + 32` (LSB), for cc < 32.
    Cc { cc: u8, hi_res: bool },
    PitchBend,
    ChannelPressure,
    /// Note on while the value is at least half, note off below.
    Note { note: u8, velocity: u8 },
    /// Program change each time the value goes past half.
    Program { program: u8 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mapping {
    pub enabled: bool,
    #[serde(flatten)]
    pub shaping: Shaping,
    pub target: Target,
    /// MIDI channel, 1..=16.
    pub channel: u8,
}

impl Default for Mapping {
    fn default() -> Self {
        Mapping { enabled: true, shaping: Shaping::default(), target: Target::Cc { cc: 20, hi_res: false }, channel: 1 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    Chromatic,
    Major,
    Minor,
    MajorPentatonic,
    MinorPentatonic,
    Blues,
    Dorian,
    HarmonicMinor,
}

impl Scale {
    fn steps(self) -> &'static [u8] {
        match self {
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::MajorPentatonic => &[0, 2, 4, 7, 9],
            Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
            Scale::Blues => &[0, 3, 5, 6, 7, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadTrigger {
    Touch,
    Click,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadPressure {
    Off,
    /// Polyphonic aftertouch on the held note.
    Poly,
    Channel,
}

/// A trackpad played as a grid of notes. Cells count from the bottom-left, along each row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PadGrid {
    pub enabled: bool,
    pub channel: u8,
    pub cols: u8,
    pub rows: u8,
    /// Note of the bottom-left cell.
    pub root: u8,
    pub scale: Scale,
    /// Scale steps from one row to the next; `cols` makes the grid one continuous run.
    pub row_step: u8,
    pub trigger: PadTrigger,
    pub velocity: u8,
    /// Velocity from how hard the pad is pressed when the note starts, instead of `velocity`.
    pub dynamic_velocity: bool,
    /// Sliding onto another cell while held plays that cell's note.
    pub slide: bool,
    pub pressure: PadPressure,
}

impl Default for PadGrid {
    fn default() -> Self {
        PadGrid {
            enabled: false,
            channel: 1,
            cols: 4,
            rows: 4,
            root: 60,
            scale: Scale::MajorPentatonic,
            row_step: 4,
            trigger: PadTrigger::Touch,
            velocity: 100,
            dynamic_velocity: false,
            slide: true,
            pressure: PadPressure::Off,
        }
    }
}

impl PadGrid {
    pub fn note(&self, cell: u8) -> Option<u8> {
        let cols = self.cols.max(1);
        let (row, col) = ((cell / cols) as u32, (cell % cols) as u32);
        let degree = row * self.row_step as u32 + col;
        let steps = self.scale.steps();
        let len = steps.len() as u32;
        let note = self.root as u32 + 12 * (degree / len) + steps[(degree % len) as usize] as u32;
        (note <= 127).then_some(note as u8)
    }

    /// The cell under a pad position (raw, +y up).
    fn cell(&self, x: i16, y: i16) -> u8 {
        let cols = self.cols.clamp(1, 8) as i32;
        let rows = self.rows.clamp(1, 8) as i32;
        let col = ((x as i32 + 32768) * cols / 65536).clamp(0, cols - 1);
        let row = ((y as i32 + 32768) * rows / 65536).clamp(0, rows - 1);
        (row * cols + col) as u8
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    pub mappings: Vec<Mapping>,
    pub left_pad: PadGrid,
    pub right_pad: PadGrid,
}

impl Default for Preset {
    fn default() -> Self {
        builtin_presets().remove(0)
    }
}

fn cc(source: &str, cc: u8) -> Mapping {
    Mapping { shaping: Shaping::source(source), target: Target::Cc { cc, hi_res: false }, ..Default::default() }
}

fn note(button: &str, note: u8, channel: u8) -> Mapping {
    Mapping { shaping: Shaping::source(&format!("button:{button}")), target: Target::Note { note, velocity: 100 }, channel, ..Default::default() }
}

/// Presets that ship with the app. The first is the default.
pub fn builtin_presets() -> Vec<Preset> {
    let stick = |source: &str, n| Mapping { shaping: Shaping { deadzone: 0.1, ..Shaping::source(source) }, ..cc(source, n) };
    // ±45° of tilt spans the output, only while the back grip button is held.
    let tilt = |source: &str, n| Mapping {
        shaping: Shaping { range: [0.25, 0.75], smoothing: 0.6, hold: Some("R4".into()), ..Shaping::source(source) },
        ..cc(source, n)
    };
    vec![
        Preset {
            name: "Effects + drums".into(),
            mappings: vec![
                cc("trigger_left", 20),
                cc("trigger_right", 21),
                stick("left_stick_x", 22),
                stick("left_stick_y", 23),
                stick("right_stick_x", 24),
                stick("right_stick_y", 25),
                tilt("tilt_pitch", 26),
                tilt("tilt_roll", 27),
                cc("left_pad_x", 28),
                cc("left_pad_y", 29),
                // General MIDI drums on channel 10.
                note("A", 36, 10),
                note("B", 38, 10),
                note("X", 42, 10),
                note("Y", 46, 10),
                note("Down", 37, 10),
                note("Right", 39, 10),
                note("Left", 49, 10),
                note("Up", 51, 10),
            ],
            ..Preset::empty()
        },
        Preset {
            name: "Pad instrument".into(),
            mappings: vec![
                cc("trigger_right", 1),
                cc("trigger_left", 11),
                Mapping {
                    shaping: Shaping { deadzone: 0.08, ..Shaping::source("right_stick_x") },
                    target: Target::PitchBend,
                    ..Default::default()
                },
                Mapping { shaping: Shaping::source("button:R"), target: Target::Cc { cc: 64, hi_res: false }, ..Default::default() },
                Mapping {
                    shaping: Shaping { toggle: true, ..Shaping::source("button:L") },
                    target: Target::Cc { cc: 64, hi_res: false },
                    ..Default::default()
                },
            ],
            left_pad: PadGrid {
                enabled: true,
                cols: 3,
                rows: 3,
                root: 36,
                scale: Scale::MinorPentatonic,
                row_step: 3,
                trigger: PadTrigger::Click,
                slide: false,
                ..Default::default()
            },
            right_pad: PadGrid { enabled: true, root: 60, scale: Scale::MinorPentatonic, pressure: PadPressure::Poly, ..Default::default() },
        },
        Preset {
            name: "Effects only".into(),
            mappings: vec![
                cc("trigger_left", 20),
                cc("trigger_right", 21),
                stick("left_stick_x", 22),
                stick("left_stick_y", 23),
                stick("right_stick_x", 24),
                stick("right_stick_y", 25),
                cc("left_pad_x", 26),
                cc("left_pad_y", 27),
                cc("right_pad_x", 28),
                cc("right_pad_y", 29),
                cc("left_pad_pressure", 30),
                cc("right_pad_pressure", 31),
                tilt("tilt_pitch", 85),
                tilt("tilt_roll", 86),
            ],
            ..Preset::empty()
        },
    ]
}

impl Preset {
    fn empty() -> Self {
        Preset { name: String::new(), mappings: Vec::new(), left_pad: PadGrid::default(), right_pad: PadGrid::default() }
    }
}

/// A MIDI message of 2 or 3 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Msg {
    bytes: [u8; 3],
    len: u8,
}

impl Msg {
    fn new(status: u8, channel: u8, a: u8, b: u8) -> Self {
        Msg { bytes: [status | (channel.clamp(1, 16) - 1), a & 0x7F, b & 0x7F], len: 3 }
    }
    fn short(status: u8, channel: u8, a: u8) -> Self {
        Msg { len: 2, ..Msg::new(status, channel, a, 0) }
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

const NOTE_OFF: u8 = 0x80;
const NOTE_ON: u8 = 0x90;
const POLY_PRESSURE: u8 = 0xA0;
const CONTROL: u8 = 0xB0;
const PROGRAM: u8 = 0xC0;
const CHANNEL_PRESSURE: u8 = 0xD0;
const PITCH_BEND: u8 = 0xE0;

#[derive(Default)]
struct MapState {
    shape: ShapeState,
    /// Value the last message carried.
    sent: Option<u16>,
    note_on: bool,
}

#[derive(Default)]
struct GridState {
    /// Cell and note being played.
    held: Option<(u8, u8)>,
    /// Cell under the finger while the pad is touched, held or not (for the UI).
    hover: Option<u8>,
    pressure: Option<u8>,
}

pub struct Engine {
    preset: Preset,
    compiled: Vec<Compiled>,
    maps: Vec<MapState>,
    grids: [GridState; 2],
    /// Notes we turned on and have not turned off, as (channel, note).
    notes: Vec<(u8, u8)>,
    learn: Learner,
}

impl Engine {
    pub fn new(preset: Preset) -> Self {
        let mut e = Engine {
            preset: Preset::empty(),
            compiled: Vec::new(),
            maps: Vec::new(),
            grids: Default::default(),
            notes: Vec::new(),
            learn: Learner::default(),
        };
        e.set_preset(preset, &mut Vec::new());
        e
    }

    pub fn preset(&self) -> &Preset {
        &self.preset
    }

    /// Switch to `preset`, turning off held notes. Mappings that did not change keep their
    /// state, so editing one row does not resend every other value.
    pub fn set_preset(&mut self, preset: Preset, out: &mut Vec<Msg>) {
        let old = std::mem::replace(&mut self.preset, preset);
        let mut old_maps: Vec<Option<MapState>> = std::mem::take(&mut self.maps).into_iter().map(Some).collect();
        let mut maps = Vec::with_capacity(self.preset.mappings.len());
        for m in &self.preset.mappings {
            let same = old.mappings.iter().position(|o| o == m).and_then(|i| old_maps.get_mut(i)?.take());
            maps.push(same.unwrap_or_default());
        }
        // Notes of mappings that went away or changed.
        for (i, st) in old_maps.into_iter().enumerate() {
            if let (Some(st), Target::Note { note, .. }) = (st, old.mappings[i].target)
                && st.note_on {
                    self.note_off(old.mappings[i].channel, note, out);
                }
        }
        self.maps = maps;
        self.compiled = self.preset.mappings.iter().map(|m| Compiled::new(&m.shaping)).collect();
        for (side, (o, n)) in [(old.left_pad, self.preset.left_pad), (old.right_pad, self.preset.right_pad)].into_iter().enumerate() {
            if o != n {
                if let Some((_, note)) = self.grids[side].held.take() {
                    self.note_off(o.channel, note, out);
                }
                self.grids[side] = GridState::default();
            }
        }
    }

    /// Turn off every note and forget what was sent, so the next state resends every value.
    pub fn release_all(&mut self, out: &mut Vec<Msg>) {
        for (ch, note) in std::mem::take(&mut self.notes) {
            out.push(Msg::new(NOTE_OFF, ch, note, 0));
        }
        self.maps.iter_mut().for_each(|m| *m = MapState::default());
        self.grids = Default::default();
    }

    /// Output level 0..1 of each mapping, `None` before it has a value.
    pub fn levels(&self) -> Vec<Option<f32>> {
        self.maps.iter().map(|m| m.shape.level).collect()
    }

    /// Cell under the finger on each pad grid, and whether it is playing.
    pub fn grid_cells(&self) -> [Option<(u8, bool)>; 2] {
        [0, 1].map(|i| {
            let g = &self.grids[i];
            g.held.map(|(c, _)| (c, true)).or(g.hover.map(|c| (c, false)))
        })
    }

    pub fn start_learn(&mut self) {
        self.learn.start();
    }

    pub fn cancel_learn(&mut self) {
        self.learn.cancel();
    }

    pub fn learning(&self) -> bool {
        self.learn.learning()
    }

    pub fn learned(&self) -> Option<&str> {
        self.learn.learned()
    }

    fn note_on(&mut self, ch: u8, note: u8, vel: u8, out: &mut Vec<Msg>) {
        out.push(Msg::new(NOTE_ON, ch, note, vel.max(1)));
        self.notes.push((ch, note));
    }

    fn note_off(&mut self, ch: u8, note: u8, out: &mut Vec<Msg>) {
        if let Some(i) = self.notes.iter().position(|n| *n == (ch, note)) {
            self.notes.swap_remove(i);
            out.push(Msg::new(NOTE_OFF, ch, note, 0));
        }
    }

    /// Feed one state report; appends the messages it causes to `out`.
    pub fn process(&mut self, s: &ControllerState, out: &mut Vec<Msg>) {
        self.learn.process(s);
        for i in 0..self.maps.len() {
            self.process_mapping(i, s, out);
        }
        for side in 0..2 {
            self.process_grid(side, s, out);
        }
    }

    fn process_mapping(&mut self, i: usize, s: &ControllerState, out: &mut Vec<Msg>) {
        let m = &self.preset.mappings[i];
        if !m.enabled {
            return;
        }
        let (ch, target) = (m.channel, m.target);
        let Some(step) = self.maps[i].shape.step(&m.shaping, &self.compiled[i], s) else {
            // Not held (or no reading): a sounding note stops; everything else keeps its value.
            if let Target::Note { note, .. } = target
                && std::mem::take(&mut self.maps[i].note_on) {
                    self.note_off(ch, note, out);
                }
            return;
        };
        let v = step.v;
        let st = &mut self.maps[i];
        match target {
            Target::Cc { cc, hi_res } => {
                let hi_res = hi_res && cc < 32;
                let q = if hi_res { (v * 16383.0).round() as u16 } else { (v * 127.0).round() as u16 };
                if st.sent != Some(q) {
                    st.sent = Some(q);
                    if hi_res {
                        out.push(Msg::new(CONTROL, ch, cc, (q >> 7) as u8));
                        out.push(Msg::new(CONTROL, ch, cc + 32, (q & 0x7F) as u8));
                    } else {
                        out.push(Msg::new(CONTROL, ch, cc, q as u8));
                    }
                }
            }
            Target::PitchBend => {
                let q = (v * 16383.0).round() as u16;
                if st.sent != Some(q) {
                    st.sent = Some(q);
                    out.push(Msg::new(PITCH_BEND, ch, (q & 0x7F) as u8, (q >> 7) as u8));
                }
            }
            Target::ChannelPressure => {
                let q = (v * 127.0).round() as u16;
                if st.sent != Some(q) {
                    st.sent = Some(q);
                    out.push(Msg::short(CHANNEL_PRESSURE, ch, q as u8));
                }
            }
            Target::Note { note, velocity } => {
                let on = v >= 0.5;
                if on != st.note_on {
                    st.note_on = on;
                    if on {
                        self.note_on(ch, note, velocity, out);
                    } else {
                        self.note_off(ch, note, out);
                    }
                }
            }
            Target::Program { program } => {
                if step.rising {
                    out.push(Msg::short(PROGRAM, ch, program));
                }
            }
        }
    }

    fn process_grid(&mut self, side: usize, s: &ControllerState, out: &mut Vec<Msg>) {
        let g = if side == 0 { self.preset.left_pad } else { self.preset.right_pad };
        let pad = if side == 0 { s.left_pad } else { s.right_pad };
        if !g.enabled {
            return;
        }
        let down = match g.trigger {
            PadTrigger::Touch => pad.touched,
            PadTrigger::Click => pad.clicked,
        };
        let cell = pad.touched.then(|| g.cell(pad.x, pad.y));
        let pressure = ((pad.pressure as f32 / FULL_PRESSURE).min(1.0).sqrt() * 127.0).round() as u8;
        self.grids[side].hover = cell;
        let held = self.grids[side].held;
        match (held, down.then_some(cell).flatten()) {
            (Some((_, note)), None) => {
                self.grids[side].held = None;
                self.grids[side].pressure = None;
                self.note_off(g.channel, note, out);
            }
            (Some((from, note)), Some(to)) if from != to && g.slide => {
                self.note_off(g.channel, note, out);
                self.grids[side].held = None;
                self.start_grid_note(side, &g, to, pressure, out);
            }
            (None, Some(to)) => self.start_grid_note(side, &g, to, pressure, out),
            _ => {}
        }
        if let (Some((_, note)), PadPressure::Poly | PadPressure::Channel) = (self.grids[side].held, g.pressure)
            && self.grids[side].pressure != Some(pressure) {
                self.grids[side].pressure = Some(pressure);
                out.push(match g.pressure {
                    PadPressure::Poly => Msg::new(POLY_PRESSURE, g.channel, note, pressure),
                    _ => Msg::short(CHANNEL_PRESSURE, g.channel, pressure),
                });
            }
    }

    fn start_grid_note(&mut self, side: usize, g: &PadGrid, cell: u8, pressure: u8, out: &mut Vec<Msg>) {
        let Some(note) = g.note(cell) else { return };
        let vel = if g.dynamic_velocity { pressure.max(1) } else { g.velocity };
        self.note_on(g.channel, note, vel, out);
        self.grids[side].held = Some((cell, note));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shaping::{button_mask, Src};
    use sc_core::protocol::buttons;
    use sc_core::protocol::Trackpad;

    fn state() -> ControllerState {
        let mut s = ControllerState::default();
        s.imu.quat = Some([32767, 0, 0, 0]);
        s
    }

    fn bytes(out: &[Msg]) -> Vec<Vec<u8>> {
        out.iter().map(|m| m.as_slice().to_vec()).collect()
    }

    fn only(m: Mapping) -> Engine {
        Engine::new(Preset { mappings: vec![m], ..Preset::empty() })
    }

    #[test]
    fn builtin_presets_parse() {
        for p in builtin_presets() {
            for m in &p.mappings {
                assert!(Src::parse(&m.shaping.source).is_some(), "{}: {}", p.name, m.shaping.source);
                if let Some(h) = &m.shaping.hold {
                    assert!(button_mask(h).is_some(), "{}: hold {h}", p.name);
                }
            }
        }
    }

    #[test]
    fn cc_sends_only_changes() {
        let mut e = only(Mapping::default());
        let mut s = state();
        let mut out = Vec::new();
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0xB0, 20, 0]]);
        out.clear();
        e.process(&s, &mut out);
        assert!(out.is_empty());
        s.trigger_right = 32767;
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0xB0, 20, 127]]);
    }

    #[test]
    fn hi_res_cc_and_pitch_bend_centre() {
        let mut e = Engine::new(Preset {
            mappings: vec![
                Mapping { shaping: Shaping::source("left_stick_x"), target: Target::Cc { cc: 1, hi_res: true }, ..Default::default() },
                Mapping { shaping: Shaping::source("left_stick_x"), target: Target::PitchBend, channel: 2, ..Default::default() },
            ],
            ..Preset::empty()
        });
        let mut out = Vec::new();
        e.process(&state(), &mut out);
        // 0.5 * 16383 rounds to 8192: the exact pitch bend centre.
        assert_eq!(bytes(&out), vec![vec![0xB0, 1, 64], vec![0xB0, 33, 0], vec![0xE1, 0, 64]]);
    }

    #[test]
    fn button_note_and_toggle() {
        let mut e = Engine::new(Preset {
            mappings: vec![
                note("A", 36, 10),
                Mapping {
                    shaping: Shaping { toggle: true, ..Shaping::source("button:B") },
                    target: Target::Cc { cc: 64, hi_res: false },
                    ..Default::default()
                },
            ],
            ..Preset::empty()
        });
        let mut s = state();
        let mut out = Vec::new();
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0xB0, 64, 0]]);
        out.clear();
        s.buttons = buttons::A | buttons::B;
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0x99, 36, 100], vec![0xB0, 64, 127]]);
        out.clear();
        s.buttons = 0;
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0x89, 36, 0]]);
        out.clear();
        s.buttons = buttons::B;
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0xB0, 64, 0]]);
    }

    #[test]
    fn hold_button_gates_mapping() {
        let mut e = only(Mapping { shaping: Shaping { hold: Some("R4".into()), ..Default::default() }, ..Default::default() });
        let mut s = state();
        s.trigger_right = 32767;
        let mut out = Vec::new();
        e.process(&s, &mut out);
        assert!(out.is_empty());
        s.buttons = buttons::R4;
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0xB0, 20, 127]]);
    }

    #[test]
    fn pad_grid_plays_and_slides() {
        let grid = PadGrid { enabled: true, cols: 2, rows: 2, root: 60, scale: Scale::Major, row_step: 2, ..Default::default() };
        assert_eq!([0, 1, 2, 3].map(|c| grid.note(c)), [Some(60), Some(62), Some(64), Some(65)]);
        let mut e = Engine::new(Preset { right_pad: grid, ..Preset::empty() });
        let mut s = state();
        let mut out = Vec::new();
        s.right_pad = Trackpad { touched: true, clicked: false, x: -20000, y: -20000, pressure: 0 };
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0x90, 60, 100]]);
        out.clear();
        s.right_pad.x = 20000; // slide right: next cell
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0x80, 60, 0], vec![0x90, 62, 100]]);
        out.clear();
        s.right_pad.touched = false;
        e.process(&s, &mut out);
        assert_eq!(bytes(&out), vec![vec![0x80, 62, 0]]);
    }

    #[test]
    fn release_all_stops_notes() {
        let mut e = only(note("A", 40, 1));
        let mut s = state();
        s.buttons = buttons::A;
        let mut out = Vec::new();
        e.process(&s, &mut out);
        out.clear();
        e.release_all(&mut out);
        assert_eq!(bytes(&out), vec![vec![0x80, 40, 0]]);
    }

    #[test]
    fn editing_one_mapping_keeps_the_others() {
        let mut e = Engine::new(Preset::default());
        let mut out = Vec::new();
        e.process(&state(), &mut out);
        out.clear();
        let mut p = e.preset().clone();
        p.mappings[0].target = Target::Cc { cc: 99, hi_res: false };
        e.set_preset(p, &mut out);
        e.process(&state(), &mut out);
        assert_eq!(bytes(&out), vec![vec![0xB0, 99, 0]]);
    }


    #[test]
    fn preset_json_keeps_its_shape() {
        let m = Mapping { shaping: Shaping { deadzone: 0.1, ..Shaping::source("left_stick_x") }, ..Default::default() };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["source"], "left_stick_x");
        assert_eq!(json["channel"], 1);
        assert!(json.get("shaping").is_none());
        // A mapping saved before the shaping fields moved, with some fields missing.
        let old: Mapping = serde_json::from_str(r#"{"enabled":false,"source":"button:A","target":{"kind":"program","program":3},"deadzone":0.2}"#).unwrap();
        assert_eq!(old.shaping.source, "button:A");
        assert_eq!(old.shaping.deadzone, 0.2);
        assert_eq!(old.shaping.range, [0.0, 1.0]);
        assert!(!old.enabled);
    }
}
