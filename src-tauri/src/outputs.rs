//! Every output that follows the controller: MIDI and OSC, and later ones such as a virtual
//! gamepad or keyboard/mouse. `forward_events` feeds them every state report (~265 Hz) from
//! whatever the UI shows, live input or a replay, and has them let go when that switches.

use std::sync::{Arc, Mutex};

use sc_core::protocol::BatteryStatus;
use sc_core::ControllerState;

use crate::midi_out::MidiOut;
use crate::osc_out::OscOut;

pub type SharedMidi = Arc<Mutex<MidiOut>>;
pub type SharedOsc = Arc<Mutex<OscOut>>;

#[derive(Clone)]
pub struct Outputs {
    pub midi: SharedMidi,
    pub osc: SharedOsc,
}

impl Outputs {
    pub fn process(&self, s: &ControllerState, battery: Option<BatteryStatus>) {
        self.midi.lock().unwrap().process(s);
        self.osc.lock().unwrap().process(s, battery.map(|b| b.level));
    }

    /// Let go of everything held (notes, buttons), e.g. when switching between live input and a
    /// replay, seeking, or losing the controller.
    pub fn release_all(&self) {
        self.midi.lock().unwrap().release_all();
        self.osc.lock().unwrap().release_all();
    }

    /// Called often from the event loop, outside other locks: ports and hosts that finished
    /// opening or looking up, retries, rates.
    pub fn tick(&self) {
        self.midi.lock().unwrap().tick();
        self.osc.lock().unwrap().tick();
    }
}
