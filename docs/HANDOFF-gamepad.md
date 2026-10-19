# Handoff: Keyboard & mouse and Gamepad outputs

Status as of 2026-10-19. Steps 1, 2 and 3 below are done and pushed to `claude/relaxed-fermat-5ncc5j`
(on top of 3a56280, the engine and backends). Don't commit or push without asking the user. Read `CLAUDE.md` first. Delete this file once the work is done.

## What the user asked for

Play non-Steam games with the controller without Steam. Two new tabs in the sidebar's "Output"
group, built like MIDI and OSC (engine in a pure, unit-tested module; owner behind `outputs.rs`;
fed every report off the UI thread, so it works in the tray; no per-report work while off unless
the tab is in view, `outputs::Watched`):

1. **Keyboard & mouse**: works with nothing installed. Make it intuitive for games and for
   productivity (desktop use).
2. **Gamepad**: a virtual Xbox 360 controller. On Windows this needs the ViGEmBus driver. The
   tab must explain why the driver is needed and what comes with it, then offer an Install
   button there. Once the driver is installed, the tab shows the real gamepad screen. Gyro is
   optional and offered in ways that make sense for games, never required.

Also from the original brief:
- A status line in the tray menu (`tray.rs`).
- A note in both tabs that gamepad mode is meant for use without Steam. With Steam running, its
  Desktop Layout sends mouse input too, so input arrives twice (`watch_steam`, `steam_running`).
- Keep `controller.ts` in step with the Rust types.
- Fit both tabs at 1100 and 960 px (`@container tab`).
- Update CHANGELOG (Unreleased), README (a section and screenshots via `tools/screenshots`; the
  mock needs the new commands), CLAUDE.md and docs.
- Measure CPU in the tray with the output on vs off.

## Research result (step 1, already reported to the user)

Windows has no driverless virtual gamepad. `InputInjector.InitializeGamepadInjection` and
`InjectGamepadInput` return OK in an unpackaged app, but XInput, Windows.Gaming.Input, GameInput
and Raw Input never see the pad (tested on Windows 11 26100). Keyboard injection through the same
injector works. Details are in the memory file `gamepad-research` and in
`src-tauri/vendor/vigembus/README.md`. The user chose: build both outputs and embed ViGEmBus.
Linux uses uinput. macOS is out of scope (shows "not available").

## Done (uncommitted, builds on Windows, tests pass)

- `src-tauri/vendor/vigembus/`: the official `ViGEmBus_1.22.0_x64_x86_arm64.exe`
  - SHA-256 `89220a78…4c6a`; Authenticode valid, signed by Nefarius.
  - Includes `LICENSE.txt` (BSD-3) and a `README.md`.
  - ViGEmBus 1.22.0 is **already installed on the dev PC**, so the gamepad can be tested end to
    end there. The install flow can't be tested from scratch without uninstalling it; ask the
    user first.
- `src-tauri/Cargo.toml`: new dependencies.
  - Windows: `windows-sys 0.61` (features listed there) and `vigem-client 0.1.4` with
    `unstable_xtarget_notification` (rumble).
  - Linux: `evdev 0.13`.
- `src-tauri/src/layout.rs`: **the pure engine, shared by both tabs. 14 tests pass**
  (`cargo test -p steam-controller-app --lib layout`).
  - `Layout`: `buttons` (control name from `CONTROLS` → `Action`), `left_stick`/`right_stick`
    (`StickMode`), `left_pad`/`right_pad` (`PadMode`), and `gyro` (`Gyro`).
  - `Action`: none, key/combination (optional repeat), mouse button, scroll notch, Xbox input
    (`PadInput`), or `Switch` (Desktop ⇄ Game).
  - `StickMode`: stick, directions (WASD/D-pad), mouse, scroll.
  - `PadMode`: touchpad mouse with trackball glide, scroll, stick position, 4-zone directions
    (optionally only while clicked).
  - `Gyro`: output none/mouse/right stick/steer (tilt → left stick X). Activation: always, right
    stick touch, right pad touch, LT, RT, hold a button, toggle with a button. Turn axis
    auto(world)/yaw/roll; invert Y. Drift is calibrated automatically while the controller is
    still.
  - `Engine::process(state, now) -> &Frame`: pad state, held keys, mouse buttons, mouse/scroll
    deltas, repeat taps, switch, gyro_active, and `lit` (bit per control in use, for the UI).
  - `Injector`: diffs frames into press/release events, keeps sub-pixel and sub-notch
    remainders, and `release()` lets go of everything.
  - Built-ins:
    - `builtin_keymouse()`: Desktop, Presentation, Shooter (WASD + mouse), Shooter + gyro aim,
      Strategy / point & click.
    - `builtin_gamepad()`: Standard, Back grips as face buttons, Gyro aim (stick), Gyro aim
      (mouse), Racing (tilt to steer), Trackpads as D-pad and mouse.
  - Key names (`KEYS`) are **physical US key positions**, matching `KeyboardEvent.code` capture.
    Modifiers come first.
  - Gyro sign conventions are derived, not yet verified on hardware: turn right = +x, front
    raised = aim up.
