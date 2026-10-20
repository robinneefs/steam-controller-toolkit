//! Linux: keyboard, mouse and a virtual Xbox 360 controller as uinput devices.
//!
//! Needs write access to /dev/uinput. Steam's udev rules grant it (as they grant the hidraw
//! access reading the controller needs); the .deb and .rpm install our own rule
//! (`src-tauri/linux/60-steam-controller-toolkit.rules`). Not run on Linux yet, only built.

use std::fs::OpenOptions;

use evdev::uinput::VirtualDevice;
use evdev::{AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, RelativeAxisCode, UinputAbsSetup};

use crate::devices::{PadSupport, VirtualPad};
use crate::layout::{xbutton, Event, MouseButton, PadState, KEYS};

const UINPUT: &str = "/dev/uinput";

pub fn support() -> PadSupport {
    match OpenOptions::new().write(true).open(UINPUT) {
        Ok(_) => PadSupport::Ready,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            PadSupport::Missing { detail: format!("No permission to use {UINPUT}") }
        }
        Err(e) => PadSupport::Missing { detail: format!("Couldn't open {UINPUT}: {e}") },
    }
}

fn open_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        format!("No permission to use {UINPUT}: install the udev rule (see the Gamepad tab)")
    } else {
        format!("Couldn't make a virtual device: {e}")
    }
}

/// Linux key code for a key name in `layout::KEYS`.
fn key_code(name: &str) -> Option<u16> {
    Some(match name {
        "Ctrl" => 29, "Shift" => 42, "Alt" => 56, "Win" => 125, "RCtrl" => 97, "RShift" => 54, "RAlt" => 100,
        "A" => 30, "B" => 48, "C" => 46, "D" => 32, "E" => 18, "F" => 33, "G" => 34, "H" => 35, "I" => 23,
        "J" => 36, "K" => 37, "L" => 38, "M" => 50, "N" => 49, "O" => 24, "P" => 25, "Q" => 16, "R" => 19,
        "S" => 31, "T" => 20, "U" => 22, "V" => 47, "W" => 17, "X" => 45, "Y" => 21, "Z" => 44,
        "1" => 2, "2" => 3, "3" => 4, "4" => 5, "5" => 6, "6" => 7, "7" => 8, "8" => 9, "9" => 10, "0" => 11,
        "F1" => 59, "F2" => 60, "F3" => 61, "F4" => 62, "F5" => 63, "F6" => 64, "F7" => 65, "F8" => 66,
        "F9" => 67, "F10" => 68, "F11" => 87, "F12" => 88, "F13" => 183, "F14" => 184, "F15" => 185,
        "F16" => 186, "F17" => 187, "F18" => 188, "F19" => 189, "F20" => 190, "F21" => 191, "F22" => 192,
        "F23" => 193, "F24" => 194,
        "Escape" => 1, "Tab" => 15, "CapsLock" => 58, "Space" => 57, "Enter" => 28, "Backspace" => 14,
        "Delete" => 111, "Insert" => 110, "Home" => 102, "End" => 107, "PageUp" => 104, "PageDown" => 109,
        "Up" => 103, "Down" => 108, "Left" => 105, "Right" => 106,
        "Minus" => 12, "Equal" => 13, "BracketLeft" => 26, "BracketRight" => 27, "Backslash" => 43,
        "Semicolon" => 39, "Quote" => 40, "Backquote" => 41, "Comma" => 51, "Period" => 52, "Slash" => 53,
        "Num0" => 82, "Num1" => 79, "Num2" => 80, "Num3" => 81, "Num4" => 75, "Num5" => 76, "Num6" => 77,
        "Num7" => 71, "Num8" => 72, "Num9" => 73, "NumAdd" => 78, "NumSubtract" => 74, "NumMultiply" => 55,
        "NumDivide" => 98, "NumDecimal" => 83, "NumEnter" => 96, "NumLock" => 69,
        "PrintScreen" => 99, "ScrollLock" => 70, "Pause" => 119, "ContextMenu" => 127,
        "VolumeUp" => 115, "VolumeDown" => 114, "VolumeMute" => 113, "MediaPlayPause" => 164,
        "MediaNext" => 163, "MediaPrevious" => 165, "MediaStop" => 166, "BrowserBack" => 158, "BrowserForward" => 159,
        _ => return None,
    })
}

