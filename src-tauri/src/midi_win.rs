//! Windows: our own "Steam Controller" MIDI port, made with Windows MIDI Services (app-to-app
//! MIDI, `MidiVirtualDeviceManager`). DAWs see it as a normal MIDI input, also through the old
//! WinMM API, so users don't need loopMIDI or a loopback port.
//!
//! It needs Windows 11 25H2 or later with the MIDI service. The API is a preview: Windows ships
//! it from late November 2026; until then we carry the DLL (`vendor/windows-midi-services`) and
//! load it only where Windows has none. Microsoft's go-live terms for that DLL: never use it
//! below 25H2, keep it up to date, tell users it's a preview, drop it once Windows has it.
//!
//! [`Support::Preview`] (our DLL, today's service): verified on 25H2 in October 2026, the service
//! makes one virtual device per service run; creating a second one hangs until `midisrv` restarts,
//! whether or not the first was closed. So the UI offers it only as an opt-in "experimental"
//! output, the app makes the device at most once per run and keeps it until it exits, and every
//! call into the service runs on a worker thread with timeouts: a stuck service never freezes
//! the app. [`Support::System`] (API in Windows, with the fixed service) is a normal output.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use windows_core::{Interface, HSTRING};

use crate::midi2_bindings::Windows::Devices::Midi2::Enumeration::{
    MidiDeclaredEndpointInfo, MidiFunctionBlock, MidiFunctionBlockDirection, MidiFunctionBlockRepresentsMidi10Connection,
};
use crate::midi2_bindings::Windows::Devices::Midi2::Transports::Virtual::{
    IMidiVirtualDeviceManagerStatics, MidiVirtualDeviceCreationConfig, MidiVirtualDeviceManager,
};
use crate::midi2_bindings::Windows::Devices::Midi2::{
    IMidiEndpointMessageProcessingPlugin, MidiApi, MidiGroup, MidiMessageProcessingPluginAddResult, MidiSession,
};

/// Version of the bundled API files; the DLL is extracted to a folder of this name.
pub const PREVIEW: &str = "0.99.92-preview.11";
/// Port name, as DAWs show it. MIDI 1.0 port names must stay under 32 characters.
pub const PORT_NAME: &str = "Steam Controller";
const PRODUCT_ID: &str = "steam-controller-toolkit";
/// Windows 11 25H2. 24H2 has an older service that never gets the API, so it must not be used.
const MIN_BUILD: u32 = 26200;
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const NOT_RESPONDING: &str = "The Windows MIDI service isn't responding. Until a Windows update fixes it, it makes \
     this port once per Windows restart: restart the PC, or pick a loopback port";

/// Where the API comes from, which says whether the service has Microsoft's November fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Windows' own copy: the released service. A normal output.
    System,
    /// Our bundled preview copy on today's service: one port per Windows restart. Opt-in.
    Preview,
}

#[cfg(target_arch = "x86_64")]
const BUNDLED: Option<(&[u8], &[u8])> = Some((
    include_bytes!("../vendor/windows-midi-services/x64/Windows.Devices.Midi2.dll"),
    include_bytes!("../vendor/windows-midi-services/x64/Windows.Devices.Midi2.pri"),
));
#[cfg(not(target_arch = "x86_64"))]
const BUNDLED: Option<(&[u8], &[u8])> = None;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static AVAILABLE: OnceLock<Result<Support, String>> = OnceLock::new();
/// The port's worker, once made. Kept for the life of the app (see the module docs).
static PORT: Mutex<Option<mpsc::Sender<u32>>> = Mutex::new(None);
/// A call into the service timed out once; don't pile up more stuck threads.
static STUCK: AtomicBool = AtomicBool::new(false);

/// Where to extract the bundled DLL if Windows has none. Call once at startup.
pub fn init(app_data: PathBuf) {
    let _ = DATA_DIR.set(app_data);
}

/// Whether this PC can have our virtual port and how, else why not (for the UI). The first
/// call checks (up to a few seconds, on another thread); later calls return the same answer.
pub fn available() -> Result<Support, String> {
    if STUCK.load(Ordering::Relaxed) {
        return Err(NOT_RESPONDING.into());
    }
    AVAILABLE
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(check());
            });
            rx.recv_timeout(CHECK_TIMEOUT).unwrap_or_else(|_| {
                STUCK.store(true, Ordering::Relaxed);
                Err(NOT_RESPONDING.into())
            })
        })
        .clone()
}

/// A call into the service timed out: today's preview service after its one port per restart.
pub fn stuck() -> bool {
    STUCK.load(Ordering::Relaxed)
}

