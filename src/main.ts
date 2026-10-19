import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { CHARGE_STATES, describeSlot, onSnapshot, type Snapshot } from "./controller";
import { setAttr, setHidden, setText } from "./dom";
import { controlPlayback, formatTime, importRecordings, playRecording, startRecording, stopRecording } from "./tape";
import { AboutTab } from "./tabs/about";
import { HapticsTab } from "./tabs/haptics";
import { MidiTab } from "./tabs/midi";
import { OscTab } from "./tabs/osc";
import { RawTab } from "./tabs/raw";
import { RecorderTab } from "./tabs/recorder";
import { SettingsTab } from "./tabs/settings";
import { ICONS } from "./tabs/icons";
import type { Tab, TabGroup } from "./tabs/tab";
import { VisualizerTab } from "./tabs/visualizer";

// Register new tools here; each becomes a tab.
const recorder = new RecorderTab();
const tabs: Tab[] = [new VisualizerTab(), new HapticsTab(), new MidiTab(), new OscTab(), new RawTab(), recorder, new SettingsTab(), new AboutTab()];

const nav = document.querySelector<HTMLElement>("#tabs")!;
const navFoot = document.querySelector<HTMLElement>("#nav-foot")!;
const tabTitle = document.querySelector<HTMLElement>("#tab-title")!;
const content = document.querySelector<HTMLElement>("#tab-content")!;
const status = document.querySelector<HTMLElement>("#status")!;
const replayEl = document.querySelector<HTMLElement>("#replay")!;

const panes = new Map<string, HTMLElement>();
let active: Tab = tabs[0];
let lastSnapshot: Snapshot | null = null;

function show(tab: Tab): void {
  if (tab !== active) active.hide?.();
  active = tab;
  if (!panes.has(tab.id)) {
    const pane = document.createElement("div");
    pane.className = "pane";
    content.appendChild(pane);
    tab.mount(pane);
    panes.set(tab.id, pane);
  }
  for (const [id, pane] of panes) pane.hidden = id !== tab.id;
  document.querySelectorAll<HTMLElement>(".sidebar [data-tab]").forEach((b) => b.classList.toggle("active", b.dataset.tab === tab.id));
  tabTitle.textContent = tab.title;
  if (lastSnapshot) tab.update?.(lastSnapshot);
  // The backend runs an output's engine while it is off only if its tab is in view (meters, Learn).
  invoke("set_tab", { tab: tab.id }).catch(() => {});
}

// Sidebar: tabs listed under their group, About and the collapse button at the bottom.
const GROUPS: [TabGroup, string][] = [["input", "Input"], ["output", "Output"], ["tools", "Tools"]];
function navItem(label: string, icon: string): HTMLButtonElement {
  const b = document.createElement("button");
  b.className = "nav-item";
  b.innerHTML = `${icon}<span class="label"></span><i class="dot" hidden></i>`;
  b.querySelector(".label")!.textContent = label;
  b.dataset.label = label;
  return b;
}
for (const [group, heading] of GROUPS) {
  const members = tabs.filter((t) => t.group === group);
  if (!members.length) continue;
  const h = document.createElement("h4");
  h.textContent = heading;
  nav.appendChild(h);
  for (const tab of members) {
    const b = navItem(tab.title, tab.icon);
    b.dataset.tab = tab.id;
    b.addEventListener("click", () => show(tab));
    nav.appendChild(b);
  }
}
for (const tab of tabs.filter((t) => t.group === "app")) {
  const b = navItem(tab.title, tab.icon);
  b.dataset.tab = tab.id;
  b.addEventListener("click", () => show(tab));
  navFoot.appendChild(b);
}

// Collapse to icons at any width (remembered). Below 1100 px the sidebar is icons anyway, and the
// same button opens it over the tab instead, until a tab is picked or you click elsewhere.
const collapse = navItem("Collapse", ICONS.collapse);
collapse.classList.add("collapse");
navFoot.appendChild(collapse);
const narrowQuery = window.matchMedia("(max-width: 1099px)");
let collapsed = false;
let navOpen = false;
try {
  collapsed = localStorage.getItem("nav-collapsed") === "1";
} catch {}
function renderNav(): void {
  const narrow = narrowQuery.matches;
  if (!narrow) navOpen = false;
  const rail = narrow ? !navOpen : collapsed;
  document.body.classList.toggle("narrow", narrow);
  document.body.classList.toggle("nav-open", navOpen);
  document.body.classList.toggle("rail", rail);
  collapse.dataset.label = rail ? "Expand" : "Collapse";
  collapse.querySelector(".label")!.textContent = collapse.dataset.label;
}
renderNav();
narrowQuery.addEventListener("change", renderNav);
collapse.addEventListener("click", () => {
  if (narrowQuery.matches) {
    navOpen = !navOpen;
  } else {
    collapsed = !collapsed;
    try {
      localStorage.setItem("nav-collapsed", collapsed ? "1" : "0");
    } catch {}
  }
  navTip.hidden = true;
  renderNav();
});
function closeNav(): void {
  if (!navOpen) return;
  navOpen = false;
  renderNav();
}
document.addEventListener("click", (e) => {
  const item = (e.target as Element).closest(".nav-item");
  if ((item && item !== collapse) || !(e.target as Element).closest(".sidebar")) closeNav();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeNav();
});

