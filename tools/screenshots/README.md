# README screenshots

`docs/screenshots/*.png` are the real app UI at the default 1100 x 720 window (1.5x), fed a real
recording, so they show real data without a controller or a window on screen.

- `snapshots.json` (not committed): a recording replayed through the app's own Rust folds and
  MIDI engine, as the 60-per-second snapshots the UI gets. Made by an ignored test.
- `shots.html` + `mock.js`: the app's page with a stand-in for the Tauri backend that streams
  those snapshots and answers the commands the tabs call. `?tab=…&frame=…` picks the tab and the
  moment; see the top of `mock.js` for the other options.
- `take.mjs`: opens each shot in headless Edge or Chrome and saves the PNGs. The list of shots
  (tab, frame) is at the top.

## Retaking them

1. Make the snapshots from a recording (record one in the Recorder tab, ~10 seconds of using
   every control; use absolute paths):

```bash
SC_RECORDING="/path/to/recording.screc" SC_SNAPSHOTS="$PWD/tools/screenshots/snapshots.json" cargo test -p steam-controller-app --lib snapshots -- --ignored
```

2. Start the frontend dev server (`npm run dev`, port 1420), then take all shots, or only some:

```bash
node tools/screenshots/take.mjs
```

```bash
node tools/screenshots/take.mjs http://localhost:1420 midi midi-guide
```

3. Look at every PNG before committing. If you used another recording, check the frames in
   `take.mjs` still show what the README says (e.g. the triggers pulled in the Visualizer).
