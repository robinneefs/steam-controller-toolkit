//! Haptics check: plays each effect on the first controller that reports state and measures how
//! much the accelerometer shakes while it plays (rest the controller on a table).
//!
//!   cargo run -p sc-core --example haptics -- [--side N] [--identify]
//!
//! --side N    only test this side value (default: 0, 1 and 2)
//! --identify  side N buzzes N + 1 times, so you can tell which actuator each side value drives
//! --buzz N [tone|click|script|pulse|sweep]
//!             three short effects on side N, then exit. To tell sides apart, rest the controller
//!             on a table with a thumb on each trackpad: held by the grips, the whole shell buzzes

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use sc_core::hub::{self, HubOptions};
use sc_core::protocol::{self, Haptic, Report};
use sc_core::{Command, HubEvent};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sides: Vec<u8> = match args.iter().position(|a| a == "--side").and_then(|i| args.get(i + 1)) {
        Some(s) => vec![s.parse().expect("--side takes a number")],
        None => vec![0, 1, 2],
    };

    let (tx, rx) = mpsc::channel();
    let hub = hub::spawn(tx, HubOptions::default());

    println!("Waiting for a controller...");
    let slot = loop {
        match rx.recv() {
            Ok(HubEvent::Packet { slot, data }) if matches!(protocol::parse(&data), Some(Report::State(_))) => break slot,
            Ok(_) => {}
            Err(_) => return,
        }
    };
    // --buzz SIDE [KIND]: three short effects of one kind on one side value, then exit.
    if let Some(side) = args.iter().position(|a| a == "--buzz").and_then(|i| args.get(i + 1)) {
        let side: u8 = side.parse().expect("--buzz takes a side value");
        let kind = args.iter().position(|a| a == "--buzz").and_then(|i| args.get(i + 2)).map_or("tone", |s| s.as_str());
        let h = match kind {
            "click" => Haptic::Command { side, command: 2, gain_db: 0 },
            "script" => Haptic::Script { side, script_id: 1, gain_db: 0 },
            "pulse" => Haptic::Pulse { side, on_us: 2000, off_us: 2000, repeat: 50 },
            "sweep" => Haptic::Sweep { side, gain_db: 0, duration_ms: 300, start_hz: 50, end_hz: 400 },
            _ => Haptic::Tone { side, gain_db: 0, frequency: 180, duration_ms: 250, lfo_freq: 0, lfo_depth: 0 },
        };
        thread::sleep(Duration::from_millis(500));
        for _ in 0..3 {
            let (reply, r) = mpsc::channel();
            hub.send(&slot, Command::Haptic { haptic: h, reply }).unwrap();
            println!("{kind} side {side}: {:?}", r.recv_timeout(Duration::from_secs(2)));
            thread::sleep(Duration::from_millis(700));
        }
        return;
    }
    if args.iter().any(|a| a == "--identify") {
        println!("Using {slot}. Hold the controller; side N buzzes N + 1 times, starting in 8 s.");
        thread::sleep(Duration::from_secs(8));
        for side in 0..4u8 {
            println!("side {side}");
            for _ in 0..=side {
                let (reply, r) = mpsc::channel();
                let tone = Haptic::Tone { side, gain_db: 0, frequency: 180, duration_ms: 180, lfo_freq: 0, lfo_depth: 0 };
                hub.send(&slot, Command::Haptic { haptic: tone, reply }).unwrap();
                let _ = r.recv_timeout(Duration::from_secs(2));
                thread::sleep(Duration::from_millis(450));
            }
            thread::sleep(Duration::from_millis(1800));
        }
        return;
    }
    println!("Using {slot}. Keep the controller still.\n");
    // Let it settle and the settings report go out.
    shake(&rx, Duration::from_millis(1500));
    let rest = shake(&rx, Duration::from_millis(800));
    println!("{:<40} shake {rest:8.1}", "at rest");

    let mut tests: Vec<(String, Haptic)> = vec![
        ("rumble left 65535".into(), Haptic::Rumble { left: 65535, right: 0 }),
        ("rumble right 65535".into(), Haptic::Rumble { left: 0, right: 65535 }),
    ];
    for &side in &sides {
        tests.push((format!("side {side}: pulse 2ms x100"), Haptic::Pulse { side, on_us: 2000, off_us: 2000, repeat: 100 }));
        tests.push((format!("side {side}: command click"), Haptic::Command { side, command: 2, gain_db: 0 }));
        tests.push((format!("side {side}: command rumble"), Haptic::Command { side, command: 4, gain_db: 0 }));
        tests.push((format!("side {side}: tone 200 Hz 600 ms"), Haptic::Tone { side, gain_db: 0, frequency: 200, duration_ms: 600, lfo_freq: 0, lfo_depth: 0 }));
        tests.push((format!("side {side}: sweep 50-400 Hz 600 ms"), Haptic::Sweep { side, gain_db: 0, duration_ms: 600, start_hz: 50, end_hz: 400 }));
        tests.push((format!("side {side}: script 1"), Haptic::Script { side, script_id: 1, gain_db: 0 }));
    }

    for (name, h) in tests {
        let (reply, r) = mpsc::channel();
        hub.send(&slot, Command::Haptic { haptic: h, reply }).unwrap();
        let sent = r.recv_timeout(Duration::from_secs(2)).map_err(|e| e.to_string()).and_then(|r| r);
        let s = shake(&rx, Duration::from_millis(800));
        if h.is_rumble() {
            let (reply, _r) = mpsc::channel();
            let _ = hub.send(&slot, Command::Haptic { haptic: Haptic::Rumble { left: 0, right: 0 }, reply });
        }
        let note = match sent {
            Ok(()) => format!("{:.1}x rest", s / rest.max(1.0)),
            Err(e) => format!("write failed: {e}"),
        };
        println!("{name:<40} shake {s:8.1}  {note}");
        shake(&rx, Duration::from_millis(500));
    }
    thread::sleep(Duration::from_millis(100));
}

/// Standard deviation of the accelerometer magnitude over `dur`, in raw counts.
fn shake(rx: &mpsc::Receiver<HubEvent>, dur: Duration) -> f64 {
    let end = Instant::now() + dur;
    let mut v = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Ok(HubEvent::Packet { data, .. }) = rx.recv_timeout(left) {
            if let Some(Report::State(s)) = protocol::parse(&data) {
                let a = s.imu.accel.map(|c| c as f64);
                v.push((a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt());
            }
        }
    }
    if v.len() < 2 {
        return 0.0;
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}