// In icon mode, hovering an item shows its name beside it.
const navTip = document.createElement("div");
navTip.className = "nav-tip";
navTip.hidden = true;
document.body.appendChild(navTip);
const sidebar = document.querySelector<HTMLElement>(".sidebar")!;
sidebar.addEventListener("mouseover", (e) => {
  const b = (e.target as Element).closest<HTMLElement>(".nav-item");
  const iconsOnly = !b?.querySelector<HTMLElement>(".label")?.offsetWidth;
  if (!b || !iconsOnly) return void (navTip.hidden = true);
  const r = b.getBoundingClientRect();
  navTip.textContent = b.dataset.label ?? "";
  if (b.dataset.note) {
    const note = document.createElement("em");
    note.textContent = b.dataset.note;
    navTip.append(" ", note);
  }
  navTip.style.left = `${r.right + 10}px`;
  navTip.style.top = `${r.top + r.height / 2}px`;
  navTip.hidden = false;
});
sidebar.addEventListener("mouseleave", () => (navTip.hidden = true));

// Live dots: an output that is sending, the Recorder while recording or replaying.
function setDot(id: string, kind: "" | "ok" | "rec" | "play", note: string): void {
  const b = nav.querySelector<HTMLElement>(`[data-tab="${id}"]`);
  const dot = b?.querySelector<HTMLElement>(".dot");
  if (!b || !dot) return;
  setHidden(dot, !kind);
  setAttr(dot, "class", `dot ${kind}`);
  if (note) setAttr(b, "data-note", note);
  else if (b.hasAttribute("data-note")) b.removeAttribute("data-note");
}
function renderDots(snap: Snapshot): void {
  setDot("midi", snap.midi.connected ? "ok" : "", snap.midi.connected ? `Sending to ${snap.midi.connected}` : "");
  const osc = (snap.osc?.destinations ?? []).filter((d) => d.address);
  setDot("osc", osc.length ? "ok" : "", osc.length > 1 ? `Sending to ${osc.length} destinations` : osc.length ? `Sending to ${osc[0].address}` : "");
  const t = snap.tape;
  setDot("recorder", t.recording ? "rec" : t.playback ? "play" : "", t.recording ? "Recording" : t.playback ? "Replaying" : "");
}
show(active);

let lastStatus = "";
let lastReplay = "";
function renderStatus(snap: Snapshot): void {
  const b = snap.battery;
  const p = snap.tape.playback;
  // While a recording plays, the snapshot is the recording's, so the pill replaces the device.
  // It sits in its own slot after the tabs, anchored left, so the changing device side cannot move it.
  const replay = p
    ? `<span class="tape-pill replay" title="Replaying ${p.name.replace(/"/g, "&quot;")}"><b>Replay</b><span class="num">${formatTime(p.position)} / ${formatTime(p.duration)}</span>` +
      `<button data-tape="${p.paused ? "resume" : "pause"}" title="${p.paused ? "Resume" : "Pause"}">${p.paused ? "▶" : "❚❚"}</button>` +
      `<button data-tape="stop" title="Stop and return to live input">■</button></span>`
    : "";
  const device = snap.state
    ? (p ? "" : `<span>${snap.slot ? describeSlot(snap.slot).label : ""}</span>`) +
      `<span class="num rate">${snap.report_rate} Hz</span>` +
      (b
        ? `<span class="battery" title="${CHARGE_STATES[b.charge_state] ?? ""}"><span class="cell${b.level <= 20 ? " low" : ""}"><i style="width:${b.level}%"></i></span><span class="num level">${b.level}%</span></span>`
        : "")
    : p ? "" : `<span class="searching">Searching for controller…</span>`;
  if (replay !== lastReplay) replayEl.innerHTML = lastReplay = replay;
  if (device !== lastStatus) status.innerHTML = lastStatus = device;
}
replayEl.addEventListener("click", (e) => {
  const b = (e.target as Element).closest<HTMLElement>("[data-tape]");
  if (b) controlPlayback({ kind: b.dataset.tape as "pause" | "resume" | "stop" }).catch((err) => showToast(String(err)));
});

