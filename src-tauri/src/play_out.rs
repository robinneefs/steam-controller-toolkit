//! The Keyboard & mouse and Gamepad outputs, one owner for both tabs. `Outputs` feeds it every
//! state report; the layout in play turns it into a frame (`layout.rs`) that goes to the system
//! through `devices.rs`.
//!
//! There are two modes, Desktop and Game, and up to three layouts: Keyboard & mouse has one for
//! each mode, the Gamepad one. With both outputs on, the desktop gets keyboard and mouse and games
//! get the gamepad ([`active_slot`]). A Switch action (the Steam button in every built-in) flips
//! the mode; with auto switch on, a full-screen app means Game.
//!
//! The Command Deck (`deck.rs`) runs next to whichever layout plays, on the controls it takes
//! from it, with a page of its own per program while that program is in front.
//!
//! The virtual pad is opened on a thread of its own (plugging one in takes a moment), and so is
//! the driver check and install; `tick` picks up the results.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use sc_core::protocol::Haptic;
use sc_core::ControllerState;
use serde::{Deserialize, Serialize};

use crate::deck::{Deck, DeckEngine, Menu, MenuView, Part};
use crate::devices::{self, Foreground, KeyMouse, PadSupport, VirtualPad};
use crate::layout::{builtin_gamepad, builtin_keymouse, Engine, Event, Frame, Injector, Layout, PadState, KEYS};

/// How often to look at the foreground window for auto switch and per-program layouts.
const FOREGROUND_POLL: Duration = Duration::from_millis(500);
/// How often to ask the system for the pad's player number.
const PLAYER_POLL: Duration = Duration::from_secs(1);
/// How long to wait before plugging the pad in again after it failed.
const RETRY: Duration = Duration::from_secs(5);
const PRESET_EXT: &str = "json";
/// The haptic click played on every mode switch.
const SWITCH_CLICK: Haptic = Haptic::Command { side: 2, command: 2, gain_db: 0 };
/// The tick a Command Deck menu plays on its side when the thumb moves to another item.
const MENU_TICK: u8 = 1;
/// How many programs seen in front the Command Deck tab offers for a page of their own.
const RECENT_APPS: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Desktop,
    Game,
}

/// One of the three layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    /// Keyboard & mouse, Desktop mode.
    Desktop,
    /// Keyboard & mouse, Game mode.
    Game,
    /// The Gamepad tab's layout.
    Pad,
}

/// Which layout plays in `mode` with the outputs that are on: in Desktop the keyboard & mouse
/// layout comes first, in Game the gamepad.
pub fn active_slot(mode: Mode, keymouse: bool, gamepad: bool) -> Option<Slot> {
    match (mode, keymouse, gamepad) {
        (Mode::Desktop, true, _) => Some(Slot::Desktop),
        (Mode::Game, _, true) => Some(Slot::Pad),
        (Mode::Game, true, false) => Some(Slot::Game),
        (Mode::Desktop, false, true) => Some(Slot::Pad),
        (_, false, false) => None,
    }
}

/// Which kind of layout a preset holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetKind {
    Keymouse,
    Gamepad,
}

impl PresetKind {
    fn builtins(self) -> Vec<Layout> {
        match self {
            PresetKind::Keymouse => builtin_keymouse(),
            PresetKind::Gamepad => builtin_gamepad(),
        }
    }

    fn folder(self) -> &'static str {
        match self {
            PresetKind::Keymouse => "keymouse",
            PresetKind::Gamepad => "gamepad",
        }
    }
}

fn builtin(kind: PresetKind, name: &str) -> Layout {
    let list = kind.builtins();
    list.iter().find(|l| l.name == name).or(list.first()).cloned().unwrap_or_default()
}

/// A Command Deck page that plays while one program is in front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppDeck {
    /// The program's file name (`Photoshop.exe`); any case, `.exe` optional.
    pub app: String,
    pub deck: Deck,
}