- `src-tauri/src/devices.rs`: platform facade.
  - Traits `KeyMouse` (`send(&[Event])`) and `VirtualPad` (`update`, `take_rumble`, `player`).
  - Functions `open_keymouse()`, `pad_support() -> PadSupport{Ready|Missing{detail}|Unsupported}`,
    `open_pad()`, `fullscreen_app() -> Option<(id, exe name)>`, and `KEYMOUSE_SUPPORTED`.
- `src-tauri/src/inject_win.rs`: SendInput.
  - Scan codes for physical keys; virtual keys for media keys; `dwExtraInfo` marker 0x5343.
  - Full-screen detection: the foreground window covers its monitor and isn't the shell or our
    own window.
- `src-tauri/src/vigem.rs`:
  - `support()`.
  - `Pad::open()`: plugin, then wait_ready (blocks, so call it on a thread). A rumble thread
    fills `take_rumble`.
  - `install(dir)`: writes the embedded exe and runs it with ShellExecuteExW (UAC), then waits.
    Blocking, so run it on a thread. Exit codes 0/3010 mean OK; 1602 or ERROR_CANCELLED mean
    cancelled.
- `src-tauri/src/uinput.rs` (Linux):
  - A keyboard+mouse device with hi-res wheel.
  - An Xbox 360 pad as xpad presents it (045e:028e, BTN_A/B/X/Y…, ABS_Y flipped, D-pad on the
    HAT0 axes).
  - No rumble yet.
  - Type-checked for `x86_64-unknown-linux-gnu` in a scratch crate (the target is now installed
    via rustup). Never run on Linux.
- `src-tauri/src/play_out.rs` (`PlayOut`, step 1): in `Outputs` as `play`, wired into
  `forward_events`, `Snapshot.play` (`PlayStatus`), the tray (two lines), `Watched`
  (`keymouse`/`gamepad`, tab ids "keymouse" and "gamepad"), and the 13 `play_*` commands.
  6 tests pass (mode logic, auto switch, preview, config round trip, preset store); the whole lib
  has 54 passing tests on Linux. Not type-checked for Windows (from Linux, hidapi's C build needs
  Windows headers); the only Windows-only call it makes is `vigem::install`.
  - Config `play/config.json` = `Config {keymouse {enabled, desktop, game}, gamepad {enabled,
    layout}, auto_switch}`; defaults: both off, Desktop + "Shooter (WASD + mouse)", "Standard",
    auto switch on. `play_setup` returns it flattened plus `mode`.
  - `play_set_keymouse(config)` / `play_set_gamepad(config)` take the whole section (the tabs
    send it after every edit, like OSC); `play_load_preset(kind, name, builtin)` only returns the
    layout. `kind` is "keymouse" | "gamepad"; `play_set_preview(slot)` takes "desktop" | "game" |
    "pad" | null.
  - Haptics: `take_haptics()` (switch click) and `rumble()`; `forward_events` sends both to the
    live slot via a clone of the `Hub`, never during a replay (rumble goes to 0 then).
  - The keyboard & mouse device opens lazily on the first event and closes when no layout plays.
  - Preview speeds (`mouse` px/s, `scroll` notches/s) are smoothed in `Speed`.

## Agreed design for the rest

**One owner, `src-tauri/src/play_out.rs` (`PlayOut`), for both tabs.** It goes in `Outputs` next
to midi/osc and gets `process`/`release_all`/`tick` calls like they do.

Config in `<app data>/play/config.json`:
- `keymouse {enabled, desktop: Layout, game: Layout}`
- `gamepad {enabled, layout: Layout}`
- `auto_switch: bool` (default true)

