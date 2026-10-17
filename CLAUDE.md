# Steam Controller Toolkit

A cross-platform desktop app (Windows, macOS, Linux) for tinkering with the **2026 Steam Controller**
(codename "Triton"). Each tool lives in its own tab. First tab: a live input visualizer.

## Stack

- **Tauri 2**: Rust backend + vanilla TypeScript/Vite frontend (no framework).
- **`crates/sc-core`**: the device layer, independent of Tauri. Discovery, configuration, report
  parsing, and the `hub` that runs one reader thread per HID interface with hot-plug.
- **`src-tauri`**: starts the hub, folds reports into a `Snapshot`, and emits it to the UI as the
  `controller` event at ~60 Hz (the controller itself reports at ~265 Hz).
- **`src/`**: `main.ts` builds the tab bar from the `tabs` array; each tab implements `Tab` in
  `src/tabs/tab.ts`. `src/controller.ts` mirrors the Rust types and must be kept in sync with
  `crates/sc-core/src/protocol.rs` and `Snapshot` in `src-tauri/src/lib.rs`.

## Adding a tab

Create `src/tabs/<name>.ts` implementing `Tab`, add it to the `tabs` array in `src/main.ts`.
If it needs data the snapshot lacks, extend `Snapshot` in Rust and `controller.ts` together.
If it needs to send commands to the controller (haptics, settings), add a Tauri command in
`src-tauri/src/lib.rs` backed by a method on `sc_core::Controller`.

## Protocol notes (verified on hardware)

Reference implementation: SDL `src/joystick/hidapi/SDL_hidapi_steam_triton.c` and
`steam/controller_structs.h`. Little-endian, packed.

- VID `0x28DE`. PIDs: `0x1302` wired, `0x1303` BLE, `0x1304` "Steam Controller Puck" wireless
  dongle, `0x1305` newer dongle. Dongle interfaces 2..=5 are four controller slots.
- On Windows each interface splits into mouse / keyboard / vendor (usage page `0xFF00`)
  collections; only the vendor one carries reports.
- Feature report `[0x01, 0x87, len, (setting u8, value u16)...]` sets settings. We send
  lizard mode (9) = 0 and IMU mode (48) = raw accel|gyro (0x18), re-sent every 2 s because the
  controller reverts. It fails on empty dongle slots, which is expected.
- Input report `0x42` (54 bytes, includes orientation quaternion) observed at ~265 Hz.
  `0x45` has no quaternion, `0x47` adds a trackpad timestamp. `0x43` battery, `0x79`/`0x46`
  wireless connect/disconnect. `0x7b` seen but undecoded.
- Accel 16384 counts = 1 g. Gyro ±2000 °/s full scale.
- Reading works while Steam is running. But Steam's Desktop Layout then turns the controller
  into mouse/keyboard input on its own, which our lizard-mode-off cannot stop (it only stops
  the firmware's emulation). The app detects Steam (`watch_steam` in `src-tauri/src/lib.rs`)
  and shows a warning banner.
- Never poll several interfaces in turn with short timeouts: Windows timeouts are ≥ ~15 ms and
  you lose ~90% of reports. Use one blocking thread per interface (see `hub.rs`).
- Only send settings to a slot after it has sent a state report (as SDL does). Sending them to
  empty dongle slots fails every time.
- Report `0x42` carries an orientation quaternion (w, x, y, z; 32767 = 1.0) that rotates sensor
  vectors into a world frame with +Z up. Sensor frame: +X right, +Y forward (towards triggers),
  +Z out of the face. Three.js mapping is (x, y, z) -> (x, z, -y). Yaw drifts / has no absolute
  reference; the visualizer's Re-center button zeroes it.

## 3D model

`public/models/steam-controller.glb` is adapted from "Steam Machine + Controller" by
dark_igorek (Sketchfab, CC BY 4.0). **Attribution is a license requirement**: keep the credit
line in the 3D view, `public/models/README.md` + `LICENSE.txt`, and the README credits, and
credit dark_igorek anywhere the model is shown or shipped (about screens, store pages, docs).

Regenerate with
`node tools/convert-model.mjs <steam_machine__controller.glb> public/models/steam-controller.glb`.
The script splits the controller's details mesh into connected pieces and names them
`part_<name>` (matched by position via `PARTS`); `controller3d.ts` gives each a pivot and
moves it from live input. Keep it light: one model, 1024px WebP textures, on-demand rendering
(draws only when orientation/inputs change), pixel ratio capped at 1.5, nothing drawn while the
window is hidden. Append `#demo` to the page URL to cycle every input without a controller.

## HID enumeration must stay filtered

Never enumerate all HID devices (`HidApi::new()` with discovery on, `refresh_devices()`,
`add_devices(0, 0)`). On Windows, hidapi asks every matching device for its strings, and some
devices never answer (seen: Corsair `1b1c:1b3a` hangs in `HidD_GetSerialNumberString`), which
blocks forever. Use `device::valve_api()` / `device::rescan()`, which only enumerate VID 28de.
`third_party/hidapi` is hidapi 2.6.7 patched so `HidApi::new()` honours
`disable_device_discovery()`; it is wired in via `[patch.crates-io]` in the root `Cargo.toml`.

## Commands

- `npm run tauri dev`: run the app
- `npm run probe -- --raw --seconds 10`: terminal dump of devices and live decoded reports
- `npm run tauri build`: release bundle for the current OS (macOS/Linux builds need those OSes; use CI)

## Platform notes

- Linux needs udev rules granting access to `hidraw` for VID 28de (Steam installs them).
- macOS may prompt for Input Monitoring permission.
