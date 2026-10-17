//! Background readers for every connected controller interface, with hot-plug.
//!
//! Each interface gets its own blocking reader thread: polling interfaces in turn with short
//! timeouts drops most reports on Windows, where a timeout lasts at least one ~15 ms tick.

use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use hidapi::HidApi;

use crate::device::{find_controller_interfaces, Controller};

const RESCAN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub enum HubEvent {
    /// A HID interface was opened. `slot` identifies it in later events.
    Opened { slot: String },
    /// A raw report arrived; `data[0]` is the report id. Parse with [`crate::protocol::parse`].
    Packet { slot: String, data: Vec<u8> },
    /// The interface failed (unplugged); it will be reopened if it comes back.
    Closed { slot: String, error: String },
}

pub struct HubOptions {
    /// Send the lizard-mode-off / IMU-on settings report. Disable to observe default behaviour.
    pub configure: bool,
}

impl Default for HubOptions {
    fn default() -> Self {
        Self { configure: true }
    }
}

/// Start the hub. Events are delivered on `tx`; the hub stops when the receiver is dropped.
pub fn spawn(tx: Sender<HubEvent>, opts: HubOptions) {
    thread::spawn(move || {
        let mut api = match HidApi::new() {
            Ok(api) => api,
            Err(e) => {
                let _ = tx.send(HubEvent::Closed { slot: "hidapi".into(), error: e.to_string() });
                return;
            }
        };
        let open: Arc<Mutex<HashSet<String>>> = Arc::default();
        loop {
            let _ = api.refresh_devices();
            for info in find_controller_interfaces(&api) {
                let path = info.path().to_string_lossy().into_owned();
                if open.lock().unwrap().contains(&path) {
                    continue;
                }
                let Ok(ctl) = Controller::open(&api, info) else { continue };
                let slot = format!("{:04x}:{}", info.product_id(), info.interface_number());
                if tx.send(HubEvent::Opened { slot: slot.clone() }).is_err() {
                    return;
                }
                open.lock().unwrap().insert(path.clone());
                let (tx, open) = (tx.clone(), open.clone());
                let configure = opts.configure;
                thread::spawn(move || {
                    let error = read_loop(ctl, &slot, &tx, configure);
                    open.lock().unwrap().remove(&path);
                    let _ = tx.send(HubEvent::Closed { slot, error });
                });
            }
            thread::sleep(RESCAN_INTERVAL);
        }
    });
}

fn read_loop(mut ctl: Controller, slot: &str, tx: &Sender<HubEvent>, configure: bool) -> String {
    let mut buf = [0u8; 64];
    loop {
        if configure {
            // Fails on empty dongle slots; harmless, so ignore it.
            let _ = ctl.keep_configured();
        }
        match ctl.read_raw(&mut buf, 500) {
            Ok(0) => {}
            Ok(n) => {
                let event = HubEvent::Packet { slot: slot.to_string(), data: buf[..n].to_vec() };
                if tx.send(event).is_err() {
                    return "hub stopped".into();
                }
            }
            Err(e) => return e.to_string(),
        }
    }
}
