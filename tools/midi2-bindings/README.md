# Windows MIDI Services bindings

The Windows virtual MIDI port (`src-tauri/src/midi_win.rs`) uses the `Windows.Devices.Midi2`
WinRT API. This tool generates its Rust bindings, `src-tauri/src/midi2_bindings.rs`, from the
API's metadata. Only the types in `filters.txt` are generated (plus what they need).

The API files come from the `Windows.Devices.Midi2` NuGet package on the
[Windows MIDI Services releases](https://github.com/microsoft/MIDI/releases) (currently
`0.99.92-preview.11`). Unzip the `.nupkg`, then copy:

- `ref/native/Windows.Devices.Midi2.winmd` to `src-tauri/vendor/windows-midi-services/`
- `runtimes/win-x64/native/Windows.Devices.Midi2.dll` and `.pri` to
  `src-tauri/vendor/windows-midi-services/x64/` (embedded in the app, used only where Windows
  doesn't have its own copy)

Then, from this folder:

```bash
cargo run -- ../../src-tauri/vendor/windows-midi-services/Windows.Devices.Midi2.winmd ../../src-tauri/src/midi2_bindings.rs
python fix.py ../../src-tauri/src/midi2_bindings.rs
```

`fix.py` works around two windows-bindgen 0.100 issues with this metadata: static methods
marked `[NoException]` don't compile, and event helpers need a constructor bindgen doesn't
emit (we don't use events). Bump `PREVIEW` in `midi_win.rs` when the files change, so the app
extracts the new DLL.
