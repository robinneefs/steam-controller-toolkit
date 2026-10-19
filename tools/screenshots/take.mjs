// Takes the README screenshots (docs/screenshots) with headless Edge or Chrome: the real UI at
// the default 1280 x 720 window, 1.5x, fed a real recording by mock.js. See README.md here.
// Usage: node tools/screenshots/take.mjs [base url]   (default http://localhost:1420, `npm run dev`)
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const base = process.argv[2] ?? "http://localhost:1420";

// file name -> page parameters. `frame` is a snapshot index (60 per second of the recording).
const SHOTS = {
  visualizer: { tab: "visualizer", frame: 245 },
  haptics: { tab: "haptics", frame: 125 },
  midi: { tab: "midi", frame: 345 },
  "midi-guide": { tab: "midi", frame: 345, guide: 1, experimental: 1 },
  osc: { tab: "osc", frame: 245, dest: 1 },
  "osc-guide": { tab: "osc", frame: 245, guide: 1 },
  "raw-data": { tab: "raw", frame: 345 },
  recorder: { tab: "recorder", frame: 132, replay: 1 },
  about: { tab: "about", frame: 125 },
};

const BROWSERS = [
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
];
const browser = process.env.BROWSER ?? BROWSERS.find(existsSync);
if (!browser) throw new Error("No Edge or Chrome found; set BROWSER to its path");

const only = process.argv.slice(3);
for (const [name, params] of Object.entries(SHOTS)) {
  if (only.length && !only.includes(name)) continue;
  const out = resolve("docs/screenshots", `${name}.png`);
  const url = `${base}/tools/screenshots/shots.html?${new URLSearchParams(Object.entries(params).map(([k, v]) => [k, String(v)]))}`;
  // A fresh profile each time, so nothing (like the MIDI guide's "seen" flag) carries over.
  const profile = mkdtempSync(join(tmpdir(), "sc-shots-"));
  execFileSync(browser, [
    "--headless=new",
    `--user-data-dir=${profile}`,
    "--window-size=1280,720",
    "--force-device-scale-factor=1.5",
    "--hide-scrollbars",
    // Software WebGL for the 3D view.
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    // Virtual time: long enough to load, play five seconds of snapshots and settle.
    "--virtual-time-budget=12000",
    `--screenshot=${out}`,
    url,
  ], { stdio: "ignore" });
  rmSync(profile, { recursive: true, force: true });
  console.log(`${name}.png`);
}
