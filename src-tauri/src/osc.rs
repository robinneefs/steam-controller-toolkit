//! Turns controller state into OSC messages for one destination. Pure logic; `osc_out.rs` owns
//! the socket.
//!
//! A preset has two parts, both optional:
//! - the **stream**: every input under a prefix (`/sc/left_stick -0.2 0.9`, `/sc/button/a 1`),
//!   grouped or one value per address, for apps you program yourself;
//! - **mappings**: one input, shaped like a MIDI mapping (`shaping.rs`), sent to an address the
//!   app already knows (`/composition/layers/1/video/opacity 0.5`, `/go`).
//!
//! Only values that changed are sent. Axes are held to the preset's rate cap; button presses and
//! releases always go out at once, so a quick tap is never lost.

use std::collections::HashMap;
use std::time::Duration;

use sc_core::protocol::buttons;
use sc_core::ControllerState;
use serde::{Deserialize, Serialize};

use crate::shaping::{self, Compiled, ShapeState, Shaping, GYRO_FULL_SCALE_DPS, QUAT_SCALE};

/// Accelerometer counts per g.
const ACCEL_PER_G: f32 = 16384.0;
/// Rates at or above this send on every state report (the controller reports at ~265 Hz).
pub const FULL_RATE: u32 = 265;

/// One OSC argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arg {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(String),
}

/// One OSC message.
#[derive(Debug, Clone, PartialEq)]
pub struct Msg {
    pub addr: String,
    pub args: Vec<Arg>,
}

/// Which parts of the controller the stream sends.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Groups {
    pub triggers: bool,
    pub sticks: bool,
    pub pads: bool,
    pub buttons: bool,
    /// Stick and grip touch sensors.
    pub touch: bool,
    pub tilt: bool,
    pub gyro: bool,
    pub accel: bool,
    pub quat: bool,
    /// Battery level and whether a controller is connected.
    pub status: bool,
}

