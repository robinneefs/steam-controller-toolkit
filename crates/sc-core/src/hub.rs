//! Background readers for every connected controller interface, with hot-plug.
//!
//! Each interface gets its own blocking reader thread: polling interfaces in turn with short
//! timeouts drops most reports on Windows, where a timeout lasts at least one ~15 ms tick.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::device::{find_controller_interfaces, rescan, valve_api, Controller};
use crate::protocol;

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

/// Something to do on one interface. Its reader thread runs it between reads.
#[derive(Debug)]
pub enum Command {
    /// Ask the controller to power off. `reply` gets the HID error, if any.
    TurnOff { reply: Sender<Result<(), String>> },
}

/// Handle for sending [`Command`]s to the interfaces the hub has open.
#[derive(Clone, Default)]
pub struct Hub {
    commands: Arc<Mutex<HashMap<String, Sender<Command>>>>,
}

impl Hub {
    /// Queue `cmd` for the interface `slot` (as in [`HubEvent`]).
    pub fn send(&self, slot: &str, cmd: Command) -> Result<(), String> {
        let commands = self.commands.lock().unwrap();
        let tx = commands.get(slot).ok_or_else(|| format!("{slot} is not connected"))?;
        tx.send(cmd).map_err(|_| format!("{slot} is not connected"))
    }
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
/// The returned [`Hub`] sends commands to open interfaces.
pub fn spawn(tx: Sender<HubEvent>, opts: HubOptions) -> Hub {
    let hub = Hub::default();
    let commands = hub.commands.clone();
    thread::spawn(move || {
        let mut api = match valve_api() {
            Ok(api) => api,
            Err(e) => {
                let _ = tx.send(HubEvent::Closed { slot: "hidapi".into(), error: e.to_string() });
                return;
            }
        };
        let open: Arc<Mutex<HashSet<String>>> = Arc::default();
        loop {
            let _ = rescan(&mut api);
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
                let (cmd_tx, cmd_rx) = mpsc::channel();
                commands.lock().unwrap().insert(slot.clone(), cmd_tx);
                let (tx, open, commands) = (tx.clone(), open.clone(), commands.clone());
                let configure = opts.configure;
                thread::spawn(move || {
                    let error = read_loop(ctl, &slot, &tx, &cmd_rx, configure);
                    open.lock().unwrap().remove(&path);
                    commands.lock().unwrap().remove(&slot);
                    let _ = tx.send(HubEvent::Closed { slot, error });
                });
            }
            thread::sleep(RESCAN_INTERVAL);
        }
    });
    hub
}

fn read_loop(mut ctl: Controller, slot: &str, tx: &Sender<HubEvent>, commands: &Receiver<Command>, configure: bool) -> String {
    let mut buf = [0u8; 64];
    // Only configure a slot once a controller in it has sent state, like SDL does. Sending
    // settings to empty dongle slots fails every time and may wedge the dongle.
    let mut active = false;
    loop {
        if configure && active {
            let _ = ctl.keep_configured();
        }
        // Commands wait at most one read: ~4 ms while a controller reports, 500 ms when idle.
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                Command::TurnOff { reply } => {
                    let _ = reply.send(ctl.turn_off().map_err(|e| e.to_string()));
                }
            }
        }
        match ctl.read_raw(&mut buf, 500) {
            Ok(0) => {}
            Ok(n) => {
                match buf[0] {
                    protocol::REPORT_STATE | protocol::REPORT_STATE_NO_QUAT | protocol::REPORT_STATE_TIMESTAMP => {
                        active = true
                    }
                    protocol::REPORT_WIRELESS_STATUS | protocol::REPORT_WIRELESS_STATUS_X => active = buf[1] == 2,
                    _ => {}
                }
                let event = HubEvent::Packet { slot: slot.to_string(), data: buf[..n].to_vec() };
                if tx.send(event).is_err() {
                    return "hub stopped".into();
                }
            }
            Err(e) => return e.to_string(),
        }
    }
}
