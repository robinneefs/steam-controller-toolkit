//! Raw-data probe: lists Valve HID devices, opens every controller slot and prints what arrives.
//!
//!   cargo run -p sc-core --example probe -- [--raw] [--no-config] [--seconds N]
//!
//! --raw        also hex-dump the first few reports of each report id
//! --no-config  do not send the lizard-mode-off / IMU-on settings report
//! --seconds N  exit after N seconds (default: run until Ctrl+C)

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sc_core::device::list_valve_devices;
use sc_core::hub::{self, HubOptions};
use sc_core::protocol::{self, buttons, Report};
use sc_core::{HidApi, HubEvent};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let raw = args.iter().any(|a| a == "--raw");
    let no_config = args.iter().any(|a| a == "--no-config");
    let seconds = args
        .iter()
        .position(|a| a == "--seconds")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok());

    println!("== Valve HID devices ==");
    let api = HidApi::new().expect("failed to initialise hidapi");
    for d in list_valve_devices(&api) {
        println!(
            "  pid={:04x} if={} usage_page={:04x} usage={:04x} product={:?}",
            d.product_id(),
            d.interface_number(),
            d.usage_page(),
            d.usage(),
            d.product_string().unwrap_or("?"),
        );
    }
    drop(api);

    let (tx, rx) = mpsc::channel();
    hub::spawn(tx, HubOptions { configure: !no_config });

    let start = Instant::now();
    let mut last_print = Instant::now() - Duration::from_secs(1);
    let mut seen: HashMap<u8, u32> = HashMap::new();
    let mut counts: HashMap<String, u64> = HashMap::new();

    println!("\nListening. Touch the pads, tilt the controller, press the back buttons...\n");
    while seconds.is_none_or(|s| start.elapsed() < Duration::from_secs(s)) {
        let (slot, data) = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(HubEvent::Packet { slot, data }) => (slot, data),
            Ok(HubEvent::Opened { slot }) => {
                println!("opened {slot}");
                counts.entry(slot).or_default();
                continue;
            }
            Ok(HubEvent::Closed { slot, error }) => {
                println!("closed {slot}: {error}");
                continue;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        *counts.entry(slot.clone()).or_default() += 1;

        let times_seen = seen.entry(data[0]).or_default();
        if *times_seen == 0 {
            println!("[{slot}] first report with id 0x{:02x} ({} bytes)", data[0], data.len());
        }
        if raw && *times_seen < 3 {
            println!("[{slot}] 0x{:02x}: {}", data[0], hex(&data));
        }
        *times_seen += 1;

        match protocol::parse(&data) {
            Some(Report::State(s)) if last_print.elapsed() >= Duration::from_millis(250) => {
                last_print = Instant::now();
                let held: Vec<&str> =
                    buttons::NAMES.iter().filter(|(m, _)| s.pressed(*m)).map(|(_, n)| *n).collect();
                println!(
                    "[{slot}] seq={:3} trig L{:6} R{:6} | LS {:6},{:6} RS {:6},{:6} | \
                     LPad {} {:6},{:6} p{:5} | RPad {} {:6},{:6} p{:5} | \
                     acc {:6},{:6},{:6} gyro {:6},{:6},{:6} | {:?}",
                    s.seq, s.trigger_left, s.trigger_right,
                    s.left_stick[0], s.left_stick[1], s.right_stick[0], s.right_stick[1],
                    if s.left_pad.touched { "T" } else { "-" }, s.left_pad.x, s.left_pad.y, s.left_pad.pressure,
                    if s.right_pad.touched { "T" } else { "-" }, s.right_pad.x, s.right_pad.y, s.right_pad.pressure,
                    s.imu.accel[0], s.imu.accel[1], s.imu.accel[2],
                    s.imu.gyro[0], s.imu.gyro[1], s.imu.gyro[2],
                    held,
                );
            }
            Some(Report::Battery(b)) => println!("[{slot}] battery: {b:?}"),
            Some(Report::Wireless(c)) => println!("[{slot}] wireless: {}", if c { "connected" } else { "disconnected" }),
            _ => {}
        }
    }

    let secs = start.elapsed().as_secs_f32();
    println!("\n== Summary after {secs:.1}s ==");
    let mut slots: Vec<_> = counts.into_iter().collect();
    slots.sort();
    for (slot, n) in slots {
        println!("  {slot}: {n} reports ({:.0}/s)", n as f32 / secs);
    }
    let mut ids: Vec<_> = seen.into_iter().collect();
    ids.sort();
    for (id, n) in ids {
        println!("  report id 0x{id:02x}: {n}");
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}
