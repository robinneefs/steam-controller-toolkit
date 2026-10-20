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
/// `controller_structs.h`). `side` picks the trackpad actuator: 0 left, 1 right, 2 both (verified
/// by hand; 3 plays nothing). `gain_db` is relative to full strength.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Haptic {
    /// Continuous rumble, like a gamepad's two motors. The controller stops it after ~50 ms
    /// unless it is resent, which the hub does until a rumble with both speeds 0 arrives.
    Rumble { left: u16, right: u16 },
    /// `repeat + 1` pulses of `on_us` with `off_us` gaps.
    Pulse { side: u8, on_us: u16, off_us: u16, repeat: u16 },
    /// A built-in effect: 0 off, 1 tick, 2 click, 3 tone, 4 rumble, 5 noise, 6 script, 7 sweep.
    /// Only tick and click play; the others need parameters this report has no room for.
    Command { side: u8, command: u8, gain_db: i8 },
    /// A tone at `frequency` Hz, its amplitude modulated at `lfo_freq` Hz by `lfo_depth`.
    Tone { side: u8, gain_db: i8, frequency: u16, duration_ms: u16, lfo_freq: u16, lfo_depth: u8 },
    /// A logarithmic frequency sweep from `start_hz` to `end_hz`.
    Sweep { side: u8, gain_db: i8, duration_ms: u16, start_hz: u16, end_hz: u16 },
    /// A haptic pattern stored in the firmware. Scripts 0..=17 play; 18 and up are empty.
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
    pub const MENU: u32 = 0x0000_0040;
    pub const R4: u32 = 0x0000_0080;
    pub const R5: u32 = 0x0000_0100;
    pub const R: u32 = 0x0000_0200;
    pub const DPAD_DOWN: u32 = 0x0000_0400;
    pub const DPAD_RIGHT: u32 = 0x0000_0800;
    pub const DPAD_LEFT: u32 = 0x0000_1000;
    pub const DPAD_UP: u32 = 0x0000_2000;
    pub const VIEW: u32 = 0x0000_4000;
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
        (A, "A"), (B, "B"), (X, "X"), (Y, "Y"), (QAM, "QAM"), (R3, "R3"), (MENU, "Menu"),
        (R4, "R4"), (R5, "R5"), (R, "R"), (DPAD_DOWN, "Down"), (DPAD_RIGHT, "Right"),
        (DPAD_LEFT, "Left"), (DPAD_UP, "Up"), (VIEW, "View"), (L3, "L3"), (STEAM, "Steam"),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The captured `0x42` report from `docs/PROTOCOL.md`, controller lying still.
    const CAPTURED: [u8; 54] = [
        0x42, 0xbb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xba, 0x01, 0x8d, 0x00, 0x51, 0xfe,
        0x58, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4f, 0xc0,
        0x0c, 0x00, 0x00, 0x00, 0xa2, 0x31, 0x6e, 0x25, 0xe0, 0xff, 0xfd, 0xff, 0x04, 0x00, 0x12, 0x6f,
        0x07, 0x32, 0xf1, 0xef, 0x29, 0xdc,
    ];

    fn state(data: &[u8]) -> ControllerState {
        match parse(data) {
            Some(Report::State(s)) => s,
            other => panic!("expected a state report, got {other:?}"),
        }
    }

    #[test]
    fn parses_the_captured_report() {
        let s = state(&CAPTURED);
        assert_eq!((s.report_id, s.seq, s.buttons), (0x42, 0xbb, 0));
        assert_eq!((s.trigger_left, s.trigger_right), (0, 0));
        assert_eq!(s.left_stick, [442, 141]);
        assert_eq!(s.right_stick, [-431, 88]);
        assert!(!s.left_pad.touched && !s.right_pad.touched);
        assert_eq!(s.imu.timestamp, 0x000c_c04f);
        assert_eq!(s.imu.accel, [0, 12706, 9582]);
        assert_eq!(s.imu.gyro, [-32, -3, 4]);
        assert_eq!(s.imu.quat.map(|q| q[0]), Some(0x6f12));
    }

    /// The docs say the quaternion rotates sensor vectors into a world frame with +Z up.
    #[test]
    fn quaternion_rotates_gravity_to_up() {
        let s = state(&CAPTURED);
        let [w, x, y, z] = s.imu.quat.unwrap().map(|v| v as f64 / 32767.0);
        let a = s.imu.accel.map(|v| v as f64 / 16384.0);
        // v' = v + 2w(q × v) + 2q × (q × v)
        let cross = |p: [f64; 3], q: [f64; 3]| [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
        let t = cross([x, y, z], a).map(|c| 2.0 * c);
        let u = cross([x, y, z], t);
        let up: Vec<f64> = (0..3).map(|i| a[i] + w * t[i] + u[i]).collect();
        assert!(up[0].abs() < 0.1 && up[1].abs() < 0.1 && up[2] > 0.9, "rotated gravity {up:?}");
    }

    #[test]
    fn parses_buttons_and_pads() {
        let mut d = CAPTURED;
        let buttons = buttons::A | buttons::LEFT_PAD_TOUCH | buttons::RIGHT_PAD_CLICK | buttons::LEFT_GRIP_TOUCH;
        d[2..6].copy_from_slice(&buttons.to_le_bytes());
        d[8..10].copy_from_slice(&32767i16.to_le_bytes());
        d[18..20].copy_from_slice(&(-1000i16).to_le_bytes());
        d[20..22].copy_from_slice(&2000i16.to_le_bytes());
        d[22..24].copy_from_slice(&500u16.to_le_bytes());
        d[28..30].copy_from_slice(&65535u16.to_le_bytes());
        let s = state(&d);
        assert!(s.pressed(buttons::A) && !s.pressed(buttons::B));
        assert_eq!(s.trigger_right, 32767);
        assert!(s.left_pad.touched && !s.left_pad.clicked);
        assert!(!s.right_pad.touched && s.right_pad.clicked);
        assert_eq!((s.left_pad.x, s.left_pad.y, s.left_pad.pressure), (-1000, 2000, 500));
        assert_eq!(s.right_pad.pressure, 65535);
    }

    #[test]
    fn report_0x45_has_no_quaternion() {
        let mut d = CAPTURED;
        d[0] = REPORT_STATE_NO_QUAT;
        let s = state(&d[..46]);
        assert_eq!(s.imu.quat, None);
        assert_eq!(s.imu.accel, [0, 12706, 9582]);
        // Even with the bytes present, only 0x42 carries a quaternion.
        assert_eq!(state(&d).imu.quat, None);
    }

    #[test]
    fn report_0x47_shifts_the_pads() {
        let mut d = [0u8; 46];
        d[0] = REPORT_STATE_TIMESTAMP;
        d[18..20].copy_from_slice(&0x1234u16.to_le_bytes()); // trackpad timestamp
        d[20..22].copy_from_slice(&111i16.to_le_bytes()); // left pad x
        d[30..32].copy_from_slice(&222u16.to_le_bytes()); // right pad pressure
        d[32..34].copy_from_slice(&0xbeefu16.to_le_bytes()); // IMU timestamp
        d[34..36].copy_from_slice(&16384i16.to_le_bytes()); // accel x
        d[44..46].copy_from_slice(&(-7i16).to_le_bytes()); // gyro z
        let s = state(&d);
        assert_eq!(s.left_pad.x, 111);
        assert_eq!(s.right_pad.pressure, 222);
        assert_eq!(s.imu.timestamp, 0xbeef);
        assert_eq!(s.imu.accel[0], 16384);
        assert_eq!(s.imu.gyro[2], -7);
        assert_eq!(s.imu.quat, None);
    }

    #[test]
    fn parses_other_reports() {
        assert!(matches!(parse(&[REPORT_BATTERY, 2, 87]), Some(Report::Battery(BatteryStatus { charge_state: 2, level: 87 }))));
        assert!(matches!(parse(&[REPORT_WIRELESS_STATUS, 2]), Some(Report::Wireless(true))));
        assert!(matches!(parse(&[REPORT_WIRELESS_STATUS_X, 1]), Some(Report::Wireless(false))));
        assert!(matches!(parse(&[0x7b, 1, 2, 3]), Some(Report::Other(0x7b))));
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn short_state_reports_are_rejected() {
        assert!(parse(&CAPTURED[..45]).is_none());
        assert!(state(&CAPTURED[..46]).imu.quat.is_none()); // quaternion cut off
        assert!(state(&CAPTURED[..53]).imu.quat.is_none());
        assert!(state(&CAPTURED[..54]).imu.quat.is_some());
    }

    /// Truncated or garbage reports of any id and length must never panic the reader.
    #[test]
    fn never_panics_on_any_id_or_length() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut buf = [0u8; 64];
        for id in 0..=255u8 {
            for len in 0..=buf.len() {
                for b in buf.iter_mut() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    *b = seed as u8;
                }
                buf[0] = id;
                let _ = parse(&buf[..len]);
            }
        }
    }

    #[test]
    fn feature_reports_match_the_docs() {
        let s = settings_report();
        assert_eq!(s.len(), 64);
        assert_eq!(&s[..9], &[0x01, 0x87, 6, 9, 0, 0, 48, 0x18, 0]);
        assert!(s[9..].iter().all(|&b| b == 0));
        let t = turn_off_report();
        assert_eq!(&t[..7], &[0x01, 0x9f, 4, b'o', b'f', b'f', b'!']);
        assert!(t[7..].iter().all(|&b| b == 0));
    }

    #[test]
    fn haptic_reports_match_the_docs() {
        let r = |h| haptic_report(&h);
        assert_eq!(r(Haptic::Rumble { left: 0x1234, right: 0xabcd }), [0x80, 0, 0, 0, 0x34, 0x12, 0, 0xcd, 0xab, 0]);
        assert_eq!(r(Haptic::Pulse { side: 2, on_us: 300, off_us: 700, repeat: 4 }), [0x81, 2, 0x2c, 0x01, 0xbc, 0x02, 4, 0]);
        assert_eq!(r(Haptic::Command { side: 1, command: 2, gain_db: -12 }), [0x82, 1, 2, 0xf4]);
        assert_eq!(
            r(Haptic::Tone { side: 0, gain_db: -6, frequency: 440, duration_ms: 250, lfo_freq: 5, lfo_depth: 128 }),
            [0x83, 0, 0xfa, 0xb8, 0x01, 0xfa, 0x00, 5, 0, 128]
        );
        assert_eq!(
            r(Haptic::Sweep { side: 2, gain_db: 0, duration_ms: 1000, start_hz: 100, end_hz: 2000 }),
            [0x84, 2, 0, 0xe8, 0x03, 100, 0, 0xd0, 0x07]
        );
        assert_eq!(r(Haptic::Script { side: 1, script_id: 17, gain_db: -3 }), [0x85, 1, 17, 0xfd]);
    }

    /// The UI sends haptics as JSON tagged by `kind`; see `src/tabs/haptics.ts`.
    #[test]
    fn haptics_deserialize_from_the_ui() {
        let h: Haptic = serde_json::from_str(r#"{"kind":"script","side":2,"script_id":3,"gain_db":-12}"#).unwrap();
        assert_eq!(h, Haptic::Script { side: 2, script_id: 3, gain_db: -12 });
        let h: Haptic = serde_json::from_str(r#"{"kind":"rumble","left":0,"right":0}"#).unwrap();
        assert!(h.is_rumble());
    }
}
