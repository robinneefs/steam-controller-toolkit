//! The system side of the Keyboard & mouse and Gamepad outputs, one facade over each platform:
//! - Windows: SendInput for keyboard and mouse (`inject_win.rs`), a virtual Xbox 360 controller
//!   through the ViGEmBus driver (`vigem.rs`), which the Gamepad tab can install.
//! - Linux: uinput devices for both (`uinput.rs`); needs write access to /dev/uinput (udev rule).
//! - macOS: not yet.

use crate::layout::{Event, PadState};

/// Sends keyboard and mouse events to the system.
pub trait KeyMouse: Send {
    fn send(&mut self, events: &[Event]) -> Result<(), String>;
}

/// A virtual game controller.
pub trait VirtualPad: Send {
    fn update(&mut self, state: &PadState) -> Result<(), String>;
    /// The latest rumble the game asked for (large, small motor, 0..255), if it changed.
    fn take_rumble(&mut self) -> Option<(u8, u8)>;
    /// Player number the system gave the pad (1..4), if it says.
    fn player(&mut self) -> Option<u8>;
}

/// The window in front (not ours, not the desktop or taskbar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    /// Stable while that window lives.
    pub id: isize,
    /// The program's file name.
    pub app: String,
    /// It fills its monitor (a full-screen game).
    pub fullscreen: bool,
}

/// Whether a virtual pad can be made here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PadSupport {
    Ready,
    /// The driver (Windows) or access to /dev/uinput (Linux) is missing; the Gamepad tab explains.
    Missing { detail: String },
    /// Not on this system at all.
    #[cfg_attr(any(windows, target_os = "linux"), allow(dead_code))]
    Unsupported,
}

#[cfg(windows)]
pub fn open_keymouse() -> Result<Box<dyn KeyMouse>, String> {
    Ok(Box::new(crate::inject_win::Injector))
}

#[cfg(windows)]
pub fn pad_support() -> PadSupport {
    crate::vigem::support()
}

#[cfg(windows)]
pub fn open_pad() -> Result<Box<dyn VirtualPad>, String> {
    Ok(Box::new(crate::vigem::Pad::open()?))
}

#[cfg(windows)]
pub fn foreground() -> Option<Foreground> {
    crate::inject_win::foreground()
}

#[cfg(target_os = "linux")]
pub fn open_keymouse() -> Result<Box<dyn KeyMouse>, String> {
    Ok(Box::new(crate::uinput::KeyMouse::open()?))
}

#[cfg(target_os = "linux")]
pub fn pad_support() -> PadSupport {
    crate::uinput::support()
}

#[cfg(target_os = "linux")]
pub fn open_pad() -> Result<Box<dyn VirtualPad>, String> {
    Ok(Box::new(crate::uinput::Pad::open()?))
}

#[cfg(target_os = "linux")]
pub fn foreground() -> Option<Foreground> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn open_keymouse() -> Result<Box<dyn KeyMouse>, String> {
    Err("Keyboard & mouse output isn't available on this system yet".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn pad_support() -> PadSupport {
    PadSupport::Unsupported
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn open_pad() -> Result<Box<dyn VirtualPad>, String> {
    Err("Gamepad output isn't available on this system".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn foreground() -> Option<Foreground> {
    None
}

/// Keyboard & mouse output works on this system.
pub const KEYMOUSE_SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));
/// [`foreground`] says which program is in front (auto switch, per-program layouts).
pub const FOREGROUND_SUPPORTED: bool = cfg!(windows);
