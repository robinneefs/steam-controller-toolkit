//! The OSC output: where the engines' messages go (several destinations, each host:port with its
//! own preset), saved presets, and what the OSC tab shows. `Outputs` feeds it every state report.
//!
//! Everything goes out over UDP from one socket per address family, non-blocking so the event
//! loop never waits. Host names are looked up on a thread of their own (a lookup can take
//! seconds); `tick` picks up the result.

use std::fs;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rosc::{encoder, OscBundle, OscMessage, OscPacket, OscTime, OscType};
use sc_core::ControllerState;
use serde::{Deserialize, Serialize};

use crate::osc::{builtin_presets, Arg, Engine, Msg, Preset};
use crate::shaping::Learner;

/// How often to look a host up again after it failed.
const RETRY: Duration = Duration::from_secs(5);
const PRESET_EXT: &str = "json";

/// Where one copy of the output goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Destination {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub preset: Preset,
}

impl Default for Destination {
    fn default() -> Self {
        let preset = Preset::default();
        Destination { enabled: false, host: "127.0.0.1".into(), port: preset.port.unwrap_or(9000), preset }
    }
}

/// What the OSC tab shows for one destination.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DestinationStatus {
    /// The address packets go to, once the host is known.
    pub address: Option<String>,
    pub resolving: bool,
    pub error: Option<String>,
    /// Messages sent over the last second.
    pub rate: u32,
    /// Output level 0..1 of each mapping, in preset order.
    pub levels: Vec<Option<f32>>,
    /// Every address with what it last carried.
    pub values: Vec<(String, Vec<Arg>)>,
}

/// What the OSC tab needs, sent in every snapshot.
#[derive(Debug, Clone, Default, Serialize)]
pub struct OscStatus {
    pub destinations: Vec<DestinationStatus>,
    pub learning: bool,
    /// The input picked by the last learn, as a source name.
    pub learned: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresetInfo {
    pub name: String,
    pub builtin: bool,
    pub port: Option<u16>,
}

/// Saved between runs in `config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    destinations: Vec<Destination>,
}

impl Default for Config {
    // Out of the box: one destination, off until the user turns it on.
    fn default() -> Self {
        Config { destinations: vec![Destination::default()] }
    }
}

type Lookup = Result<SocketAddr, String>;

struct Dest {
    cfg: Destination,
    engine: Engine,
    addr: Option<SocketAddr>,
    lookup: Option<mpsc::Receiver<Lookup>>,
    last_try: Option<Instant>,
    error: Option<String>,
    rate: u32,
    rate_count: u32,
}

impl Dest {
    fn new(cfg: Destination) -> Self {
        Dest {
            engine: Engine::new(cfg.preset.clone()),
            cfg,
            addr: None,
            lookup: None,
            last_try: None,
            error: None,
            rate: 0,
            rate_count: 0,
        }
    }

    fn lookup(&mut self) {
        self.last_try = Some(Instant::now());
        self.lookup = None;
        let (host, port) = (self.cfg.host.trim().to_string(), self.cfg.port);
        // An IP address needs no lookup.
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
            self.addr = Some(SocketAddr::new(ip, port));
            self.error = None;
            return;
        }
        self.addr = None;
        if host.is_empty() {
            self.error = Some("Enter a host".into());
            return;
        }
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let found = (host.as_str(), port)
                .to_socket_addrs()
                .map_err(|e| format!("Couldn't find \"{host}\": {e}"))
                // Most OSC apps listen on IPv4 only, and "localhost" is ::1 first on Windows.
                .and_then(|a| {
                    let all: Vec<SocketAddr> = a.collect();
                    all.iter().find(|a| a.is_ipv4()).or(all.first()).copied().ok_or_else(|| format!("Couldn't find \"{host}\""))
                });
            let _ = tx.send(found);
        });
        self.lookup = Some(rx);
    }
}

/// One UDP socket per address family, opened when first needed.
#[derive(Default)]
struct Sockets {
    v4: Option<UdpSocket>,
    v6: Option<UdpSocket>,
}

