//! Turns controller state into keyboard, mouse and gamepad input. A [`Layout`] binds every control
//! to an [`Action`] (a key or combination, a mouse button, a scroll notch, an Xbox input, opening a
//! program, or the Desktop/Game switch) and gives the sticks, trackpads and gyro a mode each. Both the Keyboard &
//! mouse tab and the Gamepad tab edit layouts; pure logic, `play_out.rs` applies the [`Frame`]s.
//!
//! Each report gives a fresh frame: what should be held now (keys, mouse buttons, the virtual
//! pad's state) plus what to move by (mouse, scroll). [`Injector`] turns frames into press and
//! release events, so letting go of everything is applying an empty frame.

use std::collections::BTreeMap;
use std::time::Duration;

use sc_core::protocol::{buttons, Trackpad};
use sc_core::ControllerState;
use serde::{Deserialize, Serialize};

use crate::shaping::{axis, tilt, up_vector, GYRO_FULL_SCALE_DPS};

/// Key names as stored in layouts, mirrored in `src/tabs/keys.ts`. Modifiers come first: a
/// combination presses them before its key and releases them after it.
pub const KEYS: &[&str] = &[
    "Ctrl", "Shift", "Alt", "Win", "RCtrl", "RShift", "RAlt",
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
    "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    "Escape", "Tab", "CapsLock", "Space", "Enter", "Backspace", "Delete", "Insert",
    "Home", "End", "PageUp", "PageDown", "Up", "Down", "Left", "Right",
    "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon", "Quote",
    "Backquote", "Comma", "Period", "Slash",
    "Num0", "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9",
    "NumAdd", "NumSubtract", "NumMultiply", "NumDivide", "NumDecimal", "NumEnter", "NumLock",
    "PrintScreen", "ScrollLock", "Pause", "ContextMenu",
    "VolumeUp", "VolumeDown", "VolumeMute", "MediaPlayPause", "MediaNext", "MediaPrevious", "MediaStop",
    "BrowserBack", "BrowserForward",
];
/// Modifiers are `KEYS[..MODIFIERS]`.
pub const MODIFIERS: usize = 7;

pub type KeyId = u16;

pub fn key_id(name: &str) -> Option<KeyId> {
    KEYS.iter().position(|k| k.eq_ignore_ascii_case(name)).map(|i| i as KeyId)
}

/// Shortcuts (Ctrl, Alt or Win with a letter or digit) go out by character, so Ctrl+Z is undo on
/// any keyboard layout (on AZERTY, the Z position types W: Ctrl+W would close the tab). Every other
/// key goes out by position, so WASD stays under the fingers.
fn by_char(keys: &[KeyId]) -> bool {
    keys.iter().any(|k| matches!(KEYS[*k as usize], "Ctrl" | "Alt" | "Win" | "RCtrl" | "RAlt"))
}

fn is_char(k: KeyId) -> bool {
    let name = KEYS[k as usize];
    name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric()
}

/// Held keys, one bit per entry of [`KEYS`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeySet([u64; 4]);

impl KeySet {
    pub fn insert(&mut self, k: KeyId) {
        self.0[k as usize / 64] |= 1 << (k % 64);
    }
    pub fn contains(&self, k: KeyId) -> bool {
        self.0[k as usize / 64] & (1 << (k % 64)) != 0
    }
    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }
    /// Keys in [`KEYS`] order (modifiers first).
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = KeyId> + '_ {
        (0..KEYS.len() as KeyId).filter(|k| self.contains(*k))
    }
}

/// Mouse buttons, one bit each in [`Frame::mouse_buttons`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButton {
    pub const ALL: [MouseButton; 5] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle, MouseButton::Back, MouseButton::Forward];
    pub fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Inputs of the virtual Xbox controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadInput {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    /// Left stick click.
    Ls,
    Rs,
    Back,
    Start,
    Guide,
    Up,
    Down,
    Left,
    Right,
    /// The triggers: analog from a trigger, full from anything else.
    Lt,
    Rt,
}

/// XInput button bits (`XINPUT_GAMEPAD_*`), as ViGEm and the live view take them.
pub mod xbutton {
    pub const UP: u16 = 0x0001;
    pub const DOWN: u16 = 0x0002;
    pub const LEFT: u16 = 0x0004;
    pub const RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LS: u16 = 0x0040;
    pub const RS: u16 = 0x0080;
    pub const LB: u16 = 0x0100;
    pub const RB: u16 = 0x0200;
    pub const GUIDE: u16 = 0x0400;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

impl PadInput {
    fn bit(self) -> u16 {
        use xbutton::*;
        match self {
            PadInput::A => A,
            PadInput::B => B,
            PadInput::X => X,
            PadInput::Y => Y,
            PadInput::Lb => LB,
            PadInput::Rb => RB,
            PadInput::Ls => LS,
            PadInput::Rs => RS,
            PadInput::Back => BACK,
            PadInput::Start => START,
            PadInput::Guide => GUIDE,
            PadInput::Up => UP,
            PadInput::Down => DOWN,
            PadInput::Left => LEFT,
            PadInput::Right => RIGHT,
            PadInput::Lt | PadInput::Rt => 0,
        }
    }
}

/// What a control does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    #[default]
    None,
    /// A key or combination (`["Ctrl", "C"]`), held while the control is. `repeat`: after a moment,
    /// the last key repeats like a held keyboard key (injected keys don't auto-repeat).
    Key {
        keys: Vec<String>,
        #[serde(default)]
        repeat: bool,
    },
    Mouse { button: MouseButton },
    /// One wheel notch per press; `repeat` keeps scrolling while held.
    Scroll {
        dir: Dir,
        #[serde(default)]
        repeat: bool,
    },
    /// An input of the virtual Xbox controller.
    Pad { input: PadInput },
    /// Switch between the Desktop and Game modes.
    Switch,
    /// Open a program, file, folder or URL, once per press (`play_out.rs` hands it to the system).
    Run { target: String },
}

impl Action {
    pub fn key(keys: &[&str]) -> Action {
        Action::Key { keys: keys.iter().map(|k| k.to_string()).collect(), repeat: false }
    }
    pub fn key_repeat(keys: &[&str]) -> Action {
        Action::Key { keys: keys.iter().map(|k| k.to_string()).collect(), repeat: true }
    }
    pub fn pad(input: PadInput) -> Action {
        Action::Pad { input }
    }
    pub fn mouse(button: MouseButton) -> Action {
        Action::Mouse { button }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

/// Four actions, one per direction (a stick's WASD, a trackpad's D-pad).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Directions {
    pub up: Action,
    pub down: Action,
    pub left: Action,
    pub right: Action,
}

impl Directions {
    pub fn keys(up: &str, left: &str, down: &str, right: &str) -> Self {
        Directions { up: Action::key(&[up]), down: Action::key(&[down]), left: Action::key(&[left]), right: Action::key(&[right]) }
    }
    pub fn dpad() -> Self {
        Directions {
            up: Action::pad(PadInput::Up),
            down: Action::pad(PadInput::Down),
            left: Action::pad(PadInput::Left),
            right: Action::pad(PadInput::Right),
        }
    }
    fn get(&self, i: usize) -> &Action {
        [&self.up, &self.down, &self.left, &self.right][i]
    }
}

/// What a thumbstick does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StickMode {
    #[default]
    None,
    /// One of the virtual pad's sticks.
    Stick { target: Side, deadzone: f32 },
    /// An action per direction, pressed past `threshold` (0..1).
    Directions {
        #[serde(flatten)]
        dirs: Directions,
        threshold: f32,
    },
    /// Moves the pointer; `speed` px/s at full tilt.
    Mouse { speed: f32, deadzone: f32 },
    /// Scrolls; `speed` notches/s at full tilt.
    Scroll { speed: f32, deadzone: f32 },
}

