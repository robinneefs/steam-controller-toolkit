// The layout editor shared by the Keyboard & mouse and Gamepad tabs: a card per group of controls,
// a binding picker, and the bars both tabs have on top (on/off, mode, presets).
import { invoke } from "@tauri-apps/api/core";
import type { Action, Activation, Directions, Layout, PadInput, PadMode, PlayStatus, Snapshot, StickMode } from "../controller";
import { setAttr, setHtml, setText } from "../dom";
import { addHelp, helpIcon } from "../help";
import { powerHtml } from "./power";
import { KEY_LISTS, KEYS, MODIFIERS, byChar, keyFromCode, keyLabel, keysLabel, shortcutKey } from "./keys";
import { BUTTONS } from "./sources";

/** Every control a layout binds; mirrors `CONTROLS` in src-tauri/src/layout.rs (its index is its lit bit). */
export const CONTROLS = [
  "A", "B", "X", "Y", "Up", "Down", "Left", "Right", "L", "R", "LT", "RT", "LTrigClick", "RTrigClick",
  "L3", "R3", "L4", "L5", "R4", "R5", "View", "Menu", "QAM", "Steam", "LPadClick", "RPadClick",
];
/** Lit bits after the controls, as `LIT_*` in layout.rs. */
const LIT = { left_stick: CONTROLS.length, right_stick: CONTROLS.length + 1, left_pad: CONTROLS.length + 2, right_pad: CONTROLS.length + 3, gyro: CONTROLS.length + 4 };

const CONTROL_LABELS: Record<string, string> = {
  Up: "D-pad up", Down: "D-pad down", Left: "D-pad left", Right: "D-pad right",
  L: "Left bumper", R: "Right bumper", LT: "Left trigger", RT: "Right trigger",
  LTrigClick: "LT full pull", RTrigClick: "RT full pull", L3: "Stick click (L3)", R3: "Stick click (R3)",
  QAM: "Quick access", LPadClick: "Pad click", RPadClick: "Pad click",
};
const controlLabel = (c: string) => CONTROL_LABELS[c] ?? c;

export const PAD_INPUTS: [PadInput, string][] = [
  ["a", "A"], ["b", "B"], ["x", "X"], ["y", "Y"], ["lb", "LB"], ["rb", "RB"], ["lt", "LT"], ["rt", "RT"],
  ["ls", "LS click"], ["rs", "RS click"], ["back", "Back"], ["start", "Start"], ["guide", "Guide"],
  ["up", "D-pad ↑"], ["down", "D-pad ↓"], ["left", "D-pad ←"], ["right", "D-pad →"],
];
const PAD_LABELS = new Map(PAD_INPUTS);
const MOUSE_LABELS: Record<string, string> = { left: "Left click", right: "Right click", middle: "Middle click", back: "Mouse back", forward: "Mouse forward" };
const SCROLL_LABELS: Record<string, string> = { up: "Scroll up", down: "Scroll down", left: "Scroll left", right: "Scroll right" };