impl Sockets {
    fn get(&mut self, to: SocketAddr) -> Option<&UdpSocket> {
        let (slot, bind) = if to.is_ipv4() { (&mut self.v4, "0.0.0.0:0") } else { (&mut self.v6, "[::]:0") };
        if slot.is_none() {
            let s = UdpSocket::bind(bind).ok()?;
            s.set_nonblocking(true).ok()?;
            // Allow sending to broadcast addresses such as 192.168.1.255.
            let _ = s.set_broadcast(true);
            *slot = Some(s);
        }
        slot.as_ref()
    }

    /// Send a destination's messages: one bundle, or one packet each.
    fn send(&mut self, d: &mut Dest, msgs: &mut Vec<Msg>) {
        if msgs.is_empty() {
            return;
        }
        let (Some(addr), true) = (d.addr, d.cfg.enabled) else {
            msgs.clear();
            return;
        };
        let packets: Vec<OscPacket> = msgs.drain(..).map(packet).collect();
        let count = packets.len() as u32;
        let datagrams: Vec<Vec<u8>> = if d.cfg.preset.bundle && packets.len() > 1 {
            // Immediately: time tag (0, 1).
            let bundle = OscPacket::Bundle(OscBundle { timetag: OscTime::from((0, 1)), content: packets });
            encoder::encode(&bundle).into_iter().collect()
        } else {
            packets.iter().filter_map(|p| encoder::encode(p).ok()).collect()
        };
        let Some(socket) = self.get(addr) else {
            d.error = Some("Couldn't open a UDP socket".into());
            return;
        };
        for bytes in datagrams {
            match socket.send_to(&bytes, addr) {
                Ok(_) => d.error = None,
                // A full send buffer just drops this one; the next report brings fresh values.
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => d.error = Some(format!("Couldn't send: {e}")),
            }
        }
        d.rate_count += count;
    }
}

pub struct OscOut {
    dests: Vec<Dest>,
    sockets: Sockets,
    learn: Learner,
    start: Instant,
    rate_window: Instant,
    dir: PathBuf,
    buf: Vec<Msg>,
    /// Writes `config.json` (see `MidiOut::save_config`, same reason).
    saver: mpsc::Sender<String>,
}

