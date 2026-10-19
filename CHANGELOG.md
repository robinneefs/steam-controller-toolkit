# Changelog

All notable changes to Steam Controller Toolkit. The app follows
[semantic versioning](https://semver.org): while it's 0.x, a new tool or feature bumps the minor
version (0.2.0) and fixes bump the patch version (0.2.1). See
[Releasing](README.md#releasing) for how to make a version.

## [Unreleased]

### Changed

- MIDI Setup guide on Windows: the experimental port and a loopback port side by side, with why
  the port is experimental, the fix in Microsoft's MIDI update, and what to do if it doesn't work.

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

[0.2.0]: https://github.com/robinneefs/steam-controller-toolkit/compare/v0.1.0-windows-portable...v0.2.0
[0.1.0]: https://github.com/robinneefs/steam-controller-toolkit/releases/tag/v0.1.0-windows-portable
