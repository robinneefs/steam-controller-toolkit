//! Macros: named lists of key presses, clicks and scrolls with waits in between, played with one
//! press from the Command Deck (a button, a wheel slice or a grid cell: `Action::Macro`). The
//! Macros tab makes and edits them. A [`Player`] turns the macros playing into a frame over time,
//! which `play_out.rs` merges with the layout's and the deck's; [`MacroStore`] keeps one JSON file
//! per macro in `<app data>/play/macros`. Pure logic apart from the store.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::layout::{apply, Act, Action, Frame, Press};

/// One press of a macro.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Step {
    /// A key or combination, a mouse button or a scroll; anything else is a pause.
    pub action: Action,
    /// How long it is held, in ms.
    pub hold: u32,
    /// How long to wait after letting go, in ms.
    pub wait: u32,
}

/// A macro; its name is its file's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Macro {
    pub steps: Vec<Step>,
}

/// Every press lasts at least this, and every let-go too, so the same key twice is two presses.
const MIN_HOLD: Duration = Duration::from_millis(20);
const MIN_GAP: Duration = Duration::from_millis(20);

/// What a step plays: keys, mouse buttons and scrolls only (no programs, mode switches or other
/// macros).
fn step_act(a: &Action) -> Act {
    match a {
        Action::Key { .. } | Action::Mouse { .. } | Action::Scroll { .. } => Act::new(a),
        _ => Act::None,
    }
}

/// A macro playing.
struct Running {
    name: String,
    /// Act, hold and wait of every step.
    steps: Vec<(Act, Duration, Duration)>,
    /// The step pressed or let go of last (None: waiting to start).
    step: Option<usize>,
    down: bool,
    /// When the current press or let-go ends.
    until: Duration,
    /// It has gone out in a frame: even late, every press and let-go is sent once.
    seen: bool,
    press: Press,
}

impl Running {
    /// Move on to where it is at `now`; false once it's done.
    fn advance(&mut self, now: Duration) -> bool {
        while now >= self.until && self.seen {
            if self.down {
                self.down = false;
                self.until += self.steps[self.step.unwrap()].2.max(MIN_GAP);
            } else {
                let next = self.step.map_or(0, |i| i + 1);
                if next >= self.steps.len() {
                    return false;
                }
                self.step = Some(next);
                self.down = true;
                self.until += self.steps[next].1.max(MIN_HOLD);
                self.press = Press::default();
            }
            self.seen = false;
        }
        true
    }
}

/// Plays macros, several at once if asked.
#[derive(Default)]
pub struct Player {
    running: Vec<Running>,
    frame: Frame,
}

impl Player {
    /// Start `m` at `now + delay`, or stop it if it is playing (a second press stops it).
    pub fn toggle(&mut self, name: &str, m: &Macro, now: Duration, delay: Duration) {
        if let Some(i) = self.running.iter().position(|r| r.name == name) {
            self.running.remove(i);
            return;
        }
        let ms = |v: u32| Duration::from_millis(v as u64);
        let steps: Vec<_> = m.steps.iter().map(|s| (step_act(&s.action), ms(s.hold), ms(s.wait))).collect();
        if steps.is_empty() {
            return;
        }
        self.running.push(Running {
            name: name.to_string(),
            steps,
            step: None,
            down: false,
            until: now + delay,
            seen: true,
            press: Press::default(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    /// Names of the macros playing.
    pub fn playing(&self) -> Vec<String> {
        self.running.iter().map(|r| r.name.clone()).collect()
    }

    /// Stop everything (the caller lets go of what was held).
    pub fn stop_all(&mut self) {
        self.running.clear();
        self.frame = Frame::default();
    }

    /// What the macros playing hold at `now` (any steady clock).
    pub fn process(&mut self, now: Duration) -> &Frame {
        let mut f = Frame::default();
        self.running.retain_mut(|r| {
            if !r.advance(now) {
                return false;
            }
            if let Some(i) = r.step {
                apply(&r.steps[i].0, r.down, 1.0, &mut r.press, now, &mut f);
                r.seen = true;
            }
            true
        });
        self.frame = f;
        &self.frame
    }
}

const EXT: &str = "json";

/// The saved macros, kept in memory (read once at start; every change is written at once).
pub struct MacroStore {
    dir: PathBuf,
    macros: BTreeMap<String, Macro>,
}

impl MacroStore {
    pub fn load(dir: PathBuf) -> Self {
        let macros = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.extension()? != EXT {
                    return None;
                }
                let m: Macro = serde_json::from_str(&fs::read_to_string(&p).ok()?).ok()?;
                Some((p.file_stem()?.to_string_lossy().into_owned(), m))
            })
            .collect();
        MacroStore { dir, macros }
    }

    /// Read the folder again (macros may have been copied in).
    pub fn reload(&mut self) {
        *self = MacroStore::load(std::mem::take(&mut self.dir));
    }

    /// Names, in alphabetical order whatever the case.
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.macros.keys().cloned().collect();
        names.sort_by_key(|s| s.to_lowercase());
        names
    }

    pub fn get(&self, name: &str) -> Option<&Macro> {
        self.macros.get(name)
    }

    /// Save `m` as `name`, overwriting the macro of that name.
    pub fn save(&mut self, name: &str, m: Macro) -> Result<(), String> {
        let file = self.file(name)?;
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
        fs::write(file, json).map_err(|e| format!("Couldn't save \"{name}\": {e}"))?;
        self.macros.insert(name.trim().to_string(), m);
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let file = self.file(name)?;
        if file.exists() {
            fs::remove_file(file).map_err(|e| format!("Couldn't delete \"{name}\": {e}"))?;
        }
        self.macros.remove(name);
        Ok(())
    }

    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = to.trim();
        let (old, new) = (self.file(from)?, self.file(to)?);
        let m = self.macros.get(from).cloned().ok_or_else(|| format!("There is no macro \"{from}\""))?;
        // Only the case changing is the same file on Windows.
        if !from.eq_ignore_ascii_case(to) && self.macros.keys().any(|k| k.eq_ignore_ascii_case(to)) {
            return Err(format!("There already is a macro \"{to}\""));
        }
        if old.exists() {
            fs::rename(&old, &new).map_err(|e| format!("Couldn't rename \"{from}\": {e}"))?;
        }
        self.macros.remove(from);
        self.macros.insert(to.to_string(), m);
        Ok(())
    }

