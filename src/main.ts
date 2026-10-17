import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { CHARGE_STATES, describeSlot, onSnapshot, type Snapshot } from "./controller";
import { AboutTab } from "./tabs/about";
import { RawTab } from "./tabs/raw";
import type { Tab } from "./tabs/tab";
import { VisualizerTab } from "./tabs/visualizer";

// Register new tools here; each becomes a tab.
const tabs: Tab[] = [new VisualizerTab(), new RawTab(), new AboutTab()];

const nav = document.querySelector<HTMLElement>("#tabs")!;
const content = document.querySelector<HTMLElement>("#tab-content")!;
const status = document.querySelector<HTMLElement>("#status")!;

const panes = new Map<string, HTMLElement>();
let active: Tab = tabs[0];
let lastSnapshot: Snapshot | null = null;

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
  if (lastSnapshot) tab.update?.(lastSnapshot);
}

for (const tab of tabs) {
  const b = document.createElement("button");
  b.textContent = tab.title;
  b.dataset.tab = tab.id;
  b.addEventListener("click", () => show(tab));
  nav.appendChild(b);
}
show(active);

let lastStatus = "";
function renderStatus(snap: Snapshot): void {
  const b = snap.battery;
  const html = snap.state
    ? `<span>${snap.slot ? describeSlot(snap.slot).label : ""}</span><span class="num">${snap.report_rate} Hz</span>` +
      (b
        ? `<span class="battery" title="${CHARGE_STATES[b.charge_state] ?? ""}"><span class="cell${b.level <= 20 ? " low" : ""}"><i style="width:${b.level}%"></i></span><span class="num">${b.level}%</span></span>`
        : "")
    : `<span class="searching">Searching for controller…</span>`;
  if (html === lastStatus) return;
  lastStatus = html;
  status.innerHTML = html;
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

getVersion()
  .then((v) => (document.querySelector("#footer-info")!.textContent = `v${v}`))
  .catch(() => {});

onSnapshot((snap) => {
  lastSnapshot = snap;
  // Nothing is visible while the window is minimised or hidden; skip all drawing.
  if (document.hidden) return;
  renderStatus(snap);
  power.hidden = !snap.state;
  renderSteamWarning(snap);
  active.update?.(snap);
});