const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;

fn ev(kind: EventType, code: u16, value: i32) -> InputEvent {
    InputEvent::new(kind.0, code, value)
}

pub struct KeyMouse {
    dev: VirtualDevice,
    /// Hi-res wheel units not yet a whole notch, for the classic wheel events.
    wheel_rest: [i32; 2],
}

impl KeyMouse {
    pub fn open() -> Result<Self, String> {
        let mut keys = AttributeSet::<KeyCode>::new();
        KEYS.iter().filter_map(|k| key_code(k)).for_each(|c| keys.insert(KeyCode::new(c)));
        [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_SIDE, BTN_EXTRA].into_iter().for_each(|c| keys.insert(KeyCode::new(c)));
        let mut rel = AttributeSet::<RelativeAxisCode>::new();
        for a in [RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y, RelativeAxisCode::REL_WHEEL, RelativeAxisCode::REL_HWHEEL, RelativeAxisCode::REL_WHEEL_HI_RES, RelativeAxisCode::REL_HWHEEL_HI_RES] {
            rel.insert(a);
        }
        let dev = VirtualDevice::builder()
            .map_err(open_error)?
            .name("Steam Controller Toolkit keyboard and mouse")
            .with_keys(&keys)
            .and_then(|b| b.with_relative_axes(&rel))
            .and_then(|b| b.build())
            .map_err(open_error)?;
        Ok(KeyMouse { dev, wheel_rest: [0; 2] })
    }
}

impl crate::devices::KeyMouse for KeyMouse {
    fn send(&mut self, events: &[Event]) -> Result<(), String> {
        let mut out = Vec::with_capacity(events.len() * 2);
        for e in events {
            match *e {
                // uinput sends key positions only; the desktop's layout picks the character.
                Event::Key { key, down, .. } => {
                    if let Some(c) = key_code(KEYS[key as usize]) {
                        out.push(ev(EventType::KEY, c, down as i32));
                    }
                }
                Event::Button { button, down } => {
                    let c = match button {
                        MouseButton::Left => BTN_LEFT,
                        MouseButton::Right => BTN_RIGHT,
                        MouseButton::Middle => BTN_MIDDLE,
                        MouseButton::Back => BTN_SIDE,
                        MouseButton::Forward => BTN_EXTRA,
                    };
                    out.push(ev(EventType::KEY, c, down as i32));
                }
                Event::Move { dx, dy } => {
                    out.push(ev(EventType::RELATIVE, RelativeAxisCode::REL_X.0, dx));
                    out.push(ev(EventType::RELATIVE, RelativeAxisCode::REL_Y.0, dy));
                }
                Event::Scroll { v, h } => {
                    for (i, (amount, hi, lo)) in [(v, RelativeAxisCode::REL_WHEEL_HI_RES, RelativeAxisCode::REL_WHEEL), (h, RelativeAxisCode::REL_HWHEEL_HI_RES, RelativeAxisCode::REL_HWHEEL)].into_iter().enumerate() {
                        if amount == 0 {
                            continue;
                        }
                        out.push(ev(EventType::RELATIVE, hi.0, amount));
                        self.wheel_rest[i] += amount;
                        let notches = self.wheel_rest[i] / 120;
                        if notches != 0 {
                            self.wheel_rest[i] -= notches * 120;
                            out.push(ev(EventType::RELATIVE, lo.0, notches));
                        }
                    }
                }
            }
        }
        if out.is_empty() {
            return Ok(());
        }
        self.dev.emit(&out).map_err(|e| format!("Couldn't send input: {e}"))
    }
}

// Xbox 360 controller as the kernel's xpad driver presents one, so SDL, Proton and Steam map it.
const BTN_A: u16 = 0x130;
const BTN_B: u16 = 0x131;
const BTN_X: u16 = 0x133;
const BTN_Y: u16 = 0x134;
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_SELECT: u16 = 0x13a;
const BTN_START: u16 = 0x13b;
const BTN_MODE: u16 = 0x13c;
const BTN_THUMBL: u16 = 0x13d;
const BTN_THUMBR: u16 = 0x13e;

