//! What the system receives from the app: the virtual pad through XInput, and the injected keys
//! and mouse through low-level hooks (events marked with the app's `dwExtraInfo`, 0x5343). While a
//! step runs, the hooks swallow those events, so nothing really gets clicked or typed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Input::XboxController::{XInputGetState, XInputSetState, XINPUT_GAMEPAD, XINPUT_STATE, XINPUT_VIBRATION};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// The app's mark on what it injects (`EXTRA` in src-tauri/src/inject_win.rs).
const EXTRA: usize = 0x5343;

// ---------- XInput ----------

pub type XGamepad = XINPUT_GAMEPAD;

pub fn pad(i: u32) -> Option<XINPUT_GAMEPAD> {
    let mut s: XINPUT_STATE = unsafe { std::mem::zeroed() };
    (unsafe { XInputGetState(i, &mut s) } == 0).then_some(s.Gamepad)
}

pub fn pads() -> Vec<u32> {
    (0..4).filter(|&i| pad(i).is_some()).collect()
}

pub fn fmt_pad(g: &XINPUT_GAMEPAD) -> String {
    format!("buttons {:04x}, LT {} RT {}, L({}, {}) R({}, {})", g.wButtons, g.bLeftTrigger, g.bRightTrigger, g.sThumbLX, g.sThumbLY, g.sThumbRX, g.sThumbRY)
}

pub fn rumble(i: u32, left: u16, right: u16) {
    let v = XINPUT_VIBRATION { wLeftMotorSpeed: left, wRightMotorSpeed: right };
    unsafe { XInputSetState(i, &v) };
}

pub fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

// ---------- Hooks ----------

#[derive(Debug, Clone)]
pub enum Ev {
    Key { vk: u32, down: bool },
    Move(i32, i32),
    Button(&'static str, bool),
    Wheel(i32),
    HWheel(i32),
}

static TX: OnceLock<Mutex<Sender<Ev>>> = OnceLock::new();
static BLOCK: AtomicBool = AtomicBool::new(true);

fn push(e: Ev) {
    if let Some(tx) = TX.get() {
        let _ = tx.lock().unwrap().send(e);
    }
}

unsafe extern "system" fn kb(code: i32, w: usize, l: isize) -> isize {
    let k = &*(l as *const KBDLLHOOKSTRUCT);
    if code >= 0 && k.dwExtraInfo == EXTRA {
        push(Ev::Key { vk: k.vkCode, down: w as u32 == WM_KEYDOWN || w as u32 == WM_SYSKEYDOWN });
        if BLOCK.load(Ordering::Relaxed) {
            return 1;
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w, l)
}

unsafe extern "system" fn mouse(code: i32, w: usize, l: isize) -> isize {
    let m = &*(l as *const MSLLHOOKSTRUCT);
    if code >= 0 && m.dwExtraInfo == EXTRA {
        let high = (m.mouseData >> 16) as u16;
        let side = if high == 1 { "back" } else { "forward" };
        let e = match w as u32 {
            WM_MOUSEMOVE => Some(Ev::Move(m.pt.x, m.pt.y)),
            WM_LBUTTONDOWN => Some(Ev::Button("left", true)),
            WM_LBUTTONUP => Some(Ev::Button("left", false)),
            WM_RBUTTONDOWN => Some(Ev::Button("right", true)),
            WM_RBUTTONUP => Some(Ev::Button("right", false)),
            WM_MBUTTONDOWN => Some(Ev::Button("middle", true)),
            WM_MBUTTONUP => Some(Ev::Button("middle", false)),
            WM_XBUTTONDOWN => Some(Ev::Button(side, true)),
            WM_XBUTTONUP => Some(Ev::Button(side, false)),
            WM_MOUSEWHEEL => Some(Ev::Wheel(high as i16 as i32)),
            WM_MOUSEHWHEEL => Some(Ev::HWheel(high as i16 as i32)),
            _ => None,
        };
        if let Some(e) = e {
            push(e);
        }
        if BLOCK.load(Ordering::Relaxed) {
            return 1;
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w, l)
}

/// Installs the hooks on a thread of their own and checks they see a marked event.
pub fn start_hooks() -> Result<Receiver<Ev>, String> {
    let (tx, rx) = channel();
    TX.set(Mutex::new(tx)).map_err(|_| "hooks already running")?;
    std::thread::spawn(|| unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb), module, 0);
        let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0);
        if k.is_null() || m.is_null() {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
    });
    std::thread::sleep(Duration::from_millis(300));
    unsafe {
        let mi = MOUSEINPUT { dx: 0, dy: 0, mouseData: 120, dwFlags: MOUSEEVENTF_WHEEL, time: 0, dwExtraInfo: EXTRA };
        let input = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi } };
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
    rx.recv_timeout(Duration::from_secs(1)).map_err(|_| "the input hooks see nothing")?;
    Ok(rx)
}

/// Stop swallowing the app's events (at the end, and for the steps that need them to work).
pub fn set_block(on: bool) {
    BLOCK.store(on, Ordering::Relaxed);
}

fn vk_name(vk: u32) -> String {
    let named = match vk {
        0x0D => "Enter", 0x1B => "Esc", 0x08 => "Backspace", 0x20 => "Space", 0x09 => "Tab",
        0x25 => "Left", 0x26 => "Up", 0x27 => "Right", 0x28 => "Down",
        0x10 | 0xA0 => "Shift", 0xA1 => "RShift", 0x11 | 0xA2 => "Ctrl", 0xA3 => "RCtrl", 0x12 | 0xA4 => "Alt",
        0x5B => "Win", 0x5C => "RWin", 0xB3 => "PlayPause",
        _ => "",
    };
    if !named.is_empty() {
        named.into()
    } else if (0x30..=0x5A).contains(&vk) {
        char::from_u32(vk).unwrap().to_string()
    } else {
        format!("vk{vk:02x}")
    }
}

/// What arrived: keys as "+Ctrl +Z -Z -Ctrl", buttons as "+left -left", and the summed pointer
/// movement (swallowed moves never move the pointer, so each is the pointer plus its delta) and wheel.
pub struct Seen {
    pub keys: String,
    pub moved: (i64, i64),
    pub wheel: (i32, i32),
}

impl Seen {
    pub fn of(evs: &[Ev], pointer: (i32, i32)) -> Seen {
        let mut parts: Vec<String> = vec![];
        let (mut mx, mut my, mut v, mut h) = (0i64, 0i64, 0, 0);
        for e in evs {
            match e {
                Ev::Key { vk, down } => {
                    let s = format!("{}{}", if *down { "+" } else { "-" }, vk_name(*vk));
                    if parts.last() != Some(&s) {
                        parts.push(s);
                    }
                }
                Ev::Button(b, down) => parts.push(format!("{}{b}", if *down { "+" } else { "-" })),
                Ev::Move(x, y) => {
                    mx += (*x - pointer.0) as i64;
                    my += (*y - pointer.1) as i64;
                }
                Ev::Wheel(d) => v += d,
                Ev::HWheel(d) => h += d,
            }
        }
        Seen { keys: parts.join(" "), moved: (mx, my), wheel: (v, h) }
    }

    pub fn describe(&self) -> String {
        let mut s = vec![];
        if !self.keys.is_empty() {
            s.push(self.keys.clone());
        }
        if self.moved != (0, 0) {
            s.push(format!("pointer moved ({}, {})", self.moved.0, self.moved.1));
        }
        if self.wheel != (0, 0) {
            s.push(format!("wheel {} up, {} right", self.wheel.0, self.wheel.1));
        }
        if s.is_empty() { "nothing".into() } else { s.join(", ") }
    }
}
