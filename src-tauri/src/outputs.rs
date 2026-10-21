//! Every output that follows the controller: MIDI, OSC, and keyboard & mouse, the virtual
//! gamepad and the Command Deck (`play_out.rs`). `forward_events` feeds them every state report (~265 Hz) from
//! whatever the UI shows, live input or a replay, and has them let go when that switches.

use std::sync::{Arc, Mutex};

use sc_core::protocol::BatteryStatus;
use sc_core::ControllerState;

use crate::midi_out::MidiOut;
use crate::osc_out::OscOut;
use crate::play_out::PlayOut;

pub type SharedMidi = Arc<Mutex<MidiOut>>;
pub type SharedOsc = Arc<Mutex<OscOut>>;
pub type SharedPlay = Arc<Mutex<PlayOut>>;

/// Output tabs someone is looking at. An output that is off does no work per report, except
/// while its tab is in view: the meters and Learn there run on the engine.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Watched {
    pub midi: bool,
    pub osc: bool,
    pub keymouse: bool,
    pub gamepad: bool,
    pub deck: bool,
}

#[derive(Clone)]
pub struct Outputs {
    pub midi: SharedMidi,
    pub osc: SharedOsc,
    pub play: SharedPlay,
}

impl Outputs {
    pub fn process(&self, s: &ControllerState, battery: Option<BatteryStatus>, watched: Watched) {
        self.midi.lock().unwrap().process(s, watched.midi);
        self.osc.lock().unwrap().process(s, battery.map(|b| b.level), watched.osc);
        self.play.lock().unwrap().process(s, watched.keymouse || watched.gamepad, watched.deck);
    }

    /// Let go of everything held (notes, buttons), e.g. when switching between live input and a
    /// replay, seeking, or losing the controller.
    pub fn release_all(&self) {
        self.midi.lock().unwrap().release_all();
        self.osc.lock().unwrap().release_all();
        self.play.lock().unwrap().release_all();
    }

    /// Called often from the event loop, outside other locks: ports and hosts that finished
    /// opening or looking up, retries, rates, the virtual pad and auto switch.
    pub fn tick(&self, watched: Watched) {
        self.midi.lock().unwrap().tick();
        self.osc.lock().unwrap().tick();
        self.play.lock().unwrap().tick(watched.deck);
    }
}
