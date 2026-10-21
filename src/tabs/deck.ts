import { invoke } from "@tauri-apps/api/core";
import { DECK_PARTS, type Deck, type DeckConfig, type DeckItem, type DeckMenu, type DeckPart, type DeckSetup, type Pick, type Snapshot } from "../controller";
import { markSelected, menuHtml } from "../deck-menus";
import { setAttr, setHidden, setHtml, setText } from "../dom";
import { addHelp, helpIcon } from "../help";
import { NONE, actionLabel, escape } from "./actions";
import { ICONS } from "./icons";
import { CONTROLS, closePicker, notesHtml, pickAction, renderNotes } from "./layout-editor";
import { powerHtml } from "./power";
import type { Tab } from "./tab";

addHelp({
  "deck-about": `Shortcuts on the controller, like a stream deck in your hands. Give buttons a shortcut, program or web page,
    put a <b>wheel</b> on a stick and a <b>grid</b> on a trackpad.<ul>
    <li>What you give the deck is taken from the Keyboard &amp; mouse or Gamepad layout; everything else keeps working
      as it does there.</li>
    <li>Click a slice, cell or button to set what it does and what it's called.</li></ul>`,
  "deck-pick": `<ul><li><b>Wheel</b>: tilt the stick to an item (you feel a tick on each), then let it spring back to use it;
    or click the stick, if set to. Back to the centre without an item: nothing.</li>
    <li><b>Grid</b>: touch the pad, slide to a cell, click to use it (held while clicked); or lift to use it, if set to.</li></ul>`,
  "deck-overlay": `While you touch a stick or trackpad with a menu, it shows in the middle of the screen with the item under
    your thumb lit. It never takes the keyboard from the app you're in. It keeps a small window ready while the deck
    is on (some memory, no CPU while nothing changes).`,
  "deck-apps": `<b>All programs</b> is the page that plays unless the program in front has a page of its own. Add one to give,
    say, Photoshop its own tools: it starts as a copy of All programs.`,
});

const PART_TITLES: Record<DeckPart, string> = { left_stick: "Left stick", right_stick: "Right stick", left_pad: "Left trackpad", right_pad: "Right trackpad" };
const WHEEL_SIZES = [2, 3, 4, 5, 6, 8];
const GRID_SIZES: [number, number][] = [[2, 2], [3, 2], [3, 3], [4, 3], [4, 4]];
/** Lit bits of the menus after the controls, as `LIT_LEFT_STICK..` in layout.rs. */
const LIT_PART = (i: number) => CONTROLS.length + i;

const BUTTON_GROUPS: [string, string[]][] = [
  ["Face buttons", ["A", "B", "X", "Y"]],
  ["D-pad", ["Up", "Down", "Left", "Right"]],
  ["Shoulders", ["L", "R", "LT", "RT"]],
  ["Back grips", ["L4", "L5", "R4", "R5"]],
  ["Menu buttons", ["View", "Menu", "QAM", "Steam"]],
  ["Clicks", ["L3", "R3", "LPadClick", "RPadClick", "LTrigClick", "RTrigClick"]],
];
const BUTTON_LABELS: Record<string, string> = {
  Up: "D-pad up", Down: "D-pad down", Left: "D-pad left", Right: "D-pad right", L: "Left bumper", R: "Right bumper",
  LT: "Left trigger", RT: "Right trigger", L3: "Left stick click", R3: "Right stick click", LPadClick: "Left pad click",
  RPadClick: "Right pad click", LTrigClick: "LT full pull", RTrigClick: "RT full pull", QAM: "Quick access",
};

const pickText = (m: Extract<DeckMenu, { kind: "wheel" | "grid" }>, stick: boolean): string =>
  m.pick === "click" ? "Click" : stick || m.kind === "wheel" ? "Let go" : "Lift";