// Record from any tab: the button starts and stops a recording of live input.
const record = document.querySelector<HTMLButtonElement>("#record")!;
const recordTime = record.querySelector<HTMLElement>("span")!;
record.addEventListener("click", async () => {
  try {
    if (lastSnapshot?.tape.recording) await stopRecording();
    else showToast(`Recording to ${await startRecording()}`);
  } catch (e) {
    showToast(String(e));
  }
});
function renderRecord(snap: Snapshot): void {
  const r = snap.tape.recording;
  setHidden(record, !r && (!snap.state || !!snap.tape.playback));
  record.classList.toggle("on", !!r);
  setAttr(record, "title", r ? `Recording to ${r.name}. Click to stop` : "Record live input (see the Recorder tab)");
  setText(recordTime, r ? formatTime(r.seconds) : "");
}

// Warn while Steam is running; a dismissal lasts until Steam is closed and reopened.
const steamWarning = document.querySelector<HTMLElement>("#steam-warning")!;
let steamWarningDismissed = false;
steamWarning.querySelector("[data-dismiss]")!.addEventListener("click", () => {
  steamWarningDismissed = true;
  steamWarning.hidden = true;
});

function renderSteamWarning(snap: Snapshot): void {
  if (!snap.steam_running) steamWarningDismissed = false;
  setHidden(steamWarning, !snap.steam_running || steamWarningDismissed);
}

// Power off: the first click arms the button, a second click within 3 s sends the command.
const power = document.querySelector<HTMLButtonElement>("#power")!;
const powerLabel = power.querySelector<HTMLElement>("[data-power-label]")!;
let powerTimer: number | undefined;
function setPower(state: "idle" | "armed" | "busy" | "error", label = "", title = "Turn off the controller"): void {
  power.className = `power ${state}`;
  powerLabel.textContent = label;
  power.title = title;
  clearTimeout(powerTimer);
  if (state === "armed" || state === "error") powerTimer = window.setTimeout(() => setPower("idle"), 3000);
}
setPower("idle");
power.addEventListener("click", async () => {
  const slot = lastSnapshot?.slot;
  if (!slot || power.classList.contains("busy")) return;
  if (!power.classList.contains("armed")) return setPower("armed", "Click again to turn off", "Click again to turn the controller off");
  setPower("busy", "Turning off…");
  try {
    await invoke("turn_off_controller", { slot });
    setPower("idle");
  } catch (e) {
    setPower("error", "Couldn't turn off", String(e));
  }
});

// Click any element marked data-copy to copy its value: the attribute's value if set, else its text.
const toast = document.createElement("div");
toast.className = "toast";
document.body.appendChild(toast);
let toastTimer: number | undefined;
function showToast(text: string): void {
  toast.textContent = text;
  toast.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toast.classList.remove("show"), 1200);
}
document.addEventListener("click", async (e) => {
  const el = (e.target as Element).closest<HTMLElement>("[data-copy]");
  if (!el) return;
  const text = el.dataset.copy || el.textContent?.trim() || "";
  if (!text || text === "–") return;
  try {
    await navigator.clipboard.writeText(text);
    showToast(`Copied ${text.length > 40 ? text.slice(0, 40) + "…" : text}`);
  } catch {
    showToast("Couldn't copy to the clipboard");
  }
});

// Drop recordings on the window to add them to the Recorder; a single one starts playing.
getCurrentWebview()
  .onDragDropEvent(async (e) => {
    if (e.payload.type !== "drop") return;
    const paths = e.payload.paths.filter((p) => p.toLowerCase().endsWith(".screc"));
    if (!paths.length) return showToast("Only .screc recordings can be dropped here");
    try {
      const names = await importRecordings(paths);
      show(recorder);
      await recorder.refresh();
      if (names.length === 1) await playRecording(names[0]);
      showToast(names.length === 1 ? `Playing ${names[0]}` : `Added ${names.length} recordings`);
    } catch (err) {
      showToast(String(err));
    }
  })
  .catch(() => {});

getVersion()
  .then((v) => (document.querySelector("#footer-info")!.textContent = `v${v}`))
  .catch(() => {});

onSnapshot((snap) => {
  lastSnapshot = snap;
  // Nothing is visible while the window is minimised or hidden; skip all drawing. (The backend
  // also stops sending snapshots while minimised, which WebView2 doesn't report as hidden.)
  if (document.hidden) return;
  renderStatus(snap);
  // During playback the state is the recording's; there is no live controller to turn off.
  setHidden(power, !snap.state || !!snap.tape.playback);
  renderRecord(snap);
  renderSteamWarning(snap);
  renderDots(snap);
  active.update?.(snap);
});