impl AppDeck {
    pub fn matches(&self, app: &str) -> bool {
        let bare = |s: &str| {
            let s = s.trim().to_lowercase();
            s.strip_suffix(".exe").map(str::to_string).unwrap_or(s)
        };
        let want = bare(&self.app);
        !want.is_empty() && want == bare(app)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyMouseConfig {
    pub enabled: bool,
    pub desktop: Layout,
    pub game: Layout,
}

impl Default for KeyMouseConfig {
    fn default() -> Self {
        KeyMouseConfig {
            enabled: false,
            desktop: builtin(PresetKind::Keymouse, "Desktop"),
            game: builtin(PresetKind::Keymouse, "Shooter (WASD + mouse)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadConfig {
    pub enabled: bool,
    pub layout: Layout,
}

impl Default for GamepadConfig {
    fn default() -> Self {
        GamepadConfig { enabled: false, layout: builtin(PresetKind::Gamepad, "Standard") }
    }
}

/// The Command Deck tab's settings and pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeckConfig {
    pub enabled: bool,
    /// Show a menu on screen while its stick or trackpad is touched.
    pub overlay: bool,
    /// The page for every program without one of its own.
    pub deck: Deck,
    /// Pages per program; the first that matches the program in front plays.
    pub apps: Vec<AppDeck>,
}

impl Default for DeckConfig {
    fn default() -> Self {
        DeckConfig { enabled: false, overlay: true, deck: Deck::default(), apps: Vec::new() }
    }
}

impl DeckConfig {
    /// A page: None the default one, else `apps[i]`'s (None if it no longer exists).
    fn page(&self, page: Option<usize>) -> Option<&Deck> {
        match page {
            None => Some(&self.deck),
            Some(i) => self.apps.get(i).map(|a| &a.deck),
        }
    }
}

/// Saved between runs in `config.json`; also what the tabs start from (`play_setup`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub keymouse: KeyMouseConfig,
    pub gamepad: GamepadConfig,
    /// A full-screen app switches to Game, anything else back to Desktop.
    pub auto_switch: bool,
    pub deck: DeckConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config { keymouse: KeyMouseConfig::default(), gamepad: GamepadConfig::default(), auto_switch: true, deck: DeckConfig::default() }
    }
}

impl Config {
    fn layout(&self, slot: Slot) -> &Layout {
        match slot {
            Slot::Desktop => &self.keymouse.desktop,
            Slot::Game => &self.keymouse.game,
            Slot::Pad => &self.gamepad.layout,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct KeyMouseStatus {
    pub enabled: bool,
    pub supported: bool,
    /// The system refused the input (a secure screen in front, or no access to /dev/uinput).
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GamepadStatus {
    pub enabled: bool,
    /// None while it is being checked.
    pub support: Option<PadSupport>,
    /// The virtual pad is plugged in.
    pub connected: bool,
    /// Being plugged in.
    pub opening: bool,
    pub player: Option<u8>,
    pub error: Option<String>,
    /// The driver installer is running.
    pub installing: bool,
    pub install_error: Option<String>,
    /// What the game asks of the large and small motor, 0..255.
    pub rumble: (u8, u8),
}

/// What a layout makes of the controller right now, for the live view of the tab in view.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Preview {
    pub slot: Option<Slot>,
    pub pad: PadState,
    /// Held keys by name.
    pub keys: Vec<&'static str>,
    pub mouse_buttons: u8,
    /// Pointer speed in px/s, x right, y down.
    pub mouse: [f32; 2],
    /// Scroll speed in notches/s, x right, y up.
    pub scroll: [f32; 2],
    pub gyro_active: bool,
    /// Bit per control in use (`layout::CONTROLS`, then `LIT_*`).
    pub lit: u64,
}

/// The Command Deck tab's live view: the page it shows, as the controller uses it now.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeckPreview {
    /// The page shown (None: the default page); `shown` false while the tab isn't in view.
    pub page: Option<usize>,
    pub shown: bool,
    /// Bit per control in use (`layout::CONTROLS`, then the sticks and pads).
    pub lit: u64,
    /// The left stick, right stick, left pad and right pad's menus.
    pub menus: [MenuView; 4],
}

/// `Snapshot.play.deck`, mirrored in `controller.ts`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeckStatus {
    pub enabled: bool,
    pub overlay: bool,
    /// Keyboard and mouse input can be sent here.
    pub supported: bool,
    /// Pages per program work here (the program in front can be told).
    pub apps_supported: bool,
    /// The page playing (None: the default page) and its program.
    pub page: Option<usize>,
    pub app: Option<String>,
    /// Programs seen in front lately, most recent first, to make pages for.
    pub recent_apps: Vec<String>,
    /// The last program, file or URL that couldn't be opened, and why.
    pub run_error: Option<String>,
    pub preview: DeckPreview,
}

/// What the on-screen menu shows (`overlay.rs`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverlayMenu {
    pub part: Part,
    pub menu: Menu,
    pub selected: Option<u8>,
}

/// What both tabs, the tray and the sidebar show; `Snapshot.play`, mirrored in `controller.ts`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PlayStatus {
    pub mode: Mode,
    pub auto_switch: bool,
    /// The full-screen app auto switch sees, by file name.
    pub fullscreen_app: Option<String>,
    /// The layout in play, if any.
    pub active: Option<Slot>,
    pub keymouse: KeyMouseStatus,
    pub gamepad: GamepadStatus,
    pub preview: Preview,
    pub deck: DeckStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresetInfo {
    pub name: String,
    pub builtin: bool,
}

/// Saved layouts of one kind in their own folder, next to the built-in ones (which can't be
/// overwritten).
pub struct PresetStore {
    dir: PathBuf,
    kind: PresetKind,
}

impl PresetStore {
    pub fn new(dir: PathBuf, kind: PresetKind) -> Self {
        PresetStore { dir, kind }
    }

    /// Built-in layouts first, then saved ones by name.
    pub fn list(&self) -> Vec<PresetInfo> {
        let mut saved: Vec<String> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension()? == PRESET_EXT).then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()))?
            })
            .collect();
        saved.sort_by_key(|s| s.to_lowercase());
        let builtin = self.kind.builtins().into_iter().map(|l| PresetInfo { name: l.name, builtin: true });
        builtin.chain(saved.into_iter().map(|name| PresetInfo { name, builtin: false })).collect()
    }

    pub fn load(&self, name: &str, builtin: bool) -> Result<Layout, String> {
        if builtin {
            return self.kind.builtins().into_iter().find(|l| l.name == name).ok_or_else(|| format!("No built-in layout \"{name}\""));
        }
        let text = fs::read_to_string(self.file(name)?).map_err(|e| format!("Couldn't read \"{name}\": {e}"))?;
        let mut layout: Layout = serde_json::from_str(&text).map_err(|e| format!("\"{name}\" is not a valid layout: {e}"))?;
        layout.name = name.to_string();
        Ok(layout)
    }

    /// Save `layout` as `name` (overwriting a saved one of that name).
    pub fn save(&self, name: &str, mut layout: Layout) -> Result<(), String> {
        let file = self.file(name)?;
        layout.name = name.to_string();
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&layout).map_err(|e| e.to_string())?;
        fs::write(file, json).map_err(|e| format!("Couldn't save \"{name}\": {e}"))
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        fs::remove_file(self.file(name)?).map_err(|e| format!("Couldn't delete \"{name}\": {e}"))
    }

    pub fn folder(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        Ok(self.dir.clone())
    }

    fn file(&self, name: &str) -> Result<PathBuf, String> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || name.starts_with('.') {
            return Err(format!("\"{name}\" can't be used as a layout name"));
        }
        if self.kind.builtins().iter().any(|l| l.name.eq_ignore_ascii_case(name)) {
            return Err(format!("\"{name}\" is a built-in layout; pick another name"));
        }
        Ok(self.dir.join(Path::new(name).with_extension(PRESET_EXT)))
    }
}