fn check() -> Result<Support, String> {
    let build = windows_build();
    if build < MIN_BUILD {
        return Err("Needs Windows 11 version 25H2 or later".into());
    }
    // Windows' own copy of the API wins; ours is only loaded where there is none.
    if windows_core::factory::<MidiVirtualDeviceManager, IMidiVirtualDeviceManagerStatics>().is_err() {
        load_bundled()?;
        windows_core::factory::<MidiVirtualDeviceManager, IMidiVirtualDeviceManagerStatics>()
            .map_err(|e| format!("Windows MIDI Services couldn't be loaded ({})", e.message()))?;
    }
    // False when the PC is set to the old MIDI stack ("Legacy API mode") or the service is off.
    if !MidiApi::EnsureServiceAvailable() {
        return Err("Windows MIDI Services is turned off on this PC".into());
    }
    if !MidiVirtualDeviceManager::IsTransportAvailable() {
        return Err("This PC's Windows MIDI Services has no app-to-app MIDI".into());
    }
    Ok(if MidiApi::IsSystemProvided() { Support::System } else { Support::Preview })
}

/// The real Windows build number (GetVersionEx lies to unmanifested callers).
fn windows_build() -> u32 {
    #[repr(C)]
    struct OsVersionInfoW {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        csd: [u16; 128],
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
    }
    let mut info = OsVersionInfoW { size: size_of::<OsVersionInfoW>() as u32, major: 0, minor: 0, build: 0, platform: 0, csd: [0; 128] };
    if unsafe { RtlGetVersion(&mut info) } == 0 { info.build } else { 0 }
}

/// Write the bundled DLL and its resources to the app data folder and load it, so activation
/// (which looks for `Windows.Devices.Midi2.dll` by name) finds this already-loaded copy.
fn load_bundled() -> Result<(), String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
    }
    let Some((dll, pri)) = BUNDLED else {
        return Err("Windows MIDI Services isn't available for this processor yet".into());
    };
    let dir = DATA_DIR.get().ok_or("No app data folder")?.join("windows-midi-services").join(PREVIEW);
    let path = dir.join("Windows.Devices.Midi2.dll");
    let fail = |e: std::io::Error| format!("Couldn't set up Windows MIDI Services: {e}");
    std::fs::create_dir_all(&dir).map_err(fail)?;
    for (name, bytes) in [("Windows.Devices.Midi2.dll", dll), ("Windows.Devices.Midi2.pri", pri)] {
        let file = dir.join(name);
        if std::fs::metadata(&file).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
            std::fs::write(&file, bytes).map_err(fail)?;
        }
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide_nul();
    if unsafe { LoadLibraryW(wide.as_ptr()) }.is_null() {
        return Err(format!("Couldn't load Windows MIDI Services ({})", std::io::Error::last_os_error()));
    }
    Ok(())
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}
impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(Some(0)).collect()
    }
}

/// A handle to our virtual MIDI port. Messages go to a worker thread that owns the WinRT
/// objects. The port lives until the app exits, so dropping a handle only stops sending.
pub struct VirtualPort {
    tx: mpsc::Sender<u32>,
}

impl VirtualPort {
    /// Make the port, or reuse it if this run already made it. Blocks up to 15 seconds the first
    /// time (opening took 2.5 to 4.5 s right after boot); call it off the event loop.
    pub fn open() -> Result<VirtualPort, String> {
        available()?;
        let mut port = PORT.lock().unwrap();
        if let Some(tx) = &*port {
            return Ok(VirtualPort { tx: tx.clone() });
        }
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        thread::Builder::new()
            .name("midi-virtual-port".into())
            .spawn(move || worker(rx, ready_tx))
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(OPEN_TIMEOUT) {
            Ok(result) => {
                result?;
                *port = Some(tx.clone());
                Ok(VirtualPort { tx })
            }
            Err(_) => {
                STUCK.store(true, Ordering::Relaxed);
                Err(NOT_RESPONDING.into())
            }
        }
    }

    /// Send one MIDI 1.0 channel message (2 or 3 bytes).
    pub fn send(&self, msg: &[u8]) -> Result<(), String> {
        self.tx.send(ump(msg)).map_err(|_| "The Steam Controller port closed".to_string())
    }
}

/// A MIDI 1.0 channel voice message as a Universal MIDI Packet (message type 2, group 0).
fn ump(msg: &[u8]) -> u32 {
    let byte = |i: usize| msg.get(i).copied().unwrap_or(0) as u32;
    0x2000_0000 | byte(0) << 16 | byte(1) << 8 | byte(2)
}

