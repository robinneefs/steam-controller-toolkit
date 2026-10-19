//! Reading controller inputs as numbers and shaping them, shared by every output (MIDI, OSC).
//!
//! Every input is read as unipolar 0..1 (triggers, pressure, buttons) or bipolar -1..1 (sticks,
//! pads, tilt, gyro). A [`Shaping`] turns that into 0..1 with a deadzone, curve, range, invert,
//! smoothing and toggle, optionally only while a button is held.

use sc_core::protocol::buttons;
use sc_core::ControllerState;
use serde::{Deserialize, Serialize};

/// Inputs that are not buttons, as stored in presets. Keep in sync with `SOURCES` in
/// `src/tabs/sources.ts`. Buttons are `button:<name>` with names from `buttons::NAMES`.
pub const AXES: &[&str] = &[
    "trigger_left", "trigger_right",
    "left_stick_x", "left_stick_y", "right_stick_x", "right_stick_y",
    "left_pad_x", "left_pad_y", "left_pad_pressure",
    "right_pad_x", "right_pad_y", "right_pad_pressure",
    "tilt_pitch", "tilt_roll",
    "gyro_pitch", "gyro_roll", "gyro_yaw",
];

/// Pad pressure that counts as full; real presses rarely go past ~30000 (see the visualizer).
pub const FULL_PRESSURE: f32 = 30000.0;
/// Turn rate that counts as full scale for the gyro sources.
const FULL_GYRO_DPS: f32 = 500.0;
pub const GYRO_FULL_SCALE_DPS: f32 = 2000.0;
pub const QUAT_SCALE: f32 = 32767.0;

/// How one input becomes a 0..1 output. Outputs embed it (`#[serde(flatten)]`), so its fields
/// sit next to the output's own in preset files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shaping {
    /// One of `AXES`, or `button:<name>`.
    pub source: String,
    /// Part of the 0..1 input that spans the output (after deadzone and curve).
    pub range: [f32; 2],
    pub invert: bool,
    /// Fraction around the centre (bipolar) or at the bottom (unipolar) that reads as rest.
    pub deadzone: f32,
    /// -1..1; 0 is linear, above 0 starts slow (fine control near rest), below 0 starts fast.
    pub curve: f32,
    /// 0..1; how much each new reading is smoothed into the last (0 = none).
    pub smoothing: f32,
    /// Each press flips the output between 0 and 1 (for buttons).
    pub toggle: bool,
    /// Only active while this button is held (`buttons::NAMES`), e.g. tilt only while R4 is down.
    pub hold: Option<String>,
}

impl Default for Shaping {
    fn default() -> Self {
        Shaping {
            source: "trigger_right".into(),
            range: [0.0, 1.0],
            invert: false,
            deadzone: 0.0,
            curve: 0.0,
            smoothing: 0.0,
            toggle: false,
            hold: None,
        }
    }
}

