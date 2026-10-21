//! Windows: keyboard and mouse through SendInput, and spotting a full-screen app in front.
//!
//! Keys go out as scan codes of their position on a US keyboard (the names in `layout::KEYS`
//! are positions, as the tabs capture them with `KeyboardEvent.code`): games that read key
//! positions get the key the user picked, and apps get the character that key types in the
//! user's own layout. Media keys have no scan code and go out as virtual keys, and so do the
//! letters and digits of shortcuts (`by_char` in `layout.rs`), so Ctrl+Z is undo on any layout.
//!
//! Windows doesn't let a normal app send input to one running as administrator (UIPI); those
//! windows simply don't react.

use std::mem::size_of;

use windows_sys::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, XBUTTON1, XBUTTON2,
};

use crate::devices::{Foreground, KeyMouse};
use crate::layout::{Event, MouseButton, KEYS};

/// Marks our own events (`dwExtraInfo`), "SC".
const EXTRA: usize = 0x5343;

/// Scan code (with 0xE000 for extended keys) or, for keys without one, 0x10000 | virtual key.
fn code(name: &str) -> Option<u32> {
    const VK: u32 = 0x10000;
    Some(match name {
        "Ctrl" => 0x1D,
        "Shift" => 0x2A,
        "Alt" => 0x38,
        "Win" => 0xE05B,
        "RCtrl" => 0xE01D,
        "RShift" => 0x36,
        "RAlt" => 0xE038,
        "A" => 0x1E, "B" => 0x30, "C" => 0x2E, "D" => 0x20, "E" => 0x12, "F" => 0x21, "G" => 0x22,
        "H" => 0x23, "I" => 0x17, "J" => 0x24, "K" => 0x25, "L" => 0x26, "M" => 0x32, "N" => 0x31,
        "O" => 0x18, "P" => 0x19, "Q" => 0x10, "R" => 0x13, "S" => 0x1F, "T" => 0x14, "U" => 0x16,
        "V" => 0x2F, "W" => 0x11, "X" => 0x2D, "Y" => 0x15, "Z" => 0x2C,
        "1" => 0x02, "2" => 0x03, "3" => 0x04, "4" => 0x05, "5" => 0x06, "6" => 0x07, "7" => 0x08,
        "8" => 0x09, "9" => 0x0A, "0" => 0x0B,
        "F1" => 0x3B, "F2" => 0x3C, "F3" => 0x3D, "F4" => 0x3E, "F5" => 0x3F, "F6" => 0x40,
        "F7" => 0x41, "F8" => 0x42, "F9" => 0x43, "F10" => 0x44, "F11" => 0x57, "F12" => 0x58,
        "F13" => 0x64, "F14" => 0x65, "F15" => 0x66, "F16" => 0x67, "F17" => 0x68, "F18" => 0x69,
        "F19" => 0x6A, "F20" => 0x6B, "F21" => 0x6C, "F22" => 0x6D, "F23" => 0x6E, "F24" => 0x76,
        "Escape" => 0x01,
        "Tab" => 0x0F,
        "CapsLock" => 0x3A,
        "Space" => 0x39,
        "Enter" => 0x1C,
        "Backspace" => 0x0E,
        "Delete" => 0xE053,
        "Insert" => 0xE052,
        "Home" => 0xE047,
        "End" => 0xE04F,
        "PageUp" => 0xE049,
        "PageDown" => 0xE051,
        "Up" => 0xE048,
        "Down" => 0xE050,
        "Left" => 0xE04B,
        "Right" => 0xE04D,
        "Minus" => 0x0C,
        "Equal" => 0x0D,
        "BracketLeft" => 0x1A,
        "BracketRight" => 0x1B,
        "Backslash" => 0x2B,
        "Semicolon" => 0x27,
        "Quote" => 0x28,
        "Backquote" => 0x29,
        "Comma" => 0x33,
        "Period" => 0x34,
        "Slash" => 0x35,
        "Num0" => 0x52, "Num1" => 0x4F, "Num2" => 0x50, "Num3" => 0x51, "Num4" => 0x4B,
        "Num5" => 0x4C, "Num6" => 0x4D, "Num7" => 0x47, "Num8" => 0x48, "Num9" => 0x49,
        "NumAdd" => 0x4E,
        "NumSubtract" => 0x4A,
        "NumMultiply" => 0x37,
        "NumDivide" => 0xE035,
        "NumDecimal" => 0x53,
        "NumEnter" => 0xE01C,
        "NumLock" => 0x45,
        "PrintScreen" => 0xE037,
        "ScrollLock" => 0x46,
        "Pause" => VK | VK_PAUSE as u32,
        "ContextMenu" => 0xE05D,
        "VolumeUp" => VK | VK_VOLUME_UP as u32,
        "VolumeDown" => VK | VK_VOLUME_DOWN as u32,
        "VolumeMute" => VK | VK_VOLUME_MUTE as u32,
        "MediaPlayPause" => VK | VK_MEDIA_PLAY_PAUSE as u32,
        "MediaNext" => VK | VK_MEDIA_NEXT_TRACK as u32,
        "MediaPrevious" => VK | VK_MEDIA_PREV_TRACK as u32,
        "MediaStop" => VK | VK_MEDIA_STOP as u32,
        "BrowserBack" => VK | VK_BROWSER_BACK as u32,
        "BrowserForward" => VK | VK_BROWSER_FORWARD as u32,
        _ => return None,
    })
}

