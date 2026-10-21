//! The Command Deck: shortcuts on the controller, like a stream deck in the hand. A [`Deck`] binds
//! buttons to actions and gives each stick a wheel (a radial menu: tilt to an item, let go to use
//! it) and each trackpad a grid (touch a cell, click to use it). Pure logic; `play_out.rs` runs
//! it next to the Keyboard & mouse or Gamepad layout, which no longer sees the controls the deck
//! takes ([`DeckEngine::mask`]), and sends what it makes through the same keyboard and mouse.
//!
//! Every menu tells which item is under the thumb ([`DeckEngine::menus`]) for the tab's live view
//! and the on-screen menu (`overlay.rs`), with a haptic tick when it changes.

use std::collections::BTreeMap;
use std::time::Duration;

use sc_core::protocol::buttons;
use sc_core::ControllerState;
use serde::{Deserialize, Serialize};

use crate::layout::{apply, Act, Action, Frame, Press, CONTROLS};
use crate::shaping::{axis, button_mask};

/// One entry of a wheel or grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Item {
    /// Shown in the menu; empty: the action's own name.
    pub label: String,
    pub action: Action,
}

/// When a menu's item plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Pick {
    /// Wheels: when the stick goes back to the centre. Grids: when the thumb lifts.
    #[default]
    Release,
    /// When the stick or trackpad is clicked; held while it stays clicked.
    Click,
}

/// What a stick or trackpad does on the deck.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Menu {
    /// Left to the Keyboard & mouse or Gamepad layout.
    #[default]
    None,
    /// A radial menu: a slice per item, the first centred at the top, clockwise.
    Wheel { items: Vec<Item>, pick: Pick },
    /// Trackpads: `cols` x `rows` cells, row by row from the top left.
    Grid { cols: u8, rows: u8, items: Vec<Item>, pick: Pick },
}

impl Menu {
    fn items(&self) -> &[Item] {
        match self {
            Menu::None => &[],
            Menu::Wheel { items, .. } | Menu::Grid { items, .. } => items,
        }
    }

    fn pick(&self) -> Pick {
        match self {
            Menu::None => Pick::Release,
            Menu::Wheel { pick, .. } | Menu::Grid { pick, .. } => *pick,
        }
    }
}

/// A page of shortcuts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Deck {
    /// Control name (`layout::CONTROLS`) to action; missing ones are left to the layout.
    pub buttons: BTreeMap<String, Action>,
    pub left_stick: Menu,
    pub right_stick: Menu,
    pub left_pad: Menu,
    pub right_pad: Menu,
}

/// The four menus, in the order of [`Deck::menus`] and [`MenuView`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    LeftStick,
    RightStick,
    LeftPad,
    RightPad,
}

pub const PARTS: [Part; 4] = [Part::LeftStick, Part::RightStick, Part::LeftPad, Part::RightPad];

impl Part {
    fn stick(self) -> bool {
        matches!(self, Part::LeftStick | Part::RightStick)
    }
    /// The haptic side: 0 left, 1 right.
    pub fn side(self) -> u8 {
        matches!(self, Part::RightStick | Part::RightPad) as u8
    }
    /// The stick's or pad's click, and its touch sensor.
    fn bits(self) -> (u32, u32) {
        match self {
            Part::LeftStick => (button_mask("L3").unwrap_or(0), buttons::LEFT_STICK_TOUCH),
            Part::RightStick => (button_mask("R3").unwrap_or(0), buttons::RIGHT_STICK_TOUCH),
            Part::LeftPad => (buttons::LEFT_PAD_CLICK, buttons::LEFT_PAD_TOUCH),
            Part::RightPad => (buttons::RIGHT_PAD_CLICK, buttons::RIGHT_PAD_TOUCH),
        }
    }
}

impl Deck {
    pub fn menus(&self) -> [&Menu; 4] {
        [&self.left_stick, &self.right_stick, &self.left_pad, &self.right_pad]
    }
}