impl Shaping {
    pub fn source(source: &str) -> Self {
        Shaping { source: source.into(), ..Default::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Src {
    Axis(usize),
    Button(u32),
}

impl Src {
    pub fn parse(s: &str) -> Option<Src> {
        if let Some(name) = s.strip_prefix("button:") {
            return button_mask(name).map(Src::Button);
        }
        AXES.iter().position(|a| *a == s).map(Src::Axis)
    }
    pub fn bipolar(self) -> bool {
        match self {
            Src::Axis(i) => !matches!(AXES[i], "trigger_left" | "trigger_right" | "left_pad_pressure" | "right_pad_pressure"),
            Src::Button(_) => false,
        }
    }
}

pub fn button_mask(name: &str) -> Option<u32> {
    buttons::NAMES.iter().find(|(_, n)| *n == name).map(|(m, _)| *m)
}

/// The world's up direction in the controller's frame (+X right, +Y forward, +Z out of the face),
/// from the orientation quaternion when the report has one, else from the accelerometer.
pub fn up_vector(s: &ControllerState) -> [f32; 3] {
    if let Some(q) = s.imu.quat {
        let [w, x, y, z] = q.map(|v| v as f32 / QUAT_SCALE);
        // Third row of the rotation matrix: world +Z seen from the sensor.
        return [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)];
    }
    let [x, y, z] = s.imu.accel.map(|v| v as f32);
    let len = (x * x + y * y + z * z).sqrt().max(1.0);
    [x / len, y / len, z / len]
}

/// Tilt as (pitch, roll), ±1 for ±90°. Pitch is the front (triggers) raised; roll is the right
/// side lowered.
pub fn tilt(s: &ControllerState) -> [f32; 2] {
    let up = up_vector(s);
    let t = |c: f32| c.clamp(-1.0, 1.0).asin() / std::f32::consts::FRAC_PI_2;
    [t(up[1]), t(-up[0])]
}

pub fn axis(v: i16) -> f32 {
    (v as f32 / 32767.0).clamp(-1.0, 1.0)
}

pub fn pressure(p: u16) -> f32 {
    (p as f32 / FULL_PRESSURE).min(1.0).sqrt()
}

/// Read one source: unipolar 0..1 or bipolar -1..1. `None` while it has no value (an untouched pad).
pub fn read(src: Src, s: &ControllerState) -> Option<f32> {
    let gyro = |v: i16| (v as f32 / 32767.0 * GYRO_FULL_SCALE_DPS / FULL_GYRO_DPS).clamp(-1.0, 1.0);
    let pad = |p: &sc_core::protocol::Trackpad, v: i16| p.touched.then(|| axis(v));
    let src = match src {
        Src::Button(mask) => return Some(if s.pressed(mask) { 1.0 } else { 0.0 }),
        Src::Axis(i) => AXES[i],
    };
    Some(match src {
        "trigger_left" => axis(s.trigger_left).max(0.0),
        "trigger_right" => axis(s.trigger_right).max(0.0),
        "left_stick_x" => axis(s.left_stick[0]),
        "left_stick_y" => axis(s.left_stick[1]),
        "right_stick_x" => axis(s.right_stick[0]),
        "right_stick_y" => axis(s.right_stick[1]),
        "left_pad_x" => return pad(&s.left_pad, s.left_pad.x),
        "left_pad_y" => return pad(&s.left_pad, s.left_pad.y),
        "right_pad_x" => return pad(&s.right_pad, s.right_pad.x),
        "right_pad_y" => return pad(&s.right_pad, s.right_pad.y),
        "left_pad_pressure" => pressure(s.left_pad.pressure),
        "right_pad_pressure" => pressure(s.right_pad.pressure),
        "tilt_pitch" => tilt(s)[0],
        "tilt_roll" => tilt(s)[1],
        "gyro_pitch" => gyro(s.imu.gyro[0]),
        "gyro_roll" => gyro(s.imu.gyro[1]),
        "gyro_yaw" => gyro(s.imu.gyro[2]),
        _ => return None,
    })
}

/// Apply a mapping's deadzone, curve, range and invert to a raw reading; returns 0..1.
pub fn shape(m: &Shaping, bipolar: bool, v: f32) -> f32 {
    let dz = m.deadzone.clamp(0.0, 0.95);
    let exp = 4f32.powf(m.curve.clamp(-1.0, 1.0));
    // Deadzone and curve work on the distance from rest, so bipolar inputs stay symmetric.
    let mag = ((v.abs() - dz) / (1.0 - dz)).clamp(0.0, 1.0).powf(exp);
    let u = if bipolar { (1.0 + mag.copysign(v)) / 2.0 } else { mag };
    let [lo, hi] = m.range;
    let u = if (hi - lo).abs() < 1e-4 { (u >= lo) as u8 as f32 } else { ((u - lo) / (hi - lo)).clamp(0.0, 1.0) };
    if m.invert { 1.0 - u } else { u }
}

/// A [`Shaping`] with its source and hold button looked up.
pub struct Compiled {
    pub src: Option<Src>,
    hold: Option<u32>,
}

impl Compiled {
    pub fn new(m: &Shaping) -> Self {
        Compiled { src: Src::parse(&m.source), hold: m.hold.as_deref().and_then(button_mask) }
    }
}

/// One shaped reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// 0..1, after smoothing and toggle.
    pub v: f32,
    /// The value went past half on this reading (a press, for buttons).
    pub rising: bool,
}

/// What a mapping remembers between readings.
#[derive(Debug, Default, Clone)]
pub struct ShapeState {
    smooth: Option<f32>,
    /// Output after shaping, smoothing and toggle; what the UI meters show.
    pub level: Option<f32>,
    /// Input was past half last time (for toggle and press detection).
    high: bool,
    latched: bool,
}

impl ShapeState {
    /// Shape one reading. `None` when the source is unknown, its hold button is up, or it has no
    /// value (an untouched pad); the previous level is then kept.
    pub fn step(&mut self, m: &Shaping, c: &Compiled, s: &ControllerState) -> Option<Step> {
        let src = c.src?;
        if !c.hold.is_none_or(|mask| s.pressed(mask)) {
            return None;
        }
        let shaped = shape(m, src.bipolar(), read(src, s)?);
        let smoothing = m.smoothing.clamp(0.0, 0.98);
        let mut v = match self.smooth {
            Some(prev) => prev + (shaped - prev) * (1.0 - smoothing),
            None => shaped,
        };
        self.smooth = Some(v);
        let high = v >= 0.5;
        let rising = high && !self.high;
        self.high = high;
        if m.toggle {
            if rising {
                self.latched = !self.latched;
            }
            v = if self.latched { 1.0 } else { 0.0 };
        }
        self.level = Some(v);
        Some(Step { v, rising })
    }

