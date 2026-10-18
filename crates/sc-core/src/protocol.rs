//! Wire format of the 2026 Steam Controller ("Triton").
//!
//! Reference: SDL's `src/joystick/hidapi/SDL_hidapi_steam_triton.c` and
//! `steam/controller_structs.h`. All multi-byte fields are little-endian and packed.

use serde::{Deserialize, Serialize};

pub const VALVE_VID: u16 = 0x28DE;
/// Wireless receiver ("Proteus" dongle). Interfaces 2..=5 are the four controller slots.
pub const PID_PROTEUS_DONGLE: u16 = 0x1304;
/// Newer wireless receiver revision ("Nereid").
pub const PID_NEREID_DONGLE: u16 = 0x1305;
/// Bluetooth LE connection.
pub const PID_TRITON_BLE: u16 = 0x1303;
/// Wired USB connection.
pub const PID_TRITON_WIRED: u16 = 0x1302;

pub const REPORT_STATE: u8 = 0x42;
pub const REPORT_BATTERY: u8 = 0x43;
pub const REPORT_STATE_NO_QUAT: u8 = 0x45;
pub const REPORT_WIRELESS_STATUS_X: u8 = 0x46;
pub const REPORT_STATE_TIMESTAMP: u8 = 0x47;
pub const REPORT_WIRELESS_STATUS: u8 = 0x79;

pub const FEATURE_REPORT_BYTES: usize = 64;
const MSG_SET_SETTINGS_VALUES: u8 = 0x87;
const MSG_TURN_OFF_CONTROLLER: u8 = 0x9F;
const SETTING_LIZARD_MODE: u8 = 9;
const SETTING_IMU_MODE: u8 = 48;
const IMU_SEND_RAW_ACCEL: u16 = 0x0008;
const IMU_SEND_RAW_GYRO: u16 = 0x0010;

/// Feature report that turns off lizard mode (keyboard/mouse emulation) and enables raw IMU data.
/// The controller re-enables lizard mode after a few seconds, so this must be resent periodically.
pub fn settings_report() -> [u8; FEATURE_REPORT_BYTES] {
    let mut buf = [0u8; FEATURE_REPORT_BYTES];
    buf[0] = 1; // report id
    buf[1] = MSG_SET_SETTINGS_VALUES;
    buf[2] = 6; // payload length: two (u8 id, u16 value) settings
    buf[3] = SETTING_LIZARD_MODE;
    buf[4..6].copy_from_slice(&0u16.to_le_bytes());
    buf[6] = SETTING_IMU_MODE;
    buf[7..9].copy_from_slice(&(IMU_SEND_RAW_ACCEL | IMU_SEND_RAW_GYRO).to_le_bytes());
    buf
}

/// Feature report asking the controller to power off: `ID_TURN_OFF_CONTROLLER` from SDL's
/// `controller_constants.h`, with the `off!` payload the original Steam Controller expects.
/// SDL's Triton driver never sends it, but it works on this controller (verified over the Puck).
pub fn turn_off_report() -> [u8; FEATURE_REPORT_BYTES] {
    let mut buf = [0u8; FEATURE_REPORT_BYTES];
    buf[0] = 1; // report id
    buf[1] = MSG_TURN_OFF_CONTROLLER;
    buf[2] = 4; // payload length
    buf[3..7].copy_from_slice(b"off!");
    buf
}

const OUT_HAPTIC_RUMBLE: u8 = 0x80;
const OUT_HAPTIC_PULSE: u8 = 0x81;
const OUT_HAPTIC_COMMAND: u8 = 0x82;
const OUT_HAPTIC_LFO_TONE: u8 = 0x83;
const OUT_HAPTIC_LOG_SWEEP: u8 = 0x84;
const OUT_HAPTIC_SCRIPT: u8 = 0x85;

/// A haptic effect, sent as an output report (`ID_OUT_REPORT_HAPTIC_*` in SDL's
/// `controller_structs.h`). `side` picks the actuators as a bitmask: 1 left, 2 right, 3 both
/// (0 also plays on the left; verified by hand). `gain_db` is relative to full strength.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Haptic {
    /// Continuous rumble, like a gamepad's two motors. The controller stops it after ~50 ms
    /// unless it is resent, which the hub does until a rumble with both speeds 0 arrives.
    Rumble { left: u16, right: u16 },
    /// `repeat + 1` pulses of `on_us` with `off_us` gaps.
    Pulse { side: u8, on_us: u16, off_us: u16, repeat: u16 },
    /// A built-in effect: 0 off, 1 tick, 2 click, 3 tone, 4 rumble, 5 noise, 6 script, 7 sweep.
    Command { side: u8, command: u8, gain_db: i8 },
    /// A tone at `frequency` Hz, its amplitude modulated at `lfo_freq` Hz by `lfo_depth`.
    Tone { side: u8, gain_db: i8, frequency: u16, duration_ms: u16, lfo_freq: u16, lfo_depth: u8 },
    /// A logarithmic frequency sweep from `start_hz` to `end_hz`.
    Sweep { side: u8, gain_db: i8, duration_ms: u16, start_hz: u16, end_hz: u16 },
    /// A haptic pattern stored in the firmware.
    Script { side: u8, script_id: u8, gain_db: i8 },
}