/** Shortcuts, programs and web pages on buttons, stick wheels and trackpad grids. */
export class DeckTab implements Tab {
  id = "deck";
  title = "Command Deck";
  group = "output" as const;
  icon = ICONS.deck;
  private root!: HTMLElement;
  private cfg: DeckConfig | null = null;
  private defaults: { wheel: DeckItem[]; grid: DeckItem[] } = { wheel: [], grid: [] };
  /** The page edited: null All programs, else `apps[i]`. */
  private page: number | null = null;
  private recent: string[] = [];
  private lastRunError: string | null = null;
  private lastLive = "";
  private shown = false;
  private pushTimer: number | undefined;

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="deck">
        <section class="tile play-bar">
          ${powerHtml("Command Deck", "Turn the Command Deck on or off")}
          <span class="midi-conn play-conn"><i data-dot></i><b data-status></b></span>
          <label class="chk muted" title="Show a menu on screen while you touch its stick or trackpad"><input type="checkbox" data-overlay />Show menus on screen</label>
          ${helpIcon("deck-overlay")}
        </section>
        <section class="tile deck-edit">
          <div class="play-presetbar">
            <span class="bar-group"><span class="muted">Page</span><select class="field deck-pages" data-pages></select>${helpIcon("deck-apps")}</span>
            <span class="hint deck-hint">Click a slice, cell or button to change it. ${helpIcon("deck-about")}</span>
          </div>
          <div class="app-row" data-app-row hidden>
            <span class="muted">Plays while</span>
            <input class="field" data-app-name list="deck-recent-apps" placeholder="Program file name, e.g. Photoshop.exe" maxlength="120" spellcheck="false" />
            <datalist id="deck-recent-apps" data-recent></datalist>
            <span class="muted">is in front</span>
            <button class="mini danger" data-app-remove title="Remove this program's page">Remove</button>
          </div>
          <div class="deck-cards" data-cards></div>
          ${notesHtml()}
          <p class="hint error" data-msg hidden></p>
        </section>
      </div>`;
    this.q<HTMLInputElement>("[data-on]").addEventListener("change", (e) => {
      if (!this.cfg) return;
      this.cfg.enabled = (e.target as HTMLInputElement).checked;
      this.push(true);
    });
    this.q<HTMLInputElement>("[data-overlay]").addEventListener("change", (e) => {
      if (!this.cfg) return;
      this.cfg.overlay = (e.target as HTMLInputElement).checked;
      this.push(true);
    });
    this.q<HTMLSelectElement>("[data-pages]").addEventListener("change", (e) => {
      const v = (e.target as HTMLSelectElement).value;
      if (v === "add") this.addApp();
      else this.select(v === "all" ? null : Number(v));
    });
    this.q<HTMLInputElement>("[data-app-name]").addEventListener("input", (e) => {
      const app = this.page === null ? null : this.cfg?.apps[this.page];
      if (!app) return;
      app.app = (e.target as HTMLInputElement).value.trim();
      this.renderPages();
      this.push(false);
    });
    this.q("[data-app-remove]").addEventListener("click", (e) => this.removeClicked(e.currentTarget as HTMLElement));
    const cards = this.q("[data-cards]");
    cards.addEventListener("click", (e) => this.clicked(e.target as Element));
    cards.addEventListener("change", (e) => this.changed(e.target as HTMLSelectElement));
    this.init();
  }

  private async init(): Promise<void> {
    try {
      const setup = await invoke<DeckSetup>("deck_setup");
      this.cfg = setup.config;
      this.defaults = { wheel: setup.wheel, grid: setup.grid };
      this.select(null);
    } catch (e) {
      this.flash(String(e));
    }
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector<T>(sel)!;
  }

  private deck(): Deck | null {
    if (!this.cfg) return null;
    return this.page === null ? this.cfg.deck : (this.cfg.apps[this.page]?.deck ?? null);
  }

  private select(page: number | null): void {
    this.page = page;
    closePicker();
    this.renderPages();
    this.renderCards();
    if (this.shown) invoke("deck_set_preview", { page, open: true }).catch(() => {});
  }

  // ---------- Pages ----------

  private renderPages(): void {
    const apps = this.cfg?.apps ?? [];
    const sel = this.q<HTMLSelectElement>("[data-pages]");
    sel.innerHTML = `<option value="all">All programs</option>` +
      (apps.length ? `<optgroup label="Programs">${apps.map((a, i) => `<option value="${i}">${escape(a.app || "(no program yet)")}</option>`).join("")}</optgroup>` : "") +
      `<option value="add">+ Add a program</option>`;
    sel.value = this.page === null ? "all" : String(this.page);
    this.q("[data-app-row]").hidden = this.page === null;
    const name = this.q<HTMLInputElement>("[data-app-name]");
    if (this.page !== null && document.activeElement !== name) name.value = apps[this.page]?.app ?? "";
  }

  /** A page for the last program used that has none, starting from All programs. */
  private addApp(): void {
    const cfg = this.cfg;
    if (!cfg) return;
    const used = (a: string) => cfg.apps.some((x) => x.app.toLowerCase() === a.toLowerCase());
    const app = this.recent.find((a) => !used(a)) ?? "";
    cfg.apps.push({ app, deck: structuredClone(cfg.deck) });
    this.select(cfg.apps.length - 1);
    this.push(true);
    if (!app) this.q<HTMLInputElement>("[data-app-name]").focus();
  }

  private removeClicked(b: HTMLElement): void {
    const cfg = this.cfg;
    if (!cfg || this.page === null) return;
    if (!b.classList.contains("armed")) {
      b.classList.add("armed");
      b.textContent = "Click to remove";
      window.setTimeout(() => {
        b.classList.remove("armed");
        b.textContent = "Remove";
      }, 3000);
      return;
    }
    b.classList.remove("armed");
    b.textContent = "Remove";
    cfg.apps.splice(this.page, 1);
    this.push(true);
    this.select(null);
  }

  // ---------- Cards ----------

  private renderCards(): void {
    const d = this.deck();
    if (!d) return;
    this.lastLive = "";
    const menus = DECK_PARTS.map((p, i) => this.menuCard(p, d[p], i)).join("");
    const groups = BUTTON_GROUPS.map(([title, controls]) => `<div class="deck-btn-group"><h5>${title}</h5>${controls.map((c) => {
      const a = d.buttons[c];
      const none = !a || a.kind === "none";
      return `<div class="bind-row" data-lit="${CONTROLS.indexOf(c)}"><span class="bind-ctl">${escape(BUTTON_LABELS[c] ?? c)}</span>
        <button class="bind${none ? " none" : ""}" data-bind="${c}" title="Click to change">${escape(none ? "–" : actionLabel(a))}</button></div>`;
    }).join("")}</div>`).join("");
    this.q("[data-cards]").innerHTML = menus + `<section class="card deck-buttons"><h4>Buttons</h4><div class="deck-btn-groups">${groups}</div></section>`;
  }

  private menuCard(part: DeckPart, m: DeckMenu, i: number): string {
    const stick = part.endsWith("stick");
    const kinds: [string, string][] = stick ? [["none", "Nothing"], ["wheel", "Wheel"]] : [["none", "Nothing"], ["grid", "Grid"], ["wheel", "Wheel"]];
    const kind = `<select class="field" data-kind="${part}" title="What the ${PART_TITLES[part].toLowerCase()} does on the deck">${kinds.map(([k, t]) => `<option value="${k}"${k === m.kind ? " selected" : ""}>${t}</option>`).join("")}</select>`;
    if (m.kind === "none") {
      return `<section class="card deck-menu" data-lit="${LIT_PART(i)}" data-part="${part}"><h4>${PART_TITLES[part]}${kind}</h4>
        <p class="hint">Left to the Keyboard &amp; mouse or Gamepad layout. Pick a ${stick ? "wheel" : "grid or wheel"} to give it shortcuts.</p></section>`;
    }
    const size = m.kind === "wheel"
      ? `<select class="field" data-size="${part}" title="How many items">${WHEEL_SIZES.map((n) => `<option value="${n}"${n === m.items.length ? " selected" : ""}>${n} items</option>`).join("")}</select>`
      : `<select class="field" data-size="${part}" title="Columns x rows">${GRID_SIZES.map(([c, r]) => `<option value="${c}x${r}"${c === m.cols && r === m.rows ? " selected" : ""}>${c} × ${r}</option>`).join("")}</select>`;
    const release = stick ? "Let go to use" : "Lift to use";
    const pick = `<select class="field" data-pick="${part}" title="When an item is used">
      <option value="release"${m.pick === "release" ? " selected" : ""}>${release}</option>
      <option value="click"${m.pick === "click" ? " selected" : ""}>Click to use</option></select>`;
    return `<section class="card deck-menu" data-lit="${LIT_PART(i)}" data-part="${part}"><h4>${PART_TITLES[part]}${kind}</h4>
      <div class="deck-opts">${size}${pick}${helpIcon("deck-pick")}</div>
      <div class="deck-visual ${m.kind}">${menuHtml(m, part, "button", pickText(m, stick))}</div></section>`;
  }

  private menuOf(part: DeckPart): Extract<DeckMenu, { kind: "wheel" | "grid" }> | null {
    const m = this.deck()?.[part];
    return m && m.kind !== "none" ? m : null;
  }

  private clicked(el: Element): void {
    const d = this.deck();
    if (!d) return;
    const itemEl = el.closest<HTMLElement>("[data-item]");
    if (itemEl) {
      const [part, i] = itemEl.dataset.item!.split(":") as [DeckPart, string];
      const m = this.menuOf(part);
      const item = m?.items[Number(i)];
      if (!m || !item) return;
      const key = `${part}:${i}`;
      pickAction(itemEl, `${PART_TITLES[part]}: item ${Number(i) + 1}`, item.action, {
        pad: false,
        run: true,
        label: item.label,
        onLabel: (label) => {
          item.label = label;
          this.relabel(key, item);
          this.push(false);
        },
      }, (a) => {
        item.action = a;
        this.renderCards();
        this.push(true);
      });
      return;
    }
    const bind = el.closest<HTMLElement>("[data-bind]");
    if (bind) {
      const c = bind.dataset.bind!;
      pickAction(bind, BUTTON_LABELS[c] ?? c, d.buttons[c] ?? NONE, { pad: false, run: true }, (a) => {
        if (a.kind === "none") delete d.buttons[c];
        else d.buttons[c] = a;
        this.renderCards();
        this.push(true);
      });
    }
  }

  /** A new name shows at once, without redrawing the menu (the picker points at it). */
  private relabel(key: string, item: DeckItem): void {
    const el = this.root.querySelector(`[data-label-of="${key}"]`);
    const m = this.menuOf(key.split(":")[0] as DeckPart);
    if (!el || !m) return;
    const text = item.label || (item.action.kind === "none" ? "–" : actionLabel(item.action));
    const max = m.kind === "grid" ? 18 : m.items.length <= 4 ? 14 : m.items.length <= 6 ? 11 : 9;
    el.textContent = text.length > max ? `${text.slice(0, max - 1)}…` : text;
  }

  private changed(el: HTMLSelectElement): void {
    const d = this.deck();
    if (!d) return;
    const fill = (items: DeckItem[], n: number, defaults: DeckItem[]): DeckItem[] =>
      Array.from({ length: n }, (_, i) => items[i] ?? structuredClone(defaults[i]) ?? { label: "", action: NONE });
    if (el.dataset.kind) {
      const part = el.dataset.kind as DeckPart;
      const kind = el.value;
      d[part] = kind === "wheel"
        ? { kind, items: fill([], 8, this.defaults.wheel), pick: "release" }
        : kind === "grid"
          ? { kind, cols: 3, rows: 3, items: fill([], 9, this.defaults.grid), pick: "click" }
          : { kind: "none" };
    } else if (el.dataset.size) {
      const m = this.menuOf(el.dataset.size as DeckPart);
      if (!m) return;
      if (m.kind === "wheel") m.items = fill(m.items, Number(el.value), []);
      else {
        const [cols, rows] = el.value.split("x").map(Number);
        Object.assign(m, { cols, rows, items: fill(m.items, cols * rows, []) });
      }
    } else if (el.dataset.pick) {
      const m = this.menuOf(el.dataset.pick as DeckPart);
      if (!m) return;
      m.pick = el.value as Pick;
    } else return;
    this.renderCards();
    this.push(true);
  }

  // ---------- Backend ----------

  /** Send the config to the backend; edits are batched a little. */
  private push(now: boolean): void {
    clearTimeout(this.pushTimer);
    const send = () => {
      if (this.cfg) invoke("deck_set", { config: this.cfg }).catch((e) => this.flash(String(e)));
    };
    if (now) send();
    else this.pushTimer = window.setTimeout(send, 150);
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
      invoke("deck_set_preview", { page: this.page, open: true }).catch(() => {});
    }
    const d = snap.play?.deck;
    if (!d) return;
    renderNotes(this.root, snap);
    const on = this.q<HTMLInputElement>("[data-on]");
    if (on.checked !== d.enabled) on.checked = d.enabled;
    const overlay = this.q<HTMLInputElement>("[data-overlay]");
    if (overlay.checked !== d.overlay) overlay.checked = d.overlay;
    const [cls, text] = !d.supported
      ? ["warn", "Not available on this system yet"]
      : !d.enabled
        ? ["", "Off"]
        : ["ok", `On · ${d.app ? `${d.app} page` : "All programs page"}`];
    setAttr(this.q("[data-dot]"), "class", cls);
    setText(this.q("[data-status]"), text);
    // Pages per program need to know the program in front (Windows for now).
    const add = this.q("[data-pages]").querySelector<HTMLElement>('option[value="add"]');
    if (add) setHidden(add, !d.apps_supported);
    if (d.recent_apps.join("\n") !== this.recent.join("\n")) {
      this.recent = d.recent_apps;
      setHtml(this.q("[data-recent]"), this.recent.map((a) => `<option value="${escape(a)}"></option>`).join(""));
    }
    if (d.run_error !== this.lastRunError) {
      this.lastRunError = d.run_error;
      if (d.run_error) this.flash(d.run_error);
    }
    // The page shown, as the controller uses it: lit cards and rows, the item under the thumb.
    const v = d.preview;
    const live = v.shown && v.page === this.page && !!snap.state;
    const key = live ? `${v.lit}|${v.menus.map((m) => `${m.open}${m.selected}`).join(",")}` : "off";
    if (key === this.lastLive) return;
    this.lastLive = key;
    const lit = live ? v.lit : 0;
    this.root.querySelectorAll<HTMLElement>("[data-cards] [data-lit]").forEach((el) => {
      el.classList.toggle("lit", Math.floor(lit / 2 ** Number(el.dataset.lit)) % 2 === 1);
    });
    DECK_PARTS.forEach((p, i) => {
      const card = this.root.querySelector(`[data-part="${p}"] .deck-visual`);
      if (card) markSelected(card, live ? v.menus[i].selected : null);
    });
  }

  hide(): void {
    this.shown = false;
    closePicker();
    invoke("deck_set_preview", { page: this.page, open: false }).catch(() => {});
  }
}

