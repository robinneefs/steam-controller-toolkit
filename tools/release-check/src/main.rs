//! Release checks for Steam Controller Toolkit on Windows: a guided run through the outputs with
//! the real controller, about ten minutes. See README.md.

#[cfg(windows)]
mod cdp;
#[cfg(windows)]
mod input;

#[cfg(not(windows))]
fn main() {
    eprintln!("The release checks run on Windows only.");
}

#[cfg(windows)]
fn main() {
    std::process::exit(match checks::run() {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("\nStopped: {e}");
            2
        }
    });
}

#[cfg(windows)]
mod checks {
    use std::fs::File;
    use std::io::{BufRead, Write};
    use std::net::UdpSocket;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};

    use crate::cdp::{self, Page};
    use crate::input::{self, Ev, Seen};

    /// XInput button bits (`XINPUT_GAMEPAD_*`).
    const PAD_BUTTONS: &[(&str, u16)] = &[
        ("A", 0x1000), ("B", 0x2000), ("X", 0x4000), ("Y", 0x8000),
        ("D-pad up", 0x1), ("D-pad down", 0x2), ("D-pad left", 0x4), ("D-pad right", 0x8),
        ("LB (left bumper)", 0x100), ("RB (right bumper)", 0x200),
        ("View (two squares, left of Steam)", 0x20), ("Menu (three lines, right of Steam)", 0x10),
        ("L3 (click the left stick)", 0x40), ("R3 (click the right stick)", 0x80),
    ];
    const STEP_TIME: Duration = Duration::from_secs(5);

    struct Run {
        page: Page,
        log: File,
        results: Vec<(String, bool, String)>,
        hooks: Receiver<Ev>,
        pad: u32,
    }

    impl Run {
        fn say(&mut self, s: &str) {
            println!("{s}");
            let _ = writeln!(self.log, "{s}");
        }

        fn enter(&mut self) {
            let mut s = String::new();
            let _ = std::io::stdin().lock().read_line(&mut s);
        }

        fn result(&mut self, name: &str, ok: bool, detail: String) {
            self.say(&format!("   {} {detail}", if ok { "OK   " } else { "FAIL " }));
            self.results.push((name.into(), ok, detail));
        }

        fn ask(&mut self, name: &str, question: &str) {
            self.say(&format!("\n>> {question}  [y/n, then Enter]"));
            let mut s = String::new();
            let _ = std::io::stdin().lock().read_line(&mut s);
            let ok = s.trim().eq_ignore_ascii_case("y");
            self.result(name, ok, format!("answer: {}", s.trim()));
        }

        fn prompt(&mut self, text: &str) {
            self.say(&format!("\n>> {text}\n   Press Enter, then do it."));
            self.enter();
        }

        fn invoke(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
            self.page.invoke(cmd, args).map_err(|e| format!("{cmd}: {e}"))
        }

        fn layout(&mut self, kind: &str, name: &str) -> Result<Value, String> {
            self.invoke("play_load_preset", json!({ "kind": kind, "name": name, "builtin": true }))
        }

        /// Only one output on: `keymouse` with its two layouts, or `gamepad` with `pad_layout`.
        fn outputs(&mut self, keymouse: bool, pad_layout: Option<&str>) -> Result<(), String> {
            let desktop = self.layout("keymouse", "Desktop")?;
            let game = self.layout("keymouse", "Shooter (WASD + mouse)")?;
            self.invoke("play_set_keymouse", json!({ "config": { "enabled": keymouse, "desktop": desktop, "game": game } }))?;
            let layout = self.layout("gamepad", pad_layout.unwrap_or("Standard"))?;
            self.invoke("play_set_gamepad", json!({ "config": { "enabled": pad_layout.is_some(), "layout": layout } }))?;
            self.invoke("play_set_mode", json!({ "mode": "desktop" }))?;
            Ok(())
        }

        fn play(&mut self) -> Result<Value, String> {
            Ok(self.page.snapshot()?["play"].clone())
        }

        // ---------- Gamepad ----------

        fn wait_neutral(&mut self) {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(10) {
                let g = input::pad(self.pad).unwrap_or(unsafe { std::mem::zeroed() });
                let sticks = [g.sThumbLX, g.sThumbLY, g.sThumbRX, g.sThumbRY].iter().all(|v| v.unsigned_abs() < 8000);
                if g.wButtons == 0 && g.bLeftTrigger < 30 && g.bRightTrigger < 30 && sticks {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        /// Waits up to STEP_TIME for `seen`, then judges the state a moment later.
        fn pad_step(&mut self, name: &str, prompt: &str, seen: impl Fn(&input::XGamepad) -> bool, ok: impl Fn(&input::XGamepad) -> bool) {
            self.prompt(prompt);
            self.wait_neutral();
            let start = Instant::now();
            while start.elapsed() < STEP_TIME {
                // Judged on the first report that shows it: a quick tap is gone a moment later.
                if let Some(g) = input::pad(self.pad).filter(|g| seen(g)) {
                    let pass = ok(&g);
                    self.result(name, pass, input::fmt_pad(&g));
                    std::thread::sleep(Duration::from_millis(800));
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            self.result(name, false, "nothing arrived".into());
        }

        /// The first strong deflection of an axis, which must have the sign of `want` (moving back
        /// afterwards deflects the other way, so only the first one counts).
        fn axis_step(&mut self, name: &str, prompt: &str, axis: fn(&input::XGamepad) -> i32, want: i32) {
            self.axis_step_at(name, prompt, axis, want, 9000);
        }

        /// Gyro aim moves the stick by turning speed, so what counts is the net turn over the step
        /// (the stick summed over time): a little backswing first doesn't flip it.
        fn gyro_step(&mut self, name: &str, prompt: &str, axis: fn(&input::XGamepad) -> i32, want: i32) {
            self.prompt(prompt);
            self.wait_neutral();
            let (mut sum, mut peak) = (0f64, 0i32);
            let end = Instant::now() + STEP_TIME;
            while Instant::now() < end {
                if let Some(g) = input::pad(self.pad) {
                    let v = axis(&g);
                    sum += v as f64 / 32767.0 * 0.005;
                    if v.abs() > peak.abs() {
                        peak = v;
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let moved = peak.abs() > 5000;
            let detail = format!("net {sum:+.2} (stick-seconds), strongest {peak:+}");
            self.result(name, moved && (sum.signum() as i32) == want, if moved { detail } else { format!("barely moved: {detail}") });
        }

        fn axis_step_at(&mut self, name: &str, prompt: &str, axis: fn(&input::XGamepad) -> i32, want: i32, threshold: i32) {
            self.prompt(prompt);
            self.wait_neutral();
            let start = Instant::now();
            while start.elapsed() < STEP_TIME {
                if let Some(g) = input::pad(self.pad).filter(|g| axis(g).abs() > threshold) {
                    self.result(name, axis(&g).signum() == want, input::fmt_pad(&g));
                    std::thread::sleep(Duration::from_millis(1500));
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            self.result(name, false, "nothing arrived".into());
        }

        // ---------- Keyboard & mouse ----------

        /// Collects what the app injects for STEP_TIME after Enter.
        fn km_step(&mut self, name: &str, prompt: &str, ok: impl Fn(&Seen) -> bool) {
            self.prompt(prompt);
            while self.hooks.try_recv().is_ok() {}
            let pointer = input::cursor();
            let end = Instant::now() + STEP_TIME;
            let mut evs = vec![];
            while Instant::now() < end {
                if let Ok(e) = self.hooks.recv_timeout(Duration::from_millis(20)) {
                    evs.push(e);
                }
            }
            let seen = Seen::of(&evs, pointer);
            let pass = ok(&seen);
            self.result(name, pass, seen.describe());
        }

        fn keys(&mut self, name: &str, prompt: &str, want: &'static str) {
            self.km_step(name, prompt, move |s| s.keys.starts_with(want));
        }
    }

    /// `--only a,b`: the sections to run.
    fn only() -> Vec<String> {
        let args: Vec<String> = std::env::args().collect();
        args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).map(|v| v.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default()
    }

    /// The app to start: a path given on the command line, else the release build.
    fn app_exe() -> PathBuf {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let only_value = args.iter().position(|a| a == "--only").map(|i| i + 1);
        args.iter().enumerate().find(|(i, a)| *a != "--only" && Some(*i) != only_value).map(|(_, a)| PathBuf::from(a)).unwrap_or_else(|| {
            let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            here.join("../../target/release/steam-controller-app.exe")
        })
    }

    /// Connects to the app's page, starting the app with the debugging port if it isn't running.
    fn connect() -> Result<Page, String> {
        if cdp::page_url().is_none() {
            let running = Command::new("tasklist").args(["/FI", "IMAGENAME eq steam-controller-app.exe", "/NH"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("steam-controller-app")).unwrap_or(false);
            if running {
                return Err("the app is running without the debugging port: quit it (tray icon → Quit) and run this again".into());
            }
            let exe = app_exe();
            if !exe.exists() {
                return Err(format!("no app at {}: build it (npm run tauri build -- --no-bundle) or pass its path", exe.display()));
            }
            println!("Starting {} ...", exe.display());
            Command::new(&exe).env("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", format!("--remote-debugging-port={}", cdp::PORT)).spawn().map_err(|e| e.to_string())?;
            let start = Instant::now();
            while cdp::page_url().is_none() {
                if start.elapsed() > Duration::from_secs(30) {
                    return Err("the app's window didn't open".into());
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        Page::connect()
    }

    pub fn run() -> Result<bool, String> {
        println!("Steam Controller Toolkit release checks. Have the controller on and in reach.");
        println!("Every step waits for Enter, then gives you {} seconds. Keep the app's window open (not minimised).\n", STEP_TIME.as_secs());
        let mut page = connect()?;
        let snap = page.snapshot()?;
        if snap["slot"].is_null() {
            return Err("no controller: turn it on (Steam button) and run this again".into());
        }
        if snap["steam_running"] == true {
            println!("Note: Steam is running; its Desktop Layout may add mouse input of its own.\n");
        }
        // Your settings, put back at the end.
        let saved_play = page.invoke("play_setup", json!({}))?;
        let saved_osc = page.invoke("osc_destinations", json!({}))?;
        let saved_osc_on = snap["osc"]["enabled"].as_bool().unwrap_or(false);
        let log_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("last-run.log");
        let mut run = Run { page, log: File::create(&log_path).map_err(|e| e.to_string())?, results: vec![], hooks: input::start_hooks()?, pad: 0 };
        let outcome = steps(&mut run, &only());
        // Restore, whatever happened.
        input::set_block(false);
        let restore = (|| -> Result<(), String> {
            let mut page = Page::connect()?;
            page.invoke("play_set_keymouse", json!({ "config": saved_play["keymouse"] }))?;
            page.invoke("play_set_gamepad", json!({ "config": saved_play["gamepad"] }))?;
            page.invoke("play_set_auto_switch", json!({ "on": saved_play["auto_switch"] }))?;
            page.invoke("play_set_mode", json!({ "mode": saved_play["mode"] }))?;
            page.invoke("osc_set_destinations", json!({ "destinations": saved_osc }))?;
            page.invoke("osc_set_enabled", json!({ "on": saved_osc_on }))?;
            Ok(())
        })();
        outcome?;
        run.say("\n===== Summary =====");
        let results = std::mem::take(&mut run.results);
        let failed = results.iter().filter(|r| !r.1).count();
        for (name, ok, detail) in &results {
            run.say(&format!("{}  {name}{}", if *ok { "OK  " } else { "FAIL" }, if *ok { String::new() } else { format!(" ({detail})") }));
        }
        run.say(&format!("\n{} of {} passed. The log is in {}.", results.len() - failed, results.len(), log_path.display()));
        match restore {
            Ok(()) => run.say("Your output settings are back as they were."),
            Err(e) => run.say(&format!("Couldn't put your output settings back ({e}); check the output tabs.")),
        }
        Ok(failed == 0)
    }

    /// Runs every section, or only those named with `--only gamepad,gyro,keymouse,modes,osc,tray`.
    fn steps(r: &mut Run, only: &[String]) -> Result<(), String> {
        let want = |s: &str| only.is_empty() || only.iter().any(|o| o == s);
        if (want("gamepad") || want("gyro")) && !find_pad(r)? {
            return Ok(());
        }
        if want("gamepad") {
            gamepad(r)?;
        }
        if want("gyro") {
            gyro(r)?;
        }
        if want("keymouse") {
            keymouse(r)?;
        }
        if want("modes") {
            modes(r)?;
        }
        if want("osc") {
            osc(r)?;
        }
        if want("tray") {
            tray(r)?;
        }
        Ok(())
    }

    /// Turns the gamepad on and finds its XInput slot; false if it never appears.
    fn find_pad(r: &mut Run) -> Result<bool, String> {
        r.say("=== Gamepad (Standard layout) ===");
        // Off first, so the pad that appears next is the app's even if it was on already.
        r.outputs(false, None)?;
        std::thread::sleep(Duration::from_millis(1500));
        let before = input::pads();
        r.outputs(false, Some("Standard"))?;
        r.page.show_tab("gamepad")?;
        let start = Instant::now();
        let pad = loop {
            if let Some(&i) = input::pads().iter().find(|i| !before.contains(i)) {
                break Some(i);
            }
            if start.elapsed() > Duration::from_secs(15) {
                break None;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let Some(pad) = pad else {
            let status = r.play()?["gamepad"].clone();
            r.result("Virtual pad appears in XInput", false, format!("no new pad (status: {status})"));
            return Ok(false);
        };
        r.pad = pad;
        std::thread::sleep(Duration::from_millis(500));
        let g = input::pad(pad).unwrap();
        let centred = [g.sThumbLX, g.sThumbLY, g.sThumbRX, g.sThumbRY].iter().all(|v| v.unsigned_abs() < 3000);
        r.result("Virtual pad appears in XInput, centred", centred, format!("XInput slot {pad}: {}", input::fmt_pad(&g)));
        let player = r.play()?["gamepad"]["player"].clone();
        r.result("Status shows the player", player.is_number(), format!("player {player}"));
        Ok(true)
    }

    fn gamepad(r: &mut Run) -> Result<(), String> {
        r.outputs(false, Some("Standard"))?;

        for &(name, bit) in PAD_BUTTONS {
            r.pad_step(&format!("Pad: {name}"), &format!("Press {name} once"), |g| g.wButtons != 0, move |g| g.wButtons == bit);
        }
        r.pad_step("Pad: LT", "Pull LT all the way", |g| g.bLeftTrigger > 200, |g| g.bRightTrigger < 30);
        r.pad_step("Pad: RT", "Pull RT all the way", |g| g.bRightTrigger > 200, |g| g.bLeftTrigger < 30);
        r.axis_step("Pad: left stick right", "Push the LEFT stick to the RIGHT", |g| g.sThumbLX as i32, 1);
        r.axis_step("Pad: left stick up", "Push the LEFT stick UP", |g| g.sThumbLY as i32, 1);
        r.axis_step("Pad: right stick right", "Push the RIGHT stick to the RIGHT", |g| g.sThumbRX as i32, 1);
        r.axis_step("Pad: right stick up", "Push the RIGHT stick UP", |g| g.sThumbRY as i32, 1);

        r.say("\n>> Rumble: the controller should buzz twice, first the left (large) motor, then the right.");
        std::thread::sleep(Duration::from_millis(800));
        for (l, rr) in [(40000, 0), (0, 40000)] {
            input::rumble(r.pad, l, rr);
            std::thread::sleep(Duration::from_millis(1000));
            input::rumble(r.pad, 0, 0);
            std::thread::sleep(Duration::from_millis(600));
        }
        r.ask("Rumble reaches the controller", "Did it buzz twice?");
        Ok(())
    }

    fn gyro(r: &mut Run) -> Result<(), String> {
        // ---------- Gyro ----------
        r.say("\n=== Gyro (hold the controller as for a game) ===");
        r.outputs(false, Some("Gyro aim (stick)"))?;
        std::thread::sleep(Duration::from_millis(500));
        r.gyro_step("Gyro aim: turn right = right", "Rest your right thumb on the right stick (don't push), TURN the controller to the RIGHT and stay turned", |g| g.sThumbRX as i32, 1);
        r.gyro_step("Gyro aim: raise front = up", "Thumb on the right stick: RAISE the FRONT (trigger side) to aim up, and stay there", |g| g.sThumbRY as i32, 1);
        r.outputs(false, Some("Racing (tilt to steer)"))?;
        std::thread::sleep(Duration::from_millis(500));
        r.axis_step("Racing: lean right = steer right", "LEAN the controller to the RIGHT like a steering wheel, and hold it there", |g| g.sThumbLX as i32, 1);
        Ok(())
    }

    fn keymouse(r: &mut Run) -> Result<(), String> {
        // ---------- Keyboard & mouse ----------
        r.say("\n=== Keyboard & mouse (Desktop layout; what it sends is caught and blocked) ===");
        r.outputs(true, None)?;
        r.page.show_tab("keymouse")?;
        r.km_step("K&M: right stick moves the pointer right", "Push the RIGHT stick to the RIGHT", |s| s.moved.0 > 100 && s.moved.0 > s.moved.1.abs());
        r.km_step("K&M: right stick up moves it up", "Push the RIGHT stick UP", |s| s.moved.1 < -100 && -s.moved.1 > s.moved.0.abs());
        r.km_step("K&M: right trackpad moves the pointer", "Slide your thumb to the RIGHT across the RIGHT trackpad", |s| s.moved.0 > 50);
        r.km_step("K&M: left stick scrolls up", "Push the LEFT stick UP", |s| s.wheel.0 > 0 && s.wheel.0 > s.wheel.1.abs());
        r.keys("K&M: RT clicks", "Pull RT once and let go", "+left -left");
        r.keys("K&M: LT right-clicks", "Pull LT once and let go", "+right -right");
        r.keys("K&M: A is Enter", "Press A once", "+Enter -Enter");
        r.keys("K&M: View is Win+Tab", "Press View (two squares) once", "+Win +Tab -Tab -Win");
        r.keys("K&M: Menu is Win", "Press Menu (three lines) once", "+Win -Win");
        r.keys("K&M: L4 is Ctrl+Z, by letter", "Press L4 (upper left back button) once", "+Ctrl +Z -Z -Ctrl");
        r.keys("K&M: R5 is Ctrl+V", "Press R5 (lower right back button) once", "+Ctrl +V -V -Ctrl");
        Ok(())
    }

    fn modes(r: &mut Run) -> Result<(), String> {
        // ---------- Modes ----------
        r.say("\n=== Modes ===");
        r.outputs(true, None)?;
        r.prompt("HOLD the STEAM button for 2 seconds, then let go");
        let mut modes: Vec<String> = vec![];
        let end = Instant::now() + STEP_TIME;
        while Instant::now() < end {
            let m = r.play()?["mode"].as_str().unwrap_or("").to_string();
            if modes.last() != Some(&m) {
                modes.push(m);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        r.result("Steam: one press, one switch", modes == ["desktop", "game"], format!("modes seen: {}", modes.join(" → ")));
        r.ask("Steam: a click you can feel", "Did you feel ONE click when it switched?");
        r.invoke("play_set_mode", json!({ "mode": "desktop" }))?;
        r.invoke("play_set_auto_switch", json!({ "on": true }))?;
        r.prompt("Within 30 seconds: put something FULL SCREEN in front (a browser with F11, or a video) for a few seconds, then leave full screen");
        let mut seen_game = None;
        let mut back = false;
        let end = Instant::now() + Duration::from_secs(30);
        while Instant::now() < end && !back {
            let p = r.play()?;
            if p["mode"] == "game" && seen_game.is_none() {
                seen_game = Some(p["fullscreen_app"].as_str().unwrap_or("?").to_string());
            }
            back = seen_game.is_some() && p["mode"] == "desktop";
            std::thread::sleep(Duration::from_millis(100));
        }
        r.result("Auto switch: full screen is Game, leaving it is Desktop", seen_game.is_some() && back, format!("full-screen app: {}, back to Desktop: {back}", seen_game.as_deref().unwrap_or("none seen")));
        Ok(())
    }

    fn osc(r: &mut Run) -> Result<(), String> {
        // ---------- OSC on/off ----------
        r.say("\n=== OSC switch (checked over UDP) ===");
        r.outputs(false, None)?;
        let sock = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        sock.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
        let port = sock.local_addr().map_err(|e| e.to_string())?.port();
        let presets = r.invoke("osc_list_presets", json!({}))?;
        let first = presets.as_array().and_then(|l| l.iter().find(|p| p["builtin"] == true)).ok_or("no OSC presets")?["name"].clone();
        let preset = r.invoke("osc_load_preset", json!({ "name": first, "builtin": true }))?;
        r.invoke("osc_set_destinations", json!({ "destinations": [{ "enabled": true, "host": "127.0.0.1", "port": port, "preset": preset }] }))?;
        r.invoke("osc_set_enabled", json!({ "on": true }))?;
        let count = |sock: &UdpSocket, secs: u64| {
            let mut buf = [0u8; 65536];
            let end = Instant::now() + Duration::from_secs(secs);
            let mut n = 0;
            while Instant::now() < end {
                if sock.recv_from(&mut buf).is_ok() {
                    n += 1;
                }
            }
            n
        };
        r.prompt("Move both sticks around for a few seconds");
        let on = count(&sock, 4);
        r.result("OSC on: packets arrive", on > 0, format!("{on} packets in 4 s"));
        r.invoke("osc_set_enabled", json!({ "on": false }))?;
        count(&sock, 1); // what it lets go of
        r.prompt("Move both sticks around again (OSC is off now)");
        let off = count(&sock, 4);
        r.result("OSC off: nothing arrives", off == 0, format!("{off} packets in 4 s"));
        Ok(())
    }

    fn tray(r: &mut Run) -> Result<(), String> {
        // ---------- Tray ----------
        r.say("\n=== Tray ===");
        r.outputs(true, None)?;
        r.say("\n>> Close the app's window with its X (it stays in the tray), then press Enter here.");
        r.enter();
        std::thread::sleep(Duration::from_secs(1));
        r.result("Closing keeps the app in the tray", cdp::page_url().is_none(), if cdp::page_url().is_none() { "window closed".into() } else { "the window is still open".into() });
        r.km_step("Tray: the controller is still a mouse", "Push the RIGHT stick to the RIGHT", |s| s.moved.0 > 100);
        r.ask("Tray: menu lines", "Right-click the tray icon: does it show \"Keyboard & mouse: Desktop layout\" and \"Gamepad: off\"? (Press Esc to close the menu.)");
        r.say("\n>> Click the tray icon to open the window again, then press Enter here.");
        r.enter();
        let start = Instant::now();
        while cdp::page_url().is_none() && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(300));
        }
        std::thread::sleep(Duration::from_secs(2));
        r.page = Page::connect()?;
        Ok(())
    }
}