/// Pointer and scroll speed from frames, smoothed for the live view.
#[derive(Debug, Default)]
pub(crate) struct Speed {
    last: Option<Duration>,
    mouse: [f32; 2],
    scroll: [f32; 2],
}

impl Speed {
    pub(crate) fn add(&mut self, f: &Frame, now: Duration) {
        let dt = self.last.map_or(0.0, |l| now.saturating_sub(l).as_secs_f32());
        self.last = Some(now);
        if dt <= 0.0 {
            return;
        }
        for i in 0..2 {
            self.mouse[i] += (f.mouse[i] / dt - self.mouse[i]) * 0.1;
            self.scroll[i] += (f.scroll[i] / dt - self.scroll[i]) * 0.1;
        }
    }
}

/// A frame as the live view shows it.
pub(crate) fn preview(slot: Option<Slot>, f: Option<&Frame>, speed: &Speed) -> Preview {
    match f {
        Some(f) => Preview {
            slot,
            pad: f.pad,
            keys: f.keys.iter().map(|k| KEYS[k as usize]).collect(),
            mouse_buttons: f.mouse_buttons,
            mouse: speed.mouse,
            scroll: speed.scroll,
            gyro_active: f.gyro_active,
            lit: f.lit,
        },
        None => Preview { slot, ..Default::default() },
    }
}

type Opened = Result<Box<dyn VirtualPad>, String>;

pub struct PlayOut {
    config: Config,
    mode: Mode,
    /// The layout `engine` runs (None: nothing in play).
    applied: Option<Slot>,
    engine: Engine,
    injector: Injector,
    keymouse: Option<Box<dyn KeyMouse>>,
    keymouse_error: Option<String>,
    /// Runs the layout of the tab in view when it isn't the one in play, for its live view only.
    preview_slot: Option<Slot>,
    preview_engine: Option<(Slot, Engine)>,
    speed: Speed,
    pad: Option<Box<dyn VirtualPad>>,
    pad_opening: Option<mpsc::Receiver<Opened>>,
    pad_last_try: Option<Instant>,
    pad_error: Option<String>,
    pad_neutral_sent: bool,
    /// A Switch fired and its button is still down.
    switch_held: bool,
    player: Option<u8>,
    player_checked: Option<Instant>,
    rumble: (u8, u8),
    support: Option<PadSupport>,
    support_check: Option<mpsc::Receiver<PadSupport>>,
    install: Option<mpsc::Receiver<Result<(), String>>>,
    install_error: Option<String>,
    /// The full-screen window auto switch last saw: a manual switch holds until it changes.
    fullscreen: Option<(isize, String)>,
    foreground_checked: Option<Instant>,
    /// The program in front, by file name, and its Command Deck page (`DeckConfig::apps` index).
    front: Option<String>,
    app: Option<usize>,
    recent_apps: Vec<String>,
    deck: DeckEngine,
    /// The page `deck` runs.
    deck_page: Option<usize>,
    /// The page the Command Deck tab shows, while it does, and an engine for its live view when
    /// it isn't the one playing.
    deck_preview: Option<Option<usize>>,
    deck_preview_engine: Option<(Option<usize>, DeckEngine)>,
    /// What the on-screen menu last showed: (wanted, menus' version, page).
    overlay_key: Option<(bool, u64, Option<usize>)>,
    /// Results of opening programs, files and URLs (Run actions), each on a thread of its own.
    run_results: (mpsc::Sender<Result<(), String>>, mpsc::Receiver<Result<(), String>>),
    run_error: Option<String>,
    /// Haptics for the controller, for `forward_events` to send.
    haptics: Vec<Haptic>,
    start: Instant,
    dir: PathBuf,
    /// Writes `config.json` (see `MidiOut::save_config`, same reason).
    saver: mpsc::Sender<String>,
}

impl PlayOut {
    /// Load the saved config from `dir` (layouts go in `dir/keymouse` and `dir/gamepad`).
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
        let mut out = PlayOut {
            engine: Engine::new(Layout::default()),
            config,
            mode: Mode::Desktop,
            applied: None,
            injector: Injector::default(),
            keymouse: None,
            keymouse_error: None,
            preview_slot: None,
            preview_engine: None,
            speed: Speed::default(),
            pad: None,
            pad_opening: None,
            pad_last_try: None,
            pad_error: None,
            pad_neutral_sent: false,
            switch_held: false,
            player: None,
            player_checked: None,
            rumble: (0, 0),
            support: None,
            support_check: None,
            install: None,
            install_error: None,
            fullscreen: None,
            foreground_checked: None,
            front: None,
            app: None,
            recent_apps: Vec::new(),
            deck: DeckEngine::new(Deck::default()),
            deck_page: None,
            deck_preview: None,
            deck_preview_engine: None,
            overlay_key: None,
            run_results: mpsc::channel(),
            run_error: None,
            haptics: Vec::new(),
            start: Instant::now(),
            dir,
            saver,
        };
        out.check_driver();
        out.apply_active();
        out.apply_deck();
        out
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn presets(&self, kind: PresetKind) -> PresetStore {
        PresetStore::new(self.dir.join(kind.folder()), kind)
    }