impl Default for Groups {
    fn default() -> Self {
        Groups {
            triggers: true,
            sticks: true,
            pads: true,
            buttons: true,
            touch: true,
            tilt: true,
            gyro: true,
            accel: false,
            quat: false,
            status: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stream {
    pub enabled: bool,
    /// Put in front of every address, e.g. `/sc`; may be empty.
    pub prefix: String,
    /// One value per address (`/sc/left_stick/x`) instead of grouped (`/sc/left_stick x y`).
    pub split: bool,
    pub groups: Groups,
}

impl Default for Stream {
    fn default() -> Self {
        Stream { enabled: true, prefix: "/sc".into(), split: false, groups: Groups::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Float,
    Int,
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendMode {
    /// The value, whenever it changes.
    Continuous,
    /// The top of the output range on press, the bottom on release (past half counts as pressed).
    Button,
    /// The address with only its fixed arguments, once per press (e.g. QLab's `/go`).
    Trigger,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mapping {
    pub enabled: bool,
    #[serde(flatten)]
    pub shaping: Shaping,
    pub address: String,
    /// Sent before the value, e.g. AbletonOSC's track number.
    pub args: Vec<Arg>,
    pub value: ValueType,
    /// What the shaped 0..1 maps to.
    pub out: [f32; 2],
    pub mode: SendMode,
}

impl Default for Mapping {
    fn default() -> Self {
        Mapping {
            enabled: true,
            shaping: Shaping::default(),
            address: "/value".into(),
            args: Vec::new(),
            value: ValueType::Float,
            out: [0.0, 1.0],
            mode: SendMode::Continuous,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    /// The port this app listens on by default; picking the preset fills it in.
    pub port: Option<u16>,
    pub stream: Stream,
    pub mappings: Vec<Mapping>,
    /// Most messages per second for axes, `FULL_RATE` for every report.
    pub rate: u32,
    /// Send each report's messages as one bundle (else one packet per message).
    pub bundle: bool,
}

impl Default for Preset {
    fn default() -> Self {
        builtin_presets().remove(0)
    }
}

impl Preset {
    fn stream(name: &str, port: u16) -> Self {
        Preset { name: name.into(), port: Some(port), stream: Stream::default(), mappings: Vec::new(), rate: FULL_RATE, bundle: true }
    }
    fn mapped(name: &str, port: u16, mappings: Vec<Mapping>) -> Self {
        Preset { stream: Stream { enabled: false, ..Default::default() }, mappings, ..Preset::stream(name, port) }
    }
    fn split(self) -> Self {
        Preset { stream: Stream { split: true, ..self.stream }, ..self }
    }
    fn rate(self, rate: u32) -> Self {
        Preset { rate, ..self }
    }
}

fn float(source: &str, address: &str, out: [f32; 2]) -> Mapping {
    Mapping { shaping: Shaping::source(source), address: address.into(), out, ..Default::default() }
}

fn int(source: &str, address: &str, out: [f32; 2]) -> Mapping {
    Mapping { value: ValueType::Int, ..float(source, address, out) }
}

/// 1 while the button is held, 0 when released.
fn button(name: &str, address: &str) -> Mapping {
    Mapping { mode: SendMode::Button, ..int(&format!("button:{name}"), address, [0.0, 1.0]) }
}

/// The address alone (plus `args`) on each press.
fn trigger(name: &str, address: &str, args: Vec<Arg>) -> Mapping {
    Mapping { mode: SendMode::Trigger, args, ..float(&format!("button:{name}"), address, [0.0, 1.0]) }
}

fn with_args(m: Mapping, args: Vec<Arg>) -> Mapping {
    Mapping { args, ..m }
}

fn deadzone(m: Mapping, dz: f32) -> Mapping {
    Mapping { shaping: Shaping { deadzone: dz, ..m.shaping }, ..m }
}

/// Presets that ship with the app, one per app with its default port (checked against each
/// app's docs or source in Oct 2026). The first is the default. The OSC tab's guide has a setup
/// step for each, by name; keep them in sync.
pub fn builtin_presets() -> Vec<Preset> {
    let stick = |src: &str, addr: &str, out| deadzone(float(src, addr, out), 0.1);
    vec![
        Preset::stream("Generic", 9000),
        // The OSC In CHOP has no default port; one value per address makes one channel each.
        Preset::stream("TouchDesigner", 10000).split(),
        Preset::stream("Max/MSP", 7400),
        Preset::stream("Pure Data", 9001),
        Preset::stream("Processing (oscP5)", 12000),
        Preset::stream("openFrameworks (ofxOsc)", 12345),
        Preset::stream("SuperCollider", 57120),
        // Every message is a cue in Sonic Pi; keep it to the essentials, 60 per second.
        Preset {
            stream: Stream {
                groups: Groups { pads: false, touch: false, tilt: false, gyro: false, status: false, ..Groups::default() },
                ..Stream::default()
            },
            ..Preset::stream("Sonic Pi", 4560).rate(60)
        },
        Preset::stream("Unity (OscJack)", 9000),
        Preset::stream("Unreal Engine", 8000),
        Preset::stream("Godot (godOSC)", 4646),
        Preset::stream("Chataigne", 12000),
        Preset::stream("Open Stage Control", 8080),
        Preset::stream("Isadora", 1234).split(),
        Preset::stream("VDMX", 1235).split(),
        Preset::stream("QLC+", 7700).split().rate(60),
        Preset {
            stream: Stream::default(),
            ..Preset::mapped("MadMapper", 8000, vec![float("trigger_right", "/master/master_level", [0.0, 1.0])])
        },
        Preset::mapped("Millumin", 5000, (1..=4).zip(["A", "B", "X", "Y"]).map(|(col, b)| {
            trigger(b, "/millumin/action/launchColumn", vec![Arg::Int(col)])
        }).collect()),
        Preset::mapped("Resolume Arena / Avenue", 7000, {
            let mut m = vec![
                float("trigger_left", "/composition/layers/1/video/opacity", [0.0, 1.0]),
                float("trigger_right", "/composition/layers/2/video/opacity", [0.0, 1.0]),
            ];
            m.extend((1..=4).zip(["A", "B", "X", "Y"]).map(|(clip, b)| button(b, &format!("/composition/layers/1/clips/{clip}/connect"))));
            m
        }),
        Preset::mapped("QLab", 53000, vec![
            trigger("A", "/go", vec![]),
            trigger("B", "/panic", vec![]),
            trigger("Down", "/select/next", vec![]),
            trigger("Up", "/select/previous", vec![]),
        ]),
        // Default.ReaperOSC: t = trigger, b = 0/1, n = normalized 0..1.
        Preset::mapped("Reaper", 8000, vec![
            trigger("A", "/play", vec![]),
            trigger("B", "/stop", vec![]),
            trigger("X", "/record", vec![]),
            button("Left", "/rewind"),
            button("Right", "/forward"),
            stick("left_stick_x", "/track/pan", [0.0, 1.0]),
            float("trigger_left", "/fxparam/1/value", [0.0, 1.0]),
            float("trigger_right", "/fxparam/2/value", [0.0, 1.0]),
        ]),
        // Tracks and scenes count from 0. Live's volume 0.85 is 0 dB.
        Preset::mapped("Ableton Live (AbletonOSC)", 11000, vec![
            trigger("A", "/live/song/start_playing", vec![]),
            trigger("B", "/live/song/stop_playing", vec![]),
            trigger("X", "/live/scene/fire", vec![Arg::Int(0)]),
            trigger("Y", "/live/scene/fire", vec![Arg::Int(1)]),
            with_args(float("trigger_left", "/live/track/set/volume", [0.0, 0.85]), vec![Arg::Int(0)]),
            with_args(float("trigger_right", "/live/track/set/volume", [0.0, 0.85]), vec![Arg::Int(1)]),
        ]),
        // DrivenByMoss' default value resolution is low: 0..127.
        Preset::mapped("Bitwig (DrivenByMoss)", 8000, vec![
            trigger("A", "/play", vec![]),
            trigger("B", "/stop", vec![]),
            int("trigger_left", "/track/1/volume", [0.0, 127.0]),
            int("trigger_right", "/track/2/volume", [0.0, 127.0]),
            deadzone(int("right_stick_x", "/device/param/1/value", [0.0, 127.0]), 0.1),
            deadzone(int("right_stick_y", "/device/param/2/value", [0.0, 127.0]), 0.1),
        ]),
        // cvOSCcv turns the value straight into volts.
        Preset::mapped("VCV Rack (cvOSCcv)", 7001, vec![
            float("trigger_left", "/ch/1", [0.0, 10.0]),
            float("trigger_right", "/ch/2", [0.0, 10.0]),
            stick("left_stick_x", "/ch/3", [-5.0, 5.0]),
            stick("left_stick_y", "/ch/4", [-5.0, 5.0]),
            stick("right_stick_x", "/ch/5", [-5.0, 5.0]),
            stick("right_stick_y", "/ch/6", [-5.0, 5.0]),
            Mapping { value: ValueType::Float, out: [0.0, 10.0], ..button("A", "/ch/7") },
            Mapping { value: ValueType::Float, out: [0.0, 10.0], ..button("B", "/ch/8") },
        ]),
        Preset::mapped("ETC Eos", 8000, vec![
            trigger("A", "/eos/key/go_0", vec![]),
            trigger("Y", "/eos/sub/1/full", vec![]),
        ]),
        // Axes must go back to 0 and buttons to 0 after 1, or VRChat keeps moving / ignores presses.
        Preset::mapped("VRChat", 9000, vec![
            stick("left_stick_y", "/input/Vertical", [-1.0, 1.0]),
            stick("left_stick_x", "/input/Horizontal", [-1.0, 1.0]),
            stick("right_stick_x", "/input/LookHorizontal", [-1.0, 1.0]),
            button("A", "/input/Jump"),
            button("L3", "/input/Run"),
            button("Menu", "/input/QuickMenuToggleLeft"),
            float("trigger_right", "/avatar/parameters/TriggerRight", [0.0, 1.0]),
        ])
        .rate(60),
    ]
}

/// Stream address of each button (stream names are lower case, d-pad spelled out).
fn stream_button(name: &str) -> Option<&'static str> {
    Some(match name {
        "A" => "a", "B" => "b", "X" => "x", "Y" => "y",
        "Up" => "dpad_up", "Down" => "dpad_down", "Left" => "dpad_left", "Right" => "dpad_right",
        "L" => "l", "R" => "r", "L3" => "l3", "R3" => "r3", "L4" => "l4", "L5" => "l5", "R4" => "r4", "R5" => "r5",
        "View" => "view", "Menu" => "menu", "QAM" => "qam", "Steam" => "steam",
        _ => return None,
    })
}

/// Floats are rounded to 1e-4 so sensor noise below that doesn't count as a change.
fn f(v: f32) -> Arg {
    Arg::Float((v * 10000.0).round() / 10000.0)
}

fn i(on: bool) -> Arg {
    Arg::Int(on as i32)
}

/// One stream address with its values; `instant` ones (buttons) skip the rate cap.
struct Value {
    addr: String,
    args: Vec<Arg>,
    instant: bool,
}

/// The stream's addresses and values for one state report.
fn stream_values(st: &Stream, s: &ControllerState, battery: Option<u8>) -> Vec<Value> {
    let p = st.prefix.trim_end_matches('/');
    let p = if p.is_empty() || p.starts_with('/') { p.to_string() } else { format!("/{p}") };
    let g = &st.groups;
    let mut out = Vec::new();
    let mut push = |name: &str, args: Vec<Arg>, instant: bool| out.push(Value { addr: format!("{p}/{name}"), args, instant });
    // A grouped value, or one address per part when split.
    let multi = |push: &mut dyn FnMut(&str, Vec<Arg>, bool), name: &str, parts: &[&str], vals: Vec<Arg>| {
        if st.split {
            for (part, v) in parts.iter().zip(vals) {
                push(&format!("{name}/{part}"), vec![v], false);
            }
        } else {
            push(name, vals, false);
        }
    };
    let pressed = |mask| s.pressed(mask);
    if g.triggers {
        push("trigger_left", vec![f(shaping::axis(s.trigger_left).max(0.0))], false);
        push("trigger_right", vec![f(shaping::axis(s.trigger_right).max(0.0))], false);
        push("trigger_left/click", vec![i(pressed(buttons::LEFT_TRIGGER_CLICK))], true);
        push("trigger_right/click", vec![i(pressed(buttons::RIGHT_TRIGGER_CLICK))], true);
    }
    if g.sticks {
        for (name, v) in [("left_stick", s.left_stick), ("right_stick", s.right_stick)] {
            multi(&mut push, name, &["x", "y"], vec![f(shaping::axis(v[0])), f(shaping::axis(v[1]))]);
        }
    }
    if g.pads {
        for (name, pad, touch, click) in [
            ("left_pad", s.left_pad, buttons::LEFT_PAD_TOUCH, buttons::LEFT_PAD_CLICK),
            ("right_pad", s.right_pad, buttons::RIGHT_PAD_TOUCH, buttons::RIGHT_PAD_CLICK),
        ] {
            // Position only while touched: an untouched pad has none.
            if pad.touched {
                multi(&mut push, name, &["x", "y"], vec![f(shaping::axis(pad.x)), f(shaping::axis(pad.y))]);
            }
            push(&format!("{name}/pressure"), vec![f(shaping::pressure(pad.pressure))], false);
            push(&format!("{name}/touch"), vec![i(pad.touched || pressed(touch))], true);
            push(&format!("{name}/click"), vec![i(pad.clicked || pressed(click))], true);
        }
    }
    if g.buttons {
        for (mask, name) in buttons::NAMES {
            if let Some(addr) = stream_button(name) {
                push(&format!("button/{addr}"), vec![i(pressed(*mask))], true);
            }
        }
    }
    if g.touch {
        push("left_stick/touch", vec![i(pressed(buttons::LEFT_STICK_TOUCH))], true);
        push("right_stick/touch", vec![i(pressed(buttons::RIGHT_STICK_TOUCH))], true);
        push("left_grip/touch", vec![i(pressed(buttons::LEFT_GRIP_TOUCH))], true);
        push("right_grip/touch", vec![i(pressed(buttons::RIGHT_GRIP_TOUCH))], true);
    }
    if g.tilt {
        let [pitch, roll] = shaping::tilt(s);
        multi(&mut push, "tilt", &["pitch", "roll"], vec![f(pitch * 90.0), f(roll * 90.0)]);
    }
    if g.gyro {
        let dps = |v: i16| f(v as f32 / 32767.0 * GYRO_FULL_SCALE_DPS);
        multi(&mut push, "gyro", &["pitch", "roll", "yaw"], s.imu.gyro.map(dps).to_vec());
    }
    if g.accel {
        multi(&mut push, "accel", &["x", "y", "z"], s.imu.accel.map(|v| f(v as f32 / ACCEL_PER_G)).to_vec());
    }
    if g.quat
        && let Some(q) = s.imu.quat
    {
        multi(&mut push, "quat", &["w", "x", "y", "z"], q.map(|v| f(v as f32 / QUAT_SCALE)).to_vec());
    }
    if g.status {
        if let Some(level) = battery {
            push("battery", vec![Arg::Int(level as i32)], false);
        }
        push("connected", vec![Arg::Int(1)], true);
    }
    out
}

/// The address `/connected` goes to, for releasing.
fn connected_addr(st: &Stream) -> String {
    let p = st.prefix.trim_end_matches('/');
    if p.is_empty() || p.starts_with('/') { format!("{p}/connected") } else { format!("/{p}/connected") }
}

#[derive(Default)]
struct MapState {
    shape: ShapeState,
    /// Arguments of the last message, value included.
    sent: Option<Vec<Arg>>,
    /// Pressed, for button and trigger mode.
    high: bool,
}

pub struct Engine {
    preset: Preset,
    compiled: Vec<Compiled>,
    maps: Vec<MapState>,
    /// Stream values as last sent, in the order first sent, with an index by address.
    sent: Vec<(String, Vec<Arg>)>,
    index: HashMap<String, usize>,
    /// When the rate cap next lets axes through.
    next_flush: Option<Duration>,
}

impl Engine {
    pub fn new(preset: Preset) -> Self {
        let mut e = Engine {
            preset: Preset::default(),
            compiled: Vec::new(),
            maps: Vec::new(),
            sent: Vec::new(),
            index: HashMap::new(),
            next_flush: None,
        };
        e.set_preset(preset);
        e
    }


    /// Switch to `preset`. Mappings that did not change keep their state; if the stream changed,
    /// it starts over (every value is sent again).
    pub fn set_preset(&mut self, preset: Preset) {
        let old = std::mem::replace(&mut self.preset, preset);
        let mut old_maps: Vec<Option<MapState>> = std::mem::take(&mut self.maps).into_iter().map(Some).collect();
        self.maps = self
            .preset
            .mappings
            .iter()
            .map(|m| old.mappings.iter().position(|o| o == m).and_then(|i| old_maps.get_mut(i)?.take()).unwrap_or_default())
            .collect();
        self.compiled = self.preset.mappings.iter().map(|m| Compiled::new(&m.shaping)).collect();
        if old.stream != self.preset.stream {
            self.sent.clear();
            self.index.clear();
        }
    }

    pub fn levels(&self) -> Vec<Option<f32>> {
        self.maps.iter().map(|m| m.shape.level).collect()
    }

    /// Every address with what it last carried: the stream's, then each mapping's.
    pub fn values(&self) -> Vec<(String, Vec<Arg>)> {
        let mut out = self.sent.clone();
        for (m, st) in self.preset.mappings.iter().zip(&self.maps) {
            if let Some(args) = &st.sent {
                out.push((m.address.clone(), args.clone()));
            }
        }
        out
    }

    /// Feed one state report at time `now` (any steady clock); appends the messages to send.
    pub fn process(&mut self, s: &ControllerState, battery: Option<u8>, now: Duration, out: &mut Vec<Msg>) {
        // Axes go out when the rate cap allows, buttons always.
        let rate = self.preset.rate.max(1);
        let flush = rate >= FULL_RATE || self.next_flush.is_none_or(|t| now >= t);
        if flush && rate < FULL_RATE {
            let interval = Duration::from_secs_f64(1.0 / rate as f64);
            // Keep to the schedule so the average rate is the cap; after a gap, start over.
            self.next_flush = Some(match self.next_flush {
                Some(t) => (t + interval).max(now),
                None => now + interval,
            });
        }
        if self.preset.stream.enabled {
            for v in stream_values(&self.preset.stream, s, battery) {
                if !(flush || v.instant) {
                    continue;
                }
                match self.index.get(&v.addr) {
                    Some(&i) if self.sent[i].1 == v.args => continue,
                    Some(&i) => self.sent[i].1 = v.args.clone(),
                    None => {
                        self.index.insert(v.addr.clone(), self.sent.len());
                        self.sent.push((v.addr.clone(), v.args.clone()));
                    }
                }
                out.push(Msg { addr: v.addr, args: v.args });
            }
        }
        for i in 0..self.maps.len() {
            self.process_mapping(i, s, flush, out);
        }
    }

    fn process_mapping(&mut self, i: usize, s: &ControllerState, flush: bool, out: &mut Vec<Msg>) {
        let m = &self.preset.mappings[i];
        if !m.enabled || !valid_address(&m.address) {
            return;
        }
        let st = &mut self.maps[i];
        let Some(step) = st.shape.step(&m.shaping, &self.compiled[i], s) else { return };
        let high = step.v >= 0.5;
        let was_high = std::mem::replace(&mut st.high, high);
        let args = match m.mode {
            SendMode::Continuous if flush || matches!(self.compiled[i].src, Some(shaping::Src::Button(_))) => value_args(m, step.v),
            SendMode::Button if high != was_high => value_args(m, if high { 1.0 } else { 0.0 }),
            SendMode::Trigger if step.rising => {
                st.sent = Some(m.args.clone());
                out.push(Msg { addr: m.address.clone(), args: m.args.clone() });
                return;
            }
            _ => return,
        };
        if st.sent.as_ref() != Some(&args) {
            st.sent = Some(args.clone());
            out.push(Msg { addr: m.address.clone(), args });
        }
    }

    /// The controller went away (or live input and a replay swapped): release every button that
    /// is down, put continuous mappings back at rest, say `/connected 0`, and forget what was
    /// sent so the next report sends everything again. Does nothing when nothing was sent.
    pub fn release(&mut self, out: &mut Vec<Msg>) {
        for (addr, args) in self.sent.drain(..) {
            let button = addr.contains("/button/") || addr.ends_with("/touch") || addr.ends_with("/click");
            if button && args != [Arg::Int(0)] {
                out.push(Msg { addr, args: vec![Arg::Int(0)] });
            }
        }
        if !self.index.is_empty() && self.preset.stream.enabled && self.preset.stream.groups.status {
            out.push(Msg { addr: connected_addr(&self.preset.stream), args: vec![Arg::Int(0)] });
        }
        self.index.clear();
        for (i, m) in self.preset.mappings.iter().enumerate() {
            let st = std::mem::take(&mut self.maps[i]);
            let Some(sent) = st.sent else { continue };
            let rest = match m.mode {
                SendMode::Continuous => ShapeState::rest(&m.shaping, &self.compiled[i]).map(|v| value_args(m, v)),
                SendMode::Button => Some(value_args(m, 0.0)),
                SendMode::Trigger => None,
            };
            if let Some(args) = rest
                && args != sent
            {
                out.push(Msg { addr: m.address.clone(), args });
            }
        }
        self.next_flush = None;
    }
}

/// OSC addresses start with `/` and have no spaces.
pub fn valid_address(a: &str) -> bool {
    a.starts_with('/') && a.len() > 1 && !a.contains(char::is_whitespace)
}

/// A mapping's arguments for the shaped value `v` (0..1): fixed ones, then the value.
fn value_args(m: &Mapping, v: f32) -> Vec<Arg> {
    let [lo, hi] = m.out;
    let x = lo + (hi - lo) * v;
    let mut args = m.args.clone();
    args.push(match m.value {
        ValueType::Float => f(x),
        ValueType::Int => Arg::Int(x.round() as i32),
        ValueType::Bool => Arg::Bool(v >= 0.5),
    });
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ControllerState {
        let mut s = ControllerState::default();
        s.imu.quat = Some([32767, 0, 0, 0]);
        s
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn stream_only(groups: Groups, split: bool) -> Engine {
        Engine::new(Preset { stream: Stream { groups, split, ..Default::default() }, ..Preset::stream("t", 9000) })
    }

    fn none() -> Groups {
        Groups {
            triggers: false, sticks: false, pads: false, buttons: false, touch: false,
            tilt: false, gyro: false, accel: false, quat: false, status: false,
        }
    }

    fn addrs(out: &[Msg]) -> Vec<&str> {
        out.iter().map(|m| m.addr.as_str()).collect()
    }

    #[test]
    fn builtin_presets_are_valid() {
        let presets = builtin_presets();
        assert_eq!(presets[0].name, "Generic");
        for p in &presets {
            assert!(p.port.is_some(), "{}", p.name);
            assert!(p.stream.enabled || !p.mappings.is_empty(), "{} sends nothing", p.name);
            for m in &p.mappings {
                assert!(shaping::Src::parse(&m.shaping.source).is_some(), "{}: {}", p.name, m.shaping.source);
                assert!(valid_address(&m.address), "{}: {}", p.name, m.address);
            }
        }
        let mut names: Vec<_> = presets.iter().map(|p| &p.name).collect();
        names.dedup();
        assert_eq!(names.len(), presets.len());
    }

    #[test]
    fn stream_sends_grouped_values_only_when_they_change() {
        let mut e = stream_only(Groups { sticks: true, ..none() }, false);
        let mut s = state();
        let mut out = Vec::new();
        e.process(&s, None, ms(0), &mut out);
        assert_eq!(out, vec![
            Msg { addr: "/sc/left_stick".into(), args: vec![Arg::Float(0.0), Arg::Float(0.0)] },
            Msg { addr: "/sc/right_stick".into(), args: vec![Arg::Float(0.0), Arg::Float(0.0)] },
        ]);
        out.clear();
        e.process(&s, None, ms(4), &mut out);
        assert!(out.is_empty());
        s.left_stick = [32767, -32767];
        e.process(&s, None, ms(8), &mut out);
        assert_eq!(out, vec![Msg { addr: "/sc/left_stick".into(), args: vec![Arg::Float(1.0), Arg::Float(-1.0)] }]);
    }

    #[test]
    fn split_and_prefix() {
        let mut e = Engine::new(Preset {
            stream: Stream { prefix: "ctl/".into(), split: true, groups: Groups { tilt: true, ..none() }, ..Default::default() },
            ..Preset::stream("t", 9000)
        });
        let mut out = Vec::new();
        e.process(&state(), None, ms(0), &mut out);
        assert_eq!(addrs(&out), ["/ctl/tilt/pitch", "/ctl/tilt/roll"]);
    }

    #[test]
    fn pads_send_position_only_while_touched() {
        let mut e = stream_only(Groups { pads: true, ..none() }, false);
        let mut s = state();
        let mut out = Vec::new();
        e.process(&s, None, ms(0), &mut out);
        assert!(!addrs(&out).contains(&"/sc/left_pad"));
        out.clear();
        s.left_pad.touched = true;
        s.left_pad.x = 32767;
        e.process(&s, None, ms(4), &mut out);
        assert_eq!(addrs(&out), ["/sc/left_pad", "/sc/left_pad/touch"]);
        assert_eq!(out[0].args, vec![Arg::Float(1.0), Arg::Float(0.0)]);
    }

    #[test]
    fn rate_cap_holds_axes_but_not_buttons() {
        let mut e = Engine::new(Preset { rate: 60, ..stream_only(Groups { triggers: true, buttons: true, ..none() }, false).preset });
        let mut s = state();
        let mut out = Vec::new();
        e.process(&s, None, ms(0), &mut out);
        out.clear();
        // 4 ms later: the trigger moved and A went down; only A gets through.
        s.trigger_right = 16384;
        s.buttons = buttons::A;
        e.process(&s, None, ms(4), &mut out);
        assert_eq!(out, vec![Msg { addr: "/sc/button/a".into(), args: vec![Arg::Int(1)] }]);
        out.clear();
        // Past 1/60 s: the trigger follows.
        e.process(&s, None, ms(17), &mut out);
        assert_eq!(addrs(&out), ["/sc/trigger_right"]);
    }

    #[test]
    fn rate_cap_averages_to_the_rate() {
        let mut e = Engine::new(Preset { rate: 120, ..stream_only(Groups { sticks: true, ..none() }, false).preset });
        let mut s = state();
        let mut out = Vec::new();
        // One second of reports at ~265 Hz with the stick always moving.
        let mut sent = 0;
        for n in 0..265u64 {
            s.left_stick[0] = (n * 100) as i16;
            e.process(&s, None, Duration::from_micros(n * 3774), &mut out);
            sent += out.iter().filter(|m| m.addr == "/sc/left_stick").count();
            out.clear();
        }
        assert!((115..=125).contains(&sent), "{sent}");
    }

    #[test]
    fn mapping_modes() {
        let mut e = Engine::new(Preset::mapped("t", 9000, vec![
            with_args(float("trigger_right", "/vol", [0.0, 2.0]), vec![Arg::Int(3)]),
            button("A", "/jump"),
            trigger("B", "/go", vec![Arg::String("x".into())]),
            Mapping { value: ValueType::Bool, ..float("trigger_left", "/on", [0.0, 1.0]) },
        ]));
        let mut s = state();
        let mut out = Vec::new();
        e.process(&s, None, ms(0), &mut out);
        assert_eq!(out, vec![
            Msg { addr: "/vol".into(), args: vec![Arg::Int(3), Arg::Float(0.0)] },
            Msg { addr: "/on".into(), args: vec![Arg::Bool(false)] },
        ]);
        out.clear();
        s.trigger_right = 32767;
        s.buttons = buttons::A | buttons::B;
        e.process(&s, None, ms(4), &mut out);
        assert_eq!(out, vec![
            Msg { addr: "/vol".into(), args: vec![Arg::Int(3), Arg::Float(2.0)] },
            Msg { addr: "/jump".into(), args: vec![Arg::Int(1)] },
            Msg { addr: "/go".into(), args: vec![Arg::String("x".into())] },
        ]);
        out.clear();
        // Held: nothing new. Released: only the button's 0.
        e.process(&s, None, ms(8), &mut out);
        assert!(out.is_empty());
        s.buttons = 0;
        e.process(&s, None, ms(12), &mut out);
        assert_eq!(out, vec![Msg { addr: "/jump".into(), args: vec![Arg::Int(0)] }]);
    }

    #[test]
    fn int_mapping_sends_only_when_the_integer_changes() {
        let mut e = Engine::new(Preset::mapped("t", 9000, vec![int("trigger_right", "/v", [0.0, 127.0])]));
        let mut s = state();
        let mut out = Vec::new();
        e.process(&s, None, ms(0), &mut out);
        out.clear();
        s.trigger_right = 50; // well under one step of 127
        e.process(&s, None, ms(4), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn release_lets_go_of_everything_once() {
        let mut e = Engine::new(Preset {
            stream: Stream { groups: Groups { buttons: true, status: true, ..none() }, ..Default::default() },
            mappings: vec![
                deadzone(float("left_stick_y", "/input/Vertical", [-1.0, 1.0]), 0.1),
                button("A", "/input/Jump"),
            ],
            ..Preset::stream("t", 9000)
        });
        let mut s = state();
        s.buttons = buttons::A;
        s.left_stick = [0, 32767];
        let mut out = Vec::new();
        e.process(&s, Some(80), ms(0), &mut out);
        out.clear();
        e.release(&mut out);
        assert_eq!(out, vec![
            Msg { addr: "/sc/button/a".into(), args: vec![Arg::Int(0)] },
            Msg { addr: "/sc/connected".into(), args: vec![Arg::Int(0)] },
            Msg { addr: "/input/Vertical".into(), args: vec![Arg::Float(0.0)] },
            Msg { addr: "/input/Jump".into(), args: vec![Arg::Int(0)] },
        ]);
        out.clear();
        e.release(&mut out);
        assert!(out.is_empty());
        // The next report sends everything again.
        e.process(&s, Some(80), ms(4), &mut out);
        assert!(addrs(&out).contains(&"/sc/connected"));
        assert!(addrs(&out).contains(&"/input/Jump"));
    }

    #[test]
    fn units() {
        let mut e = stream_only(Groups { gyro: true, accel: true, status: true, ..none() }, false);
        let mut s = state();
        s.imu.gyro = [32767, 0, 0];
        s.imu.accel = [0, 0, 16384];
        let mut out = Vec::new();
        e.process(&s, Some(55), ms(0), &mut out);
        let get = |a: &str| out.iter().find(|m| m.addr == a).unwrap().args.clone();
        assert_eq!(get("/sc/gyro")[0], Arg::Float(2000.0));
        assert_eq!(get("/sc/accel")[2], Arg::Float(1.0));
        assert_eq!(get("/sc/battery"), vec![Arg::Int(55)]);
    }

    #[test]
    fn preset_json_round_trips() {
        for p in builtin_presets() {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<Preset>(&json).unwrap(), p);
        }
        let m: Mapping = serde_json::from_str(r#"{"source":"button:A","address":"/go","args":[{"int":2}],"mode":"trigger"}"#).unwrap();
        assert_eq!(m.args, vec![Arg::Int(2)]);
        assert_eq!(m.shaping.source, "button:A");
    }
}