    /// The output of this mapping with its input at rest (stick centred, nothing pressed): what
    /// outputs send when the controller goes away, so nothing is left held.
    pub fn rest(m: &Shaping, c: &Compiled) -> Option<f32> {
        let src = c.src?;
        Some(if m.toggle { 0.0 } else { shape(m, src.bipolar(), 0.0) })
    }
}

/// Inputs learn can pick, in priority order. Touch sensors are left out: touching a pad or stick
/// to move it would always win.
fn learnable() -> Vec<(String, Src)> {
    let mut out: Vec<(String, Src)> = buttons::NAMES
        .iter()
        .filter(|(_, n)| !n.contains("Touch") && !n.contains("Grip"))
        .map(|(m, n)| (format!("button:{n}"), Src::Button(*m)))
        .collect();
    out.extend(AXES.iter().enumerate().filter(|(_, a)| !a.starts_with("gyro")).map(|(i, a)| (a.to_string(), Src::Axis(i))));
    out
}

/// How far an input has to move from where it was when learning started.
fn learn_threshold(name: &str) -> f32 {
    if name.starts_with("tilt") {
        0.3
    } else if name.ends_with("pressure") {
        0.7
    } else {
        0.4
    }
}

/// Waits for an input to move: the first one that moves away from where it was when learning
/// started wins.
#[derive(Default)]
pub struct Learner {
    /// The readings when learning started; `None` while not learning.
    base: Option<Vec<Option<f32>>>,
    learned: Option<String>,
}

impl Learner {
    pub fn start(&mut self) {
        self.base = Some(Vec::new());
        self.learned = None;
    }

    pub fn cancel(&mut self) {
        self.base = None;
    }

    pub fn learning(&self) -> bool {
        self.base.is_some()
    }

    /// The input picked by the last learn, as a source name.
    pub fn learned(&self) -> Option<&str> {
        self.learned.as_deref()
    }

    pub fn process(&mut self, s: &ControllerState) {
        let Some(base) = self.base.as_mut() else { return };
        let sources = learnable();
        let now: Vec<Option<f32>> = sources.iter().map(|(_, src)| read(*src, s)).collect();
        if base.is_empty() {
            *base = now;
            return;
        }
        for (i, (name, _)) in sources.iter().enumerate() {
            match (base[i], now[i]) {
                (Some(b), Some(v)) if (v - b).abs() > learn_threshold(name) => {
                    self.learned = Some(name.clone());
                    self.base = None;
                    return;
                }
                // A pad that was untouched: its first reading becomes the starting point.
                (None, Some(v)) => base[i] = Some(v),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ControllerState {
        let mut s = ControllerState::default();
        s.imu.quat = Some([32767, 0, 0, 0]);
        s
    }

    #[test]
    fn deadzone_curve_range_invert() {
        let m = Shaping { deadzone: 0.1, ..Default::default() };
        assert_eq!(shape(&m, true, 0.05), 0.5);
        assert!((shape(&m, true, 1.0) - 1.0).abs() < 1e-6);
        assert!((shape(&m, true, -1.0)).abs() < 1e-6);
        let m = Shaping { range: [0.5, 1.0], invert: true, ..Default::default() };
        assert_eq!(shape(&m, true, 0.0), 1.0);
        assert_eq!(shape(&m, true, 1.0), 0.0);
        let m = Shaping { curve: 0.5, ..Default::default() };
        assert!((shape(&m, false, 0.5) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn tilt_from_quaternion() {
        let mut s = state();
        // Front raised 30°: rotation about +X by 30°.
        let h = 15f32.to_radians();
        s.imu.quat = Some([(h.cos() * 32767.0) as i16, (h.sin() * 32767.0) as i16, 0, 0]);
        let pitch = read(Src::parse("tilt_pitch").unwrap(), &s).unwrap();
        assert!((pitch - 30.0 / 90.0).abs() < 0.01, "{pitch}");
        let roll = read(Src::parse("tilt_roll").unwrap(), &s).unwrap();
        assert!(roll.abs() < 0.01, "{roll}");
    }

    #[test]
    fn rest_value() {
        let stick = Shaping::source("left_stick_x");
        assert_eq!(ShapeState::rest(&stick, &Compiled::new(&stick)), Some(0.5));
        let inv = Shaping { invert: true, ..Shaping::source("trigger_left") };
        assert_eq!(ShapeState::rest(&inv, &Compiled::new(&inv)), Some(1.0));
    }

    #[test]
    fn learn_picks_the_moved_input() {
        let mut l = Learner::default();
        l.start();
        let mut s = state();
        l.process(&s);
        s.left_stick = [0, 30000];
        l.process(&s);
        assert_eq!(l.learned(), Some("left_stick_y"));
        assert!(!l.learning());
    }
}