/// A wheel's stick selects past this tilt, and counts as back in the centre below the second.
const WHEEL_SELECT: f32 = 0.55;
const WHEEL_CENTRE: f32 = 0.3;
/// A trackpad wheel selects this far from the centre (fraction of the radius).
const PAD_WHEEL_SELECT: f32 = 0.35;
/// An item picked on release is pressed this long.
const TAP: Duration = Duration::from_millis(40);
/// The thumb has to go this far (radians) past a slice's edge to change item, so resting on the
/// edge doesn't tick back and forth.
const WHEEL_HYSTERESIS: f32 = 0.08;
/// The same for grids, as a fraction of a cell.
const GRID_HYSTERESIS: f32 = 0.12;
/// A trigger bound on the deck presses past this pull.
const TRIGGER_PRESS: f32 = 0.3;

/// The slice of `n` at (x, y) (y up), the first centred at the top, clockwise. `current` stays
/// until the thumb is clearly past its edge.
pub fn wheel_slice(x: f32, y: f32, n: usize, current: Option<usize>) -> usize {
    use std::f32::consts::TAU;
    let width = TAU / n as f32;
    let angle = x.atan2(y).rem_euclid(TAU);
    let slice = ((angle / width).round() as usize) % n;
    match current {
        Some(c) if c < n && c != slice => {
            let off = (angle - c as f32 * width + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0;
            if off.abs() > width / 2.0 + WHEEL_HYSTERESIS { slice } else { c }
        }
        _ => slice,
    }
}

/// The cell at (x, y) in -1..1 (y up) of a `cols` x `rows` grid, row by row from the top left.
/// `current` stays until the thumb is clearly inside another cell.
pub fn grid_cell(x: f32, y: f32, cols: usize, rows: usize, current: Option<usize>) -> usize {
    let fx = ((x + 1.0) / 2.0 * cols as f32).clamp(0.0, cols as f32 - 0.001);
    let fy = ((1.0 - y) / 2.0 * rows as f32).clamp(0.0, rows as f32 - 0.001);
    let cell = fy as usize * cols + fx as usize;
    match current {
        Some(c) if c < cols * rows && c != cell => {
            let (cx, cy) = ((c % cols) as f32, (c / cols) as f32);
            let inside = |v: f32, lo: f32| v > lo - GRID_HYSTERESIS && v < lo + 1.0 + GRID_HYSTERESIS;
            if inside(fx, cx) && inside(fy, cy) { c } else { cell }
        }
        _ => cell,
    }
}

/// Where one menu is.
#[derive(Debug, Clone, Copy, Default)]
struct MenuState {
    /// Touched: the on-screen menu shows.
    open: bool,
    selected: Option<usize>,
    /// Picked by a click, held until it's let go.
    held: Option<usize>,
    /// Picked on release, pressed until then.
    tap: Option<(usize, Duration)>,
    clicked: bool,
}

/// A menu as the tab and the on-screen menu show it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct MenuView {
    pub open: bool,
    /// The item under the thumb, or playing.
    pub selected: Option<u8>,
}

/// What the deck takes from the Keyboard & mouse or Gamepad layout.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Claims {
    buttons: u32,
    triggers: [bool; 2],
    sticks: [bool; 2],
    pads: [bool; 2],
}

struct Compiled {
    buttons: Vec<(Act, Option<u32>)>,
    menus: [Vec<Act>; 4],
    claims: Claims,
}