impl Haptic {
    pub fn is_rumble(&self) -> bool {
        matches!(self, Haptic::Rumble { .. })
    }
}

/// Output report for `h`, report id first.
pub fn haptic_report(h: &Haptic) -> Vec<u8> {
    let mut b = Vec::with_capacity(10);
    match *h {
        Haptic::Rumble { left, right } => {
            b.extend([OUT_HAPTIC_RUMBLE, 0]); // type
            b.extend(0u16.to_le_bytes()); // intensity
            b.extend(left.to_le_bytes());
            b.push(0); // left gain
            b.extend(right.to_le_bytes());
            b.push(0); // right gain
        }
        Haptic::Pulse { side, on_us, off_us, repeat } => {
            b.extend([OUT_HAPTIC_PULSE, side]);
            b.extend(on_us.to_le_bytes());
            b.extend(off_us.to_le_bytes());
            b.extend(repeat.to_le_bytes());
        }
        Haptic::Command { side, command, gain_db } => b.extend([OUT_HAPTIC_COMMAND, side, command, gain_db as u8]),
        Haptic::Tone { side, gain_db, frequency, duration_ms, lfo_freq, lfo_depth } => {
            b.extend([OUT_HAPTIC_LFO_TONE, side, gain_db as u8]);
            b.extend(frequency.to_le_bytes());
            b.extend(duration_ms.to_le_bytes());
            b.extend(lfo_freq.to_le_bytes());
            b.push(lfo_depth);
        }
        Haptic::Sweep { side, gain_db, duration_ms, start_hz, end_hz } => {
            b.extend([OUT_HAPTIC_LOG_SWEEP, side, gain_db as u8]);
            b.extend(duration_ms.to_le_bytes());
            b.extend(start_hz.to_le_bytes());
            b.extend(end_hz.to_le_bytes());
        }
        Haptic::Script { side, script_id, gain_db } => b.extend([OUT_HAPTIC_SCRIPT, side, script_id, gain_db as u8]),
    }
    b
}

pub mod buttons {
    pub const A: u32 = 0x0000_0001;
    pub const B: u32 = 0x0000_0002;
    pub const X: u32 = 0x0000_0004;
    pub const Y: u32 = 0x0000_0008;
    pub const QAM: u32 = 0x0000_0010;
    pub const R3: u32 = 0x0000_0020;
    pub const VIEW: u32 = 0x0000_0040;
    pub const R4: u32 = 0x0000_0080;
    pub const R5: u32 = 0x0000_0100;
    pub const R: u32 = 0x0000_0200;
    pub const DPAD_DOWN: u32 = 0x0000_0400;
    pub const DPAD_RIGHT: u32 = 0x0000_0800;
    pub const DPAD_LEFT: u32 = 0x0000_1000;
    pub const DPAD_UP: u32 = 0x0000_2000;
    pub const MENU: u32 = 0x0000_4000;
    pub const L3: u32 = 0x0000_8000;
    pub const STEAM: u32 = 0x0001_0000;
    pub const L4: u32 = 0x0002_0000;
    pub const L5: u32 = 0x0004_0000;
    pub const L: u32 = 0x0008_0000;
    pub const RIGHT_STICK_TOUCH: u32 = 0x0010_0000;
    pub const RIGHT_PAD_TOUCH: u32 = 0x0020_0000;
    pub const RIGHT_PAD_CLICK: u32 = 0x0040_0000;
    pub const RIGHT_TRIGGER_CLICK: u32 = 0x0080_0000;
    pub const LEFT_STICK_TOUCH: u32 = 0x0100_0000;
    pub const LEFT_PAD_TOUCH: u32 = 0x0200_0000;
    pub const LEFT_PAD_CLICK: u32 = 0x0400_0000;
    pub const LEFT_TRIGGER_CLICK: u32 = 0x0800_0000;
    pub const RIGHT_GRIP_TOUCH: u32 = 0x1000_0000;
    pub const LEFT_GRIP_TOUCH: u32 = 0x2000_0000;

