// Stands in for the Tauri backend so the real frontend can run in a browser for screenshots.
// Streams the snapshots in snapshots.json (a real recording, folded by the app's own Rust code)
// at 60 per second and answers the commands the tabs call at start-up.
//
// URL parameters: tab=<tab id>, frame=<snapshot to stop at>, replay=1 (show it as a paused
// replay of the recording, as the Recorder tab does), guide=1 (open the tab's Setup guide),
// experimental=1 (the Windows port is experimental, as on Windows before Microsoft's MIDI update),
// stuck=1 (that port is out until a restart: the MIDI service stopped responding), dest=<n> (the
// OSC tab edits destination n, from 0), slot=game (the Keyboard & mouse tab edits its Game layout),
// driver=missing (the Gamepad tab without its driver, as on Windows before installing ViGEmBus).
(() => {
  const params = new URLSearchParams(location.search);
  const stopAt = Number(params.get("frame") ?? 120);
  const replay = params.get("replay") === "1";
  const experimental = params.get("experimental") === "1";
  const stuck = params.get("stuck") === "1";
  const driverMissing = params.get("driver") === "missing";
  const tab = params.get("tab") ?? "visualizer";
  const callbacks = {};
  // The OSC tab opens its guide on the first visit; shots show it only with guide=1.
  try {
    localStorage.setItem("osc-guide-seen", "1");
  } catch {}
  const listeners = {};
  let nextId = 1;
  let data = null;

  const recordings = [
    { name: "2026-10-18 15-30-21", bytes: 305790, seconds: 9.22, modified: Date.parse("2026-10-18T15:30:30") },
    { name: "Trackpad swipes", bytes: 412672, seconds: 12.4, modified: Date.parse("2026-10-18T14:52:00") },
    { name: "Gyro figure eight", bytes: 198656, seconds: 6.1, modified: Date.parse("2026-10-18T14:41:00") },
  ];

  const answers = {
    "plugin:app|version": async () => (await (await fetch("/package.json")).json()).version,
    "plugin:event|listen": (args) => {
      listeners[args.event] = args.handler;
      return 1;
    },
    "plugin:event|unlisten": () => null,
    midi_setup: () => ({ preset: data.preset, output: { kind: "virtual" } }),
    midi_ports: () => ({ ports: ["loopMIDI Port"], virtual_port: !stuck, virtual_experimental: experimental || stuck, virtual_note: stuck ? "The Windows MIDI service isn't responding" : null }),
    midi_list_presets: () => [
      { name: "Effects + drums", builtin: true },
      { name: "Pad instrument", builtin: true },
      { name: "Effects only", builtin: true },
    ],
    list_recordings: () => recordings,
    osc_destinations: () => structuredClone(data.osc.destinations),
    osc_list_presets: () => data.osc.presets,
    osc_load_preset: (args) => structuredClone(data.osc.builtin.find((p) => p.name === args.name)),
    background_settings: () => ({ close_to_tray: true, autostart: true }),
    play_setup: () => ({ ...structuredClone(data.play.setup), mode: params.get("slot") === "game" ? "game" : "desktop" }),
    play_list_presets: (args) => data.play.presets[args.kind],
    play_load_preset: (args) => structuredClone(data.play.builtin[args.kind].find((l) => l.name === args.name)),
  };

  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback(cb) {
      const id = nextId++;
      callbacks[id] = cb;
      return id;
    },
    unregisterCallback(id) {
      delete callbacks[id];
    },
    convertFileSrc: (p) => p,
    async invoke(cmd, args) {
      while (!data) await new Promise((r) => setTimeout(r, 10));
      const answer = answers[cmd];
      return answer ? answer(args) : null;
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };

  function emit(snap, index) {
    if (replay) {
      snap.slot = null;
      snap.tape = {
        recording: null,
        playback: { name: recordings[0].name, position: stopAt / 60, duration: recordings[0].seconds, paused: true, looping: false },
        message: null,
      };
    }
    // The Gamepad tab's live view shows the pad layout; the snapshots carry it on the side.
    if (snap.play && tab === "gamepad") snap.play.preview = data.play.pad_previews[Math.min(index, data.play.pad_previews.length - 1)];
    if (snap.play && driverMissing) {
      snap.play.gamepad = { ...snap.play.gamepad, support: { kind: "missing", detail: "The ViGEmBus driver isn't installed" }, connected: false, player: null };
    }
    // The app sends the battery from the start of a replay; this recording reports it later.
    snap.battery ??= { charge_state: 1, level: 89 };
    const handler = callbacks[listeners.controller];
    if (handler) handler({ event: "controller", id: 1, payload: snap });
  }

  fetch("/tools/screenshots/snapshots.json")
    .then((r) => r.json())
    .then((json) => {
      data = json;
      // Play the five seconds before the chosen frame, so graphs have history, then stop there
      // (resending the last frame would scroll the graphs flat).
      let i = Math.max(0, stopAt - 300);
      let clicked = false;
      const timer = setInterval(() => {
        const last = Math.min(stopAt, data.frames.length - 1);
        emit(structuredClone(data.frames[Math.min(i, last)]), Math.min(i, last));
        if (i >= last) clearInterval(timer);
        if (!clicked && listeners.controller) {
          clicked = true;
          document.querySelector(`.sidebar button[data-tab="${tab}"]`)?.click();
          const dest = params.get("dest");
          if (dest) setTimeout(() => document.querySelectorAll(".dest-row")[Number(dest)]?.click(), 300);
          if (params.get("guide") === "1") setTimeout(() => document.querySelector("[data-guide]")?.click(), 500);
        }
        i++;
      }, 16);
    });
})();
