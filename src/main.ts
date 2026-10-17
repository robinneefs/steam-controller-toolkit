import { onSnapshot, type Snapshot } from "./controller";
import type { Tab } from "./tabs/tab";
import { VisualizerTab } from "./tabs/visualizer";

// Register new tools here; each becomes a tab.
const tabs: Tab[] = [new VisualizerTab()];

const nav = document.querySelector<HTMLElement>("#tabs")!;
const content = document.querySelector<HTMLElement>("#tab-content")!;
const status = document.querySelector<HTMLElement>("#status")!;

const panes = new Map<string, HTMLElement>();
let active: Tab = tabs[0];

function show(tab: Tab): void {
  active = tab;
  if (!panes.has(tab.id)) {
    const pane = document.createElement("div");
    pane.className = "pane";
    content.appendChild(pane);
    tab.mount(pane);
    panes.set(tab.id, pane);
  }
  for (const [id, pane] of panes) pane.hidden = id !== tab.id;
  nav.querySelectorAll("button").forEach((b) => b.classList.toggle("active", b.dataset.tab === tab.id));
}

for (const tab of tabs) {
  const b = document.createElement("button");
  b.textContent = tab.title;
  b.dataset.tab = tab.id;
  b.addEventListener("click", () => show(tab));
  nav.appendChild(b);
}
show(active);

function renderStatus(snap: Snapshot): void {
  if (!snap.state) {
    status.textContent = "Searching for controller…";
    status.className = "status";
    return;
  }
  const parts = [`Connected · ${snap.slot}`, `${snap.report_rate} Hz`];
  if (snap.battery) parts.push(`battery ${snap.battery.level}%`);
  status.textContent = parts.join(" · ");
  status.className = "status connected";
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
  steamWarning.hidden = !snap.steam_running || steamWarningDismissed;
}

onSnapshot((snap) => {
  // Nothing is visible while the window is minimised or hidden; skip all drawing.
  if (document.hidden) return;
  renderStatus(snap);
  renderSteamWarning(snap);
  active.update?.(snap);
});