fn key_input(name: &str, down: bool, by_char: bool) -> Option<INPUT> {
    let c = code(name)?;
    let up = if down { 0 } else { KEYEVENTF_KEYUP };
    // A letter or digit by character: its virtual key ('A'..'Z' and '0'..'9' are their own codes),
    // with the scan code the user's layout has for it.
    let ki = if by_char && name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric() {
        let vk = name.as_bytes()[0] as u16;
        // SAFETY: a plain lookup in the current keyboard layout.
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
        KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: up, time: 0, dwExtraInfo: EXTRA }
    } else if c & 0x10000 != 0 {
        KEYBDINPUT { wVk: (c & 0xFFFF) as u16, wScan: 0, dwFlags: up, time: 0, dwExtraInfo: EXTRA }
    } else {
        let ext = if c & 0xE000 == 0xE000 { KEYEVENTF_EXTENDEDKEY } else { 0 };
        KEYBDINPUT { wVk: 0, wScan: (c & 0xFF) as u16, dwFlags: KEYEVENTF_SCANCODE | ext | up, time: 0, dwExtraInfo: EXTRA }
    };
    Some(INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki } })
}

fn mouse_input(dx: i32, dy: i32, data: i32, flags: u32) -> INPUT {
    let mi = MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: EXTRA };
    INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi } }
}

pub struct Injector;

impl KeyMouse for Injector {
    fn send(&mut self, events: &[Event]) -> Result<(), String> {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(events.len() + 1);
        for e in events {
            match *e {
                Event::Key { key, down, by_char } => inputs.extend(key_input(KEYS[key as usize], down, by_char)),
                Event::Button { button, down } => {
                    let (flags, data) = match (button, down) {
                        (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                        (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
                        (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                        (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
                        (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                        (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                        (MouseButton::Back, true) => (MOUSEEVENTF_XDOWN, XBUTTON1 as i32),
                        (MouseButton::Back, false) => (MOUSEEVENTF_XUP, XBUTTON1 as i32),
                        (MouseButton::Forward, true) => (MOUSEEVENTF_XDOWN, XBUTTON2 as i32),
                        (MouseButton::Forward, false) => (MOUSEEVENTF_XUP, XBUTTON2 as i32),
                    };
                    inputs.push(mouse_input(0, 0, data, flags));
                }
                Event::Move { dx, dy } => inputs.push(mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE)),
                Event::Scroll { v, h } => {
                    if v != 0 {
                        inputs.push(mouse_input(0, 0, v, MOUSEEVENTF_WHEEL));
                    }
                    if h != 0 {
                        inputs.push(mouse_input(0, 0, h, MOUSEEVENTF_HWHEEL));
                    }
                }
            }
        }
        if inputs.is_empty() {
            return Ok(());
        }
        // SAFETY: a slice of initialised INPUTs and its length.
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            // Usually a secure screen (lock screen, UAC prompt) is in front.
            return Err(format!("Windows blocked the input ({})", std::io::Error::last_os_error()));
        }
        Ok(())
    }
}

/// Window classes of the desktop and taskbar, which fill the screen but aren't apps.
const SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

pub fn foreground() -> Option<Foreground> {
    // SAFETY: plain Win32 queries on the foreground window, with buffers of the sizes passed.
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_null() || IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0 {
            return None;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
        if SHELL_CLASSES.contains(&class.as_str()) {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == GetCurrentProcessId() {
            return None;
        }
        let mut rect: RECT = std::mem::zeroed();
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        let fullscreen = GetWindowRect(hwnd, &mut rect) != 0
            && GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) != 0
            && {
                let m = info.rcMonitor;
                rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
            };
        // A program we can't name only matters to auto switch.
        let app = process_name(pid).or_else(|| fullscreen.then(|| "Full-screen app".into()))?;
        Some(Foreground { id: hwnd as isize, app, fullscreen })
    }
}

fn process_name(pid: u32) -> Option<String> {
    // SAFETY: the handle is checked and closed; the buffer's size is passed.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_a_code() {
        for k in KEYS {
            assert!(code(k).is_some(), "{k}");
        }
    }
}
