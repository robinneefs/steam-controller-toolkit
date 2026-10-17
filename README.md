# Steam Controller Toolkit

Tools for the 2026 Steam Controller, one tab each. Runs on Windows, macOS and Linux.

- **Visualizer**: live view of buttons, triggers, sticks, trackpads (position and pressure),
  capacitive touch, back buttons and IMU.

## Setup

1. Install [Rust](https://rustup.rs), [Node.js](https://nodejs.org) and the
   [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.
2. `npm install`
3. `npm run tauri dev`

## Credits

The 3D controller model is adapted from
["Steam Machine + Controller"](https://sketchfab.com/3d-models/steam-machine-controller-c1fcf77d71984c11818a5e2412816705)
by [dark_igorek](https://sketchfab.com/dark_igorek), licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). We extracted the controller, split
its inputs into moving parts and compressed it; see `public/models/` for details.

This project is not affiliated with or endorsed by Valve or the model's author.

## Probe

To just see what the controller sends, without the UI:

```bash
npm run probe -- --raw --seconds 10
```