    pub fn folder(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        Ok(self.dir.clone())
    }

    fn file(&self, name: &str) -> Result<PathBuf, String> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || name.starts_with('.') {
            return Err(format!("\"{name}\" can't be used as a macro name"));
        }
        Ok(self.dir.join(Path::new(name).with_extension(EXT)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::key_id;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    fn step(keys: &[&str], hold: u32, wait: u32) -> Step {
        Step { action: Action::key(keys), hold, wait }
    }

    /// Which of `names` the player holds at `t`.
    fn held(p: &mut Player, t: u64, names: &[&str]) -> Vec<String> {
        let f = p.process(ms(t));
        names.iter().filter(|n| f.keys.contains(key_id(n).unwrap())).map(|n| n.to_string()).collect()
    }

    #[test]
    fn steps_play_in_turn_with_their_hold_and_wait() {
        let m = Macro { steps: vec![step(&["A"], 50, 100), step(&["B"], 30, 0)] };
        let mut p = Player::default();
        p.toggle("ab", &m, ms(0), ms(10));
        assert!(held(&mut p, 5, &["A"]).is_empty());
        assert_eq!(held(&mut p, 12, &["A", "B"]), ["A"]);
        assert_eq!(held(&mut p, 55, &["A", "B"]), ["A"]);
        assert!(held(&mut p, 62, &["A", "B"]).is_empty());
        assert_eq!(held(&mut p, 165, &["A", "B"]), ["B"]);
        // B's wait is 0, but it is let go of for a moment, then the macro ends.
        assert!(held(&mut p, 200, &["A", "B"]).is_empty());
        assert!(!p.is_empty());
        p.process(ms(230));
        assert!(p.is_empty());
    }

    #[test]
    fn the_same_key_twice_is_two_presses_even_when_reports_come_late() {
        let m = Macro { steps: vec![step(&["A"], 0, 0), step(&["A"], 0, 0)] };
        let mut p = Player::default();
        p.toggle("aa", &m, ms(0), ms(0));
        // A report every 500 ms: each press and let-go still goes out once.
        let seen: Vec<bool> = (0..5).map(|i| !held(&mut p, i * 500, &["A"]).is_empty()).collect();
        assert_eq!(seen, [true, false, true, false, false]);
        assert!(p.is_empty());
    }

    #[test]
    fn a_second_press_stops_it() {
        let m = Macro { steps: vec![step(&["A"], 1000, 0)] };
        let mut p = Player::default();
        p.toggle("a", &m, ms(0), ms(0));
        assert_eq!(held(&mut p, 10, &["A"]), ["A"]);
        assert_eq!(p.playing(), ["a"]);
        p.toggle("a", &m, ms(20), ms(0));
        assert!(held(&mut p, 30, &["A"]).is_empty());
        assert!(p.is_empty());
    }

    #[test]
    fn scrolls_go_once_and_programs_dont_play() {
        let m = Macro {
            steps: vec![
                Step { action: Action::Scroll { dir: crate::layout::Dir::Down, repeat: false }, hold: 100, wait: 0 },
                Step { action: Action::Run { target: "calc".into() }, hold: 0, wait: 0 },
            ],
        };
        let mut p = Player::default();
        p.toggle("s", &m, ms(0), ms(0));
        assert_eq!(p.process(ms(0)).scroll, [0.0, -1.0]);
        assert_eq!(p.process(ms(50)).scroll, [0.0, 0.0]);
        for t in (100..300).step_by(10) {
            assert!(p.process(ms(t)).runs.is_empty());
        }
    }

    #[test]
    fn the_store_saves_renames_and_deletes() {
        let dir = std::env::temp_dir().join(format!("sc-macros-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut s = MacroStore::load(dir.clone());
        let m = Macro { steps: vec![step(&["Ctrl", "S"], 30, 0)] };
        s.save("Save", m.clone()).unwrap();
        s.save("another", Macro::default()).unwrap();
        assert_eq!(s.list(), ["another", "Save"]);
        assert!(s.save("a/b", Macro::default()).is_err());
        assert!(s.rename("Save", "another").is_err());
        s.rename("Save", "Save all").unwrap();
        assert_eq!(MacroStore::load(dir.clone()).get("Save all"), Some(&m));
        s.delete("Save all").unwrap();
        assert_eq!(MacroStore::load(dir.clone()).list(), ["another"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