    fn save_config(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.config) {
            let _ = self.saver.send(json);
        }
    }

    fn active(&self) -> Option<Slot> {
        active_slot(self.mode, self.config.keymouse.enabled, self.config.gamepad.enabled)
    }

    /// After a change of mode or config: put the layout that should play in the engine, letting
    /// go of what the old one held.
    fn apply_active(&mut self) {
        let want = self.active();
        let same = want == self.applied && want.is_none_or(|s| self.engine.layout() == self.config.layout(s));
        if same {
            return;
        }
        self.release_input();
        self.applied = want;
        self.engine.set_layout(want.map(|s| self.config.layout(s).clone()).unwrap_or_default());
        if want.is_none() && !self.config.deck.enabled {
            // Nothing to send: let the keyboard and mouse device go (on Linux it's a device).
            self.keymouse = None;
            self.keymouse_error = None;
        }
    }

    /// Let go of held keys and buttons and center the pad.
    fn release_input(&mut self) {
        let events = self.injector.release().to_vec();
        self.send_keymouse(&events);
        self.pad_neutral_sent = false;
        if let Some(pad) = &mut self.pad {
            let _ = pad.update(&PadState::default());
            self.pad_neutral_sent = true;
        }
    }

    fn send_keymouse(&mut self, events: &[Event]) {
        if events.is_empty() {
            return;
        }
        if self.keymouse.is_none() {
            match devices::open_keymouse() {
                Ok(k) => self.keymouse = Some(k),
                Err(e) => {
                    self.keymouse_error = Some(e);
                    return;
                }
            }
        }
        if let Some(k) = &mut self.keymouse {
            self.keymouse_error = k.send(events).err();
        }
    }

    /// After a change of the deck's config or the program in front: run the page that should
    /// play, letting go of what the old one held.
    fn apply_deck(&mut self) {
        let page = if self.config.deck.enabled { self.app } else { None };
        let deck = self.config.deck.page(page).cloned().unwrap_or_default();
        if page == self.deck_page && *self.deck.deck() == deck {
            return;
        }
        self.release_input();
        self.deck_page = page;
        self.deck = DeckEngine::new(deck);
        if !self.config.deck.enabled && self.applied.is_none() {
            self.keymouse = None;
            self.keymouse_error = None;
        }
    }

    pub fn set_keymouse(&mut self, cfg: KeyMouseConfig) {
        self.config.keymouse = cfg;
        self.save_config();
        self.apply_active();
    }

    /// The Command Deck's on/off, on-screen menu and pages (the tab sends them after every edit).
    pub fn set_deck(&mut self, cfg: DeckConfig) {
        self.config.deck = cfg;
        self.save_config();
        // The pages may have changed: match the program in front again.
        self.app = self.app_page();
        self.apply_deck();
        self.apply_active();
    }

    /// Which page the Command Deck tab shows, while it is open (None: closed).
    pub fn set_deck_preview(&mut self, page: Option<Option<usize>>) {
        self.deck_preview = page;
        self.deck_preview_engine = None;
    }

    pub fn set_gamepad(&mut self, cfg: GamepadConfig) {
        let was_on = self.config.gamepad.enabled;
        self.config.gamepad = cfg;
        self.save_config();
        self.apply_active();
        if was_on && !self.config.gamepad.enabled {
            self.close_pad();
        }
        if !was_on && self.config.gamepad.enabled {
            self.pad_last_try = None;
            self.pad_error = None;
        }
    }

    pub fn set_auto_switch(&mut self, on: bool) {
        self.config.auto_switch = on;
        // Start over: the window in front now decides at the next poll.
        self.fullscreen = None;
        self.foreground_checked = None;
        self.save_config();
    }

    /// Switch modes by hand (the tabs' Desktop | Game control).
    pub fn set_mode(&mut self, mode: Mode) {
        if mode != self.mode {
            self.mode = mode;
            self.apply_active();
            self.haptics.push(SWITCH_CLICK);
        }
    }

    /// Which layout the tab in view shows, for its live view.
    pub fn set_preview(&mut self, slot: Option<Slot>) {
        self.preview_slot = slot;
        self.preview_engine = None;
    }

    /// Feed one state report from whatever the UI is showing (live input or a replay).
    /// `watched`: the Keyboard & mouse or Gamepad tab is in view, so its layout runs for the
    /// live view even when it isn't the one in play.
    /// `watched`: the Keyboard & mouse or Gamepad tab, and the Command Deck tab, are in view.
    pub fn process(&mut self, s: &ControllerState, watched: bool, deck_watched: bool) {
        let now = self.start.elapsed();
        let deck_on = self.config.deck.enabled;
        // The layout gets the controller without the controls the deck takes.
        let masked;
        let layout_state = if deck_on {
            self.deck.process(s, now);
            masked = self.deck.mask(s);
            &masked
        } else {
            s
        };
        if self.applied.is_some() {
            self.engine.process(layout_state, now);
        }
        if self.applied.is_some() || deck_on {
            let mut frame = if self.applied.is_some() { self.engine.frame().clone() } else { Frame::default() };
            if deck_on {
                frame.merge(self.deck.frame());
            }
            // The other mode's engine starts fresh and would take the still-held button for a new
            // press, flipping back on every report: wait for the button to be let go.
            let switch = frame.switch && !self.switch_held;
            self.switch_held = frame.switch_held && (switch || self.switch_held);
            let events = self.injector.apply(&frame).to_vec();
            for side in 0..2 {
                if frame.ticks & (1 << side) != 0 {
                    self.haptics.push(Haptic::Command { side, command: MENU_TICK, gain_db: 0 });
                }
            }
            for target in frame.runs {
                let done = self.run_results.0.clone();
                thread::spawn(move || {
                    let result = tauri_plugin_opener::open_url(&target, None::<&str>).map_err(|e| format!("Couldn't open {target}: {e}"));
                    let _ = done.send(result);
                });
            }
            if self.applied == self.preview_slot {
                self.speed.add(self.engine.frame(), now);
            }
            self.send_keymouse(&events);
            if switch {
                let mode = match self.mode {
                    Mode::Desktop => Mode::Game,
                    Mode::Game => Mode::Desktop,
                };
                self.set_mode(mode);
            }
        }
        if let Some(pad) = &mut self.pad {
            let state = if self.applied == Some(Slot::Pad) { Some(self.engine.frame().pad) } else { None };
            let result = match state {
                Some(state) => {
                    self.pad_neutral_sent = false;
                    pad.update(&state)
                }
                None if !self.pad_neutral_sent => {
                    self.pad_neutral_sent = true;
                    pad.update(&PadState::default())
                }
                None => Ok(()),
            };
            if let Err(e) = result {
                self.pad_error = Some(e);
                self.close_pad();
                self.pad_last_try = Some(Instant::now());
            }
        }
        match self.preview_slot {
            Some(slot) if watched && self.applied != Some(slot) => {
                let layout = self.config.layout(slot);
                if self.preview_engine.as_ref().is_none_or(|(s, e)| *s != slot || e.layout() != layout) {
                    self.preview_engine = Some((slot, Engine::new(layout.clone())));
                }
                let (_, engine) = self.preview_engine.as_mut().unwrap();
                self.speed.add(engine.process(s, now), now);
            }
            _ => self.preview_engine = None,
        }
        // The Command Deck tab's page, when it isn't the one playing.
        match self.deck_preview {
            Some(page) if deck_watched && !(deck_on && page == self.deck_page) && self.config.deck.page(page).is_some() => {
                let deck = self.config.deck.page(page).unwrap();
                if self.deck_preview_engine.as_ref().is_none_or(|(p, e)| *p != page || e.deck() != deck) {
                    self.deck_preview_engine = Some((page, DeckEngine::new(deck.clone())));
                }
                self.deck_preview_engine.as_mut().unwrap().1.process(s, now);
            }
            _ => self.deck_preview_engine = None,
        }
    }

    /// Let go of everything held, e.g. when switching between live input and a replay or losing
    /// the controller.
    pub fn release_all(&mut self) {
        self.release_input();
        self.engine.reset();
        self.deck.reset();
        self.preview_engine = None;
        self.deck_preview_engine = None;
        self.speed = Speed::default();
    }

    fn close_pad(&mut self) {
        self.pad = None;
        self.player = None;
        self.rumble = (0, 0);
    }

    /// Look again whether a virtual pad can be made (the driver, or access to /dev/uinput).
    /// Slow on Windows, so on a thread; the Gamepad tab asks when it opens.
    pub fn check_driver(&mut self) {
        if self.support_check.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(devices::pad_support());
        });
        self.support_check = Some(rx);
    }

    /// Run the embedded ViGEmBus installer (Windows asks for administrator permission).
    pub fn install_driver(&mut self) -> Result<(), String> {
        if self.install.is_some() {
            return Ok(());
        }
        let dir = self.dir.join("installer");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(install(&dir));
        });
        self.install = Some(rx);
        self.install_error = None;
        Ok(())
    }

    /// Called often from the event loop: picks up the pad that finished plugging in, driver checks
    /// and installs, rumble, and polls the foreground window for auto switch. `deck_watched`:
    /// the Command Deck tab is in view.
    pub fn tick(&mut self, deck_watched: bool) {
        if let Some(rx) = &self.support_check {
            match rx.try_recv() {
                Ok(support) => {
                    self.support = Some(support);
                    self.support_check = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.support_check = None,
            }
        }
        if let Some(rx) = &self.install {
            match rx.try_recv() {
                Ok(result) => {
                    self.install_error = result.err();
                    self.install = None;
                    self.check_driver();
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.install = None,
            }
        }
        if let Some(rx) = &self.pad_opening {
            match rx.try_recv() {
                Ok(result) => {
                    self.pad_opening = None;
                    match result {
                        // Turned off while it was plugging in: it unplugs as it drops.
                        Ok(_) if !self.config.gamepad.enabled => {}
                        Ok(pad) => {
                            self.pad = Some(pad);
                            self.pad_error = None;
                            self.pad_neutral_sent = false;
                        }
                        Err(e) => {
                            self.pad_error = Some(e);
                            self.pad_last_try = Some(Instant::now());
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.pad_opening = None,
            }
        } else if self.config.gamepad.enabled
            && self.pad.is_none()
            && self.support == Some(PadSupport::Ready)
            && self.pad_last_try.is_none_or(|t| t.elapsed() >= RETRY)
        {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(devices::open_pad());
            });
            self.pad_opening = Some(rx);
            self.pad_last_try = Some(Instant::now());
        }
        if let Some(pad) = &mut self.pad {
            if let Some(r) = pad.take_rumble() {
                self.rumble = r;
            }
            if self.player.is_none() && self.player_checked.is_none_or(|t| t.elapsed() >= PLAYER_POLL) {
                self.player = pad.player();
                self.player_checked = Some(Instant::now());
            }
        }
        while let Ok(result) = self.run_results.1.try_recv() {
            self.run_error = result.err();
        }
        // Also while the Command Deck tab is in view, for the programs it offers.
        let watching = deck_watched && self.deck_preview.is_some();
        if self.config.keymouse.enabled || self.config.deck.enabled || watching {
            if self.foreground_checked.is_none_or(|t| t.elapsed() >= FOREGROUND_POLL) {
                self.foreground_checked = Some(Instant::now());
                self.foreground(devices::foreground());
            }
        } else {
            self.fullscreen = None;
            self.front = None;
            self.set_app(None);
        }
    }

    /// The window in front. Auto switch switches only when the full-screen one changed, so a
    /// manual switch holds; a program with a layout of its own gets it.
    fn foreground(&mut self, seen: Option<Foreground>) {
        if let Some(f) = &seen {
            if self.recent_apps.first() != Some(&f.app) {
                self.recent_apps.retain(|a| a != &f.app);
                self.recent_apps.insert(0, f.app.clone());
                self.recent_apps.truncate(RECENT_APPS);
            }
        }
        if self.config.auto_switch && self.config.keymouse.enabled {
            let full = seen.as_ref().filter(|f| f.fullscreen).map(|f| (f.id, f.app.clone()));
            if full.as_ref().map(|s| s.0) != self.fullscreen.as_ref().map(|s| s.0) {
                self.set_mode(if full.is_some() { Mode::Game } else { Mode::Desktop });
            }
            self.fullscreen = full;
        } else {
            self.fullscreen = None;
        }
        self.front = seen.map(|f| f.app);
        self.set_app(self.app_page());
    }

    /// The Command Deck page of the program in front.
    fn app_page(&self) -> Option<usize> {
        let front = self.front.as_deref()?;
        self.config.deck.apps.iter().position(|a| a.matches(front))
    }

    fn set_app(&mut self, app: Option<usize>) {
        if app != self.app {
            self.app = app;
            self.apply_deck();
        }
    }

    /// The on-screen menu should exist (it is made ahead, hidden, to show without a delay).
    pub fn overlay_wanted(&self) -> bool {
        let d = &self.config.deck;
        d.enabled && d.overlay && devices::KEYMOUSE_SUPPORTED && d.page(self.deck_page).is_some_and(|p| p.menus().iter().any(|m| **m != Menu::None))
    }

    /// The on-screen menu, when it changed since the last call: Some(None) hides it.
    pub fn take_overlay(&mut self) -> Option<Option<OverlayMenu>> {
        let wanted = self.overlay_wanted();
        let key = (wanted, self.deck.version(), self.deck_page);
        if self.overlay_key == Some(key) {
            return None;
        }
        self.overlay_key = Some(key);
        let part = self.deck.focus().filter(|_| wanted);
        Some(part.map(|part| {
            let i = crate::deck::PARTS.iter().position(|p| *p == part).unwrap();
            OverlayMenu { part, menu: self.deck.deck().menus()[i].clone(), selected: self.deck.menus()[i].selected }
        }))
    }

    /// Haptics to play on the controller: the mode switch click.
    pub fn take_haptics(&mut self) -> Vec<Haptic> {
        std::mem::take(&mut self.haptics)
    }

    /// What the game asks the pad's motors for, 0..255 (large, small).
    pub fn rumble(&self) -> (u8, u8) {
        self.rumble
    }

    pub fn status(&self) -> PlayStatus {
        let frame = match (&self.preview_engine, self.preview_slot) {
            (Some((_, e)), _) => Some(e.frame()),
            (None, Some(slot)) if self.applied == Some(slot) => Some(self.engine.frame()),
            _ => None,
        };
        let preview = preview(self.preview_slot, frame, &self.speed);
        PlayStatus {
            mode: self.mode,
            auto_switch: self.config.auto_switch,
            fullscreen_app: self.fullscreen.as_ref().map(|f| f.1.clone()),
            active: self.applied,
            keymouse: KeyMouseStatus {
                enabled: self.config.keymouse.enabled,
                supported: devices::KEYMOUSE_SUPPORTED,
                error: self.keymouse_error.clone(),
            },
            gamepad: GamepadStatus {
                enabled: self.config.gamepad.enabled,
                support: self.support.clone(),
                connected: self.pad.is_some(),
                opening: self.pad_opening.is_some(),
                player: self.player,
                error: self.pad_error.clone(),
                installing: self.install.is_some(),
                install_error: self.install_error.clone(),
                rumble: self.rumble,
            },
            preview,
            deck: self.deck_status(),
        }
    }

    fn deck_status(&self) -> DeckStatus {
        let d = &self.config.deck;
        let page = self.deck_preview.flatten();
        let engine = match (&self.deck_preview_engine, self.deck_preview) {
            (Some((_, e)), _) => Some(e),
            (None, Some(p)) if d.enabled && p == self.deck_page => Some(&self.deck),
            _ => None,
        };
        DeckStatus {
            enabled: d.enabled,
            overlay: d.overlay,
            supported: devices::KEYMOUSE_SUPPORTED,
            apps_supported: devices::FOREGROUND_SUPPORTED,
            page: self.deck_page,
            app: self.deck_page.and_then(|i| d.apps.get(i)).map(|a| a.app.clone()),
            recent_apps: self.recent_apps.clone(),
            run_error: self.run_error.clone(),
            preview: DeckPreview {
                page,
                shown: self.deck_preview.is_some(),
                lit: engine.map_or(0, |e| e.frame().lit),
                menus: engine.map_or_else(Default::default, |e| e.menus()),
            },
        }
    }
}

#[cfg(windows)]
fn install(dir: &Path) -> Result<(), String> {
    crate::vigem::install(dir)
}

#[cfg(not(windows))]
fn install(_dir: &Path) -> Result<(), String> {
    Err("There is no driver to install on this system".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Action;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-play-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn full(id: isize, app: &str) -> Foreground {
        Foreground { id, app: app.into(), fullscreen: true }
    }

    fn deck_with_wheel() -> Deck {
        use crate::deck::{Item, Pick};
        let items = vec![Item { label: "Up".into(), action: Action::key(&["F1"]) }, Item { label: "Down".into(), action: Action::key(&["F2"]) }];
        Deck { right_stick: Menu::Wheel { items, pick: Pick::Release }, ..Deck::default() }
    }

    fn stick(y: i16) -> ControllerState {
        ControllerState { right_stick: [0, y], buttons: sc_core::protocol::buttons::RIGHT_STICK_TOUCH, ..Default::default() }
    }

    #[test]
    fn a_program_with_its_own_page_gets_it() {
        let dir = temp_dir("apps");
        let mut play = PlayOut::new(dir.clone());
        let photo = AppDeck { app: "Photoshop".into(), deck: deck_with_wheel() };
        assert!(photo.matches("photoshop.EXE") && !photo.matches("photoshop2.exe"));
        assert!(!AppDeck::default().matches(""));
        play.set_deck(DeckConfig { enabled: true, apps: vec![photo], ..Default::default() });
        assert_eq!(play.status().deck.page, None);
        play.foreground(Some(Foreground { fullscreen: false, ..full(1, "Photoshop.exe") }));
        assert_eq!((play.status().deck.page, play.status().deck.app.as_deref()), (Some(0), Some("Photoshop")));
        // Removed and added again while it's in front.
        let apps = play.config().deck.apps.clone();
        play.set_deck(DeckConfig { apps: Vec::new(), ..play.config().deck.clone() });
        assert_eq!(play.status().deck.page, None);
        play.set_deck(DeckConfig { apps, ..play.config().deck.clone() });
        assert_eq!(play.status().deck.page, Some(0));
        play.foreground(Some(Foreground { fullscreen: false, ..full(2, "explorer.exe") }));
        assert_eq!(play.status().deck.page, None);
        // Off: no page plays.
        play.foreground(Some(Foreground { fullscreen: false, ..full(1, "Photoshop.exe") }));
        play.set_deck(DeckConfig { enabled: false, ..play.config().deck.clone() });
        assert_eq!(play.status().deck.page, None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_deck_ticks_and_takes_its_stick_from_the_layout() {
        let dir = temp_dir("deck");
        let mut play = PlayOut::new(dir.clone());
        // The Desktop layout moves the pointer with the right stick; the deck takes it.
        play.set_keymouse(KeyMouseConfig { enabled: true, ..Default::default() });
        play.set_deck(DeckConfig { enabled: true, deck: deck_with_wheel(), ..Default::default() });
        play.set_preview(Some(Slot::Desktop));
        for _ in 0..3 {
            play.process(&stick(30000), true, false);
        }
        play.process(&stick(-30000), true, false);
        assert_eq!(play.take_haptics(), vec![Haptic::Command { side: 1, command: MENU_TICK, gain_db: 0 }; 2]);
        assert_eq!(play.status().preview.mouse, [0.0, 0.0]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_overlay_follows_the_menu_touched() {
        let dir = temp_dir("overlay");
        let mut play = PlayOut::new(dir.clone());
        // Off: nothing to show, said once.
        assert_eq!(play.take_overlay(), Some(None));
        assert_eq!(play.take_overlay(), None);
        play.set_deck(DeckConfig { enabled: true, deck: deck_with_wheel(), ..Default::default() });
        assert!(play.overlay_wanted());
        play.process(&stick(30000), false, false);
        let shown = play.take_overlay().unwrap().unwrap();
        assert_eq!((shown.part, shown.selected), (Part::RightStick, Some(0)));
        play.process(&stick(30000), false, false);
        assert_eq!(play.take_overlay(), None);
        play.process(&ControllerState::default(), false, false);
        assert_eq!(play.take_overlay(), Some(None));
        // Without the on-screen menu.
        play.set_deck(DeckConfig { overlay: false, ..play.config().deck.clone() });
        assert!(!play.overlay_wanted());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_layout_by_mode() {
        use Mode::*;
        assert_eq!(active_slot(Desktop, false, false), None);
        assert_eq!(active_slot(Game, false, false), None);
        // Only one on: it plays in both modes.
        assert_eq!(active_slot(Desktop, true, false), Some(Slot::Desktop));
        assert_eq!(active_slot(Game, true, false), Some(Slot::Game));
        assert_eq!(active_slot(Desktop, false, true), Some(Slot::Pad));
        assert_eq!(active_slot(Game, false, true), Some(Slot::Pad));
        // Both: keyboard and mouse on the desktop, the gamepad in games.
        assert_eq!(active_slot(Desktop, true, true), Some(Slot::Desktop));
        assert_eq!(active_slot(Game, true, true), Some(Slot::Pad));
    }

    #[test]
    fn switching_modes_clicks_and_changes_layout() {
        let dir = temp_dir("mode");
        let mut play = PlayOut::new(dir.clone());
        assert_eq!(play.status().active, None);
        play.set_keymouse(KeyMouseConfig { enabled: true, ..Default::default() });
        assert_eq!(play.status().active, Some(Slot::Desktop));
        assert!(play.take_haptics().is_empty());
        play.set_mode(Mode::Game);
        assert_eq!(play.status().active, Some(Slot::Game));
        assert_eq!(play.take_haptics(), vec![SWITCH_CLICK]);
        // The same mode again: no click.
        play.set_mode(Mode::Game);
        assert!(play.take_haptics().is_empty());
        play.set_gamepad(GamepadConfig { enabled: true, ..Default::default() });
        assert_eq!(play.status().active, Some(Slot::Pad));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn holding_the_switch_button_switches_once() {
        let dir = temp_dir("hold");
        let mut play = PlayOut::new(dir.clone());
        play.set_keymouse(KeyMouseConfig { enabled: true, ..Default::default() });
        let steam = ControllerState { buttons: sc_core::protocol::buttons::STEAM, ..Default::default() };
        let idle = ControllerState::default();
        for _ in 0..10 {
            play.process(&steam, false, false);
        }
        assert_eq!(play.mode(), Mode::Game);
        assert_eq!(play.take_haptics().len(), 1);
        play.process(&idle, false, false);
        play.process(&steam, false, false);
        assert_eq!(play.mode(), Mode::Desktop);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_switch_follows_full_screen_and_manual_switch_holds() {
        let dir = temp_dir("auto");
        let mut play = PlayOut::new(dir.clone());
        // Keyboard & mouse off: auto switch leaves the mode alone.
        play.foreground(Some(full(7, "game.exe")));
        assert_eq!(play.mode(), Mode::Desktop);
        play.set_keymouse(KeyMouseConfig { enabled: true, ..Default::default() });
        play.foreground(Some(full(7, "game.exe")));
        assert_eq!(play.mode(), Mode::Game);
        assert_eq!(play.status().fullscreen_app.as_deref(), Some("game.exe"));
        // Back to the desktop by hand: holds while the same game stays in front.
        play.set_mode(Mode::Desktop);
        play.foreground(Some(full(7, "game.exe")));
        assert_eq!(play.mode(), Mode::Desktop);
        // The game closes: Desktop (already); a window that isn't full screen: still Desktop.
        play.foreground(None);
        assert_eq!(play.mode(), Mode::Desktop);
        play.foreground(Some(Foreground { fullscreen: false, ..full(8, "notepad.exe") }));
        assert_eq!(play.mode(), Mode::Desktop);
        // Another one goes full screen: Game.
        play.foreground(Some(full(9, "other.exe")));
        assert_eq!(play.mode(), Mode::Game);
        assert_eq!(play.status().deck.recent_apps, ["other.exe", "notepad.exe", "game.exe"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_runs_only_while_watched() {
        let dir = temp_dir("preview");
        let mut play = PlayOut::new(dir.clone());
        play.set_preview(Some(Slot::Pad));
        let s = ControllerState { buttons: crate::shaping::button_mask("A").unwrap(), ..Default::default() };
        play.process(&s, false, false);
        assert_eq!(play.status().preview.pad.buttons, 0);
        play.process(&s, true, false);
        assert_eq!(play.status().preview.pad.buttons, crate::layout::xbutton::A);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_is_saved_and_restored() {
        let dir = temp_dir("config");
        let mut play = PlayOut::new(dir.clone());
        assert_eq!(play.config(), &Config::default());
        let present = play.presets(PresetKind::Keymouse).load("Presentation", true).unwrap();
        play.set_auto_switch(false);
        play.set_keymouse(KeyMouseConfig { enabled: false, desktop: present.clone(), ..Default::default() });
        let started = Instant::now();
        loop {
            let saved = fs::read_to_string(dir.join("config.json")).ok().and_then(|s| serde_json::from_str::<Config>(&s).ok());
            match saved {
                Some(c) if c.keymouse.desktop.name == "Presentation" => break,
                _ if started.elapsed() > Duration::from_secs(5) => panic!("config.json was not saved"),
                _ => thread::sleep(Duration::from_millis(10)),
            }
        }
        let restored = PlayOut::new(dir.clone());
        assert_eq!(restored.config(), play.config());
        assert!(!restored.config().auto_switch);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn presets_are_stored_per_kind() {
        let dir = temp_dir("presets");
        let play = PlayOut::new(dir.clone());
        let pads = play.presets(PresetKind::Gamepad);
        let keys = play.presets(PresetKind::Keymouse);
        assert!(pads.list().iter().all(|p| p.builtin));
        let racing = pads.load("Racing (tilt to steer)", true).unwrap();
        pads.save("My racer", racing.clone()).unwrap();
        assert_eq!(pads.list().last().map(|p| (p.name.as_str(), p.builtin)), Some(("My racer", false)));
        assert!(keys.list().iter().all(|p| p.name != "My racer"));
        assert_eq!(pads.load("My racer", false).unwrap(), Layout { name: "My racer".into(), ..racing });
        // Built-in names and odd names are refused.
        assert!(pads.save("standard", Layout::default()).is_err());
        assert!(keys.save("a/b", Layout::default()).is_err());
        pads.delete("My racer").unwrap();
        assert!(pads.list().iter().all(|p| p.builtin));
        let _ = fs::remove_dir_all(&dir);
    }
}
