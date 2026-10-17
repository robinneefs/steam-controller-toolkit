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

The 3D controller model is derived from the Steam Controller CAD files © 2026 Valve Corporation,
released under [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/) (see
`public/models/`). This project is not affiliated with or endorsed by Valve, and builds that
include the model must not be used commercially.

## Probe

To just see what the controller sends, without the UI:

```bash
npm run probe -- --raw --seconds 10
```
