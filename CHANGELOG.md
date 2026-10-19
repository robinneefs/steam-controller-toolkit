# Changelog

All notable changes to Steam Controller Toolkit. The app follows
[semantic versioning](https://semver.org): while it's 0.x, a new tool or feature bumps the minor
version (0.2.0) and fixes bump the patch version (0.2.1). See
[Releasing](README.md#releasing) for how to make a version.

## [Unreleased]

### Added

- **OSC tab**: send the controller to any app that speaks OSC, on this computer or others.
  - A stream of every input under `/sc` (grouped or one value per address), with a rate cap,
    bundles and groups to turn off; only changed values are sent, button presses at once.
  - Mappings from any input, shaped like MIDI's, to any address as a float, int or bool, with an
    output range, fixed values before it, and on-change, button (1/0) or trigger sends. Learn
    picks the input.
  - Several destinations at once, each with its own host, port and preset.
  - Presets for 26 apps (TouchDesigner, Max, Pure Data, Resolume, QLab, Reaper, Ableton via
    AbletonOSC, Bitwig via DrivenByMoss, VCV Rack, ETC Eos, VRChat and more) with their default
    ports and a setup step each in the Setup guide; save your own.
  - A live list of every address sent and its value; click to copy.
  - Losing the controller, or starting, stopping or seeking a replay, lets go of held buttons
    and mapped sticks and sends `/sc/connected 0`.
- **Run in the background**: closing the window keeps the app running in the system tray, so
  MIDI and OSC keep sending. The tray icon shows the controller, battery and outputs; click it to
  open the window again, right-click to quit. In the tray the app uses about 30 MB of memory and
  under 2% of one CPU core (the window took about 500 MB).
- **Settings** tab: keep running in the tray when the window closes (on by default), and start
  in the tray when you sign in.

### Changed

- The tabs moved from the top bar to a sidebar, grouped as Input, Output and Tools, so new tools
  have room. It shows names from 1100 px wide and icons only below that (hover for the name);
  Collapse picks icons at any size; below 1100 px, Expand opens the sidebar over the tab. Dots show MIDI and OSC sending and the Recorder recording or replaying.
- The window now opens at 1280 x 720 (was 1100 x 720).
- A minimised window now costs almost nothing: about 3% of one CPU core, down from about 45%,
  because the app stops updating it. MIDI and OSC also do no work while they're off, unless
  their tab is open.
- Starting the app again while it runs (say from the Start menu while it sits in the tray) opens
  its window instead of a second copy.
- Quitting lets go of held MIDI notes and OSC buttons.

## [0.2.1] - 2026-10-19

### Changed

- MIDI Setup guide on Windows: the experimental port and a loopback port side by side, with why
  the port is experimental, the fix in Microsoft's MIDI update, and what to do if it doesn't work.

### Fixed

- MIDI Setup guide on Windows: once the MIDI service stopped responding, the guide (e.g. after
  ↻ Look for ports) dropped the experimental explanation; it now says the port is out until a restart.
- Recorder on Windows: seeking or resuming further into a recording than the PC had been on
  froze the app.
- Opening or importing a long recording no longer pauses live input, and editing a MIDI preset
  while playing no longer stalls MIDI output.
- Steadier live updates: the UI now gets fresh state at an even ~62 per second (was ~53, with
  some frames skipped).

## [0.2.0] - 2026-10-18

### Added

- **MIDI tab**: use the controller as a MIDI controller in a DAW.
  - Mappings from any input (triggers, stick and pad axes, pad pressure, tilt, turn speed,
    buttons) to CC (7- or 14-bit), pitch bend, aftertouch, notes or program changes, with
    range, deadzone, curve, smoothing, invert, toggle and "only while a button is held".
  - Learn: click, then move a control to pick the input.
  - Pad notes: play either trackpad as a grid of notes in a scale, with slide and pressure as
    aftertouch.
  - Built-in presets (Effects + drums, Pad instrument, Effects only); save and share your own.
  - Live meters, an activity log and Panic; a replayed recording drives MIDI too.
  - The app's own **Steam Controller** MIDI port: on macOS (CoreMIDI) and Linux (ALSA) by
    default; on Windows 11 25H2+ through Windows MIDI Services, offered as *experimental* until
    Microsoft's MIDI update (Windows makes it once per restart until then). Loopback ports
    (loopMIDI) work everywhere.
  - Help tips on every section and a Setup guide for each system and common DAWs.
- **Haptics tab**: play every haptic effect: rumble, taps, the 18 stored scripts, tones with a
  piano, pulse trains and sweeps.
- **Recorder tab**: record everything the controller sends and replay it in every tab, with
  seek, loop, pause and single-step; drop `.screc` files on the window to add them.
- Visualizer: trigger clicks and L3/R3 in the Buttons tile.

### Changed

- Visualizer: View and Menu swapped to match the controller; the Steam button shows the logo.
- Top bar: the replay pill stays in place; tighter tabs below 1200 px so six tabs fit.

### Fixed

- Haptics played on the wrong side.
- Recorder: Show and Delete could freeze the app.

## [0.1.0] - 2026-10-18

First release.

- **Visualizer**: 3D controller with every input moving and lighting up (hold the right
  trigger to rotate it with the controller), pitch/roll/yaw, analog gauges, a motion graph and
  a buttons panel.
- **Raw data**: connection details, a colour-coded byte map of the latest report, decoded
  inputs and motion, and undecoded reports.
- **About** tab with credits.
- Power off the controller, click any value to copy it, and a warning while Steam is running.
- Builds for Windows, macOS and Linux, with portable versions.

[0.2.1]: https://github.com/robinneefs/steam-controller-toolkit/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/robinneefs/steam-controller-toolkit/compare/v0.1.0-windows-portable...v0.2.0
[0.1.0]: https://github.com/robinneefs/steam-controller-toolkit/releases/tag/v0.1.0-windows-portable
