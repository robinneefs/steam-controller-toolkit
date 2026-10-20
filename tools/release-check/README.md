# Release checks

A guided run through the app's outputs with the real controller, on Windows, before a release.
About ten minutes: each step tells you what to press or move, waits for Enter, and checks the
result itself. At the end it prints a summary (also in `last-run.log` here) and exits with 0 if
everything passed.

## What it checks

- **Gamepad**: the virtual Xbox 360 pad appears in XInput, centred, with a player number; every
  button, the triggers and both sticks arrive as XInput reports them (View is Back, Menu is
  Start); rumble reaches the controller (you answer y/n).
- **Gyro**: turning right aims right, raising the front aims up (Gyro aim, stick), leaning right
  steers right (Racing).
- **Keyboard & mouse** (Desktop layout): pointer and scrolling directions, clicks, Enter,
  Win+Tab, Win, and Ctrl+Z sent by letter (undo on any keyboard layout). Low-level hooks catch
  what the app sends and block it, so nothing really clicks or types.
- **Modes**: holding the Steam button switches once, with a click you feel; a full-screen app
  switches to Game and leaving it switches back.
- **OSC switch**: on, packets arrive on a local UDP port; off, nothing does.
- **Tray**: closing the window keeps the app (and its outputs) running; the tray menu's lines.

It turns outputs on and off and loads built-in layouts as it goes, and puts your output
settings back at the end. If you stop it halfway (Ctrl+C), check the output tabs afterwards.

Not covered, check these by hand when they changed: installing ViGEmBus from scratch
(uninstall "ViGEm Bus Driver" in Settings → Apps, then use the Gamepad tab's Install button),
MIDI, a real game, and macOS and Linux.

## Running it

Build the release app first, then run the checks (they start the app with WebView2's debugging
port, which they talk to; quit the app first if it's running):

```bash
npm run tauri build -- --no-bundle
```

```bash
cargo run --manifest-path tools/release-check/Cargo.toml
```

To check another build, pass its path: `cargo run --manifest-path tools/release-check/Cargo.toml -- path\to\steam-controller-app.exe`.
An app already running with the debugging port (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`,
e.g. under `npm run tauri dev`) is used as it is.

Have the controller on, Steam closed, and the app's window open (not minimised) next to the
terminal.