Presets go in `play/keymouse/` and `play/gamepad/`. Consider a small generic preset store,
modelled on `osc_out.rs` (list/load/save/delete/folder; built-ins can't be overwritten).

**Mode** is Desktop or Game, held at runtime. The active layout:
- Desktop: the K&M desktop layout if K&M is on, else the gamepad layout if the gamepad is on.
- Game: the gamepad layout if the gamepad is on, else the K&M game layout if K&M is on.

So with both on: mouse and keyboard on the desktop, gamepad in games.

**Switching modes:**
- A `Switch` action (the Steam button in every built-in) flips the mode.
- `auto_switch` also polls `fullscreen_app()` about every 500 ms in `tick`, only while K&M is
  on. A full-screen app means Game, otherwise Desktop. A manual switch overrides until the
  foreground window changes.
- On every switch, play a haptic click: `Command::Haptic` with `Haptic::Command{side:2,
  command:2, gain_db:0}`, via `Hub::send` on the live slot. Not during a replay.

**Per report:**
- Run the active layout's engine.
- Merge in nothing else. A gamepad layout's own keys/mouse (gyro mouse, grips to keys) are in its
  frame.
- Apply it with `Injector` → `KeyMouse::send`, and `PadState` → `VirtualPad::update`. The pad
  gets a neutral state while it is on but not active.
- When the active layout changes: release everything and `engine.reset()`.
- When a tab is watched and its layout isn't active, run a separate preview engine for that
  layout, for the status only, with no injection. `play_set_preview(which)` tells the backend
  which layout the visible tab shows: "desktop", "game" or "pad".

**Opening devices off the event loop:**
- Open the virtual pad on a thread (as `midi_out` opens ports); `tick` picks up the result.
- Keep the pad plugged in while the gamepad is on.
- Keep the driver installer thread's result in the status.
- `pad_support()` is fairly slow (SetupDi). Call it at start-up, when the Gamepad tab opens, and
  after an install. Never per report.

**Rumble:** in `tick`, take `take_rumble()`. When it changes, send `Haptic::Rumble{left: large*257,
right: small*257}` to the live slot (the hub resends it until both are 0). Send 0 when the pad
turns off.

**Snapshot.** Add `play: PlayStatus` and mirror it in `controller.ts`:
- `mode`, `auto_switch`, `fullscreen_app`, `active` (which layout)
- `keymouse {enabled, supported, error}`
- `gamepad {enabled, support: PadSupport, connected, player, installing, install_error, rumble}`
- `steam_running` is already in the snapshot
- `preview`: pad state, keys (names), mouse buttons, mouse px/s, scroll, gyro_active, lit

**Commands:**
- `play_setup`
- `play_set_keymouse`, `play_set_gamepad`, `play_set_auto_switch`, `play_set_mode`,
  `play_set_preview`
- `play_list_presets(kind)`, `play_load_preset`, `play_save_preset`, `play_delete_preset`,
  `play_reveal_presets`
- `play_install_driver`, `play_check_driver`

**Tray** (`TrayLines` plus the menu in `tray.rs`): add a line for "Keyboard & mouse: Desktop
layout / off" and one for "Gamepad: on · player 1 / off / driver missing". Also update the
Settings tab's background list and the sidebar dots in `main.ts`.

### UI (`src/tabs/keymouse.ts`, `src/tabs/gamepad.ts`, shared `src/tabs/layout-editor.ts` + `keys.ts`)

Icons go in `icons.ts`. The tabs follow the MIDI/OSC look (`docs/screenshots/midi.png`).

- **Top bar:** on/off, a status dot and text, a Desktop | Game segmented control showing the mode
  (clickable), an "Auto: full-screen apps use Game" checkbox, and a Steam-running warning.
- **K&M tab:** a slot switcher (Desktop layout / Game layout), each with a preset picker plus Save
  as / Delete / Folder.
- **Gamepad tab:** one layout preset picker.
- **Editor:** a grid of cards, `repeat(auto-fit, minmax(250px, 1fr))`, scrolling:
  - Face buttons, D-pad, Shoulders (LB RB LT RT and full pulls), Back grips (L4 L5 R4 R5),
    Sticks (mode, options, L3/R3), Trackpads (mode, options, click), Menu buttons
    (View Menu QAM Steam), Gyro (output, activation, speed, axis, invert; live "gyro on" pill).
  - Rows light up from `preview.lit`.
- **Binding picker:** a popover opened by a click.
  - "Press a key or combination" capture via `KeyboardEvent.code` → `KEYS` name.
  - Lists for keys hard to press there (Win, media keys, PrintScreen).
  - Mouse buttons, scroll, Xbox inputs (Gamepad tab only), Switch, None.
  - A "Repeat while held" checkbox.
- **Live view tile:**
  - K&M: held keys as chips, mouse buttons, a pointer-velocity dot, scroll.
  - Gamepad: an SVG Xbox controller with live sticks, triggers and buttons, plus extra keys/mouse,
    player number and rumble.
- **Gamepad tab with the driver missing (Windows):** an explainer replaces the editor.
  - Why: Windows has no built-in way; we tested Windows' own API and games don't see its pad.
  - What it is: ViGEmBus 1.22 by Nefarius, open source (BSD-3), signed, also used by DS4Windows
    and Parsec.
  - What comes with it:
    - An administrator prompt, once.
    - It's installed for the whole PC until removed in Settings → Apps → "ViGEm Bus Driver".
    - The project was retired in 2023, so no future fixes.
    - Some anti-cheat blocks virtual controllers.
    - Games see an "Xbox 360 Controller", and rumble is passed on.
  - Buttons: [Install driver] and [Use Keyboard & mouse instead].
- **Linux, missing:** show the udev rule and its one-line install. **macOS:** unsupported.
- **Note in both tabs:** apps running as administrator don't receive the input (UIPI).

## Remaining steps

1. ~~`play_out.rs` + `Outputs`/`Watched` (add keymouse/gamepad to `View::watched` in `tray.rs`) +
   `forward_events` (rumble and switch haptics need `app.state::<Hub>()` and the live slot) +
   commands + Snapshot. Unit-test what you can: mode logic, preset store, config round trip.~~ Done.
2. ~~Frontend: both tabs, `controller.ts` types, `main.ts` registration and dots, Settings list.~~ Done:
   - `src/tabs/keys.ts` (KEYS mirror, `KeyboardEvent.code` → name, labels, lists for hard keys),
     `src/tabs/layout-editor.ts` (`CONTROLS` mirror, `LayoutEditor` cards, the binding picker
     popover with key capture, `playBarHtml`/`renderPlayBar`, `PresetBar`, notes, chips),
     `src/tabs/keymouse.ts`, `src/tabs/gamepad.ts` (SVG Xbox pad; driver explainer for Windows,
     udev rule for Linux, "not available" on macOS; `play_check_driver` each time the tab opens).
   - `Tab.navTitle`: the sidebar says "Keys & mouse" ("Keyboard & mouse" didn't fit in 208 px).
   - Below 900 px the auto switch checkbox reads "Auto".
   - Icons `keymouse`, `gamepad`; sidebar dots; Settings' background list.
   - Screenshot tool: the snapshots test now adds `play` (setup, preset lists, built-ins and
     `pad_previews`); `mock.js` answers the `play_*` commands and takes `slot=game` and
     `driver=missing`; `take.mjs` has `keymouse`, `gamepad`, `gamepad-driver` shots. Checked at
     1280, 1100 and 960 px with a synthetic recording, and driven in headless Chromium (key capture
     binds Ctrl+Shift+K, no console errors). The README shots still need a real recording.
3. ~~Linux packaging:~~ Done: `src-tauri/linux/60-steam-controller-toolkit.rules` (hidraw over
   USB/dongle and Bluetooth, uinput) and `linux/postinst.sh` (reloads udev), in the .deb and .rpm
   via `bundle.linux` in `tauri.conf.json`; checked inside packages built on Linux, not installed
   anywhere yet. README and PROTOCOL.md updated. Originally:
   - Add `src-tauri/linux/60-steam-controller-toolkit.rules` with:
     - hidraw 28de with `TAG+="uaccess"`
     - `KERNEL=="uinput", SUBSYSTEM=="misc", OPTIONS+="static_node=uinput", TAG+="uaccess"`
   - Ship it via `bundle.linux.deb.files` / `rpm.files` in `tauri.conf.json` to
     `/usr/lib/udev/rules.d/`.
4. Verify on hardware (the controller and ViGEmBus are on the dev PC):
   - XInput sees the pad (reuse the throwaway reader idea; a scratch test program existed in the
     session scratchpad, now gone).
   - Gyro signs, SendInput in a game, rumble.
   - Check both tabs at 1100 and 960 px.
5. Measure CPU in the tray with the outputs on vs off, using the remote-debugging recipe in
   CLAUDE.md.
6. Docs:
   - CHANGELOG Unreleased, README (sections and screenshots: add the new commands to
     `tools/screenshots/mock.js` and shots to `take.mjs`).
   - CLAUDE.md: stack bullets for the layout engine, play_out, devices, vigem/uinput; the vendor
     note; and the About tab credit for ViGEmBus (BSD-3).
   - Update memory `future-ideas`.
7. Ask the user before committing.

## Possible follow-ups

- macOS keyboard & mouse via CGEvent (needs the Accessibility permission).
- Linux rumble via uinput force feedback.
- Flick stick.
- Per-game automatic layouts.
