# Steam Controller Toolkit

Tools for the 2026 Steam Controller, one tab each. Runs on Windows, macOS and Linux.

- **Visualizer**: live view of buttons, triggers, sticks, trackpads (position and pressure),
  capacitive touch, back buttons and IMU.

## Setup

1. Install [Rust](https://rustup.rs), [Node.js](https://nodejs.org) and the
   [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.
2. `npm install`
3. `npm run tauri dev`

To just see what the controller sends, without the UI:

```bash
npm run probe -- --raw --seconds 10
```