const PAD_BUTTONS: &[(u16, u16)] = &[
    (xbutton::A, BTN_A),
    (xbutton::B, BTN_B),
    (xbutton::X, BTN_X),
    (xbutton::Y, BTN_Y),
    (xbutton::LB, BTN_TL),
    (xbutton::RB, BTN_TR),
    (xbutton::BACK, BTN_SELECT),
    (xbutton::START, BTN_START),
    (xbutton::GUIDE, BTN_MODE),
    (xbutton::LS, BTN_THUMBL),
    (xbutton::RS, BTN_THUMBR),
];

pub struct Pad {
    dev: VirtualDevice,
    last: Option<PadState>,
}

impl Pad {
    pub fn open() -> Result<Self, String> {
        let mut keys = AttributeSet::<KeyCode>::new();
        PAD_BUTTONS.iter().for_each(|(_, c)| keys.insert(KeyCode::new(*c)));
        let stick = AbsInfo::new(0, -32768, 32767, 16, 128, 0);
        let trigger = AbsInfo::new(0, 0, 255, 0, 0, 0);
        let hat = AbsInfo::new(0, -1, 1, 0, 0, 0);
        let mut b = VirtualDevice::builder()
            .map_err(open_error)?
            .name("Microsoft X-Box 360 pad")
            .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0114))
            .with_keys(&keys)
            .map_err(open_error)?;
        for (axis, info) in [
            (AbsoluteAxisCode::ABS_X, stick),
            (AbsoluteAxisCode::ABS_Y, stick),
            (AbsoluteAxisCode::ABS_RX, stick),
            (AbsoluteAxisCode::ABS_RY, stick),
            (AbsoluteAxisCode::ABS_Z, trigger),
            (AbsoluteAxisCode::ABS_RZ, trigger),
            (AbsoluteAxisCode::ABS_HAT0X, hat),
            (AbsoluteAxisCode::ABS_HAT0Y, hat),
        ] {
            b = b.with_absolute_axis(&UinputAbsSetup::new(axis, info)).map_err(open_error)?;
        }
        Ok(Pad { dev: b.build().map_err(open_error)?, last: None })
    }
}

impl VirtualPad for Pad {
    fn update(&mut self, s: &PadState) -> Result<(), String> {
        if self.last == Some(*s) {
            return Ok(());
        }
        let mut out = Vec::with_capacity(20);
        for (bit, code) in PAD_BUTTONS {
            out.push(ev(EventType::KEY, *code, (s.buttons & bit != 0) as i32));
        }
        let hat = |neg: u16, pos: u16| (s.buttons & pos != 0) as i32 - (s.buttons & neg != 0) as i32;
        // xpad reports Y down as positive.
        let flip = |v: i16| -(v as i32).max(-32767);
        for (axis, v) in [
            (AbsoluteAxisCode::ABS_X, s.lx as i32),
            (AbsoluteAxisCode::ABS_Y, flip(s.ly)),
            (AbsoluteAxisCode::ABS_RX, s.rx as i32),
            (AbsoluteAxisCode::ABS_RY, flip(s.ry)),
            (AbsoluteAxisCode::ABS_Z, s.lt as i32),
            (AbsoluteAxisCode::ABS_RZ, s.rt as i32),
            (AbsoluteAxisCode::ABS_HAT0X, hat(xbutton::LEFT, xbutton::RIGHT)),
            (AbsoluteAxisCode::ABS_HAT0Y, hat(xbutton::UP, xbutton::DOWN)),
        ] {
            out.push(ev(EventType::ABSOLUTE, axis.0, v));
        }
        self.last = Some(*s);
        self.dev.emit(&out).map_err(|e| format!("The virtual controller stopped: {e}"))
    }

    // Rumble needs force-feedback uploads handled; not yet.
    fn take_rumble(&mut self) -> Option<(u8, u8)> {
        None
    }

    fn player(&mut self) -> Option<u8> {
        None
    }
}