/// What a trackpad does. Its click is a control of its own (`LPadClick`, `RPadClick`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PadMode {
    #[default]
    None,
    /// Like a laptop's touchpad; `speed` px across the whole pad. `trackball`: a flick keeps
    /// gliding after the thumb lifts.
    Mouse { speed: f32, trackball: bool },
    /// Swipe to scroll; `speed` notches across the whole pad.
    Scroll { speed: f32 },
    /// The thumb's position is a stick's, while touched.
    Stick { target: Side },
    /// Four zones (with diagonals); only while clicked if `click`.
    Directions {
        #[serde(flatten)]
        dirs: Directions,
        click: bool,
    },
    /// A menu used by feel: the ring is cut into one slice per item, the first centred at the top,
    /// clockwise, with a haptic tick on entering one. The item under the thumb plays when the pad
    /// is clicked (`click`), else when the thumb lifts; lifting in the centre picks nothing.
    Radial { items: Vec<Action>, click: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GyroOutput {
    #[default]
    None,
    /// Aim with the mouse: precise, for games that take mouse and controller together.
    Mouse,
    /// Aim with the virtual right stick: works in every game, less precise.
    Stick,
    /// Tilt left/right steers with the virtual left stick.
    Steer,
}

/// When the gyro is on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Activation {
    #[default]
    Always,
    /// While a thumb rests on the right stick (its touch sensor): lift it to reposition.
    RightStickTouch,
    RightPadTouch,
    /// While the left trigger is pulled, e.g. only when aiming down sights.
    LeftTrigger,
    RightTrigger,
    /// While a button is held (`buttons::NAMES`).
    Hold { button: String },
    /// Each press of a button turns it on or off.
    Toggle { button: String },
}

/// Which turn moves the pointer sideways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TurnAxis {
    /// Turning about the real vertical, however the controller is held.
    #[default]
    Auto,
    /// About the controller's own vertical (flat on a table: turning it).
    Yaw,
    /// Leaning it left/right like a steering wheel.
    Roll,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gyro {
    pub output: GyroOutput,
    pub activation: Activation,
    pub axis: TurnAxis,
    pub invert_y: bool,
    /// Mouse: pixels per degree turned.
    pub mouse_speed: f32,
    /// Stick: turn rate (°/s) that tilts the stick fully.
    pub stick_range: f32,
    /// Steer: lean (°) for full lock.
    pub steer_angle: f32,
}

impl Default for Gyro {
    fn default() -> Self {
        Gyro {
            output: GyroOutput::None,
            activation: Activation::Always,
            axis: TurnAxis::Auto,
            invert_y: false,
            mouse_speed: 12.0,
            stick_range: 180.0,
            steer_angle: 40.0,
        }
    }
}

/// Every control a layout binds, by name (`buttons::NAMES`, plus the trigger pulls `LT`/`RT`).
/// Mirrored in `CONTROLS` in `src/tabs/layout-editor.ts`.
pub const CONTROLS: &[&str] = &[
    "A", "B", "X", "Y", "Up", "Down", "Left", "Right", "L", "R", "LT", "RT", "LTrigClick", "RTrigClick",
    "L3", "R3", "L4", "L5", "R4", "R5", "View", "Menu", "QAM", "Steam", "LPadClick", "RPadClick",
];
/// Bits of [`Frame::lit`] after the controls: the analog parts in use.
pub const LIT_LEFT_STICK: u32 = CONTROLS.len() as u32;
pub const LIT_RIGHT_STICK: u32 = LIT_LEFT_STICK + 1;
pub const LIT_LEFT_PAD: u32 = LIT_LEFT_STICK + 2;
pub const LIT_RIGHT_PAD: u32 = LIT_LEFT_STICK + 3;
pub const LIT_GYRO: u32 = LIT_LEFT_STICK + 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub name: String,
    /// Control name ([`CONTROLS`]) to action; missing ones do nothing.
    pub buttons: BTreeMap<String, Action>,
    pub left_stick: StickMode,
    pub right_stick: StickMode,
    pub left_pad: PadMode,
    pub right_pad: PadMode,
    pub gyro: Gyro,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            name: "Empty".into(),
            buttons: BTreeMap::new(),
            left_stick: StickMode::None,
            right_stick: StickMode::None,
            left_pad: PadMode::None,
            right_pad: PadMode::None,
            gyro: Gyro::default(),
        }
    }
}

/// The virtual Xbox controller's state (XInput ranges).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PadState {
    pub buttons: u16,
    pub lt: u8,
    pub rt: u8,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

/// What a layout makes of one report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub pad: PadState,
    /// Keys to hold.
    pub keys: KeySet,
    /// Mouse buttons to hold ([`MouseButton::bit`]).
    pub mouse_buttons: u8,
    /// Pointer movement for this report, in pixels (fractions carry over in [`Injector`]).
    pub mouse: [f32; 2],
    /// Wheel movement for this report, in notches: x right, y up.
    pub scroll: [f32; 2],
    /// Keys pressed again while held (repeat).
    pub taps: Vec<KeyId>,
    /// Keys of shortcuts, to send by character rather than by position (see [`by_char`]).
    pub by_char: KeySet,
    /// A Switch action was pressed.
    pub switch: bool,
    /// A Switch action's control is held down.
    pub switch_held: bool,
    /// Run actions pressed.
    pub runs: Vec<String>,
    /// Bit per trackpad (0 left, 1 right) whose radial menu moved to another item: tick it.
    pub ticks: u8,
    /// Each trackpad's radial item under the thumb or playing, for the tabs to light up.
    pub radial: [Option<u8>; 2],
    pub gyro_active: bool,
    /// Bit per control in use ([`CONTROLS`] index, then the `LIT_*` parts), for the tabs to light up.
    pub lit: u64,
}

// ---------- Compiled form ----------

#[derive(Debug, Clone, PartialEq)]
enum Act {
    None,
    Keys { keys: Vec<KeyId>, repeat: bool, by_char: bool },
    Mouse(u8),
    Scroll(Dir, bool),
    Pad(PadInput),
    Switch,
    Run(String),
}

impl Act {
    fn new(a: &Action) -> Act {
        match a {
            Action::None => Act::None,
            Action::Key { keys, repeat } => {
                let mut ids: Vec<KeyId> = keys.iter().filter_map(|k| key_id(k)).collect();
                if ids.is_empty() {
                    return Act::None;
                }
                // Modifiers first, else as written; the last key is the one that repeats.
                ids.sort_by_key(|k| (*k as usize) >= MODIFIERS);
                Act::Keys { by_char: by_char(&ids), keys: ids, repeat: *repeat }
            }
            Action::Mouse { button } => Act::Mouse(button.bit()),
            Action::Scroll { dir, repeat } => Act::Scroll(*dir, *repeat),
            Action::Pad { input } => Act::Pad(*input),
            Action::Switch => Act::Switch,
            Action::Run { target } if target.trim().is_empty() => Act::None,
            Action::Run { target } => Act::Run(target.trim().to_string()),
        }
    }
}

/// Where a binding is in its press, for edges and repeats.
#[derive(Debug, Clone, Copy, Default)]
struct Press {
    down: bool,
    since: Duration,
    last_repeat: Duration,
}

/// Held keys repeat after this, then this often (like a keyboard's typematic rate).
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(50);
const SCROLL_REPEAT_EVERY: Duration = Duration::from_millis(90);
/// A trigger counts as pressed past this pull (and released below 70% of it).
const TRIGGER_PRESS: f32 = 0.3;
/// Pad zones start this far from the centre (fraction of the radius).
const PAD_ZONE: f32 = 0.35;
/// A radial item picked by lifting the thumb is pressed this long.
const RADIAL_TAP: Duration = Duration::from_millis(40);
/// The thumb has to go this far (radians) past a radial slice's edge to change item, so resting on
/// the edge doesn't tick back and forth.
const RADIAL_HYSTERESIS: f32 = 0.08;
const PAD_RANGE: f32 = 65536.0;

impl Press {
    /// Update with `pressed`; true on the press itself.
    fn step(&mut self, pressed: bool, now: Duration) -> bool {
        let rising = pressed && !self.down;
        if rising {
            self.since = now;
            self.last_repeat = now;
        }
        self.down = pressed;
        rising
    }
    /// Due for a repeat at `now`.
    fn repeat(&mut self, now: Duration, every: Duration) -> bool {
        if self.down && now.saturating_sub(self.since) >= REPEAT_DELAY && now.saturating_sub(self.last_repeat) >= every {
            self.last_repeat = now;
            return true;
        }
        false
    }
}

