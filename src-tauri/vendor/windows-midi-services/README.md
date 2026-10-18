# Windows MIDI Services API (preview)

Microsoft's `Windows.Devices.Midi2` API, version `0.99.92-preview.11` ("In-box Preview 11",
12 Oct 2026), from the `Windows.Devices.Midi2` NuGet package on
<https://github.com/microsoft/MIDI/releases>. MIT licensed (see the package), distributed under
Microsoft's preview "go-live" terms in the release notes.

- `Windows.Devices.Midi2.winmd`: metadata, input for `tools/midi2-bindings`.
- `x64/Windows.Devices.Midi2.dll` + `.pri`: embedded in the Windows build by
  `src-tauri/src/midi_win.rs`, which loads them only where Windows has no copy of its own
  (Windows 11 25H2+ before the in-box release, expected late November 2026).

Remove the DLL (and the embedding) once Windows ships the API; keep the `.winmd` for bindings.