    /// Display names, in bit order, for debugging and UIs.
    pub const NAMES: &[(u32, &str)] = &[
        (A, "A"), (B, "B"), (X, "X"), (Y, "Y"), (QAM, "QAM"), (R3, "R3"), (VIEW, "View"),
        (R4, "R4"), (R5, "R5"), (R, "R"), (DPAD_DOWN, "Down"), (DPAD_RIGHT, "Right"),
        (DPAD_LEFT, "Left"), (DPAD_UP, "Up"), (MENU, "Menu"), (L3, "L3"), (STEAM, "Steam"),
        (L4, "L4"), (L5, "L5"), (L, "L"), (RIGHT_STICK_TOUCH, "RStickTouch"),
        (RIGHT_PAD_TOUCH, "RPadTouch"), (RIGHT_PAD_CLICK, "RPadClick"),
        (RIGHT_TRIGGER_CLICK, "RTrigClick"), (LEFT_STICK_TOUCH, "LStickTouch"),
        (LEFT_PAD_TOUCH, "LPadTouch"), (LEFT_PAD_CLICK, "LPadClick"),
        (LEFT_TRIGGER_CLICK, "LTrigClick"), (RIGHT_GRIP_TOUCH, "RGrip"), (LEFT_GRIP_TOUCH, "LGrip"),
    ];
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Trackpad {
    pub touched: bool,
    pub clicked: bool,
    /// Raw signed position; roughly -32768..32767 on each axis, +y is up.
    pub x: i16,
    pub y: i16,
    pub pressure: u16,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Imu {
    /// Raw sensor counts. Accel full scale is ±2 g, gyro full scale is ±2000 °/s.
    pub accel: [i16; 3],
    pub gyro: [i16; 3],
    /// Orientation quaternion (w, x, y, z) computed on the controller, only in report 0x42.
    pub quat: Option<[i16; 4]>,
    /// Sensor timestamp; microseconds for 0x42/0x45, 32 µs ticks for 0x47.
    pub timestamp: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ControllerState {
    pub report_id: u8,
    pub seq: u8,
    pub buttons: u32,
    /// Analog trigger travel, 0..=32767.
    pub trigger_left: i16,
    pub trigger_right: i16,
    pub left_stick: [i16; 2],
    pub right_stick: [i16; 2],
    pub left_pad: Trackpad,
    pub right_pad: Trackpad,
    pub imu: Imu,
}

impl ControllerState {
    pub fn pressed(&self, mask: u32) -> bool {
        self.buttons & mask != 0
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BatteryStatus {
    pub charge_state: u8,
    pub level: u8,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum Report {
    State(ControllerState),
    Battery(BatteryStatus),
    /// `true` when a controller paired to this dongle slot connected, `false` when it left.
    Wireless(bool),
    Other(u8),
}

fn i16_at(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([d[o], d[o + 1]])
}
fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// Parse one input report. `data[0]` is the report id. Returns `None` if it is too short.
pub fn parse(data: &[u8]) -> Option<Report> {
    let id = *data.first()?;
    match id {
        REPORT_STATE | REPORT_STATE_NO_QUAT | REPORT_STATE_TIMESTAMP => parse_state(data).map(Report::State),
        REPORT_BATTERY if data.len() >= 3 => Some(Report::Battery(BatteryStatus { charge_state: data[1], level: data[2] })),
        REPORT_WIRELESS_STATUS | REPORT_WIRELESS_STATUS_X if data.len() >= 2 => Some(Report::Wireless(data[1] == 2)),
        _ => Some(Report::Other(id)),
    }
}

fn parse_state(d: &[u8]) -> Option<ControllerState> {
    let id = d[0];
    // 0x47 inserts a u16 trackpad timestamp before the pads and shrinks the IMU timestamp to u16.
    let (pads, imu_ts_len) = if id == REPORT_STATE_TIMESTAMP { (20, 2) } else { (18, 4) };
    let imu = pads + 12;
    let imu_data = imu + imu_ts_len;
    if d.len() < imu_data + 12 {
        return None;
    }
    let buttons = u32_at(d, 2);
    let quat = (id == REPORT_STATE && d.len() >= imu_data + 20)
        .then(|| [0, 1, 2, 3].map(|i| i16_at(d, imu_data + 12 + i * 2)));
    Some(ControllerState {
        report_id: id,
        seq: d[1],
        buttons,
        trigger_left: i16_at(d, 6),
        trigger_right: i16_at(d, 8),
        left_stick: [i16_at(d, 10), i16_at(d, 12)],
        right_stick: [i16_at(d, 14), i16_at(d, 16)],
        left_pad: Trackpad {
            touched: buttons & buttons::LEFT_PAD_TOUCH != 0,
            clicked: buttons & buttons::LEFT_PAD_CLICK != 0,
            x: i16_at(d, pads),
            y: i16_at(d, pads + 2),
            pressure: u16_at(d, pads + 4),
        },
        right_pad: Trackpad {
            touched: buttons & buttons::RIGHT_PAD_TOUCH != 0,
            clicked: buttons & buttons::RIGHT_PAD_CLICK != 0,
            x: i16_at(d, pads + 6),
            y: i16_at(d, pads + 8),
            pressure: u16_at(d, pads + 10),
        },
        imu: Imu {
            timestamp: if imu_ts_len == 4 { u32_at(d, imu) } else { u16_at(d, imu) as u32 },
            accel: [i16_at(d, imu_data), i16_at(d, imu_data + 2), i16_at(d, imu_data + 4)],
            gyro: [i16_at(d, imu_data + 6), i16_at(d, imu_data + 8), i16_at(d, imu_data + 10)],
            quat,
        },
    })
}