impl OscOut {
    /// Load the saved config from `dir` (presets go in `dir/presets`).
    pub fn new(dir: PathBuf) -> Self {
        let config: Config = fs::read_to_string(dir.join("config.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let (saver, configs) = mpsc::channel::<String>();
        let config_dir = dir.clone();
        thread::spawn(move || {
            while let Ok(mut json) = configs.recv() {
                while let Ok(newer) = configs.try_recv() {
                    json = newer;
                }
                let _ = fs::create_dir_all(&config_dir);
                let _ = fs::write(config_dir.join("config.json"), json);
            }
        });
        let mut out = OscOut {
            dests: config.destinations.into_iter().map(Dest::new).collect(),
            sockets: Sockets::default(),
            learn: Learner::default(),
            start: Instant::now(),
            rate_window: Instant::now(),
            dir,
            buf: Vec::new(),
            saver,
        };
        out.dests.iter_mut().filter(|d| d.cfg.enabled).for_each(Dest::lookup);
        out
    }

    fn presets_dir(&self) -> PathBuf {
        self.dir.join("presets")
    }

    fn save_config(&self) {
        let config = Config { destinations: self.destinations() };
        if let Ok(json) = serde_json::to_string_pretty(&config) {
            let _ = self.saver.send(json);
        }
    }

    pub fn destinations(&self) -> Vec<Destination> {
        self.dests.iter().map(|d| d.cfg.clone()).collect()
    }

    /// Replace the destinations (the OSC tab sends the whole list after every edit). One that
    /// stops or moves elsewhere first lets go of what it was holding at its old address.
    pub fn set_destinations(&mut self, list: Vec<Destination>) {
        let mut old: Vec<Option<Dest>> = std::mem::take(&mut self.dests).into_iter().map(Some).collect();
        for (i, cfg) in list.into_iter().enumerate() {
            let Some(mut d) = old.get_mut(i).and_then(Option::take) else {
                let mut d = Dest::new(cfg);
                if d.cfg.enabled {
                    d.lookup();
                }
                self.dests.push(d);
                continue;
            };
            let moved = d.cfg.host != cfg.host || d.cfg.port != cfg.port;
            if moved || !cfg.enabled {
                self.release_dest(&mut d);
            }
            if d.cfg.preset != cfg.preset {
                d.engine.set_preset(cfg.preset.clone());
            }
            let turned_on = cfg.enabled && (!d.cfg.enabled || moved);
            d.cfg = cfg;
            if turned_on {
                d.error = None;
                d.lookup();
            }
            if !d.cfg.enabled {
                d.lookup = None;
                d.error = None;
            }
            self.dests.push(d);
        }
        // Removed ones.
        for mut d in old.into_iter().flatten() {
            self.release_dest(&mut d);
        }
        self.save_config();
    }

    /// Feed one state report from whatever the UI is showing (live input or a replay).
    pub fn process(&mut self, s: &ControllerState, battery: Option<u8>) {
        self.learn.process(s);
        let now = self.start.elapsed();
        for d in &mut self.dests {
            // Disabled destinations still run, so the tab's meters and values work.
            d.engine.process(s, battery, now, &mut self.buf);
            self.sockets.send(d, &mut self.buf);
        }
    }

    /// Let go of every held button and say `/connected 0`, e.g. when switching between live input
    /// and a replay or losing the controller.
    pub fn release_all(&mut self) {
        for d in &mut self.dests {
            d.engine.release(&mut self.buf);
            self.sockets.send(d, &mut self.buf);
        }
    }

    fn release_dest(&mut self, d: &mut Dest) {
        d.engine.release(&mut self.buf);
        self.sockets.send(d, &mut self.buf);
    }

    /// Called often from the event loop: picks up finished host lookups, retries failed ones.
    pub fn tick(&mut self) {
        for d in &mut self.dests {
            if let Some(rx) = &d.lookup {
                match rx.try_recv() {
                    Ok(Ok(addr)) => {
                        d.addr = Some(addr);
                        d.error = None;
                        d.lookup = None;
                    }
                    Ok(Err(e)) => {
                        d.error = Some(e);
                        d.lookup = None;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => d.lookup = None,
                }
            } else if d.cfg.enabled && d.addr.is_none() && d.last_try.is_none_or(|t| t.elapsed() >= RETRY) {
                d.lookup();
            }
        }
        if self.rate_window.elapsed() >= Duration::from_secs(1) {
            for d in &mut self.dests {
                d.rate = std::mem::take(&mut d.rate_count);
            }
            self.rate_window = Instant::now();
        }
    }

    pub fn start_learn(&mut self) {
        self.learn.start();
    }

    pub fn cancel_learn(&mut self) {
        self.learn.cancel();
    }

    pub fn status(&self) -> OscStatus {
        OscStatus {
            destinations: self
                .dests
                .iter()
                .map(|d| DestinationStatus {
                    address: d.addr.filter(|_| d.cfg.enabled).map(|a| a.to_string()),
                    resolving: d.lookup.is_some(),
                    error: d.error.clone(),
                    rate: if d.cfg.enabled { d.rate } else { 0 },
                    levels: d.engine.levels(),
                    values: d.engine.values(),
                })
                .collect(),
            learning: self.learn.learning(),
            learned: self.learn.learned().map(str::to_string),
        }
    }

    /// Built-in presets first, then saved ones by name.
    pub fn list_presets(&self) -> Vec<PresetInfo> {
        let mut saved: Vec<(String, Option<u16>)> = fs::read_dir(self.presets_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.extension()? != PRESET_EXT {
                    return None;
                }
                let port = fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Preset>(&s).ok()).and_then(|p| p.port);
                Some((p.file_stem()?.to_string_lossy().into_owned(), port))
            })
            .collect();
        saved.sort_by_key(|(s, _)| s.to_lowercase());
        let builtin = builtin_presets().into_iter().map(|p| PresetInfo { name: p.name, builtin: true, port: p.port });
        builtin.chain(saved.into_iter().map(|(name, port)| PresetInfo { name, builtin: false, port })).collect()
    }

    /// A saved preset by name, or a built-in one.
    pub fn load_preset(&self, name: &str, builtin: bool) -> Result<Preset, String> {
        if builtin {
            return builtin_presets().into_iter().find(|p| p.name == name).ok_or_else(|| format!("No built-in preset \"{name}\""));
        }
        let text = fs::read_to_string(self.preset_file(name)?).map_err(|e| format!("Couldn't read \"{name}\": {e}"))?;
        let mut preset: Preset = serde_json::from_str(&text).map_err(|e| format!("\"{name}\" is not a valid preset: {e}"))?;
        preset.name = name.to_string();
        Ok(preset)
    }

    /// Save `preset` as `name` (overwriting a saved one of that name).
    pub fn save_preset(&self, name: &str, mut preset: Preset) -> Result<(), String> {
        let file = self.preset_file(name)?;
        preset.name = name.to_string();
        fs::create_dir_all(self.presets_dir()).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
        fs::write(file, json).map_err(|e| format!("Couldn't save \"{name}\": {e}"))
    }

    pub fn delete_preset(&self, name: &str) -> Result<(), String> {
        fs::remove_file(self.preset_file(name)?).map_err(|e| format!("Couldn't delete \"{name}\": {e}"))
    }

    pub fn presets_folder(&self) -> Result<PathBuf, String> {
        let dir = self.presets_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }

    fn preset_file(&self, name: &str) -> Result<PathBuf, String> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || name.starts_with('.') {
            return Err(format!("\"{name}\" can't be used as a preset name"));
        }
        if builtin_presets().iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
            return Err(format!("\"{name}\" is a built-in preset; pick another name"));
        }
        Ok(self.presets_dir().join(Path::new(name).with_extension(PRESET_EXT)))
    }
}

