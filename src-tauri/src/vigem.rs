//! Windows: a virtual Xbox 360 controller through the ViGEmBus driver, and installing that driver.
//!
//! Windows has no driverless way to add a game controller (its InputInjector gamepad is never
//! seen by XInput or GameInput; see `vendor/vigembus/README.md`). ViGEmBus 1.22.0's installer is
//! embedded and run on request; it asks for administrator permission itself.

use std::path::Path;
use std::sync::{Arc, Mutex};

use vigem_client::{Client, Error, TargetId, XButtons, XGamepad, Xbox360Wired};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::devices::{PadSupport, VirtualPad};
use crate::layout::PadState;

pub const INSTALLER_NAME: &str = "ViGEmBus_1.22.0_x64_x86_arm64.exe";
static INSTALLER: &[u8] = include_bytes!("../vendor/vigembus/ViGEmBus_1.22.0_x64_x86_arm64.exe");

pub fn support() -> PadSupport {
    match Client::connect() {
        Ok(_) => PadSupport::Ready,
        Err(Error::BusNotFound) => PadSupport::Missing { detail: "The ViGEmBus driver isn't installed".into() },
        Err(Error::BusVersionMismatch) => PadSupport::Missing { detail: "The installed ViGEmBus driver is too old".into() },
        Err(e) => PadSupport::Missing { detail: format!("Couldn't reach the ViGEmBus driver: {e}") },
    }
}

pub struct Pad {
    target: Xbox360Wired<Client>,
    rumble: Arc<Mutex<Option<(u8, u8)>>>,
    player: Option<u8>,
}

impl Pad {
    /// Plug in a virtual Xbox 360 controller. Takes a moment (Windows sets the device up).
    pub fn open() -> Result<Pad, String> {
        let client = Client::connect().map_err(|e| format!("Couldn't reach the ViGEmBus driver: {e}"))?;
        let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
        target.plugin().map_err(|e| format!("Couldn't plug in the virtual controller: {e}"))?;
        target.wait_ready().map_err(|e| format!("The virtual controller didn't get ready: {e}"))?;
        let rumble = Arc::new(Mutex::new(None));
        // Rumble from games arrives on a thread of its own; it ends when the target is unplugged.
        if let Ok(req) = target.request_notification() {
            let slot = rumble.clone();
            req.spawn_thread(move |_, n| *slot.lock().unwrap() = Some((n.large_motor, n.small_motor)));
        }
        let player = target.get_user_index().ok().map(|i| i as u8 + 1);
        Ok(Pad { target, rumble, player })
    }
}

impl VirtualPad for Pad {
    fn update(&mut self, s: &PadState) -> Result<(), String> {
        let g = XGamepad {
            buttons: XButtons { raw: s.buttons },
            left_trigger: s.lt,
            right_trigger: s.rt,
            thumb_lx: s.lx,
            thumb_ly: s.ly,
            thumb_rx: s.rx,
            thumb_ry: s.ry,
        };
        match self.target.update(&g) {
            Ok(()) => Ok(()),
            // Right after plugging in, Windows may not have finished; the next report retries.
            Err(Error::TargetNotReady) => Ok(()),
            Err(e) => Err(format!("The virtual controller stopped: {e}")),
        }
    }

    fn take_rumble(&mut self) -> Option<(u8, u8)> {
        self.rumble.lock().unwrap().take()
    }

    fn player(&mut self) -> Option<u8> {
        if self.player.is_none() {
            self.player = self.target.get_user_index().ok().map(|i| i as u8 + 1);
        }
        self.player
    }
}

/// Write the embedded installer to `dir` and run it, waiting until it closes. Windows asks the
/// user for administrator permission. Blocks: call from a thread of its own.
pub fn install(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't write the installer: {e}"))?;
    let file = dir.join(INSTALLER_NAME);
    std::fs::write(&file, INSTALLER).map_err(|e| format!("Couldn't write the installer: {e}"))?;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let path = wide(&file.to_string_lossy());
    let verb = wide("open");
    // SAFETY: the strings outlive the call; the process handle is waited on and closed.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = path.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        if ShellExecuteExW(&mut info) == 0 {
            let err = std::io::Error::last_os_error();
            return Err(if err.raw_os_error() == Some(ERROR_CANCELLED as i32) {
                "The installation was cancelled".into()
            } else {
                format!("Couldn't start the installer: {err}")
            });
        }
        if info.hProcess.is_null() {
            return Ok(());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 0u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        // 1602: the user cancelled the installer; 3010: done, a restart is recommended.
        match code {
            0 | 3010 => Ok(()),
            1602 => Err("The installation was cancelled".into()),
            c => Err(format!("The installer stopped with code {c}")),
        }
    }
}