/// Apply an action that is `pressed` (and for pad triggers, how far: `analog` 0..1).
fn apply(a: &Act, pressed: bool, analog: f32, p: &mut Press, now: Duration, f: &mut Frame) {
    let rising = p.step(pressed, now);
    match a {
        Act::None => {}
        Act::Keys { keys, repeat, by_char } => {
            if pressed {
                keys.iter().for_each(|k| f.keys.insert(*k));
                if *by_char {
                    keys.iter().filter(|k| is_char(**k)).for_each(|k| f.by_char.insert(*k));
                }
                if *repeat && p.repeat(now, REPEAT_EVERY) {
                    f.taps.push(*keys.last().unwrap());
                }
            }
        }
        Act::Mouse(bit) => {
            if pressed {
                f.mouse_buttons |= bit;
            }
        }
        Act::Scroll(dir, repeat) => {
            if rising || (*repeat && p.repeat(now, SCROLL_REPEAT_EVERY)) {
                match dir {
                    Dir::Up => f.scroll[1] += 1.0,
                    Dir::Down => f.scroll[1] -= 1.0,
                    Dir::Left => f.scroll[0] -= 1.0,
                    Dir::Right => f.scroll[0] += 1.0,
                }
            }
        }
        Act::Pad(input) => {
            let v = (analog.clamp(0.0, 1.0) * 255.0).round() as u8;
            match input {
                PadInput::Lt => f.pad.lt = f.pad.lt.max(if pressed { v } else { 0 }),
                PadInput::Rt => f.pad.rt = f.pad.rt.max(if pressed { v } else { 0 }),
                _ if pressed => f.pad.buttons |= input.bit(),
                _ => {}
            }
        }
        Act::Switch => {
            f.switch |= rising;
            f.switch_held |= pressed;
        }
        Act::Run(target) => {
            if rising {
                f.runs.push(target.clone());
            }
        }
    }
}

fn add_stick(f: &mut Frame, target: Side, x: f32, y: f32) {
    let to = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let (sx, sy) = match target {
        Side::Left => (&mut f.pad.lx, &mut f.pad.ly),
        Side::Right => (&mut f.pad.rx, &mut f.pad.ry),
    };
    *sx = to(*sx as f32 / 32767.0 + x);
    *sy = to(*sy as f32 / 32767.0 + y);
}

/// Radial deadzone, rescaled so the edge of the deadzone is 0. Returns (x, y, magnitude).
fn deadzone(x: f32, y: f32, dz: f32) -> (f32, f32, f32) {
    let mag = (x * x + y * y).sqrt();
    let dz = dz.clamp(0.0, 0.9);
    if mag <= dz || mag < 1e-6 {
        return (0.0, 0.0, 0.0);
    }
    let m = ((mag - dz) / (1.0 - dz)).min(1.0);
    (x / mag * m, y / mag * m, m)
}

/// Stillness-based gyro calibration: while the controller rests (gyro steady, accelerometer
/// unchanged) for a moment, what the gyro reads is its drift, and is taken off from then on.
#[derive(Debug, Clone, Default)]
struct Calibration {
    bias: [f32; 3],
    mean: Option<[f32; 3]>,
    accel: Option<[f32; 3]>,
    still: Duration,
}

const STILL_DPS: f32 = 2.5;
const STILL_G: f32 = 0.02;
const STILL_FOR: Duration = Duration::from_millis(750);
/// Drift bigger than this is not drift: something turns slowly on purpose.
const MAX_BIAS_DPS: f32 = 8.0;

impl Calibration {
    /// Raw gyro in °/s with the drift taken off.
    fn step(&mut self, gyro: [f32; 3], accel: [f32; 3], dt: Duration) -> [f32; 3] {
        let mean = self.mean.get_or_insert(gyro);
        for i in 0..3 {
            mean[i] += (gyro[i] - mean[i]) * 0.05;
        }
        let steady = (0..3).all(|i| (gyro[i] - mean[i]).abs() < STILL_DPS);
        let unmoved = self.accel.is_some_and(|a| (0..3).all(|i| (accel[i] - a[i]).abs() < STILL_G));
        self.accel = Some(accel);
        if steady && unmoved {
            self.still += dt;
            if self.still >= STILL_FOR && mean.iter().all(|m| m.abs() < MAX_BIAS_DPS) {
                self.bias = *mean;
            }
        } else {
            self.still = Duration::ZERO;
        }
        [gyro[0] - self.bias[0], gyro[1] - self.bias[1], gyro[2] - self.bias[2]]
    }
}

struct Compiled {
    buttons: Vec<(Act, Option<u32>)>,
    left_stick: [Act; 4],
    right_stick: [Act; 4],
    /// A trackpad's directions (up, down, left, right) or radial items.
    left_pad: Vec<Act>,
    right_pad: Vec<Act>,
    gyro_button: Option<u32>,
}

fn dirs_compiled(d: &Directions) -> [Act; 4] {
    [Act::new(&d.up), Act::new(&d.down), Act::new(&d.left), Act::new(&d.right)]
}

fn button_mask(name: &str) -> Option<u32> {
    crate::shaping::button_mask(name)
}

impl Compiled {
    fn new(l: &Layout) -> Self {
        let stick = |m: &StickMode| match m {
            StickMode::Directions { dirs, .. } => dirs_compiled(dirs),
            _ => Default::default(),
        };
        let pad = |m: &PadMode| match m {
            PadMode::Directions { dirs, .. } => dirs_compiled(dirs).into(),
            PadMode::Radial { items, .. } => items.iter().map(Act::new).collect(),
            _ => Vec::new(),
        };
        Compiled {
            buttons: CONTROLS
                .iter()
                .map(|c| (l.buttons.get(*c).map_or(Act::None, Act::new), button_mask(c)))
                .collect(),
            left_stick: stick(&l.left_stick),
            right_stick: stick(&l.right_stick),
            left_pad: pad(&l.left_pad),
            right_pad: pad(&l.right_pad),
            gyro_button: match &l.gyro.activation {
                Activation::Hold { button } | Activation::Toggle { button } => button_mask(button),
                _ => None,
            },
        }
    }
}

impl Default for Act {
    fn default() -> Self {
        Act::None
    }
}

/// Where a radial menu is: the item under the thumb, the one held by a click, a lift's tap.
#[derive(Debug, Clone, Copy, Default)]
struct Radial {
    selected: Option<usize>,
    held: Option<usize>,
    tap: Option<(usize, Duration)>,
    clicked: bool,
}