impl Compiled {
    fn new(d: &Deck) -> Self {
        let buttons: Vec<(Act, Option<u32>)> =
            CONTROLS.iter().map(|c| (d.buttons.get(*c).map_or(Act::None, Act::new), button_mask(c))).collect();
        let mut claims = Claims::default();
        for (i, (act, mask)) in buttons.iter().enumerate() {
            if *act == Act::None {
                continue;
            }
            match CONTROLS[i] {
                "LT" => claims.triggers[0] = true,
                "RT" => claims.triggers[1] = true,
                _ => claims.buttons |= mask.unwrap_or(0),
            }
        }
        let menus = d.menus().map(|m| m.items().iter().map(|i| Act::new(&i.action)).collect::<Vec<_>>());
        for (part, menu) in PARTS.into_iter().zip(d.menus()) {
            if *menu == Menu::None {
                continue;
            }
            let (click, touch) = part.bits();
            let side = part.side() as usize;
            if part.stick() {
                claims.sticks[side] = true;
                // The stick's click is the menu's only when it picks.
                claims.buttons |= touch | if menu.pick() == Pick::Click { click } else { 0 };
            } else {
                claims.pads[side] = true;
                claims.buttons |= touch | click;
            }
        }
        Compiled { buttons, menus, claims }
    }
}

/// Turns state reports into frames for one deck.
pub struct DeckEngine {
    deck: Deck,
    c: Compiled,
    buttons: Vec<Press>,
    items: [Vec<Press>; 4],
    menus: [MenuState; 4],
    /// The menu touched last, for the on-screen menu.
    focus: Option<usize>,
    /// Changes whenever a menu opens, closes or selects another item.
    version: u64,
    triggers_down: [bool; 2],
    frame: Frame,
}

impl DeckEngine {
    pub fn new(deck: Deck) -> Self {
        let c = Compiled::new(&deck);
        DeckEngine {
            buttons: vec![Press::default(); CONTROLS.len()],
            items: c.menus.clone().map(|m| vec![Press::default(); m.len()]),
            c,
            deck,
            menus: Default::default(),
            focus: None,
            version: 0,
            triggers_down: [false; 2],
            frame: Frame::default(),
        }
    }

    pub fn deck(&self) -> &Deck {
        &self.deck
    }

