use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use sc_core::hub::{self, HubOptions};
use sc_core::protocol::{self, BatteryStatus, Report};
use sc_core::{ControllerState, HubEvent};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// How often the UI gets a fresh snapshot (~60 fps). The controller itself reports at ~250 Hz.
const EMIT_INTERVAL: Duration = Duration::from_millis(16);

/// Everything the UI needs, sent as the `controller` event.
#[derive(Debug, Clone, Default, Serialize)]
struct Snapshot {
    /// HID interface the state came from, e.g. "1304:2" (dongle slot 2).
    slot: Option<String>,
    state: Option<ControllerState>,
    battery: Option<BatteryStatus>,
    /// Input reports received per second over the last second.
    report_rate: u32,
    /// Report ids seen that the parser does not decode yet.
    unknown_reports: Vec<u8>,
}

fn forward_events(app: AppHandle) {
    let (tx, rx) = mpsc::channel();
    hub::spawn(tx, HubOptions::default());

    let mut snap = Snapshot::default();
    let mut last_emit = Instant::now();
    let mut rate_window = Instant::now();
    let mut rate_count = 0u32;
    loop {
        match rx.recv_timeout(EMIT_INTERVAL) {
            Ok(HubEvent::Packet { slot, data }) => match protocol::parse(&data) {
                Some(Report::State(s)) => {
                    snap.slot = Some(slot);
                    snap.state = Some(s);
                    rate_count += 1;
                }
                Some(Report::Battery(b)) => snap.battery = Some(b),
                Some(Report::Wireless(false)) if snap.slot.as_deref() == Some(slot.as_str()) => {
                    snap.slot = None;
                    snap.state = None;
                }
                Some(Report::Other(id)) if !snap.unknown_reports.contains(&id) => snap.unknown_reports.push(id),
                _ => {}
            },
            Ok(HubEvent::Closed { slot, .. }) if snap.slot.as_deref() == Some(slot.as_str()) => {
                snap.slot = None;
                snap.state = None;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if rate_window.elapsed() >= Duration::from_secs(1) {
            snap.report_rate = rate_count;            rate_count = 0;
            rate_window = Instant::now();
        }
        if last_emit.elapsed() >= EMIT_INTERVAL {
            last_emit = Instant::now();
            let _ = app.emit("controller", &snap);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            thread::spawn(move || forward_events(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
