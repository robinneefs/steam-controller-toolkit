import { invoke } from "@tauri-apps/api/core";
import type { Macro, MacroStep, Snapshot } from "../controller";
import { setAttr, setHidden, setText } from "../dom";
import { NONE, actionLabel, escape } from "./actions";
import { ICONS } from "./icons";
import { closePicker, pickAction } from "./layout-editor";
import type { Tab } from "./tab";

/** The Macros tab's Play waits this long, to leave time to click into the window it's for. */
const PLAY_DELAY_MS = 3000;
/** A new step: a short press, no wait. */
const NEW_STEP: MacroStep = { action: NONE, hold: 30, wait: 0 };
/** The backend holds and lets go of every step at least this long (`MIN_HOLD`, `MIN_GAP` in macros.rs). */
const MIN_MS = 20;
const PLAY_ICON = `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 4.5v15l13-7.5z"/></svg>`;

/** Key presses, clicks and scrolls with waits in between, played with one press from the Command Deck. */
export class MacrosTab implements Tab {
  id = "macros";
  title = "Macros";
  group = "tools" as const;
  icon = ICONS.macros;
  private root!: HTMLElement;
  private names: string[] = [];
  private name: string | null = null;
  private macro: Macro | null = null;
  private playing: string[] = [];
  private countdown: number | undefined;
  private countdownLeft = 0;
  private saveTimer: number | undefined;
  private shown = false;

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="macros">
        <section class="tile macro-list">
          <h3>Macros <span><button class="mini" data-new title="Make a new macro">+ New</button>
            <button class="mini" data-folder title="Open the macros folder in the file manager">Folder</button></span></h3>
          <div class="macro-names" data-names></div>
          <p class="hint">Put a macro on a button, wheel slice or grid cell in the <b>Command Deck</b>: pressing it plays the
            macro, pressing it again while it plays stops it.</p>
        </section>
        <section class="tile macro-edit" data-edit>
          <div class="macro-head">
            <input class="field macro-name" data-name maxlength="60" spellcheck="false" title="Rename" />
            <button class="mini macro-play" data-play></button>
            <button class="mini danger" data-delete title="Delete this macro">Delete</button>
          </div>
          <div class="macro-steps-wrap">
            <table class="macro-steps">
              <thead><tr><th>#</th><th>Press</th><th title="How long it is held">Hold</th><th title="How long to wait after letting go">Then wait</th><th></th></tr></thead>
              <tbody data-steps></tbody>
            </table>
          </div>
          <div class="macro-foot">
            <button class="mini" data-add>+ Add step</button>
            <span class="muted num" data-total></span>
          </div>
          <p class="hint">Times are in milliseconds. <b>Play</b> starts after 3 seconds, so you can click into the window
            it's for. Keys go out by position, like the Keyboard &amp; mouse tab's, except shortcuts with Ctrl, Alt or Win.</p>
          <p class="hint error" data-msg hidden></p>
        </section>
        <section class="tile macro-none" data-none hidden>
          <p class="muted">No macro yet. Click <b>+ New</b> to make one, then give each step a key, a combination, a click or a scroll.</p>
        </section>
      </div>`;
    this.q("[data-new]").addEventListener("click", () => this.create());
    this.q("[data-folder]").addEventListener("click", () => invoke("macro_reveal").catch((e) => this.flash(String(e))));
    this.q("[data-names]").addEventListener("click", (e) => {
      const b = (e.target as Element).closest<HTMLElement>("[data-pick]");
      if (b) this.open(b.dataset.pick!);
    });
    const nameEl = this.q<HTMLInputElement>("[data-name]");
    nameEl.addEventListener("change", () => this.rename(nameEl.value.trim()));
    nameEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") nameEl.blur();
      if (e.key === "Escape") {
        nameEl.value = this.name ?? "";
        nameEl.blur();
      }
    });
    this.q("[data-play]").addEventListener("click", () => this.play());
    this.q("[data-delete]").addEventListener("click", (e) => this.deleteClicked(e.currentTarget as HTMLElement));
    this.q("[data-add]").addEventListener("click", () => {
      if (!this.macro) return;
      this.macro.steps.push(structuredClone(NEW_STEP));
      this.renderSteps();
      this.save();
    });
    const steps = this.q("[data-steps]");
    steps.addEventListener("click", (e) => this.stepClicked(e.target as Element));
    steps.addEventListener("input", (e) => this.stepInput(e.target as HTMLInputElement));
    this.shown = true;
    this.load(null);
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector<T>(sel)!;
  }

  /** Read the list again and open `select` (else the one open, else the first). */
  private async load(select: string | null): Promise<void> {
    try {
      this.names = await invoke<string[]>("macro_list");
    } catch (e) {
      return this.flash(String(e));
    }
    const want = [select, this.name].find((n) => n !== null && this.names.includes(n)) ?? this.names[0] ?? null;
    this.renderNames();
    if (want !== this.name || !this.macro) await this.open(want);
  }

  private async open(name: string | null): Promise<void> {
    this.flushSave();
    closePicker();
    this.stopCountdown();
    this.name = name;
    this.macro = null;
    if (name !== null) {
      try {
        this.macro = await invoke<Macro>("macro_load", { name });
      } catch (e) {
        this.flash(String(e));
      }
    }
    this.renderNames();
    setHidden(this.q("[data-edit]"), !this.macro);
    setHidden(this.q("[data-none]"), !!this.macro || this.names.length > 0);
    if (!this.macro) return;
    this.q<HTMLInputElement>("[data-name]").value = this.name ?? "";
    this.renderSteps();
    this.renderPlay();
  }

  private renderNames(): void {
    this.q("[data-names]").innerHTML = this.names.length
      ? this.names.map((n) => `<button class="macro-pick${n === this.name ? " on" : ""}${this.playing.includes(n) ? " playing" : ""}" data-pick="${escape(n)}">
          <span>${escape(n)}</span><i title="Playing">${PLAY_ICON}</i></button>`).join("")
      : `<p class="muted">None yet.</p>`;
  }

  private renderSteps(): void {
    const steps = this.macro?.steps ?? [];
    const n = steps.length;
    this.q("[data-steps]").innerHTML = steps.map((s, i) => {
      const none = s.action.kind === "none";
      return `<tr data-row="${i}">
        <td class="muted num">${i + 1}</td>
        <td><button class="bind${none ? " none" : ""}" data-bind="${i}" title="Click to change">${escape(none ? "Pause" : actionLabel(s.action))}</button></td>
        <td><input class="field num" type="number" min="0" max="60000" step="10" data-hold="${i}" value="${s.hold}"${none ? " disabled" : ""} /></td>
        <td><input class="field num" type="number" min="0" max="600000" step="10" data-wait="${i}" value="${s.wait}" /></td>
        <td class="macro-row-btns">
          <button class="icon-btn" data-up="${i}" title="Move up"${i === 0 ? " disabled" : ""}>↑</button>
          <button class="icon-btn" data-down="${i}" title="Move down"${i === n - 1 ? " disabled" : ""}>↓</button>
          <button class="icon-btn" data-remove="${i}" title="Remove this step">×</button></td></tr>`;
    }).join("") || `<tr><td colspan="5" class="muted empty">No steps. Add one below.</td></tr>`;
    this.renderTotal();
  }

  /** How long it plays, as the backend times it. */
  private renderTotal(): void {
    const ms = (this.macro?.steps ?? []).reduce((t, s) => t + Math.max(s.hold, MIN_MS) + Math.max(s.wait, MIN_MS), 0);
    setText(this.q("[data-total]"), ms ? `Plays for about ${ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`}` : "");
  }

  private renderPlay(): void {
    const b = this.q("[data-play]");
    const on = this.name !== null && this.playing.includes(this.name);
    const text = this.countdownLeft > 0 ? `Starts in ${this.countdownLeft}… (stop)` : on ? "■ Stop" : "▶ Play";
    setText(b, text);
    setAttr(b, "class", `mini macro-play${on || this.countdownLeft > 0 ? " on" : ""}`);
    setAttr(b, "title", on ? "Stop it" : "Play it after 3 seconds");
  }

  // ---------- Editing ----------

  private stepClicked(el: Element): void {
    const m = this.macro;
    if (!m) return;
    const at = (attr: string) => {
      const b = el.closest<HTMLElement>(`[data-${attr}]`);
      return b ? Number(b.dataset[attr]) : null;
    };
    const bind = el.closest<HTMLElement>("[data-bind]");
    if (bind) {
      const step = m.steps[Number(bind.dataset.bind)];
      pickAction(bind, `Step ${Number(bind.dataset.bind) + 1}`, step.action, { pad: false, run: false, switch: false }, (a) => {
        step.action = a;
        this.renderSteps();
        this.save();
      });
      return;
    }
    const up = at("up");
    const down = at("down");
    const remove = at("remove");
    if (up !== null && up > 0) m.steps.splice(up - 1, 0, ...m.steps.splice(up, 1));
    else if (down !== null && down < m.steps.length - 1) m.steps.splice(down + 1, 0, ...m.steps.splice(down, 1));
    else if (remove !== null) m.steps.splice(remove, 1);
    else return;
    closePicker();
    this.renderSteps();
    this.save();
  }

  private stepInput(el: HTMLInputElement): void {
    const m = this.macro;
    const key = el.dataset.hold !== undefined ? "hold" : el.dataset.wait !== undefined ? "wait" : null;
    if (!m || !key) return;
    const step = m.steps[Number(el.dataset[key])];
    const v = Math.round(Number(el.value));
    const ok = el.value !== "" && Number.isFinite(v) && v >= 0;
    el.classList.toggle("bad", !ok);
    if (!ok || !step) return;
    step[key] = Math.min(v, key === "hold" ? 60000 : 600000);
    this.renderTotal();
    this.save();
  }

  /** A new macro with one empty step, named "Macro N". */
  private async create(): Promise<void> {
    let n = this.names.length + 1;
    while (this.names.some((x) => x.toLowerCase() === `macro ${n}`)) n++;
    const name = `Macro ${n}`;
    try {
      await invoke("macro_save", { name, content: { steps: [structuredClone(NEW_STEP)] } satisfies Macro });
      await this.load(name);
      const input = this.q<HTMLInputElement>("[data-name]");
      input.focus();
      input.select();
    } catch (e) {
      this.flash(String(e));
    }
  }

  private async rename(to: string): Promise<void> {
    const from = this.name;
    if (from === null || to === from) return;
    this.flushSave();
    try {
      await invoke("macro_rename", { from, to });
      this.name = to;
      await this.load(to);
    } catch (e) {
      this.flash(String(e));
      this.q<HTMLInputElement>("[data-name]").value = from;
    }
  }

  private async deleteClicked(b: HTMLElement): Promise<void> {
    if (this.name === null) return;
    if (!b.classList.contains("armed")) {
      b.classList.add("armed");
      b.textContent = "Click to delete";
      window.setTimeout(() => {
        b.classList.remove("armed");
        b.textContent = "Delete";
      }, 3000);
      return;
    }
    b.classList.remove("armed");
    b.textContent = "Delete";
    clearTimeout(this.saveTimer);
    this.saveTimer = undefined;
    try {
      await invoke("macro_delete", { name: this.name });
      this.name = null;
      this.macro = null;
      await this.load(null);
    } catch (e) {
      this.flash(String(e));
    }
  }

  /** Save the macro open; edits are batched a little. */
  private save(): void {
    clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => this.flushSave(), 300);
  }

  private flushSave(): void {
    if (this.saveTimer === undefined) return;
    clearTimeout(this.saveTimer);
    this.saveTimer = undefined;
    if (this.name !== null && this.macro) invoke("macro_save", { name: this.name, content: this.macro }).catch((e) => this.flash(String(e)));
  }

  // ---------- Playing ----------

  /** Play after a countdown, or stop: the countdown, or the macro playing. */
  private play(): void {
    const name = this.name;
    if (name === null) return;
    this.flushSave();
    if (this.countdownLeft > 0 || this.playing.includes(name)) {
      // A second toggle stops it, whether it is waiting to start or playing.
      invoke("macro_play", { name, delayMs: 0 }).catch((e) => this.flash(String(e)));
      this.stopCountdown();
      return;
    }
    invoke("macro_play", { name, delayMs: PLAY_DELAY_MS }).catch((e) => this.flash(String(e)));
    this.countdownLeft = PLAY_DELAY_MS / 1000;
    this.renderPlay();
    this.countdown = window.setInterval(() => {
      this.countdownLeft--;
      if (this.countdownLeft <= 0) this.stopCountdown();
      else this.renderPlay();
    }, 1000);
  }

  private stopCountdown(): void {
    clearInterval(this.countdown);
    this.countdown = undefined;
    this.countdownLeft = 0;
    this.renderPlay();
  }

  private flash(text: string): void {
    const el = this.q("[data-msg]");
    el.textContent = text;
    el.hidden = !text;
    window.setTimeout(() => (el.hidden = true), 6000);
  }

  update(snap: Snapshot): void {
    if (!this.shown) {
      this.shown = true;
      // Macros may have been copied into the folder: read them again.
      this.load(null);
    }
    const playing = snap.play?.macros ?? [];
    if (playing.join("\n") === this.playing.join("\n")) return;
    this.playing = playing;
    this.renderNames();
    this.renderPlay();
  }

  hide(): void {
    this.shown = false;
    this.flushSave();
    closePicker();
  }
}
