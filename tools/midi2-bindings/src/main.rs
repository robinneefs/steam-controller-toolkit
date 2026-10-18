//! Generates `src-tauri/src/midi2_bindings.rs` from the Windows MIDI Services metadata.
//! Usage: cargo run -- <Windows.Devices.Midi2.winmd> <out.rs> (types from filters.txt).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [winmd, out] = &args[..] else {
        panic!("usage: midi2-bindings <Windows.Devices.Midi2.winmd> <out.rs>");
    };
    let filters = include_str!("../filters.txt");
    let mut b = windows_bindgen::Bindgen::new();
    b.input_default().input(winmd).output(out);
    for f in filters.lines().map(str::trim).filter(|l| !l.is_empty()) {
        b.filter(f);
    }
    b.implement("Windows.Foundation.TypedEventHandler");
    b.write();
}