const escape = (s: string) => s.replace(/[&<>"]/g, (c) => `&${{ "&": "amp", "<": "lt", ">": "gt", '"': "quot" }[c]};`);
const NONE: Action = { kind: "none" };

export function actionLabel(a: Action | undefined): string {
  switch (a?.kind ?? "none") {
    case "key": {
      const k = a as Extract<Action, { kind: "key" }>;
      return keysLabel(k.keys) + (k.repeat ? " ⟳" : "");
    }
    case "mouse":
      return MOUSE_LABELS[(a as Extract<Action, { kind: "mouse" }>).button];
    case "scroll": {
      const s = a as Extract<Action, { kind: "scroll" }>;
      return SCROLL_LABELS[s.dir] + (s.repeat ? " ⟳" : "");
    }
    case "pad":
      return `Xbox ${PAD_LABELS.get((a as Extract<Action, { kind: "pad" }>).input)}`;
    case "switch":
      return "Desktop ⇄ Game";
    case "run": {
      // The file or page name is enough to tell them apart.
      const t = (a as Extract<Action, { kind: "run" }>).target.replace(/[\\/]+$/, "");
      return `Open ${/^[a-z][\w+.-]*:\/\//i.test(t) ? t.replace(/^[a-z][\w+.-]*:\/\//i, "") : t.split(/[\\/]/).pop()}`;
    }
    default:
      return "–";
  }
}

addHelp({
  "play-mode": `<b>Desktop</b> and <b>Game</b> each have a layout. With both outputs on, the desktop gets keyboard and mouse
    and games get the gamepad. The <b>Steam button</b> switches (in every built-in layout), with a click you can feel.<ul>
    <li><b>Full-screen apps use Game</b>: switches to Game when a full-screen app is in front, and back when it isn't.
      A switch by hand holds until another app goes full screen.</li></ul>`,
  "play-bind": `Click a binding to change it: press a key or combination, or pick a mouse button, a scroll, an Xbox
    input (Gamepad tab), <b>Desktop ⇄ Game</b>, a program, file or web page to <b>open</b>, or nothing. <b>⟳</b>
    repeats while held, like a held key. Rows light up as you press them.`,
  "play-radial": `A menu you use by feel, like a ring of shortcuts under your thumb. The pad is cut into a slice per
    item: the first at the top, then clockwise. Slide your thumb to an item (you feel a tick on each), then
    <b>lift</b> to use it, or <b>click</b> the pad if <b>Click to pick</b> is on. Lift or click in the centre to
    pick nothing.`,
  "play-gyro": `Aim by moving the controller. It is never needed; turn it on where it helps.<ul>
    <li><b>Mouse</b>: precise; for games that take mouse and controller at once, and the desktop.</li>
    <li><b>Right stick</b>: works in every game, a little less precise.</li>
    <li><b>Steer</b>: lean the controller like a wheel to steer with the left stick.</li>
    <li><b>When</b>: <b>thumb on the right stick</b> is the usual choice: lift it to stop, reposition, and go on.</li>
    <li><b>Turn axis</b>: <b>Auto</b> follows the real vertical however you hold it. <b>Yaw</b> as if flat on a table,
      <b>Roll</b> by leaning.</li></ul>
    Drift is calibrated automatically while the controller lies still.`,
});

// ---------- Binding picker ----------

interface PickOptions {
  /** Offer the virtual Xbox controller's inputs. */
  pad: boolean;
}

/** The popover that picks an action; one for both tabs. */
class BindPicker {
  private el = document.createElement("div");
  private choices: Action[] = [];
  private onPick: ((a: Action) => void) | null = null;
  private current: Action = NONE;
  private capturing = false;
  private held = new Set<string>();
  private combo: string[] = [];
  private anchor: HTMLElement | null = null;
  private openedAt = 0;

  constructor() {
    this.el.className = "bind-pop";
    this.el.hidden = true;
    document.body.appendChild(this.el);
    this.el.addEventListener("click", (e) => this.clicked(e.target as Element));
    this.el.addEventListener("change", (e) => this.changed(e.target as HTMLElement));
    document.addEventListener("keydown", (e) => this.keydown(e), true);
    document.addEventListener("keyup", (e) => this.keyup(e), true);
    document.addEventListener("mousedown", (e) => {
      if (!this.el.hidden && !this.el.contains(e.target as Node) && e.target !== this.anchor) this.close();
    });
    // Scrolling moves the anchor away; but a click can scroll its row into view just before.
    document.addEventListener("scroll", (e) => {
      if (!this.el.contains(e.target as Node) && performance.now() - this.openedAt > 200) this.close();
    }, true);
  }

  open(anchor: HTMLElement, title: string, current: Action, opts: PickOptions, onPick: (a: Action) => void): void {
    if (this.anchor === anchor && !this.el.hidden) return this.close();
    this.anchor = anchor;
    this.current = current;
    this.onPick = onPick;
    this.capturing = false;
    this.choices = [];
    const chip = (a: Action, label: string) => {
      this.choices.push(a);
      const on = JSON.stringify(a) === JSON.stringify(current) ? " on" : "";
      return `<button class="chip${on}" data-i="${this.choices.length - 1}">${escape(label)}</button>`;
    };
    const repeat = current.kind === "key" || current.kind === "scroll" ? current.repeat : false;
    const more = KEY_LISTS.map(([group, keys]) => `<optgroup label="${group}">${keys.map((k) => `<option value="${k}">${escape(keyLabel(k))}</option>`).join("")}</optgroup>`).join("");
    this.el.innerHTML = `
      <div class="bind-pop-head"><b>${escape(title)}</b><button class="icon-btn" data-close title="Close">×</button></div>
      <button class="capture" data-capture>${current.kind === "key" ? `<b>${escape(actionLabel(current))}</b> · ` : ""}Click, then press a key or combination</button>
      <div class="bind-sec"><span>More keys</span>
        <select class="field" data-more><option value="">Win, media keys, F13…</option>${more}</select>
        <label class="chk muted" title="After a moment, the key repeats like a held keyboard key"><input type="checkbox" data-repeat ${repeat ? "checked" : ""} />Repeat while held</label></div>
      <div class="bind-sec"><span>Mouse</span><div class="chips">${(["left", "right", "middle", "back", "forward"] as const).map((b) => chip({ kind: "mouse", button: b }, MOUSE_LABELS[b])).join("")}</div></div>
      <div class="bind-sec"><span>Scroll</span><div class="chips">${(["up", "down", "left", "right"] as const).map((d) => chip({ kind: "scroll", dir: d, repeat }, SCROLL_LABELS[d].replace("Scroll ", ""))).join("")}</div></div>
      ${opts.pad ? `<div class="bind-sec"><span>Xbox</span><div class="chips">${PAD_INPUTS.map(([i, l]) => chip({ kind: "pad", input: i }, l)).join("")}</div></div>` : ""}
      <div class="bind-sec"><span>Open</span><div class="run-row">
        <input class="field" data-run placeholder="Program, file, folder or https://…" value="${current.kind === "run" ? escape(current.target) : ""}" spellcheck="false" />
        <button class="mini" data-run-set title="Open this once per press">Set</button></div></div>
      <div class="bind-sec"><span>Other</span><div class="chips">${chip({ kind: "switch" }, "Desktop ⇄ Game")}${chip(NONE, "Nothing")}</div></div>`;
    this.el.hidden = false;
    this.openedAt = performance.now();
    const r = anchor.getBoundingClientRect();
    const b = this.el.getBoundingClientRect();
    const left = Math.min(Math.max(8, r.left), innerWidth - b.width - 8);
    const below = r.bottom + 6 + b.height <= innerHeight - 8;
    this.el.style.left = `${left}px`;
    this.el.style.top = `${below ? r.bottom + 6 : Math.max(8, r.top - 6 - b.height)}px`;
  }

  close(): void {
    this.el.hidden = true;
    this.capturing = false;
    this.anchor = null;
    this.onPick = null;
  }

  private repeat(): boolean {
    return !!this.el.querySelector<HTMLInputElement>("[data-repeat]")?.checked;
  }

  private pick(a: Action): void {
    const cb = this.onPick;
    this.close();
    cb?.(a);
  }

  private clicked(el: Element): void {
    if (el.closest("[data-close]")) return this.close();
    if (el.closest("[data-capture]")) {
      this.capturing = true;
      this.held.clear();
      this.combo = [];
      this.captureText("Press a key or combination… (Esc twice to cancel)");
      return;
    }
    if (el.closest("[data-run-set]")) return this.pickRun();
    const chip = el.closest<HTMLElement>("[data-i]");
    if (chip) this.pick(this.choices[Number(chip.dataset.i)]);
  }

  private pickRun(): void {
    const target = this.el.querySelector<HTMLInputElement>("[data-run]")?.value.trim() ?? "";
    // Windows paths are often copied with quotes ("Copy as path").
    const unquoted = target.replace(/^"(.*)"$/, "$1");
    if (unquoted) this.pick({ kind: "run", target: unquoted });
  }

  private changed(el: HTMLElement): void {
    if (el.matches("[data-more]")) {
      const key = (el as HTMLSelectElement).value;
      if (key) this.pick({ kind: "key", keys: [key], repeat: this.repeat() });
    } else if (el.matches("[data-repeat]")) {
      // Changes the binding there is, if it can repeat.
      const c = this.current;
      if (c.kind === "key" || c.kind === "scroll") {
        this.current = { ...c, repeat: this.repeat() };
        this.onPick?.(this.current);
      }
    }
  }

  private captureText(text: string): void {
    const el = this.el.querySelector<HTMLElement>("[data-capture]");
    if (el) el.textContent = text;
    el?.classList.toggle("on", this.capturing);
  }

  private keydown(e: KeyboardEvent): void {
    if (this.el.hidden) return;
    if (!this.capturing) {
      if (e.key === "Escape") this.close();
      else if (e.key === "Enter" && (e.target as Element).matches?.("[data-run]")) this.pickRun();
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;
    const key = keyFromCode(e.code);
    if (!key) return;
    // Esc twice cancels (once binds Escape).
    if (key === "Escape" && this.combo.length === 1 && this.combo[0] === "Escape" && !this.held.size) {
      this.capturing = false;
      return this.captureText("Click, then press a key or combination");
    }
    this.held.add(key);
    if (!this.combo.includes(key)) this.combo.push(key);
    this.captureText(keysLabel(this.ordered()));
  }

  private keyup(e: KeyboardEvent): void {
    if (this.el.hidden || !this.capturing) return;
    e.preventDefault();
    e.stopPropagation();
    const key = keyFromCode(e.code);
    if (key) this.held.delete(key);
    // Win's key up often goes to the Start menu instead: don't wait for it.
    if (this.held.size === 1 && this.held.has("Win")) this.held.clear();
    if (this.held.size || !this.combo.length) return;
    const keys = this.ordered();
    if (keys.length === 1 && keys[0] === "Escape") {
      // Wait a moment for a second Esc (cancel); else bind Escape.
      window.setTimeout(() => {
        if (this.capturing && this.combo.length === 1 && !this.held.size) this.pick({ kind: "key", keys, repeat: this.repeat() });
      }, 400);
      return;
    }
    this.pick({ kind: "key", keys, repeat: this.repeat() });
  }

  /** Modifiers first, as the backend presses them. */
  private ordered(): string[] {
    const order = (k: string) => (MODIFIERS.has(k as (typeof KEYS)[number]) ? KEYS.indexOf(k as (typeof KEYS)[number]) : 100);
    const keys = [...this.combo].sort((a, b) => order(a) - order(b));
    // A shortcut's letters are characters: the letter the pressed key types on this layout.
    return byChar(keys) ? keys.map((k) => (MODIFIERS.has(k as (typeof KEYS)[number]) ? k : shortcutKey(k))) : keys;
  }
}

let picker: BindPicker | null = null;
const getPicker = () => (picker ??= new BindPicker());

// ---------- The editor ----------

type Part = "left_stick" | "right_stick" | "left_pad" | "right_pad";
const PART_TITLES: Record<Part, string> = { left_stick: "Left stick", right_stick: "Right stick", left_pad: "Left trackpad", right_pad: "Right trackpad" };
const PART_CLICK: Record<Part, string> = { left_stick: "L3", right_stick: "R3", left_pad: "LPadClick", right_pad: "RPadClick" };
const DIRS = ["up", "down", "left", "right"] as const;
const DIR_LABELS = { up: "Up", down: "Down", left: "Left", right: "Right" };
/** How many items a radial menu can have. */
const RADIAL_SIZES = [2, 3, 4, 5, 6, 8];
const ARROWS = ["↑", "↗", "→", "↘", "↓", "↙", "←", "↖"];
/** Item `i` of `n`: its number and the direction it points (the first is at the top, clockwise). */
const radialLabel = (i: number, n: number) => `${i + 1}  ${ARROWS[Math.round((i * 8) / n) % 8]}`;
/** A new radial menu's items: copy, paste, undo, redo (the D-pad on the Gamepad tab). */
const radialItems = (pad: boolean): Action[] =>
  pad
    ? (["up", "right", "down", "left"] as const).map((input) => ({ kind: "pad", input }))
    : ["C", "V", "Z", "Y"].map((k) => ({ kind: "key", keys: ["Ctrl", k], repeat: false }));

const keys4 = (u: string, l: string, d: string, r: string): Directions => ({
  up: { kind: "key", keys: [u], repeat: false },
  left: { kind: "key", keys: [l], repeat: false },
  down: { kind: "key", keys: [d], repeat: false },
  right: { kind: "key", keys: [r], repeat: false },
});
const FILLS: Record<string, () => Directions> = {
  wasd: () => keys4("W", "A", "S", "D"),
  arrows: () => keys4("Up", "Left", "Down", "Right"),
  dpad: () => ({ up: { kind: "pad", input: "up" }, down: { kind: "pad", input: "down" }, left: { kind: "pad", input: "left" }, right: { kind: "pad", input: "right" } }),
};

const CARDS: [string, string[]][] = [
  ["Face buttons", ["A", "B", "X", "Y"]],
  ["D-pad", ["Up", "Down", "Left", "Right"]],
  ["Shoulders", ["L", "R", "LT", "RT", "LTrigClick", "RTrigClick"]],
  ["Back grips", ["L4", "L5", "R4", "R5"]],
  ["Menu buttons", ["View", "Menu", "QAM", "Steam"]],
];

const ACTIVATIONS: [Activation["kind"], string][] = [
  ["right_stick_touch", "Thumb on the right stick"],
  ["right_pad_touch", "Thumb on the right pad"],
  ["left_trigger", "While LT is pulled"],
  ["right_trigger", "While RT is pulled"],
  ["hold", "While a button is held"],
  ["toggle", "Toggle with a button"],
  ["always", "Always"],
];

const pct = (v: number) => `${Math.round(v * 100)}%`;

interface Slider {
  label: string;
  min: number;
  max: number;
  step: number;
  fmt: (v: number) => string;
  title?: string;
}
const SLIDERS: Record<string, Slider> = {
  deadzone: { label: "Deadzone", min: 0, max: 0.5, step: 0.01, fmt: pct, title: "Travel at rest that is ignored" },
  threshold: { label: "Press at", min: 0.1, max: 0.9, step: 0.05, fmt: pct, title: "How far to push before a direction presses" },
  "stick.mouse": { label: "Speed", min: 200, max: 4000, step: 50, fmt: (v) => `${v} px/s`, title: "Pointer speed at full tilt" },
  "stick.scroll": { label: "Speed", min: 2, max: 40, step: 1, fmt: (v) => `${v} /s`, title: "Notches per second at full tilt" },
  "pad.mouse": { label: "Speed", min: 300, max: 3000, step: 50, fmt: (v) => `${v} px`, title: "Pixels for a swipe across the whole pad" },
  "pad.scroll": { label: "Speed", min: 4, max: 40, step: 1, fmt: (v) => `${v}`, title: "Notches for a swipe across the whole pad" },
  mouse_speed: { label: "Speed", min: 2, max: 60, step: 1, fmt: (v) => `${v} px/°`, title: "Pixels per degree turned" },
  stick_range: { label: "Full stick at", min: 45, max: 720, step: 15, fmt: (v) => `${v} °/s`, title: "Turn speed that tilts the stick fully; lower is more sensitive" },
  steer_angle: { label: "Full lock at", min: 10, max: 90, step: 5, fmt: (v) => `${v}°`, title: "How far to lean for full steering" },
};

export interface EditorOptions {
  /** The Gamepad tab: Xbox inputs, stick and steer modes. */
  pad: boolean;
  /** After every edit. */
  onChange: () => void;
}

/** Edits one layout in place. */
export class LayoutEditor {
  private layout: Layout | null = null;
  private lastLit = -1;
  private lastGyro = false;
  private lastRadial = "";

  constructor(private root: HTMLElement, private opts: EditorOptions) {
    root.classList.add("layout-editor");
    // Labels follow the keyboard layout, read once it's known and again when it changes.
    window.addEventListener("keylayout", () => this.layout && this.render());
    root.addEventListener("click", (e) => this.clicked(e.target as Element));
    root.addEventListener("change", (e) => this.changed(e.target as HTMLElement, true));
    root.addEventListener("input", (e) => {
      if ((e.target as HTMLInputElement).type === "range") this.changed(e.target as HTMLElement, false);
    });
  }

  set(layout: Layout): void {
    this.layout = layout;
    getPicker().close();
    this.render();
  }

  get(): Layout | null {
    return this.layout;
  }

  /** Light each trackpad's radial menu item under the thumb (`preview.radial`). */
  showRadial(radial: [number | null, number | null]): void {
    const at = `${radial[0]},${radial[1]}`;
    if (at === this.lastRadial) return;
    this.lastRadial = at;
    this.root.querySelectorAll<HTMLElement>("[data-radial]").forEach((el) => {
      const [side, i] = el.dataset.radial!.split(":").map(Number);
      el.classList.toggle("lit", radial[side] === i);
    });
  }

  /** Light the rows and cards in use (`preview.lit`). */
  showLit(lit: number, gyro: boolean): void {
    if (lit === this.lastLit && gyro === this.lastGyro) return;
    const changed = this.lastLit < 0 ? ~0 : lit ^ this.lastLit;
    this.lastLit = lit;
    this.lastGyro = gyro;
    this.root.querySelectorAll<HTMLElement>("[data-lit]").forEach((el) => {
      const bit = Number(el.dataset.lit);
      if (changed & (1 << bit)) el.classList.toggle("lit", (lit & (1 << bit)) !== 0);
    });
    const pill = this.root.querySelector<HTMLElement>("[data-gyro-pill]");
    if (pill) pill.classList.toggle("on", gyro);
  }

  private render(): void {
    const l = this.layout;
    if (!l) return;
    this.lastLit = -1;
    this.lastRadial = "";
    const cards = [
      ...CARDS.slice(0, 4).map(([title, controls]) => this.buttonsCard(title, controls)),
      ...(["left_stick", "right_stick", "left_pad", "right_pad"] as Part[]).map((p) => this.partCard(p)),
      this.buttonsCard(...CARDS[4]),
      this.gyroCard(),
    ];
    this.root.innerHTML = cards.join("");
  }

  private bindRow(path: string, label: string, action: Action | undefined, lit?: number): string {
    const none = !action || action.kind === "none";
    return `<div class="bind-row"${lit === undefined ? "" : ` data-lit="${lit}"`}>
      <span class="bind-ctl">${escape(label)}</span>
      <button class="bind${none ? " none" : ""}" data-bind="${path}" title="Click to change">${escape(actionLabel(action))}</button></div>`;
  }

  private buttonsCard(title: string, controls: string[]): string {
    const rows = controls.map((c) => this.bindRow(`c:${c}`, controlLabel(c), this.layout!.buttons[c], CONTROLS.indexOf(c))).join("");
    return `<section class="card"><h4>${title}</h4>${rows}</section>`;
  }

  private partCard(part: Part): string {
    const mode = this.layout![part];
    const stick = part.endsWith("stick");
    const kinds: [string, string][] = stick
      ? [["none", "Nothing"], ...(this.opts.pad || mode.kind === "stick" ? [["stick", "Xbox stick"] as [string, string]] : []), ["directions", "Directions"], ["mouse", "Mouse pointer"], ["scroll", "Scroll"]]
      : [["none", "Nothing"], ["mouse", "Touchpad mouse"], ["scroll", "Swipe to scroll"], ...(this.opts.pad || mode.kind === "stick" ? [["stick", "Xbox stick"] as [string, string]] : []), ["directions", "Directions (4 zones)"], ["radial", "Radial menu"]];
    const select = `<select class="field" data-mode="${part}" title="What the ${PART_TITLES[part].toLowerCase()} does">${kinds.map(([k, t]) => `<option value="${k}"${k === mode.kind ? " selected" : ""}>${t}</option>`).join("")}</select>`;
    const click = PART_CLICK[part];
    return `<section class="card" data-lit="${LIT[part]}"><h4>${PART_TITLES[part]}${select}</h4>
      <div class="card-opts">${this.partOptions(part, mode)}</div>
      ${this.bindRow(`c:${click}`, controlLabel(click), this.layout!.buttons[click], CONTROLS.indexOf(click))}</section>`;
  }

  private slider(path: string, key: string, value: number): string {
    const s = SLIDERS[key];
    return `<label class="opt"${s.title ? ` title="${s.title}"` : ""}><span>${s.label}</span>
      <input type="range" data-o="${path}" min="${s.min}" max="${s.max}" step="${s.step}" value="${value}" /><b data-v="${path}">${s.fmt(value)}</b></label>`;
  }

  private sideSelect(path: string, value: string): string {
    return `<label class="opt"><span>Moves</span><select class="field" data-o="${path}">
      <option value="left"${value === "left" ? " selected" : ""}>Left stick</option>
      <option value="right"${value === "right" ? " selected" : ""}>Right stick</option></select></label>`;
  }

  private partOptions(part: Part, m: StickMode | PadMode): string {
    const stick = part.endsWith("stick");
    switch (m.kind) {
      case "stick":
        return this.sideSelect(`${part}.target`, m.target) + ("deadzone" in m ? this.slider(`${part}.deadzone`, "deadzone", m.deadzone) : "");
      case "mouse":
        return stick
          ? this.slider(`${part}.speed`, "stick.mouse", m.speed) + this.slider(`${part}.deadzone`, "deadzone", (m as Extract<StickMode, { kind: "mouse" }>).deadzone)
          : this.slider(`${part}.speed`, "pad.mouse", m.speed) +
              `<label class="chk opt-chk" title="A flick keeps the pointer gliding after your thumb lifts"><input type="checkbox" data-o="${part}.trackball" ${(m as Extract<PadMode, { kind: "mouse" }>).trackball ? "checked" : ""} />Trackball glide</label>`;
      case "scroll":
        return stick
          ? this.slider(`${part}.speed`, "stick.scroll", m.speed) + this.slider(`${part}.deadzone`, "deadzone", (m as Extract<StickMode, { kind: "scroll" }>).deadzone)
          : this.slider(`${part}.speed`, "pad.scroll", m.speed);
      case "directions": {
        const fills = `<span class="fills muted">Fill:
          <button class="chip" data-fill="${part}:wasd">WASD</button><button class="chip" data-fill="${part}:arrows">Arrows</button>${this.opts.pad ? `<button class="chip" data-fill="${part}:dpad">D-pad</button>` : ""}</span>`;
        const extra = "threshold" in m
          ? this.slider(`${part}.threshold`, "threshold", m.threshold)
          : `<label class="chk opt-chk" title="Directions press only while the pad is clicked; off: as soon as it's touched"><input type="checkbox" data-o="${part}.click" ${m.click ? "checked" : ""} />Only while clicked</label>`;
        return extra + DIRS.map((d) => this.bindRow(`d:${part}.${d}`, DIR_LABELS[d], m[d])).join("") + fills;
      }
      case "radial": {
        const n = m.items.length;
        const side = part === "left_pad" ? 0 : 1;
        return `<label class="opt"><span>Items ${helpIcon("play-radial")}</span><select class="field" data-radial-n="${part}">${RADIAL_SIZES.map((k) => `<option value="${k}"${k === n ? " selected" : ""}>${k}</option>`).join("")}</select></label>
          <label class="chk opt-chk" title="Pick by clicking the pad; off: by lifting your thumb"><input type="checkbox" data-o="${part}.click" ${m.click ? "checked" : ""} />Click to pick</label>` +
          m.items.map((a, i) => this.bindRow(`r:${part}.${i}`, radialLabel(i, n), a).replace('class="bind-row"', `class="bind-row" data-radial="${side}:${i}"`)).join("");
      }
      default:
        return "";
    }
  }

  private gyroCard(): string {
    const g = this.layout!.gyro;
    const outs: [string, string][] = [["none", "Off"], ["mouse", "Mouse"], ...(this.opts.pad || g.output === "stick" ? [["stick", "Right stick"] as [string, string]] : []), ...(this.opts.pad || g.output === "steer" ? [["steer", "Steer (left stick)"] as [string, string]] : [])];
    const out = `<select class="field" data-o="gyro.output">${outs.map(([k, t]) => `<option value="${k}"${k === g.output ? " selected" : ""}>${t}</option>`).join("")}</select>`;
    let body = `<p class="hint">Off. Turn it on to aim by moving the controller; never needed.</p>`;
    if (g.output !== "none") {
      const act = g.activation;
      const btn = act.kind === "hold" || act.kind === "toggle" ? act.button : null;
      body = `<label class="opt"><span>When</span><select class="field" data-act>${ACTIVATIONS.map(([k, t]) => `<option value="${k}"${k === act.kind ? " selected" : ""}>${t}</option>`).join("")}</select></label>` +
        (btn !== null ? `<label class="opt"><span>Button</span><select class="field" data-act-button>${BUTTONS.map(([n, t]) => `<option value="${n}"${n === btn ? " selected" : ""}>${t}</option>`).join("")}</select></label>` : "") +
        (g.output === "mouse" ? this.slider("gyro.mouse_speed", "mouse_speed", g.mouse_speed) : g.output === "stick" ? this.slider("gyro.stick_range", "stick_range", g.stick_range) : this.slider("gyro.steer_angle", "steer_angle", g.steer_angle)) +
        (g.output !== "steer"
          ? `<label class="opt"><span>Turn axis</span><select class="field" data-o="gyro.axis">
              <option value="auto"${g.axis === "auto" ? " selected" : ""}>Auto (real vertical)</option>
              <option value="yaw"${g.axis === "yaw" ? " selected" : ""}>Yaw (turn it flat)</option>
              <option value="roll"${g.axis === "roll" ? " selected" : ""}>Roll (lean it)</option></select></label>
            <label class="chk opt-chk"><input type="checkbox" data-o="gyro.invert_y" ${g.invert_y ? "checked" : ""} />Invert up/down</label>`
          : "");
    }
    return `<section class="card" data-lit="${LIT.gyro}"><h4><span class="h3-title">Gyro ${helpIcon("play-gyro")}</span>
      <span class="gyro-pill" data-gyro-pill>gyro on</span>${out}</h4><div class="card-opts">${body}</div></section>`;
  }

  // ---------- Edits ----------

  private getAction(path: string): Action {
    const l = this.layout!;
    if (path.startsWith("c:")) return l.buttons[path.slice(2)] ?? NONE;
    if (path.startsWith("r:")) {
      const [part, i] = path.slice(2).split(".");
      return (l[part as Part] as Extract<PadMode, { kind: "radial" }>).items[Number(i)] ?? NONE;
    }
    const [part, dir] = path.slice(2).split(".") as [Part, (typeof DIRS)[number]];
    return (l[part] as unknown as Directions)[dir] ?? NONE;
  }

  private setAction(path: string, a: Action): void {
    const l = this.layout!;
    if (path.startsWith("c:")) {
      if (a.kind === "none") delete l.buttons[path.slice(2)];
      else l.buttons[path.slice(2)] = a;
    } else if (path.startsWith("r:")) {
      const [part, i] = path.slice(2).split(".");
      (l[part as Part] as Extract<PadMode, { kind: "radial" }>).items[Number(i)] = a;
    } else {
      const [part, dir] = path.slice(2).split(".") as [Part, (typeof DIRS)[number]];
      (l[part] as unknown as Directions)[dir] = a;
    }
  }

  private clicked(el: Element): void {
    if (!this.layout) return;
    const bind = el.closest<HTMLElement>("[data-bind]");
    if (bind) {
      const path = bind.dataset.bind!;
      const label = bind.closest(".bind-row")?.querySelector(".bind-ctl")?.textContent ?? "";
      const card = bind.closest(".card")?.querySelector("h4")?.firstChild?.textContent?.trim() ?? "";
      const title = path.startsWith("d:") ? `${card}: ${label}` : path.startsWith("r:") ? `${card}: item ${label}` : label;
      getPicker().open(bind, title, this.getAction(path), { pad: this.opts.pad }, (a) => {
        this.setAction(path, a);
        bind.textContent = actionLabel(a);
        bind.classList.toggle("none", a.kind === "none");
        this.opts.onChange();
      });
      return;
    }
    const fill = el.closest<HTMLElement>("[data-fill]");
    if (fill) {
      const [part, which] = fill.dataset.fill!.split(":") as [Part, string];
      Object.assign(this.layout[part], FILLS[which]());
      this.render();
      this.opts.onChange();
    }
  }

  private changed(el: HTMLElement, done: boolean): void {
    const l = this.layout;
    if (!l) return;
    if (el.matches("[data-mode]")) {
      const part = el.dataset.mode as Part;
      (l as unknown as Record<Part, StickMode | PadMode>)[part] = this.defaultMode(part, (el as HTMLSelectElement).value);
      this.render();
      return this.opts.onChange();
    }
    if (el.matches("[data-radial-n]")) {
      const m = l[el.dataset.radialN as Part] as Extract<PadMode, { kind: "radial" }>;
      const n = Number((el as HTMLSelectElement).value);
      m.items = Array.from({ length: n }, (_, i) => m.items[i] ?? NONE);
      this.render();
      return this.opts.onChange();
    }
    if (el.matches("[data-act]")) {
      const kind = (el as HTMLSelectElement).value as Activation["kind"];
      l.gyro.activation = kind === "hold" || kind === "toggle" ? { kind, button: "R5" } : ({ kind } as Activation);
      this.render();
      return this.opts.onChange();
    }
    if (el.matches("[data-act-button]")) {
      const a = l.gyro.activation;
      if (a.kind === "hold" || a.kind === "toggle") a.button = (el as HTMLSelectElement).value;
      return this.opts.onChange();
    }
    const path = el.dataset.o;
    if (!path) return;
    const [part, field] = path.split(".");
    const obj = (l as unknown as Record<string, Record<string, unknown>>)[part];
    const input = el as HTMLInputElement;
    obj[field] = input.type === "checkbox" ? input.checked : input.type === "range" ? Number(input.value) : input.value;
    if (input.type === "range") {
      const v = this.root.querySelector<HTMLElement>(`[data-v="${path}"]`);
      const key = field === "speed" ? `${part.endsWith("stick") ? "stick" : "pad"}.${(obj as { kind: string }).kind}` : field;
      if (v) setText(v, SLIDERS[key].fmt(Number(input.value)));
    }
    // Turning the gyro on: aim while the thumb rests on the right stick, steer all the time.
    if (path === "gyro.output") {
      const out = input.value;
      const kind = l.gyro.activation.kind;
      if (out === "steer") l.gyro.activation = { kind: "always" };
      else if (out !== "none" && kind === "always") l.gyro.activation = { kind: "right_stick_touch" };
      // The gyro card shows other options per output.
      this.render();
    }
    if (done || input.type === "range") this.opts.onChange();
  }

  private defaultMode(part: Part, kind: string): StickMode | PadMode {
    const side = part.startsWith("left") ? "left" : "right";
    const dirs = this.opts.pad ? FILLS.dpad() : side === "left" ? FILLS.wasd() : FILLS.arrows();
    if (part.endsWith("stick")) {
      switch (kind) {
        case "stick": return { kind, target: side, deadzone: 0.08 };
        case "directions": return { kind, threshold: 0.4, ...dirs };
        case "mouse": return { kind, speed: 1400, deadzone: 0.12 };
        case "scroll": return { kind, speed: 18, deadzone: 0.15 };
        default: return { kind: "none" };
      }
    }
    switch (kind) {
      case "mouse": return { kind, speed: 1100, trackball: false };
      case "scroll": return { kind, speed: 12 };
      case "stick": return { kind, target: side };
      case "directions": return { kind, click: true, ...dirs };
      case "radial": return { kind, click: false, items: radialItems(this.opts.pad) };
      default: return { kind: "none" };
    }
  }
}

// ---------- Bars shared by both tabs ----------

export type PlayKind = "keymouse" | "gamepad";

/** On/off, status, the Desktop | Game mode and auto switch. */
export function playBarHtml(kind: PlayKind): string {
  return `<section class="tile play-bar">
    ${powerHtml(kind === "keymouse" ? "Keyboard &amp; mouse" : "Gamepad", "Turn this output on or off")}
    <span class="midi-conn play-conn"><i data-dot></i><b data-status></b></span>
    ${kind === "keymouse" ? `<span class="bar-group"><span class="muted">Mode</span>
      <span class="segmented" data-modes><button data-mode="desktop" title="Use the Desktop layout">Desktop</button><button data-mode="game" title="Use the Game layout">Game</button></span>
      ${helpIcon("play-mode")}</span>
    <label class="chk muted" title="A full-screen app switches to Game, anything else back to Desktop"><input type="checkbox" data-auto /><span class="long">Full-screen apps use Game</span><span class="short">Auto</span></label>` : ""}
  </section>`;
}

/** Wires the bar's mode and auto switch, which only Keyboard & mouse has: the gamepad is for games
 * (on/off is the tab's, it sends its whole config). */
export function wirePlayBar(root: HTMLElement, onToggle: (on: boolean) => void): void {
  root.querySelector<HTMLInputElement>("[data-on]")!.addEventListener("change", (e) => onToggle((e.target as HTMLInputElement).checked));
  root.querySelector("[data-modes]")?.addEventListener("click", (e) => {
    const b = (e.target as Element).closest<HTMLElement>("[data-mode]");
    if (b) invoke("play_set_mode", { mode: b.dataset.mode }).catch(() => {});
  });
  root.querySelector<HTMLInputElement>("[data-auto]")?.addEventListener("change", (e) =>
    invoke("play_set_auto_switch", { on: (e.target as HTMLInputElement).checked }).catch(() => {}),
  );
}

/** Status per output as [dot class, text]. */
export function playStatusText(p: PlayStatus, kind: PlayKind): [string, string] {
  if (kind === "keymouse") {
    const k = p.keymouse;
    if (!k.supported) return ["warn", "Not available on this system yet"];
    if (!k.enabled) return ["", "Off"];
    if (k.error) return ["warn", k.error];
    if (p.active === "desktop") return ["ok", "Sending · Desktop layout"];
    if (p.active === "game") return ["ok", `Sending · Game layout${p.fullscreen_app ? ` (${p.fullscreen_app})` : ""}`];
    if (p.active?.startsWith("app:")) return ["ok", `Sending · ${p.app} layout`];
    return ["", "On · the gamepad plays in Game mode"];
  }
  const g = p.gamepad;
  if (g.support?.kind === "unsupported") return ["warn", "Not available on this system"];
  if (!g.enabled) return ["", "Off"];
  if (!g.support) return ["", "Checking the driver…"];
  if (g.support.kind === "missing") return ["warn", g.support.detail];
  if (g.connected) {
    const who = g.player ? ` · player ${g.player}` : "";
    return p.active === "pad" ? ["ok", `Connected${who}`] : ["ok", `Connected${who} · Keys & mouse has the desktop`];
  }
  if (g.error) return ["warn", g.error];
  return ["", "Plugging in the virtual controller…"];
}

export function renderPlayBar(root: HTMLElement, snap: Snapshot, kind: PlayKind): void {
  const p = snap.play;
  if (!p) return;
  const on = root.querySelector<HTMLInputElement>("[data-on]")!;
  const enabled = kind === "keymouse" ? p.keymouse.enabled : p.gamepad.enabled;
  if (on.checked !== enabled) on.checked = enabled;
  const [cls, text] = playStatusText(p, kind);
  setAttr(root.querySelector("[data-dot]")!, "class", cls);
  setText(root.querySelector("[data-status]")!, text);
  root.querySelectorAll<HTMLElement>("[data-mode]").forEach((b) => b.classList.toggle("on", b.dataset.mode === p.mode));
  const auto = root.querySelector<HTMLInputElement>("[data-auto]");
  if (auto && auto.checked !== p.auto_switch) auto.checked = p.auto_switch;
}

/** Notes both tabs show beside the live view. */
export function notesHtml(): string {
  return `<section class="tile play-notes">
    <p class="callout" data-steam-note hidden><b>Steam is running.</b> Its Desktop Layout turns the controller into mouse and
      keyboard input as well, so input arrives twice. These outputs are meant for use without Steam: quit it, or turn
      its controller support off for this controller.</p>
    <p class="hint">Apps running as administrator don't receive this input unless this app runs as administrator too.</p>
  </section>`;
}

export function renderNotes(root: HTMLElement, snap: Snapshot): void {
  const el = root.querySelector<HTMLElement>("[data-steam-note]");
  if (el && el.hidden === snap.steam_running) el.hidden = !snap.steam_running;
}

/** Held keys and mouse buttons as chips. */
export function chipsHtml(keys: string[], mouseButtons: number): string {
  const buttons = (["left", "right", "middle", "back", "forward"] as const).filter((_, i) => mouseButtons & (1 << i)).map((b) => MOUSE_LABELS[b]);
  const chips = [...keys.map((k) => keyLabel(k, byChar(keys))), ...buttons].map((t) => `<span class="key-chip">${escape(t)}</span>`).join("");
  return chips || `<span class="muted">Nothing held</span>`;
}

export function renderChips(el: HTMLElement, keys: string[], mouseButtons: number): void {
  setHtml(el, chipsHtml(keys, mouseButtons));
}

// ---------- Presets ----------

interface PresetInfo {
  name: string;
  builtin: boolean;
}

interface Loaded {
  info: PresetInfo | null;
  edited: boolean;
}

/** The preset picker with Save as / Delete / Folder, for one or more layouts (`key`). */
export class PresetBar {
  private presets: PresetInfo[] = [];
  private state = new Map<string, Loaded>();
  private key = "";
  private armTimer: number | undefined;

  static html(label: string): string {
    return `<span class="bar-group preset-bar">
      <span class="muted">${label}</span>
      <select class="field" data-preset></select>
      <input class="field" data-save-name placeholder="Layout name" maxlength="60" hidden />
      <button class="mini" data-save title="Save this layout under a name">Save as…</button>
      <button class="mini" data-save-cancel hidden>Cancel</button>
      <button class="mini danger" data-delete title="Delete this saved layout">Delete</button>
      <button class="mini" data-folder title="Open the layouts folder (share layouts by copying the .json files)">Folder</button>
    </span>`;
  }

  constructor(
    private root: HTMLElement,
    private kind: PlayKind,
    private hooks: { layout(): Layout | null; load(l: Layout): void; flash(text: string): void },
  ) {
    const q = <T extends HTMLElement>(s: string) => root.querySelector<T>(s)!;
    q<HTMLSelectElement>("[data-preset]").addEventListener("change", (e) => this.load((e.target as HTMLSelectElement).value));
    q("[data-save]").addEventListener("click", () => this.saveClicked());
    q("[data-save-cancel]").addEventListener("click", () => this.showName(false));
    q<HTMLInputElement>("[data-save-name]").addEventListener("keydown", (e) => {
      if (e.key === "Enter") this.saveClicked();
      if (e.key === "Escape") this.showName(false);
    });
    q("[data-delete]").addEventListener("click", () => this.deleteClicked());
    q("[data-folder]").addEventListener("click", () => invoke("play_reveal_presets", { kind }).catch((e) => hooks.flash(String(e))));
  }

  /** Load the list; each key's layout is matched by name (saved first, then built-in). */
  async init(names: Record<string, string>): Promise<void> {
    this.presets = await invoke<PresetInfo[]>("play_list_presets", { kind: this.kind });
    for (const [key, name] of Object.entries(names)) {
      const info = this.presets.find((p) => p.name === name && !p.builtin) ?? this.presets.find((p) => p.name === name) ?? null;
      this.state.set(key, { info, edited: false });
    }
    this.render();
  }

  select(key: string): void {
    this.key = key;
    this.showName(false);
    this.render();
  }

  edited(): void {
    const s = this.state.get(this.key);
    if (s && !s.edited) {
      s.edited = true;
      this.render();
    }
  }

  private cur(): Loaded {
    let s = this.state.get(this.key);
    if (!s) this.state.set(this.key, (s = { info: null, edited: false }));
    return s;
  }

  private render(): void {
    const { info, edited } = this.cur();
    const opt = (i: PresetInfo) => `<option value="${i.builtin ? "b" : "s"}:${escape(i.name)}">${escape(i.name)}</option>`;
    const builtin = this.presets.filter((i) => i.builtin);
    const saved = this.presets.filter((i) => !i.builtin);
    const sel = this.root.querySelector<HTMLSelectElement>("[data-preset]")!;
    const name = this.hooks.layout()?.name || "Custom";
    sel.innerHTML = (info ? "" : `<option value="">${escape(name)} (unsaved)</option>`) +
      `<optgroup label="Built-in">${builtin.map(opt).join("")}</optgroup>` +
      (saved.length ? `<optgroup label="Saved">${saved.map(opt).join("")}</optgroup>` : "");
    sel.value = info ? `${info.builtin ? "b" : "s"}:${info.name}` : "";
    sel.classList.toggle("edited", edited);
    sel.title = edited ? "Changed since it was loaded; Save as… to keep the changes" : "";
    this.root.querySelector<HTMLElement>("[data-delete]")!.hidden = !info || info.builtin;
    this.disarm();
  }

  private async load(value: string): Promise<void> {
    if (!value) return;
    const builtin = value.startsWith("b:");
    const name = value.slice(2);
    try {
      const layout = await invoke<Layout>("play_load_preset", { kind: this.kind, name, builtin });
      this.state.set(this.key, { info: this.presets.find((p) => p.name === name && p.builtin === builtin) ?? null, edited: false });
      this.hooks.load(layout);
      this.render();
    } catch (e) {
      this.hooks.flash(String(e));
      this.render();
    }
  }

  private showName(on: boolean): void {
    const q = (s: string) => this.root.querySelector<HTMLElement>(s)!;
    const input = q("[data-save-name]") as HTMLInputElement;
    input.hidden = !on;
    q("[data-preset]").hidden = on;
    q("[data-save-cancel]").hidden = !on;
    q("[data-save]").textContent = on ? "Save" : "Save as…";
    if (on) {
      const { info } = this.cur();
      input.value = info && !info.builtin ? info.name : "";
      input.focus();
      input.select();
    }
  }

  private async saveClicked(): Promise<void> {
    const input = this.root.querySelector<HTMLInputElement>("[data-save-name]")!;
    if (input.hidden) return this.showName(true);
    const name = input.value.trim();
    const layout = this.hooks.layout();
    if (!name || !layout) return;
    try {
      await invoke("play_save_preset", { kind: this.kind, name, layout });
      this.presets = await invoke<PresetInfo[]>("play_list_presets", { kind: this.kind });
      this.state.set(this.key, { info: this.presets.find((p) => p.name === name && !p.builtin) ?? null, edited: false });
      this.hooks.load({ ...layout, name });
      this.showName(false);
      this.render();
    } catch (e) {
      this.hooks.flash(String(e));
    }
  }

  private async deleteClicked(): Promise<void> {
    const b = this.root.querySelector<HTMLElement>("[data-delete]")!;
    const { info } = this.cur();
    if (!info || info.builtin) return;
    if (!b.classList.contains("armed")) {
      b.classList.add("armed");
      b.textContent = "Click to delete";
      this.armTimer = window.setTimeout(() => this.disarm(), 3000);
      return;
    }
    try {
      await invoke("play_delete_preset", { kind: this.kind, name: info.name });
      this.presets = await invoke<PresetInfo[]>("play_list_presets", { kind: this.kind });
      // The layout stays in play, now unsaved.
      this.state.set(this.key, { info: null, edited: false });
      this.render();
    } catch (e) {
      this.hooks.flash(String(e));
    }
  }

  private disarm(): void {
    clearTimeout(this.armTimer);
    const b = this.root.querySelector<HTMLElement>("[data-delete]")!;
    b.classList.remove("armed");
    b.textContent = "Delete";
  }
}