fn worker(rx: mpsc::Receiver<u32>, ready: mpsc::Sender<Result<(), String>>) {
    let opened = (|| -> windows_core::Result<_> {
        let info = MidiDeclaredEndpointInfo::new()?;
        info.SetName(&HSTRING::from(PORT_NAME));
        info.SetProductInstanceId(&HSTRING::from(PRODUCT_ID));
        info.SetSupportsMidi10Protocol(true);
        info.SetSupportsMidi20Protocol(false);
        info.SetHasStaticFunctionBlocks(true);
        info.SetSpecificationVersionMajor(1);
        info.SetSpecificationVersionMinor(1);
        let config = MidiVirtualDeviceCreationConfig::CreateInstance(
            &HSTRING::from(PORT_NAME),
            &HSTRING::from("The 2026 Steam Controller as a MIDI controller"),
            &HSTRING::from("Steam Controller Toolkit"),
            &info,
        )?;
        // One block that only sends, on group 1: a single MIDI 1.0 input port for DAWs.
        let block = MidiFunctionBlock::new()?;
        block.SetNumber(0);
        block.SetIsActive(true);
        block.SetName(&HSTRING::from("Out"));
        block.SetFirstGroup(&MidiGroup::CreateInstance(0)?);
        block.SetGroupCount(1);
        block.SetDirection(MidiFunctionBlockDirection::BlockOutput);
        block.SetRepresentsMidi10Connection(MidiFunctionBlockRepresentsMidi10Connection::YesBandwidthUnrestricted);
        config.FunctionBlocks().ok_or_else(windows_core::Error::empty)?.Append(&block)?;
        let device = MidiVirtualDeviceManager::CreateVirtualDevice(&config).ok_or_else(windows_core::Error::empty)?;
        let session = MidiSession::Create(&HSTRING::from("Steam Controller Toolkit")).ok_or_else(windows_core::Error::empty)?;
        let conn = session.CreateEndpointConnection(&device.DeviceEndpointDeviceId()).ok_or_else(windows_core::Error::empty)?;
        if conn.AddMessageProcessingPlugin(&device.cast::<IMidiEndpointMessageProcessingPlugin>()?)
            != MidiMessageProcessingPluginAddResult::Succeeded
            || !conn.Open()
        {
            return Err(windows_core::Error::empty());
        }
        Ok((session, conn, device))
    })();
    // Held until the app exits: closing doesn't help the preview service, and a later port
    // couldn't be made anyway (see the module docs).
    let (_session, conn, _device) = match opened {
        Ok(objects) => objects,
        Err(e) => {
            let detail = if e.code().is_ok() { String::new() } else { format!(" ({})", e.message()) };
            let _ = ready.send(Err(format!("Couldn't create the {PORT_NAME} port{detail}")));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    // Timestamp 0 sends immediately. `PORT` keeps a sender, so this runs until the app exits.
    while let Ok(word) = rx.recv() {
        conn.SendSingleMessageWords(0, word);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On a real PC: makes the port, checks DAWs (WinMM) can see it and receive from it. On the
    /// preview service this works once per `midisrv` run; restart the service to run it again.
    /// `cargo test -p steam-controller-app --lib virtual_port -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn virtual_port_reaches_winmm() {
        init(std::env::temp_dir().join("sc-midi-win-test"));
        let started = std::time::Instant::now();
        let port = VirtualPort::open();
        println!("open: {:?} after {:?}", port.as_ref().map(|_| "ok"), started.elapsed());
        let port = port.unwrap();
        thread::sleep(Duration::from_millis(500));
        let input = midir::MidiInput::new("sc-test").unwrap();
        let ours = input.ports().into_iter().find(|p| input.port_name(p).is_ok_and(|n| n == PORT_NAME));
        let ours = ours.expect("no WinMM input named Steam Controller");
        let (tx, rx) = mpsc::channel();
        let listen = input.connect(&ours, "sc-test", move |_, m, _| { let _ = tx.send(m.to_vec()); }, ()).unwrap();
        thread::sleep(Duration::from_millis(200));
        port.send(&[0xB0, 21, 64]).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), vec![0xB0, 21, 64]);
        listen.close();
        // A second handle reuses the same port instead of making another.
        assert!(VirtualPort::open().is_ok());
        drop(port);
    }

    #[test]
    fn midi1_messages_as_ump() {
        assert_eq!(ump(&[0xB0, 21, 64]), 0x20B0_1540);
        assert_eq!(ump(&[0xC9, 5]), 0x20C9_0500);
    }
}