    /// Forget presses and menus in progress (the caller lets go of what was held).
    pub fn reset(&mut self) {
        let version = self.version + 1;
        *self = DeckEngine::new(std::mem::take(&mut self.deck));
        self.version = version;
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    pub fn menus(&self) -> [MenuView; 4] {
        self.menus.map(|m| MenuView { open: m.open, selected: m.held.or(m.tap.map(|t| t.0)).or(m.selected).map(|i| i as u8) })
    }

    /// The open menu to show on screen: the one touched last.
    pub fn focus(&self) -> Option<Part> {
        self.focus.map(|i| PARTS[i])
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// `s` without what the deck takes, for the Keyboard & mouse or Gamepad layout.
    pub fn mask(&self, s: &ControllerState) -> ControllerState {
        let c = &self.c.claims;
        let mut m = *s;
        m.buttons &= !c.buttons;
        if c.triggers[0] {
            m.trigger_left = 0;
        }
        if c.triggers[1] {
            m.trigger_right = 0;
        }
        if c.sticks[0] {
            m.left_stick = [0, 0];
        }
        if c.sticks[1] {
            m.right_stick = [0, 0];
        }
        if c.pads[0] {
            m.left_pad = Default::default();
        }
        if c.pads[1] {
            m.right_pad = Default::default();
        }
        m
    }

    /// One state report at `now` (any steady clock).
    pub fn process(&mut self, s: &ControllerState, now: Duration) -> &Frame {
        let mut f = Frame::default();
        let triggers = [axis(s.trigger_left).max(0.0), axis(s.trigger_right).max(0.0)];
        for (i, t) in triggers.iter().enumerate() {
            self.triggers_down[i] = *t > if self.triggers_down[i] { TRIGGER_PRESS * 0.7 } else { TRIGGER_PRESS };
        }
        for (i, (act, mask)) in self.c.buttons.iter().enumerate() {
            let pressed = match (CONTROLS[i], mask) {
                ("LT", _) => self.triggers_down[0],
                ("RT", _) => self.triggers_down[1],
                (_, Some(m)) => s.pressed(*m),
                _ => false,
            };
            apply(act, pressed, 1.0, &mut self.buttons[i], now, &mut f);
            if pressed && *act != Act::None {
                f.lit |= 1 << i;
            }
        }

        let mut changed = false;
        for (i, part) in PARTS.into_iter().enumerate() {
            let menu = self.deck.menus()[i];
            let acts = &self.c.menus[i];
            let n = acts.len();
            let st = &mut self.menus[i];
            let before = (st.open, st.selected);
            let (click, touch) = part.bits();
            let clicked = s.pressed(click);
            let (open, selected) = match (part, menu) {
                (_, Menu::None) => (false, None),
                (Part::LeftStick | Part::RightStick, Menu::Wheel { .. }) => {
                    let [x, y] = if part == Part::LeftStick { s.left_stick } else { s.right_stick }.map(axis);
                    let m = x.hypot(y);
                    let sel = if n == 0 || m < WHEEL_CENTRE {
                        None
                    } else if m >= WHEEL_SELECT || st.selected.is_some() {
                        Some(wheel_slice(x, y, n, st.selected))
                    } else {
                        None
                    };
                    (s.pressed(touch) || m >= WHEEL_CENTRE, sel)
                }
                (_, Menu::Wheel { .. }) | (_, Menu::Grid { .. }) => {
                    let pad = if part == Part::LeftPad { &s.left_pad } else { &s.right_pad };
                    let [x, y] = [pad.x, pad.y].map(axis);
                    let sel = match menu {
                        _ if !pad.touched || n == 0 => None,
                        Menu::Grid { cols, rows, .. } => {
                            let (cols, rows) = ((*cols).max(1) as usize, (*rows).max(1) as usize);
                            Some(grid_cell(x, y, cols, rows, st.selected)).filter(|c| *c < n)
                        }
                        _ if x.hypot(y) > PAD_WHEEL_SELECT => Some(wheel_slice(x, y, n, st.selected)),
                        _ => None,
                    };
                    (pad.touched, sel)
                }
            };
            if selected.is_some() && selected != st.selected {
                f.ticks |= 1 << part.side();
            }
            match menu.pick() {
                Pick::Click => {
                    if clicked && !st.clicked {
                        st.held = selected;
                    } else if !clicked {
                        st.held = None;
                    }
                }
                // Back to the centre, or lifted: the item that was under the thumb.
                Pick::Release => {
                    if let (None, Some(was)) = (selected, st.selected) {
                        let lifted = if part.stick() { true } else { !open };
                        if lifted {
                            st.tap = Some((was, now + TAP));
                        }
                    }
                }
            }
            st.clicked = clicked;
            st.selected = selected;
            st.open = open;
            st.tap = st.tap.filter(|(_, until)| now < *until);
            let tap = st.tap.map(|t| t.0);
            for (j, act) in acts.iter().enumerate() {
                apply(act, st.held == Some(j) || tap == Some(j), 1.0, &mut self.items[i][j], now, &mut f);
            }
            if open {
                f.lit |= 1 << (crate::layout::LIT_LEFT_STICK + i as u32);
            }
            if (st.open, st.selected) != before {
                changed = true;
                if st.open && !before.0 {
                    self.focus = Some(i);
                }
            }
        }
        if changed {
            self.version += 1;
            if self.focus.is_some_and(|i| !self.menus[i].open) {
                self.focus = (0..4).find(|i| self.menus[*i].open);
            }
        }
        self.frame = f;
        &self.frame
    }
}

// ---------- Defaults ----------

fn keys(label: &str, keys: &[&str]) -> Item {
    Item { label: label.into(), action: Action::key(keys) }
}

/// What a new wheel holds: everyday desktop shortcuts.
pub fn default_wheel() -> Vec<Item> {
    vec![
        keys("Copy", &["Ctrl", "C"]),
        keys("Paste", &["Ctrl", "V"]),
        keys("Undo", &["Ctrl", "Z"]),
        keys("Redo", &["Ctrl", "Y"]),
        keys("Screenshot", &["Win", "Shift", "S"]),
        keys("Task view", &["Win", "Tab"]),
        keys("Desktop", &["Win", "D"]),
        keys("Files", &["Win", "E"]),
    ]
}

/// What a new 3 x 3 grid holds: media keys and volume.
pub fn default_grid() -> Vec<Item> {
    vec![
        keys("Previous", &["MediaPrevious"]),
        keys("Play / pause", &["MediaPlayPause"]),
        keys("Next", &["MediaNext"]),
        keys("Volume down", &["VolumeDown"]),
        keys("Mute", &["VolumeMute"]),
        keys("Volume up", &["VolumeUp"]),
        keys("Back", &["BrowserBack"]),
        keys("Stop", &["MediaStop"]),
        keys("Forward", &["BrowserForward"]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::protocol::Trackpad;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    fn wheel(items: Vec<Item>, pick: Pick) -> Menu {
        Menu::Wheel { items, pick }
    }

    fn four() -> Vec<Item> {
        vec![keys("", &["A"]), keys("", &["B"]), Item { label: "Calc".into(), action: Action::Run { target: "calc".into() } }, keys("", &["D"])]
    }

    fn stick(x: i16, y: i16) -> ControllerState {
        ControllerState { right_stick: [x, y], buttons: buttons::RIGHT_STICK_TOUCH, ..Default::default() }
    }

    fn pad(x: i16, y: i16, clicked: bool) -> ControllerState {
        let mut s = ControllerState { left_pad: Trackpad { touched: true, clicked, x, y, ..Default::default() }, ..Default::default() };
        s.buttons = buttons::LEFT_PAD_TOUCH | if clicked { buttons::LEFT_PAD_CLICK } else { 0 };
        s
    }

    #[test]
    fn wheel_slices_go_clockwise_from_the_top_and_hold_at_their_edges() {
        assert_eq!(wheel_slice(0.0, 1.0, 4, None), 0);
        assert_eq!(wheel_slice(1.0, 0.0, 4, None), 1);
        assert_eq!(wheel_slice(0.0, -1.0, 4, None), 2);
        assert_eq!(wheel_slice(-1.0, 0.0, 4, None), 3);
        let at = |deg: f32| (deg.to_radians().sin(), deg.to_radians().cos());
        let (x, y) = at(48.0);
        assert_eq!(wheel_slice(x, y, 4, None), 1);
        assert_eq!(wheel_slice(x, y, 4, Some(0)), 0);
        let (x, y) = at(60.0);
        assert_eq!(wheel_slice(x, y, 4, Some(0)), 1);
        assert_eq!(wheel_slice(-0.1, 1.0, 4, Some(3)), 0);
    }

    #[test]
    fn grid_cells_go_row_by_row_and_hold_at_their_edges() {
        // 3 x 2: top row 0 1 2, bottom row 3 4 5.
        assert_eq!(grid_cell(-0.9, 0.9, 3, 2, None), 0);
        assert_eq!(grid_cell(0.9, 0.9, 3, 2, None), 2);
        assert_eq!(grid_cell(0.0, -0.9, 3, 2, None), 4);
        assert_eq!(grid_cell(1.0, -1.0, 3, 2, None), 5);
        // Just over the line into cell 1: stays 0; well inside: 1.
        assert_eq!(grid_cell(-0.3, 0.5, 3, 2, Some(0)), 0);
        assert_eq!(grid_cell(-0.2, 0.5, 3, 2, Some(0)), 1);
    }

    #[test]
    fn a_wheel_plays_the_item_tilted_to_when_the_stick_comes_back() {
        let mut e = DeckEngine::new(Deck { right_stick: wheel(four(), Pick::Release), ..Deck::default() });
        let b = crate::layout::key_id("B").unwrap();
        // Touch opens it, a small tilt selects nothing.
        assert!(e.process(&stick(0, 5000), ms(0)).keys.is_empty());
        assert_eq!(e.menus()[1], MenuView { open: true, selected: None });
        assert_eq!(e.focus(), Some(Part::RightStick));
        // Right: B, with a tick on the right.
        assert_eq!(e.process(&stick(30000, 0), ms(4)).ticks, 2);
        assert_eq!(e.menus()[1].selected, Some(1));
        // Back to the centre: B for a moment.
        assert!(e.process(&stick(0, 0), ms(8)).keys.contains(b));
        assert!(e.process(&stick(0, 0), ms(60)).keys.is_empty());
        // Down opens the calculator, once.
        e.process(&stick(0, -30000), ms(70));
        assert_eq!(e.process(&stick(0, 0), ms(74)).runs, ["calc"]);
        assert!(e.process(&stick(0, 0), ms(78)).runs.is_empty());
        // Thumb off: closed.
        e.process(&ControllerState::default(), ms(80));
        assert_eq!((e.menus()[1].open, e.focus()), (false, None));
    }

    #[test]
    fn a_grid_plays_the_cell_clicked_on_and_holds_it() {
        let grid = Menu::Grid { cols: 2, rows: 2, items: four(), pick: Pick::Click };
        let mut e = DeckEngine::new(Deck { left_pad: grid, ..Deck::default() });
        let d = crate::layout::key_id("D").unwrap();
        // Bottom right is D; touching ticks on the left.
        assert_eq!(e.process(&pad(20000, -20000, false), ms(0)).ticks, 1);
        assert!(e.frame().keys.is_empty());
        assert!(e.process(&pad(20000, -20000, true), ms(4)).keys.contains(d));
        // Sliding while clicked keeps it.
        assert!(e.process(&pad(-20000, 20000, true), ms(8)).keys.contains(d));
        assert!(e.process(&pad(-20000, 20000, false), ms(12)).keys.is_empty());
        // Lifting doesn't play anything.
        assert!(e.process(&ControllerState::default(), ms(16)).keys.is_empty());
    }

    #[test]
    fn the_deck_takes_its_controls_from_the_layout() {
        let deck = Deck {
            buttons: [("L4".to_string(), Action::key(&["F1"])), ("LT".to_string(), Action::key(&["F2"]))].into(),
            right_stick: wheel(four(), Pick::Release),
            left_pad: Menu::Grid { cols: 2, rows: 2, items: four(), pick: Pick::Click },
            ..Deck::default()
        };
        let e = DeckEngine::new(deck);
        let l4 = button_mask("L4").unwrap();
        let r3 = button_mask("R3").unwrap();
        let mut s = pad(100, 100, true);
        s.buttons |= l4 | r3 | buttons::A | buttons::RIGHT_STICK_TOUCH;
        s.trigger_left = 20000;
        s.trigger_right = 20000;
        s.right_stick = [1000, 2000];
        s.left_stick = [3000, 4000];
        let m = e.mask(&s);
        // Kept: A, the right stick's click (the wheel picks on release), RT, the left stick.
        assert_eq!(m.buttons, buttons::A | r3);
        assert_eq!((m.trigger_left, m.trigger_right), (0, 20000));
        assert_eq!((m.right_stick, m.left_stick), ([0, 0], [3000, 4000]));
        assert!(!m.left_pad.touched && !m.left_pad.clicked);
    }

    #[test]
    fn menus_change_version_only_when_they_change() {
        let mut e = DeckEngine::new(Deck { right_stick: wheel(four(), Pick::Release), ..Deck::default() });
        let v = e.version();
        e.process(&ControllerState::default(), ms(0));
        assert_eq!(e.version(), v);
        e.process(&stick(0, 30000), ms(4));
        let v = e.version();
        assert!(v > 0);
        e.process(&stick(0, 30000), ms(8));
        assert_eq!(e.version(), v);
    }

    #[test]
    fn decks_read_back_from_json() {
        let deck = Deck { right_pad: Menu::Grid { cols: 3, rows: 3, items: default_grid(), pick: Pick::Click }, ..Deck::default() };
        let json = serde_json::to_string(&deck).unwrap();
        assert_eq!(serde_json::from_str::<Deck>(&json).unwrap(), deck);
        assert_eq!(serde_json::from_str::<Deck>("{}").unwrap(), Deck::default());
    }
}