fn packet(m: Msg) -> OscPacket {
    let args = m
        .args
        .into_iter()
        .map(|a| match a {
            Arg::Int(v) => OscType::Int(v),
            Arg::Float(v) => OscType::Float(v),
            Arg::Bool(v) => OscType::Bool(v),
            Arg::String(v) => OscType::String(v),
        })
        .collect();
    OscPacket::Message(OscMessage { addr: m.addr, args })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::{Groups, Stream};
    use rosc::decoder;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-osc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn receiver() -> (UdpSocket, u16) {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let port = sock.local_addr().unwrap().port();
        (sock, port)
    }

    fn recv(sock: &UdpSocket) -> OscPacket {
        let mut buf = [0u8; 65536];
        let (n, _) = sock.recv_from(&mut buf).expect("nothing arrived");
        decoder::decode_udp(&buf[..n]).unwrap().1
    }

    fn messages(p: OscPacket) -> Vec<OscMessage> {
        match p {
            OscPacket::Message(m) => vec![m],
            OscPacket::Bundle(b) => b.content.into_iter().flat_map(messages).collect(),
        }
    }

    /// Real UDP over localhost: what arrives decodes as the stream's bundle, then single messages.
    #[test]
    fn sends_over_udp() {
        let (sock, port) = receiver();
        let dir = temp_dir("udp");
        let mut osc = OscOut::new(dir.clone());
        let groups = Groups { triggers: false, pads: false, buttons: false, touch: false, tilt: false, gyro: false, status: false, ..Groups::default() };
        let preset = Preset { stream: Stream { groups, ..Stream::default() }, ..Preset::default() };
        osc.set_destinations(vec![Destination { enabled: true, host: "127.0.0.1".into(), port, preset: preset.clone() }]);
        let mut s = ControllerState { left_stick: [32767, 0], ..Default::default() };
        osc.process(&s, None);
        let first = recv(&sock);
        assert!(matches!(first, OscPacket::Bundle(_)));
        let msgs = messages(first);
        assert_eq!(msgs[0].addr, "/sc/left_stick");
        assert_eq!(msgs[0].args, vec![OscType::Float(1.0), OscType::Float(0.0)]);
        assert_eq!(msgs[1].addr, "/sc/right_stick");
        // Without bundles, and only what changed: one plain message.
        osc.set_destinations(vec![Destination { enabled: true, host: "127.0.0.1".into(), port, preset: Preset { bundle: false, ..preset } }]);
        s.right_stick = [0, -32767];
        osc.process(&s, None);
        let msgs = messages(recv(&sock));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].addr, "/sc/right_stick");
        assert_eq!(msgs[0].args, vec![OscType::Float(0.0), OscType::Float(-1.0)]);
        assert_eq!(osc.status().destinations[0].address.as_deref(), Some(format!("127.0.0.1:{port}").as_str()));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Turning a destination off releases what it held, at its address, then sends nothing more.
    #[test]
    fn disabling_releases() {
        let (sock, port) = receiver();
        let dir = temp_dir("release");
        let mut osc = OscOut::new(dir.clone());
        let preset = builtin_presets().into_iter().find(|p| p.name == "VRChat").unwrap();
        let dest = Destination { enabled: true, host: "localhost".into(), port, preset };
        osc.set_destinations(vec![dest.clone()]);
        // "localhost" is looked up on a thread.
        let started = Instant::now();
        while osc.status().destinations[0].address.is_none() {
            assert!(started.elapsed() < Duration::from_secs(5), "lookup never finished");
            osc.tick();
            thread::sleep(Duration::from_millis(5));
        }
        let s = ControllerState { buttons: sc_core::protocol::buttons::A, ..Default::default() };
        osc.process(&s, None);
        let held: Vec<_> = messages(recv(&sock)).into_iter().map(|m| (m.addr, m.args)).collect();
        assert!(held.contains(&("/input/Jump".into(), vec![OscType::Int(1)])));
        osc.set_destinations(vec![Destination { enabled: false, ..dest }]);
        let released: Vec<_> = messages(recv(&sock)).into_iter().map(|m| (m.addr, m.args)).collect();
        assert!(released.contains(&("/input/Jump".into(), vec![OscType::Int(0)])), "{released:?}");
        osc.process(&s, None);
        sock.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        let mut buf = [0u8; 1024];
        assert!(sock.recv_from(&mut buf).is_err(), "sent while off");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_is_saved_and_restored() {
        let dir = temp_dir("config");
        let mut osc = OscOut::new(dir.clone());
        assert_eq!(osc.destinations().len(), 1);
        assert!(!osc.destinations()[0].enabled);
        let qlab = osc.load_preset("QLab", true).unwrap();
        osc.set_destinations(vec![
            Destination::default(),
            Destination { host: "10.0.0.5".into(), port: 53000, preset: qlab, ..Default::default() },
        ]);
        let started = Instant::now();
        let saved = loop {
            let saved = fs::read_to_string(dir.join("config.json")).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok());
            match saved {
                Some(c) if c.destinations.len() == 2 => break c,
                _ if started.elapsed() > Duration::from_secs(5) => panic!("config.json was not saved"),
                _ => thread::sleep(Duration::from_millis(10)),
            }
        };
        assert_eq!(saved.destinations[1].preset.name, "QLab");
        let restored = OscOut::new(dir.clone());
        assert_eq!(restored.destinations(), osc.destinations());
        // Saved presets list after the built-in ones, with their port.
        osc.save_preset("Mine", Preset { port: Some(1234), ..Preset::default() }).unwrap();
        let list = osc.list_presets();
        assert_eq!(list.last().map(|p| (p.name.as_str(), p.builtin, p.port)), Some(("Mine", false, Some(1234))));
        assert!(osc.save_preset("qlab", Preset::default()).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