/// The slice of `n` under the thumb at (x, y) (y up), the first centred at the top, clockwise.
/// `current` stays until the thumb is clearly past its edge.
fn radial_slice(x: f32, y: f32, n: usize, current: Option<usize>) -> usize {
    use std::f32::consts::TAU;
    let width = TAU / n as f32;
    let angle = x.atan2(y).rem_euclid(TAU);
    let slice = ((angle / width).round() as usize) % n;
    match current {
        Some(c) if c < n && c != slice => {
            let off = (angle - c as f32 * width + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0;
            if off.abs() > width / 2.0 + RADIAL_HYSTERESIS { slice } else { c }
        }
        _ => slice,
    }
}

/// A trackpad's motion between reports, and a trackball's glide.
#[derive(Debug, Clone, Copy, Default)]
struct PadTrack {
    last: Option<[f32; 2]>,
    /// Pointer velocity in px/s, kept for the trackball.
    velocity: [f32; 2],
}

/// Turns state reports into frames for one layout.
pub struct Engine {
    layout: Layout,
    c: Compiled,
    buttons: Vec<Press>,
    sticks: [[Press; 4]; 2],
    pads: [Vec<Press>; 2],
    radials: [Radial; 2],
    tracks: [PadTrack; 2],
    calibration: Calibration,
    gyro_toggled: bool,
    gyro_button_down: bool,
    triggers_down: [bool; 2],
    last: Option<Duration>,
    frame: Frame,
}

/// A trackball's glide slows by this factor per second.
const TRACKBALL_FRICTION: f32 = 4.0;

impl Engine {
    pub fn new(layout: Layout) -> Self {
        let c = Compiled::new(&layout);
        Engine {
            pads: [vec![Press::default(); c.left_pad.len()], vec![Press::default(); c.right_pad.len()]],
            c,
            layout,
            buttons: vec![Press::default(); CONTROLS.len()],
            sticks: Default::default(),
            radials: Default::default(),
            tracks: Default::default(),
            calibration: Calibration::default(),
            gyro_toggled: false,
            gyro_button_down: false,
            triggers_down: [false; 2],
            last: None,
            frame: Frame::default(),
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Use another layout. Presses in progress are forgotten (the caller releases what was held);
    /// the gyro calibration is kept.
    pub fn set_layout(&mut self, layout: Layout) {
        let calibration = std::mem::take(&mut self.calibration);
        *self = Engine::new(layout);
        self.calibration = calibration;
    }

    /// Forget presses in progress and pointer glides, e.g. after letting go of everything.
    pub fn reset(&mut self) {
        let layout = std::mem::take(&mut self.layout);
        self.set_layout(layout);
    }

    /// The last frame.
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// One state report at `now` (any steady clock).
    pub fn process(&mut self, s: &ControllerState, now: Duration) -> &Frame {
        let dt = self.last.map_or(Duration::from_micros(3800), |l| now.saturating_sub(l)).min(Duration::from_millis(50));
        self.last = Some(now);
        let secs = dt.as_secs_f32();
        let mut f = Frame::default();
        let triggers = [axis(s.trigger_left).max(0.0), axis(s.trigger_right).max(0.0)];
        for (i, t) in triggers.iter().enumerate() {
            // Hysteresis, so a trigger held at the threshold doesn't chatter.
            self.triggers_down[i] = *t > if self.triggers_down[i] { TRIGGER_PRESS * 0.7 } else { TRIGGER_PRESS };
        }

        // Buttons and trigger pulls.
        for (i, (act, mask)) in self.c.buttons.iter().enumerate() {
            let (pressed, analog) = match (CONTROLS[i], mask) {
                ("LT", _) => (triggers[0] > 0.02, triggers[0]),
                ("RT", _) => (triggers[1] > 0.02, triggers[1]),
                (_, Some(m)) => (s.pressed(*m), 1.0),
                _ => (false, 0.0),
            };
            // A trigger is "pressed" from a light touch for the pad's analog trigger, past the
            // threshold for everything else.
            let pressed = match (CONTROLS[i], act) {
                ("LT", Act::Pad(PadInput::Lt | PadInput::Rt)) | ("RT", Act::Pad(PadInput::Lt | PadInput::Rt)) => pressed,
                ("LT", _) => self.triggers_down[0],
                ("RT", _) => self.triggers_down[1],
                _ => pressed,
            };
            apply(act, pressed, analog, &mut self.buttons[i], now, &mut f);
            if pressed && *act != Act::None {
                f.lit |= 1 << i;
            }
        }

        // Sticks.
        for (side, mode) in [(0, &self.layout.left_stick), (1, &self.layout.right_stick)] {
            let [x, y] = if side == 0 { s.left_stick } else { s.right_stick }.map(axis);
            let acts = if side == 0 { &self.c.left_stick } else { &self.c.right_stick };
            let used = match mode {
                StickMode::None => false,
                StickMode::Stick { target, deadzone: dz } => {
                    let (x, y, m) = deadzone(x, y, *dz);
                    add_stick(&mut f, *target, x, y);
                    m > 0.0
                }
                StickMode::Directions { threshold, .. } => {
                    let th = threshold.clamp(0.1, 0.95);
                    let mut any = false;
                    for (i, v) in [y, -y, -x, x].into_iter().enumerate() {
                        let p = &mut self.sticks[side][i];
                        let pressed = v > if p.down { th * 0.7 } else { th };
                        apply(&acts[i], pressed, 1.0, p, now, &mut f);
                        any |= pressed;
                    }
                    any
                }
                StickMode::Mouse { speed, deadzone: dz } => {
                    let (x, y, m) = deadzone(x, y, *dz);
                    // Squared: fine control near the centre, full speed at the edge.
                    let k = m * speed * secs;
                    f.mouse[0] += x * k;
                    f.mouse[1] -= y * k;
                    m > 0.0
                }
                StickMode::Scroll { speed, deadzone: dz } => {
                    let (x, y, m) = deadzone(x, y, *dz);
                    let k = m * speed * secs;
                    f.scroll[0] += x * k;
                    f.scroll[1] += y * k;
                    m > 0.0
                }
            };
            if used {
                f.lit |= 1 << (LIT_LEFT_STICK + side as u32);
            }
        }

        // Trackpads.
        for side in 0..2 {
            let pad: &Trackpad = if side == 0 { &s.left_pad } else { &s.right_pad };
            let mode = if side == 0 { &self.layout.left_pad } else { &self.layout.right_pad };
            let acts = if side == 0 { &self.c.left_pad } else { &self.c.right_pad };
            let track = &mut self.tracks[side];
            let pos = [pad.x as f32 / PAD_RANGE, pad.y as f32 / PAD_RANGE];
            let delta = match (pad.touched, track.last) {
                (true, Some(l)) => Some([pos[0] - l[0], pos[1] - l[1]]),
                _ => None,
            };
            track.last = pad.touched.then_some(pos);
            match mode {
                PadMode::None => {}
                PadMode::Mouse { speed, trackball } => {
                    if let Some(d) = delta {
                        let m = [d[0] * speed, -d[1] * speed];
                        f.mouse[0] += m[0];
                        f.mouse[1] += m[1];
                        if secs > 0.0 {
                            // Smoothed, so the last tiny wobble before lifting doesn't decide the glide.
                            for i in 0..2 {
                                track.velocity[i] += (m[i] / secs - track.velocity[i]) * 0.3;
                            }
                        }
                    } else if pad.touched {
                        track.velocity = [0.0; 2];
                    } else if *trackball {
                        let decay = (-TRACKBALL_FRICTION * secs).exp();
                        track.velocity = track.velocity.map(|v| v * decay);
                        if track.velocity[0].hypot(track.velocity[1]) < 30.0 {
                            track.velocity = [0.0; 2];
                        }
                        f.mouse[0] += track.velocity[0] * secs;
                        f.mouse[1] += track.velocity[1] * secs;
                    }
                }
                PadMode::Scroll { speed } => {
                    if let Some(d) = delta {
                        f.scroll[0] += d[0] * speed;
                        f.scroll[1] += d[1] * speed;
                    }
                }
                PadMode::Stick { target } => {
                    if pad.touched {
                        let (x, y, _) = deadzone(pos[0] * 2.0, pos[1] * 2.0, 0.1);
                        add_stick(&mut f, *target, x, y);
                    }
                }
                PadMode::Directions { click, .. } => {
                    let on = pad.touched && (!click || pad.clicked);
                    let [x, y] = [pos[0] * 2.0, pos[1] * 2.0];
                    for (i, v) in [y, -y, -x, x].into_iter().enumerate() {
                        apply(&acts[i], on && v > PAD_ZONE, 1.0, &mut self.pads[side][i], now, &mut f);
                    }
                }
                PadMode::Radial { click, .. } => {
                    let r = &mut self.radials[side];
                    let [x, y] = [pos[0] * 2.0, pos[1] * 2.0];
                    let n = acts.len();
                    let was = r.selected;
                    r.selected = if n > 0 && pad.touched && x.hypot(y) > PAD_ZONE { Some(radial_slice(x, y, n, r.selected)) } else { None };
                    if r.selected.is_some() && r.selected != was {
                        f.ticks |= 1 << side;
                    }
                    if *click {
                        // The item under the thumb when the click went down holds until it's let go.
                        if pad.clicked && !r.clicked {
                            r.held = r.selected;
                        } else if !pad.clicked {
                            r.held = None;
                        }
                    } else if let (false, Some(i)) = (pad.touched, was) {
                        r.tap = Some((i, now + RADIAL_TAP));
                    }
                    r.clicked = pad.clicked;
                    let tap = r.tap.filter(|(_, until)| now < *until).map(|(i, _)| i);
                    r.tap = r.tap.filter(|(_, until)| now < *until);
                    for (i, act) in acts.iter().enumerate() {
                        apply(act, r.held == Some(i) || tap == Some(i), 1.0, &mut self.pads[side][i], now, &mut f);
                    }
                    f.radial[side] = r.held.or(tap).or(r.selected).map(|i| i as u8);
                }
            }
            if pad.touched && *mode != PadMode::None {
                f.lit |= 1 << (LIT_LEFT_PAD + side as u32);
            }
        }

        // Gyro: always calibrating, so it's ready when turned on.
        let raw = s.imu.gyro.map(|v| v as f32 / 32767.0 * GYRO_FULL_SCALE_DPS);
        let accel = s.imu.accel.map(|v| v as f32 / 16384.0);
        let g = self.calibration.step(raw, accel, dt);
        let gyro = &self.layout.gyro;
        if gyro.output != GyroOutput::None {
            let button = self.c.gyro_button.is_some_and(|m| s.pressed(m));
            let active = match &gyro.activation {
                Activation::Always => true,
                Activation::RightStickTouch => s.pressed(buttons::RIGHT_STICK_TOUCH),
                Activation::RightPadTouch => s.right_pad.touched,
                Activation::LeftTrigger => self.triggers_down[0],
                Activation::RightTrigger => self.triggers_down[1],
                Activation::Hold { .. } => button,
                Activation::Toggle { .. } => {
                    if button && !self.gyro_button_down {
                        self.gyro_toggled = !self.gyro_toggled;
                    }
                    self.gyro_toggled
                }
            };
            self.gyro_button_down = button;
            f.gyro_active = active;
            if active {
                f.lit |= 1 << LIT_GYRO;
                // °/s: positive turn is to the right, positive pitch is aiming up.
                let turn = match gyro.axis {
                    TurnAxis::Auto => {
                        let up = up_vector(s);
                        -(g[0] * up[0] + g[1] * up[1] + g[2] * up[2])
                    }
                    TurnAxis::Yaw => -g[2],
                    TurnAxis::Roll => g[1],
                };
                let pitch = if gyro.invert_y { -g[0] } else { g[0] };
                // Below ~2°/s, scale down: hands shake, aim shouldn't.
                let rate = turn.hypot(pitch);
                let soften = (rate / 2.0).min(1.0);
                let (turn, pitch) = (turn * soften, pitch * soften);
                match gyro.output {
                    GyroOutput::None => {}
                    GyroOutput::Mouse => {
                        f.mouse[0] += turn * secs * gyro.mouse_speed;
                        f.mouse[1] -= pitch * secs * gyro.mouse_speed;
                    }
                    GyroOutput::Stick => {
                        let range = gyro.stick_range.max(10.0);
                        let (x, y) = (turn / range, pitch / range);
                        let m = x.hypot(y);
                        // Past the game's own stick deadzone right away, so slow turns still move.
                        const ANTI_DEADZONE: f32 = 0.15;
                        if m > 0.002 {
                            let out = (ANTI_DEADZONE + (1.0 - ANTI_DEADZONE) * m.min(1.0)) / m;
                            add_stick(&mut f, Side::Right, x * out, y * out);
                        }
                    }
                    GyroOutput::Steer => {
                        let lean = tilt(s)[1] * 90.0;
                        let lean = if lean.abs() < 2.0 { 0.0 } else { lean - 2.0f32.copysign(lean) };
                        add_stick(&mut f, Side::Left, lean / gyro.steer_angle.max(5.0), 0.0);
                    }
                }
            }
        }
        self.frame = f;
        &self.frame
    }
}

// ---------- From frames to events ----------

/// One input event for the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// `by_char`: send the character the key name stands for, not the key at its position.
    Key { key: KeyId, down: bool, by_char: bool },
    Button { button: MouseButton, down: bool },
    /// Relative pointer movement in pixels.
    Move { dx: i32, dy: i32 },
    /// Wheel movement in 1/120 notch units: `v` up, `h` right.
    Scroll { v: i32, h: i32 },
}

/// Wheel units per notch (Windows' WHEEL_DELTA; Linux' hi-res wheel uses the same).
pub const WHEEL_DELTA: f32 = 120.0;
/// Smooth scrolling goes out in steps of a third of a notch: apps that ignore anything below a
/// notch still get whole notches added up, and the rest scroll smoothly.
const WHEEL_STEP: i32 = 40;

/// Turns frames into press/release events, keeping what is held and the fractions of pixels and
/// notches not sent yet.
#[derive(Debug, Default)]
pub struct Injector {
    keys: KeySet,
    /// Held keys that went down by character: they go up the same way.
    by_char: KeySet,
    buttons: u8,
    mouse_rest: [f32; 2],
    scroll_rest: [f32; 2],
    events: Vec<Event>,
}

impl Injector {
    pub fn held_anything(&self) -> bool {
        !self.keys.is_empty() || self.buttons != 0
    }

    /// The events that take the system from what was applied last to `f`.
    pub fn apply(&mut self, f: &Frame) -> &[Event] {
        self.events.clear();
        // Releases first (non-modifiers before modifiers), then presses (modifiers first).
        let old = self.keys;
        let old_by_char = self.by_char;
        for k in old.iter().rev().filter(|k| !f.keys.contains(*k)) {
            self.events.push(Event::Key { key: k, down: false, by_char: old_by_char.contains(k) });
        }
        // A key keeps the way it went down until it's let go.
        self.by_char = KeySet::default();
        for k in f.keys.iter() {
            if if old.contains(k) { old_by_char.contains(k) } else { f.by_char.contains(k) } {
                self.by_char.insert(k);
            }
        }
        for b in MouseButton::ALL {
            if self.buttons & b.bit() != 0 && f.mouse_buttons & b.bit() == 0 {
                self.events.push(Event::Button { button: b, down: false });
            }
        }
        for k in f.keys.iter().filter(|k| !old.contains(*k)) {
            self.events.push(Event::Key { key: k, down: true, by_char: self.by_char.contains(k) });
        }
        for k in &f.taps {
            if f.keys.contains(*k) && old.contains(*k) {
                self.events.push(Event::Key { key: *k, down: true, by_char: self.by_char.contains(*k) });
            }
        }
        for b in MouseButton::ALL {
            if self.buttons & b.bit() == 0 && f.mouse_buttons & b.bit() != 0 {
                self.events.push(Event::Button { button: b, down: true });
            }
        }
        self.keys = f.keys;
        self.buttons = f.mouse_buttons;

        let take = |rest: &mut f32, add: f32, step: i32| -> i32 {
            *rest += add;
            let n = (*rest / step as f32).trunc() as i32 * step;
            *rest -= n as f32;
            n
        };
        let dx = take(&mut self.mouse_rest[0], f.mouse[0], 1);
        let dy = take(&mut self.mouse_rest[1], f.mouse[1], 1);
        if dx != 0 || dy != 0 {
            self.events.push(Event::Move { dx, dy });
        }
        let v = take(&mut self.scroll_rest[1], f.scroll[1] * WHEEL_DELTA, WHEEL_STEP);
        let h = take(&mut self.scroll_rest[0], f.scroll[0] * WHEEL_DELTA, WHEEL_STEP);
        if v != 0 || h != 0 {
            self.events.push(Event::Scroll { v, h });
        }
        &self.events
    }

    /// Let go of everything held.
    pub fn release(&mut self) -> &[Event] {
        self.mouse_rest = [0.0; 2];
        self.scroll_rest = [0.0; 2];
        self.apply(&Frame::default())
    }
}

// ---------- Built-in layouts ----------

fn buttons(list: &[(&str, Action)]) -> BTreeMap<String, Action> {
    list.iter().map(|(c, a)| (c.to_string(), a.clone())).collect()
}

/// Layouts for the Keyboard & mouse tab. Mirrored by name in the tab's descriptions
/// (`LAYOUT_NOTES` in `src/tabs/keymouse.ts`).
pub fn builtin_keymouse() -> Vec<Layout> {
    use Action as A;
    use MouseButton::*;
    let desktop = Layout {
        name: "Desktop".into(),
        buttons: buttons(&[
            ("RT", A::mouse(Left)),
            ("LT", A::mouse(Right)),
            ("RPadClick", A::mouse(Left)),
            ("LPadClick", A::mouse(Middle)),
            ("A", A::key(&["Enter"])),
            ("B", A::key(&["Escape"])),
            ("X", A::key_repeat(&["Backspace"])),
            ("Y", A::key(&["Space"])),
            ("Up", A::key_repeat(&["Up"])),
            ("Down", A::key_repeat(&["Down"])),
            ("Left", A::key_repeat(&["Left"])),
            ("Right", A::key_repeat(&["Right"])),
            ("L", A::key(&["Ctrl", "Shift", "Tab"])),
            ("R", A::key(&["Ctrl", "Tab"])),
            ("L3", A::mouse(Back)),
            ("R3", A::mouse(Forward)),
            ("L4", A::key(&["Ctrl", "Z"])),
            ("R4", A::key(&["Ctrl", "Y"])),
            ("L5", A::key(&["Ctrl", "C"])),
            ("R5", A::key(&["Ctrl", "V"])),
            ("View", A::key(&["Win", "Tab"])),
            ("Menu", A::key(&["Win"])),
            ("QAM", A::key(&["MediaPlayPause"])),
            ("Steam", A::Switch),
        ]),
        left_stick: StickMode::Scroll { speed: 18.0, deadzone: 0.15 },
        right_stick: StickMode::Mouse { speed: 1400.0, deadzone: 0.12 },
        left_pad: PadMode::Scroll { speed: 12.0 },
        right_pad: PadMode::Mouse { speed: 1100.0, trackball: false },
        gyro: Gyro::default(),
    };
    let presentation = Layout {
        name: "Presentation".into(),
        buttons: buttons(&[
            ("A", A::key(&["Right"])),
            ("B", A::key(&["Left"])),
            ("R", A::key(&["Right"])),
            ("L", A::key(&["Left"])),
            ("Right", A::key(&["Right"])),
            ("Left", A::key(&["Left"])),
            ("Up", A::key(&["Up"])),
            ("Down", A::key(&["Down"])),
            ("Y", A::key(&["B"])),
            ("X", A::key(&["W"])),
            ("Menu", A::key(&["F5"])),
            ("View", A::key(&["Escape"])),
            ("LT", A::mouse(Left)),
            ("RPadClick", A::mouse(Left)),
            ("QAM", A::key(&["Ctrl", "L"])),
            ("Steam", A::Switch),
        ]),
        right_pad: PadMode::Mouse { speed: 1100.0, trackball: false },
        // A laser pointer: aim while the right trigger is held.
        gyro: Gyro { output: GyroOutput::Mouse, activation: Activation::RightTrigger, mouse_speed: 25.0, ..Gyro::default() },
        ..Layout::default()
    };
    let shooter = Layout {
        name: "Shooter (WASD + mouse)".into(),
        buttons: buttons(&[
            ("RT", A::mouse(Left)),
            ("LT", A::mouse(Right)),
            ("A", A::key(&["Space"])),
            ("B", A::key(&["C"])),
            ("X", A::key(&["R"])),
            ("Y", A::key(&["E"])),
            ("L", A::key(&["Q"])),
            ("R", A::key(&["G"])),
            ("L3", A::key(&["Shift"])),
            ("R3", A::key(&["V"])),
            ("Up", A::key(&["1"])),
            ("Right", A::key(&["2"])),
            ("Down", A::key(&["3"])),
            ("Left", A::key(&["4"])),
            ("L4", A::key(&["F"])),
            ("R4", A::key(&["Ctrl"])),
            ("L5", A::key(&["M"])),
            ("R5", A::Scroll { dir: Dir::Down, repeat: false }),
            ("View", A::key(&["Tab"])),
            ("Menu", A::key(&["Escape"])),
            ("RPadClick", A::mouse(Left)),
            ("Steam", A::Switch),
        ]),
        left_stick: StickMode::Directions { dirs: Directions::keys("W", "A", "S", "D"), threshold: 0.4 },
        right_stick: StickMode::Mouse { speed: 1800.0, deadzone: 0.1 },
        right_pad: PadMode::Mouse { speed: 1400.0, trackball: false },
        ..Layout::default()
    };
    let shooter_gyro = Layout {
        name: "Shooter + gyro aim".into(),
        gyro: Gyro { output: GyroOutput::Mouse, activation: Activation::RightStickTouch, ..Gyro::default() },
        ..shooter.clone()
    };
    let strategy = Layout {
        name: "Strategy / point & click".into(),
        buttons: buttons(&[
            ("RT", A::mouse(Left)),
            ("LT", A::mouse(Right)),
            ("RPadClick", A::mouse(Left)),
            ("LPadClick", A::mouse(Middle)),
            ("A", A::key(&["Space"])),
            ("B", A::key(&["Escape"])),
            ("X", A::key(&["Delete"])),
            ("Y", A::key(&["Enter"])),
            ("L", A::key(&["Ctrl"])),
            ("R", A::key(&["Shift"])),
            ("Up", A::key(&["1"])),
            ("Right", A::key(&["2"])),
            ("Down", A::key(&["3"])),
            ("Left", A::key(&["4"])),
            ("L4", A::key(&["Ctrl", "Z"])),
            ("R4", A::key(&["H"])),
            ("L5", A::key(&["F5"])),
            ("R5", A::key(&["F9"])),
            ("View", A::key(&["Tab"])),
            ("Menu", A::key(&["F10"])),
            ("Steam", A::Switch),
        ]),
        left_stick: StickMode::Directions { dirs: Directions::keys("Up", "Left", "Down", "Right"), threshold: 0.4 },
        right_stick: StickMode::Scroll { speed: 10.0, deadzone: 0.2 },
        left_pad: PadMode::Scroll { speed: 12.0 },
        right_pad: PadMode::Mouse { speed: 1200.0, trackball: false },
        ..Layout::default()
    };
    vec![desktop, presentation, shooter, shooter_gyro, strategy]
}

/// Layouts for the Gamepad tab. The Steam Controller's own buttons go to the Xbox ones by position.
pub fn builtin_gamepad() -> Vec<Layout> {
    use Action as A;
    use PadInput::*;
    let standard = Layout {
        name: "Standard".into(),
        buttons: buttons(&[
            ("A", A::pad(A)),
            ("B", A::pad(B)),
            ("X", A::pad(X)),
            ("Y", A::pad(Y)),
            ("Up", A::pad(Up)),
            ("Down", A::pad(Down)),
            ("Left", A::pad(Left)),
            ("Right", A::pad(Right)),
            ("L", A::pad(Lb)),
            ("R", A::pad(Rb)),
            ("LT", A::pad(Lt)),
            ("RT", A::pad(Rt)),
            ("L3", A::pad(Ls)),
            ("R3", A::pad(Rs)),
            ("View", A::pad(Back)),
            ("Menu", A::pad(Start)),
            ("QAM", A::pad(Guide)),
            ("Steam", A::Switch),
        ]),
        left_stick: StickMode::Stick { target: Side::Left, deadzone: 0.08 },
        right_stick: StickMode::Stick { target: Side::Right, deadzone: 0.08 },
        ..Layout::default()
    };
    let with = |name: &str, f: &dyn Fn(&mut Layout)| {
        let mut l = Layout { name: name.into(), ..standard.clone() };
        f(&mut l);
        l
    };
    vec![
        standard.clone(),
        // Back grips repeat the face buttons, so thumbs never leave the sticks.
        with("Back grips as face buttons", &|l| {
            l.buttons.insert("R4".into(), A::pad(A));
            l.buttons.insert("R5".into(), A::pad(B));
            l.buttons.insert("L4".into(), A::pad(X));
            l.buttons.insert("L5".into(), A::pad(Y));
        }),
        with("Gyro aim (stick)", &|l| {
            l.gyro = Gyro { output: GyroOutput::Stick, activation: Activation::RightStickTouch, ..Gyro::default() };
        }),
        with("Gyro aim (mouse)", &|l| {
            l.gyro = Gyro { output: GyroOutput::Mouse, activation: Activation::RightStickTouch, ..Gyro::default() };
            l.right_pad = PadMode::Mouse { speed: 1200.0, trackball: false };
        }),
        with("Racing (tilt to steer)", &|l| {
            l.gyro = Gyro { output: GyroOutput::Steer, activation: Activation::Always, ..Gyro::default() };
            l.buttons.insert("L4".into(), A::pad(Lb));
            l.buttons.insert("R4".into(), A::pad(Rb));
        }),
        with("Trackpads as D-pad and mouse", &|l| {
            l.left_pad = PadMode::Directions { dirs: Directions::dpad(), click: true };
            l.right_pad = PadMode::Mouse { speed: 1200.0, trackball: false };
            l.buttons.insert("RPadClick".into(), A::mouse(MouseButton::Left));
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    fn layout(list: &[(&str, Action)]) -> Layout {
        Layout { buttons: buttons(list), ..Layout::default() }
    }

    #[test]
    fn combination_presses_modifiers_first_and_releases_them_last() {
        let mut e = Engine::new(layout(&[("A", Action::key(&["C", "Ctrl"]))]));
        let mut inj = Injector::default();
        let s = ControllerState { buttons: buttons::A, ..Default::default() };
        let ctrl = key_id("Ctrl").unwrap();
        let c = key_id("C").unwrap();
        let down: Vec<Event> = inj.apply(e.process(&s, ms(0))).to_vec();
        assert_eq!(down, vec![Event::Key { key: ctrl, down: true, by_char: false }, Event::Key { key: c, down: true, by_char: true }]);
        // Held: nothing new.
        assert!(inj.apply(e.process(&s, ms(4))).is_empty());
        let up: Vec<Event> = inj.apply(e.process(&ControllerState::default(), ms(8))).to_vec();
        assert_eq!(up, vec![Event::Key { key: c, down: false, by_char: true }, Event::Key { key: ctrl, down: false, by_char: false }]);
    }

    #[test]
    fn run_fires_once_per_press() {
        let mut e = Engine::new(layout(&[("A", Action::Run { target: " notepad.exe ".into() }), ("B", Action::Run { target: "  ".into() })]));
        let a = ControllerState { buttons: buttons::A | buttons::B, ..Default::default() };
        assert_eq!(e.process(&a, ms(0)).runs, ["notepad.exe"]);
        assert!(e.process(&a, ms(4)).runs.is_empty());
        assert!(e.process(&ControllerState::default(), ms(8)).runs.is_empty());
        assert_eq!(e.process(&a, ms(12)).runs, ["notepad.exe"]);
    }

    #[test]
    fn radial_slices_go_clockwise_from_the_top_and_hold_at_their_edges() {
        // Four slices: up, right, down, left.
        assert_eq!(radial_slice(0.0, 1.0, 4, None), 0);
        assert_eq!(radial_slice(1.0, 0.0, 4, None), 1);
        assert_eq!(radial_slice(0.0, -1.0, 4, None), 2);
        assert_eq!(radial_slice(-1.0, 0.0, 4, None), 3);
        assert_eq!(radial_slice(-0.1, 1.0, 4, None), 0);
        // Just past the edge between up and right: stays up, further on it's right.
        let (s, c) = (48f32.to_radians().sin(), 48f32.to_radians().cos());
        assert_eq!(radial_slice(s, c, 4, None), 1);
        assert_eq!(radial_slice(s, c, 4, Some(0)), 0);
        let (s, c) = (60f32.to_radians().sin(), 60f32.to_radians().cos());
        assert_eq!(radial_slice(s, c, 4, Some(0)), 1);
        // From the far side of the circle.
        assert_eq!(radial_slice(-0.1, 1.0, 4, Some(3)), 0);
    }

    fn thumb(x: i16, y: i16, clicked: bool) -> ControllerState {
        ControllerState { left_pad: Trackpad { touched: true, clicked, x, y, ..Default::default() }, ..Default::default() }
    }

    fn radial(click: bool) -> Engine {
        let items = vec![Action::key(&["A"]), Action::key(&["B"]), Action::Run { target: "calc".into() }, Action::key(&["D"])];
        Engine::new(Layout { left_pad: PadMode::Radial { items, click }, ..Layout::default() })
    }

    #[test]
    fn radial_plays_the_item_under_the_thumb_on_lift() {
        let mut e = radial(false);
        let b = key_id("B").unwrap();
        // The centre picks nothing, and lifting there plays nothing.
        assert_eq!(e.process(&thumb(0, 0, false), ms(0)).ticks, 0);
        assert!(e.process(&ControllerState::default(), ms(4)).keys.is_empty());
        // Up ticks, right ticks again, holding still doesn't.
        assert_eq!(e.process(&thumb(0, 30000, false), ms(8)).ticks, 1);
        let f = e.process(&thumb(30000, 0, false), ms(12));
        assert_eq!((f.ticks, f.radial[0]), (1, Some(1)));
        assert!(f.keys.is_empty());
        assert_eq!(e.process(&thumb(30000, 0, false), ms(16)).ticks, 0);
        // Lift: B for a moment, then let go.
        assert!(e.process(&ControllerState::default(), ms(20)).keys.contains(b));
        assert!(e.process(&ControllerState::default(), ms(40)).keys.contains(b));
        assert!(e.process(&ControllerState::default(), ms(64)).keys.is_empty());
        // Down opens the program once.
        e.process(&thumb(0, -30000, false), ms(70));
        assert_eq!(e.process(&ControllerState::default(), ms(74)).runs, ["calc"]);
        assert!(e.process(&ControllerState::default(), ms(78)).runs.is_empty());
    }

    #[test]
    fn radial_with_click_holds_the_item_clicked_on() {
        let mut e = radial(true);
        let a = key_id("A").unwrap();
        e.process(&thumb(0, 30000, false), ms(0));
        assert!(e.process(&thumb(0, 30000, true), ms(4)).keys.contains(a));
        // Sliding while clicked keeps the item it was clicked on.
        let f = e.process(&thumb(30000, 0, true), ms(8));
        assert!(f.keys.contains(a) && f.radial[0] == Some(0));
        assert!(e.process(&thumb(30000, 0, false), ms(12)).keys.is_empty());
        // Lifting without a click plays nothing.
        assert!(e.process(&ControllerState::default(), ms(16)).keys.is_empty());
    }

    #[test]
    fn shortcuts_go_by_character_and_plain_keys_by_position() {
        let mut e = Engine::new(layout(&[
            ("A", Action::key(&["Ctrl", "Z"])),
            ("B", Action::key(&["Shift", "W"])),
            ("X", Action::key(&["D"])),
            ("Y", Action::key(&["Win", "Tab"])),
        ]));
        let mut inj = Injector::default();
        let s = ControllerState { buttons: buttons::A | buttons::B | buttons::X | buttons::Y, ..Default::default() };
        let down: Vec<(&str, bool)> = inj
            .apply(e.process(&s, ms(0)))
            .iter()
            .filter_map(|ev| match ev {
                Event::Key { key, by_char, down: true } => Some((KEYS[*key as usize], *by_char)),
                _ => None,
            })
            .collect();
        // Z is in a Ctrl shortcut: by character. W with Shift, D alone, Tab and the modifiers: by position.
        for (k, want) in [("Ctrl", false), ("Shift", false), ("Win", false), ("W", false), ("D", false), ("Z", true), ("Tab", false)] {
            assert!(down.contains(&(k, want)), "{k}: {down:?}");
        }
        let up = inj.release().to_vec();
        assert!(up.contains(&Event::Key { key: key_id("Z").unwrap(), down: false, by_char: true }));
    }

    #[test]
    fn held_key_repeats() {
        let mut e = Engine::new(layout(&[("Up", Action::key_repeat(&["Up"]))]));
        let s = ControllerState { buttons: buttons::DPAD_UP, ..Default::default() };
        let mut taps = 0;
        for t in (0..1000).step_by(4) {
            taps += e.process(&s, ms(t)).taps.len();
        }
        // Repeats start at 400 ms, then every 50 ms: 400, 452, … up to 996.
        assert!((11..=13).contains(&taps), "{taps}");
    }

    #[test]
    fn triggers_analog_for_the_pad_threshold_for_keys() {
        let mut e = Engine::new(layout(&[("LT", Action::pad(PadInput::Lt)), ("RT", Action::mouse(MouseButton::Left))]));
        let s = ControllerState { trigger_left: 16384, trigger_right: 6000, ..Default::default() };
        let f = e.process(&s, ms(0));
        assert_eq!(f.pad.lt, 128);
        assert_eq!(f.mouse_buttons, 0, "a light pull isn't a click");
        let s = ControllerState { trigger_right: 16000, ..s };
        assert_eq!(e.process(&s, ms(4)).mouse_buttons, MouseButton::Left.bit());
    }

    #[test]
    fn stick_to_keys_with_hysteresis() {
        let l = Layout { left_stick: StickMode::Directions { dirs: Directions::keys("W", "A", "S", "D"), threshold: 0.5 }, ..Layout::default() };
        let mut e = Engine::new(l);
        let w = key_id("W").unwrap();
        let s = |y: i16| ControllerState { left_stick: [0, y], ..Default::default() };
        assert!(e.process(&s(18000), ms(0)).keys.contains(w));
        // Back to 40%: still held (releases below 35%).
        assert!(e.process(&s(13000), ms(4)).keys.contains(w));
        assert!(!e.process(&s(10000), ms(8)).keys.contains(w));
        assert!(!e.process(&s(13000), ms(12)).keys.contains(w));
    }

    #[test]
    fn stick_to_mouse_moves_by_speed_and_time() {
        let l = Layout { right_stick: StickMode::Mouse { speed: 1000.0, deadzone: 0.0 }, ..Layout::default() };
        let mut e = Engine::new(l);
        let mut inj = Injector::default();
        let s = ControllerState { right_stick: [32767, 0], ..Default::default() };
        e.process(&s, ms(0));
        let mut dx = 0;
        for t in 1..=100 {
            for ev in inj.apply(e.process(&s, ms(t * 10))) {
                if let Event::Move { dx: x, .. } = ev {
                    dx += x;
                }
            }
        }
        // One second at full tilt: 1000 px.
        assert!((dx - 1000).abs() <= 1, "{dx}");
    }

    #[test]
    fn trackpad_mouse_and_trackball() {
        let l = Layout { right_pad: PadMode::Mouse { speed: 1000.0, trackball: true }, ..Layout::default() };
        let mut e = Engine::new(l);
        let pad = |x: i16, touched: bool| ControllerState { right_pad: Trackpad { touched, x, ..Default::default() }, ..Default::default() };
        e.process(&pad(0, true), ms(0));
        let f = e.process(&pad(6554, true), ms(10));
        // A tenth of the pad: 100 px.
        assert!((f.mouse[0] - 100.0).abs() < 0.5, "{:?}", f.mouse);
        e.process(&pad(13108, true), ms(20));
        // Lifted: it glides on, slowing down.
        let glide1 = e.process(&pad(0, false), ms(30)).mouse[0];
        let glide2 = e.process(&pad(0, false), ms(40)).mouse[0];
        assert!(glide1 > 0.0 && glide2 > 0.0 && glide2 < glide1, "{glide1} {glide2}");
        // Touching again stops it.
        assert_eq!(e.process(&pad(0, true), ms(50)).mouse[0], 0.0);
    }

    #[test]
    fn scroll_buttons_and_smooth_scroll_steps() {
        let mut e = Engine::new(layout(&[("A", Action::Scroll { dir: Dir::Down, repeat: false })]));
        let mut inj = Injector::default();
        let s = ControllerState { buttons: buttons::A, ..Default::default() };
        let evs = inj.apply(e.process(&s, ms(0))).to_vec();
        assert_eq!(evs, vec![Event::Scroll { v: -120, h: 0 }]);
        assert!(inj.apply(e.process(&s, ms(500))).is_empty(), "no repeat");
        // A quarter notch waits until a third has built up.
        let mut f = Frame { scroll: [0.0, 0.25], ..Default::default() };
        assert!(inj.apply(&f).is_empty());
        f.scroll = [0.0, 0.1];
        assert_eq!(inj.apply(&f), &[Event::Scroll { v: 40, h: 0 }]);
    }

    #[test]
    fn gyro_only_while_activated_and_calibrates_drift() {
        let l = Layout { gyro: Gyro { output: GyroOutput::Mouse, activation: Activation::RightStickTouch, axis: TurnAxis::Yaw, mouse_speed: 10.0, ..Gyro::default() }, ..Layout::default() };
        let mut e = Engine::new(l);
        // Resting flat with 1°/s of drift on the yaw axis.
        let drift = (1.0 / GYRO_FULL_SCALE_DPS * 32767.0) as i16;
        let mut s = ControllerState::default();
        s.imu.accel = [0, 0, 16384];
        s.imu.gyro = [0, 0, drift];
        s.buttons = buttons::RIGHT_STICK_TOUCH;
        let mut t = 0;
        let mut first = 0.0;
        for i in 0..400 {
            t += 4;
            let f = e.process(&s, ms(t));
            assert!(f.gyro_active);
            if i == 0 {
                first = f.mouse[0];
            }
        }
        assert!(first != 0.0, "drift moves the pointer before calibrating");
        assert_eq!(e.process(&s, ms(t + 4)).mouse[0], 0.0, "calibrated out");
        // Turning right (clockwise seen from above, negative yaw) at 90°/s for 4 ms moves right.
        s.imu.gyro[2] = drift - (90.0 / GYRO_FULL_SCALE_DPS * 32767.0) as i16;
        s.imu.accel = [100, 0, 16384];
        let f = e.process(&s, ms(t + 8));
        assert!((f.mouse[0] - 90.0 * 0.004 * 10.0).abs() < 0.2, "{:?}", f.mouse);
        // Thumb off the stick: nothing.
        s.buttons = 0;
        let f = e.process(&s, ms(t + 12));
        assert!(!f.gyro_active);
        assert_eq!(f.mouse, [0.0, 0.0]);
    }

    #[test]
    fn gyro_toggle_button() {
        let l = Layout { gyro: Gyro { output: GyroOutput::Stick, activation: Activation::Toggle { button: "R4".into() }, ..Gyro::default() }, ..Layout::default() };
        let mut e = Engine::new(l);
        let press = ControllerState { buttons: buttons::R4, ..Default::default() };
        let rest = ControllerState::default();
        assert!(!e.process(&rest, ms(0)).gyro_active);
        assert!(e.process(&press, ms(4)).gyro_active);
        assert!(e.process(&rest, ms(8)).gyro_active);
        e.process(&press, ms(12));
        assert!(!e.process(&rest, ms(16)).gyro_active);
    }

    #[test]
    fn steer_by_tilt() {
        let l = Layout { gyro: Gyro { output: GyroOutput::Steer, steer_angle: 40.0, ..Gyro::default() }, ..Layout::default() };
        let mut e = Engine::new(l);
        // Right side lowered by 20°: rotation about +Y (forward) by 20°.
        let h = 10f32.to_radians();
        let mut s = ControllerState::default();
        s.imu.quat = Some([(h.cos() * 32767.0) as i16, 0, (h.sin() * 32767.0) as i16, 0]);
        let lx = e.process(&s, ms(0)).pad.lx as f32 / 32767.0;
        assert!((lx - 18.0 / 40.0).abs() < 0.02, "{lx}");
    }

    #[test]
    fn pad_buttons_sticks_and_switch() {
        let mut l = builtin_gamepad().remove(0);
        l.buttons.insert("R4".into(), Action::key(&["F"]));
        let mut e = Engine::new(l);
        let s = ControllerState { buttons: buttons::A | buttons::DPAD_LEFT | buttons::R4 | buttons::STEAM, left_stick: [32767, -32767], ..Default::default() };
        let f = e.process(&s, ms(0)).clone();
        assert_eq!(f.pad.buttons, xbutton::A | xbutton::LEFT);
        assert_eq!((f.pad.lx, f.pad.ly), (23170, -23170), "clamped to the circle");
        assert!(f.keys.contains(key_id("F").unwrap()));
        assert!(f.switch);
        assert!(!e.process(&s, ms(4)).switch, "once per press");
        assert!(f.lit & (1 << CONTROLS.iter().position(|c| *c == "A").unwrap()) != 0);
    }

    #[test]
    fn trackpad_dpad_needs_a_click() {
        let l = Layout { left_pad: PadMode::Directions { dirs: Directions::dpad(), click: true }, ..Layout::default() };
        let mut e = Engine::new(l);
        let mut s = ControllerState { left_pad: Trackpad { touched: true, x: 0, y: 20000, ..Default::default() }, ..Default::default() };
        assert_eq!(e.process(&s, ms(0)).pad.buttons, 0);
        s.left_pad.clicked = true;
        assert_eq!(e.process(&s, ms(4)).pad.buttons, xbutton::UP);
    }

    #[test]
    fn release_lets_go_of_everything() {
        let mut inj = Injector::default();
        let mut f = Frame::default();
        f.keys.insert(3);
        f.mouse_buttons = MouseButton::Right.bit();
        inj.apply(&f);
        assert!(inj.held_anything());
        let evs = inj.release().to_vec();
        assert_eq!(evs, vec![Event::Key { key: 3, down: false, by_char: false }, Event::Button { button: MouseButton::Right, down: false }]);
        assert!(!inj.held_anything());
    }

    #[test]
    fn builtins_use_known_keys_and_controls() {
        for l in builtin_keymouse().iter().chain(&builtin_gamepad()) {
            for (c, a) in &l.buttons {
                assert!(CONTROLS.contains(&c.as_str()), "{}: {c}", l.name);
                if let Action::Key { keys, .. } = a {
                    keys.iter().for_each(|k| assert!(key_id(k).is_some(), "{}: {k}", l.name));
                }
            }
            // Round trip through JSON, as presets are saved.
            let json = serde_json::to_string(l).unwrap();
            assert_eq!(&serde_json::from_str::<Layout>(&json).unwrap(), l);
        }
    }
}
