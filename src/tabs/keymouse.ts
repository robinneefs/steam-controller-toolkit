import { invoke } from "@tauri-apps/api/core";
import type { KeyMouseConfig, Layout, PlaySetup, PlaySlot, Snapshot } from "../controller";
import { setAttr, setHidden, setHtml, setText } from "../dom";
import { addHelp, helpIcon } from "../help";
import type { Tab } from "./tab";
import { ICONS } from "./icons";
import { LayoutEditor, PresetBar, notesHtml, playBarHtml, renderChips, renderNotes, renderPlayBar, wirePlayBar } from "./layout-editor";

addHelp({
  "km-slots": `Keyboard &amp; mouse has a layout for each mode: <b>Desktop</b> for using the computer, <b>Game</b> for
    games that want keys and a mouse. Pick which one to edit here; the bar above shows which one plays.<ul>
    <li><b>Programs</b>: a layout of its own for one program, which plays instead while that program is in front,
      in either mode. Start from the Desktop layout and give the grips, the pads' radial menus and the rest
      shortcuts for that program.</li></ul>`,
});

/** What the built-in layouts are for, by name (`builtin_keymouse` in src-tauri/src/layout.rs). */
const LAYOUT_NOTES: Record<string, string> = {
  Desktop: "Right pad and right stick move the pointer, the triggers click, the left stick and pad scroll. Grips undo, redo, copy and paste.",
  Presentation: "A and B, the bumpers and the D-pad flip slides; Menu starts the show. Hold RT to point with the gyro.",
  "Shooter (WASD + mouse)": "Left stick is WASD, right stick or right pad aims, RT fires, LT aims down sights.",
  "Shooter + gyro aim": "Like Shooter, plus aiming by moving the controller while your thumb rests on the right stick.",
  "Strategy / point & click": "Right pad is the mouse, the left stick pans with the arrow keys, the right stick zooms.",
};

const MAX_SPEED = 2500;

const escape = (s: string) => s.replace(/[&<>"]/g, (c) => `&${{ "&": "amp", "<": "lt", ">": "gt", '"': "quot" }[c]};`);

/** A layout of the tab: a mode's, or a program's. */
type KmSlot = Exclude<PlaySlot, "pad">;
const appIndex = (slot: KmSlot) => (slot.startsWith("app:") ? Number(slot.slice(4)) : -1);

/** The controller as keyboard and mouse, for the desktop and for games, with nothing to install. */
export class KeyMouseTab implements Tab {
  id = "keymouse";
  title = "Keyboard & mouse";
  navTitle = "Keys & mouse";
  group = "output" as const;
  icon = ICONS.keymouse;
  private root!: HTMLElement;
  private cfg: KeyMouseConfig | null = null;
  private slot: KmSlot = "desktop";
  private recent: string[] = [];
  private lastRunError: string | null = null;
  private editor!: LayoutEditor;
  private presets!: PresetBar;
  private pushTimer: number | undefined;
  private shown = false;
  private lastDot = "";

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="play">
        ${playBarHtml("keymouse")}
        <section class="tile play-edit">
          <div class="play-presetbar">
            <span class="bar-group"><span class="segmented" data-slots>
              <button data-slot="desktop">Desktop layout</button><button data-slot="game">Game layout</button></span>
              <select class="field app-pick" data-apps title="A layout of its own for one program"></select>${helpIcon("km-slots")}</span>
            ${PresetBar.html("Layout")}
          </div>
          <div class="app-row" data-app-row hidden>
            <span class="muted">Plays while</span>
            <input class="field" data-app-name list="km-recent-apps" placeholder="Program file name, e.g. Photoshop.exe" maxlength="120" spellcheck="false" />
            <datalist id="km-recent-apps" data-recent></datalist>
            <span class="muted">is in front</span>
            <button class="mini danger" data-app-remove title="Remove this program's layout">Remove</button>
          </div>
          <p class="hint layout-note" data-note></p>
          <div data-editor></div>
        </section>
        <aside class="play-side">
          <section class="tile play-live">
            <h3><span class="h3-title">Live ${helpIcon("play-bind")}</span><em data-live-slot></em></h3>
            <div class="key-chips" data-keys></div>
            <div class="live-mouse">
              <div class="mouse-field" title="Pointer speed and direction"><i data-mdot></i></div>
              <dl class="facts">
                <dt>Pointer</dt><dd class="num" data-speed>0 px/s</dd>
                <dt>Scroll</dt><dd class="num" data-scroll>–</dd>
                <dt>Gyro</dt><dd data-gyro>off</dd>
              </dl>
            </div>
          </section>
          ${notesHtml()}
          <p class="hint error" data-msg hidden></p>
        </aside>
      </div>`;
    wirePlayBar(root, (on) => {
      if (!this.cfg) return;
      this.cfg.enabled = on;
      this.push(true);
    });
    this.root.querySelector("[data-slots]")!.addEventListener("click", (e) => {
      const b = (e.target as Element).closest<HTMLElement>("[data-slot]");
      if (b) this.select(b.dataset.slot as KmSlot);
    });
    this.q<HTMLSelectElement>("[data-apps]").addEventListener("change", (e) => {
      const v = (e.target as HTMLSelectElement).value;
      if (v === "add") this.addApp();
      else if (v) this.select(v as KmSlot);
      else this.renderApps();
    });
    this.q<HTMLInputElement>("[data-app-name]").addEventListener("input", (e) => {
      const app = this.cfg?.apps[appIndex(this.slot)];
      if (!app) return;
      app.app = (e.target as HTMLInputElement).value.trim();
      this.renderApps();
      this.push(false);
    });
    this.q("[data-app-remove]").addEventListener("click", (e) => this.removeClicked(e.currentTarget as HTMLElement));
    this.editor = new LayoutEditor(this.q("[data-editor]"), {
      pad: false,
      onChange: () => {
        this.presets.edited();
        this.push(false);
      },
    });
    this.presets = new PresetBar(this.q(".play-presetbar"), "keymouse", {
      layout: () => this.layout(),
      load: (l) => {
        if (!this.cfg) return;
        this.setLayout(l);
        this.editor.set(l);
        this.renderNote();
        this.push(true);
      },
      flash: (t) => this.flash(t),
    });
    this.init();
  }

  private async init(): Promise<void> {
    try {
      const setup = await invoke<PlaySetup>("play_setup");
      this.cfg = setup.keymouse;
      // Start on the layout in play.
      if (setup.mode === "game") this.slot = "game";
      await this.initPresets();
      this.select(this.slot);
    } catch (e) {
      this.flash(String(e));
    }
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector<T>(sel)!;
  }

  private initPresets(): Promise<void> {
    const cfg = this.cfg!;
    const names: Record<string, string> = { desktop: cfg.desktop.name, game: cfg.game.name };
    cfg.apps.forEach((a, i) => (names[`app:${i}`] = a.layout.name));
    return this.presets.init(names);
  }

  private layout(): Layout | null {
    const cfg = this.cfg;
    if (!cfg) return null;
    const i = appIndex(this.slot);
    return i >= 0 ? (cfg.apps[i]?.layout ?? null) : cfg[this.slot as "desktop" | "game"];
  }

  private setLayout(l: Layout): void {
    const cfg = this.cfg!;
    const i = appIndex(this.slot);
    if (i < 0) cfg[this.slot as "desktop" | "game"] = l;
    else if (cfg.apps[i]) cfg.apps[i].layout = l;
  }

  /** The programs list, and the row of the program being edited. */
  private renderApps(): void {
    const apps = this.cfg?.apps ?? [];
    const sel = this.q<HTMLSelectElement>("[data-apps]");
    sel.innerHTML = `<option value="">${apps.length ? "Programs…" : "Per program…"}</option>` +
      apps.map((a, i) => `<option value="app:${i}">${escape(a.app || "(no program yet)")}</option>`).join("") +
      `<option value="add">+ Add a program</option>`;
    const i = appIndex(this.slot);
    sel.value = i >= 0 ? this.slot : "";
    sel.classList.toggle("on", i >= 0);
    const row = this.q("[data-app-row]");
    row.hidden = i < 0;
    const name = this.q<HTMLInputElement>("[data-app-name]");
    if (i >= 0 && document.activeElement !== name) name.value = apps[i]?.app ?? "";
  }

  /** A new program layout, from the Desktop layout, for the last program used that has none. */
  private async addApp(): Promise<void> {
    const cfg = this.cfg;
    if (!cfg) return;
    const used = (a: string) => cfg.apps.some((x) => x.app.toLowerCase() === a.toLowerCase());
    const app = this.recent.find((a) => !used(a)) ?? "";
    cfg.apps.push({ app, layout: structuredClone(cfg.desktop) });
    await this.initPresets();
    this.select(`app:${cfg.apps.length - 1}`);
    this.push(true);
    if (!app) this.q<HTMLInputElement>("[data-app-name]").focus();
  }

  private async removeClicked(b: HTMLElement): Promise<void> {
    const cfg = this.cfg;
    const i = appIndex(this.slot);
    if (!cfg || i < 0) return;
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
    cfg.apps.splice(i, 1);
    this.push(true);
    await this.initPresets();
    this.select("desktop");
  }

  private select(slot: KmSlot): void {
    this.slot = slot;
    this.root.querySelectorAll<HTMLElement>("[data-slot]").forEach((b) => b.classList.toggle("on", b.dataset.slot === slot));
    this.renderApps();
    this.presets.select(slot);
    const l = this.layout();
    if (l) this.editor.set(l);
    this.renderNote();
    if (this.shown) invoke("play_set_preview", { slot }).catch(() => {});
  }

  private renderNote(): void {
    const name = this.layout()?.name ?? "";
    setText(this.q("[data-note]"), LAYOUT_NOTES[name] ?? "");
  }

  /** Send the config to the backend; edits are batched a little. */
  private push(now: boolean): void {
    clearTimeout(this.pushTimer);
    const send = () => {
      if (this.cfg) invoke("play_set_keymouse", { config: this.cfg }).catch((e) => this.flash(String(e)));
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
      invoke("play_set_preview", { slot: this.slot }).catch(() => {});
    }
    const p = snap.play;
    if (!p) return;
    renderPlayBar(this.root, snap, "keymouse");
    renderNotes(this.root, snap);
    const v = p.preview;
    const live = v.slot === this.slot && !!snap.state;
    const app = this.cfg?.apps[appIndex(this.slot)]?.app;
    setText(this.q("[data-live-slot]"), this.slot === "desktop" ? "Desktop layout" : this.slot === "game" ? "Game layout" : `${app || "Program"} layout`);
    this.editor.showLit(live ? v.lit : 0, live && v.gyro_active);
    this.editor.showRadial(live ? v.radial : [null, null]);
    // Per-program layouts need to know the program in front (Windows for now).
    setHidden(this.q("[data-apps]"), !p.keymouse.apps_supported && !this.cfg?.apps.length);
    if (p.recent_apps.join("\n") !== this.recent.join("\n")) {
      this.recent = p.recent_apps;
      setHtml(this.q("[data-recent]"), this.recent.map((a) => `<option value="${escape(a)}"></option>`).join(""));
    }
    if (p.keymouse.run_error !== this.lastRunError) {
      this.lastRunError = p.keymouse.run_error;
      if (this.lastRunError) this.flash(this.lastRunError);
    }
    renderChips(this.q("[data-keys]"), live ? v.keys : [], live ? v.mouse_buttons : 0);
    const [mx, my] = live ? v.mouse : [0, 0];
    const speed = Math.hypot(mx, my);
    // In whole pixels, so jitter at rest writes nothing.
    const at = (v: number) => Math.round(Math.max(-1, Math.min(1, v / MAX_SPEED)) * 40) || 0;
    const dot = `transform: translate(${at(mx)}px, ${at(my)}px)`;
    if (dot !== this.lastDot) {
      setAttr(this.q("[data-mdot]"), "style", dot);
      this.lastDot = dot;
    }
    setText(this.q("[data-speed]"), speed < 5 ? "0 px/s" : `${Math.round(speed / 10) * 10} px/s`);
    const [sx, sy] = live ? v.scroll : [0, 0];
    setText(this.q("[data-scroll]"), Math.abs(sy) < 0.2 && Math.abs(sx) < 0.2 ? "–" : `${sy >= 0.2 ? "↑" : sy <= -0.2 ? "↓" : ""}${sx >= 0.2 ? "→" : sx <= -0.2 ? "←" : ""} ${Math.hypot(sx, sy).toFixed(1)} /s`);
    const gyro = this.layout()?.gyro.output ?? "none";
    setText(this.q("[data-gyro]"), gyro === "none" ? "off" : live && v.gyro_active ? "aiming" : "waiting");
    setHidden(this.q("[data-mdot]"), !live);
  }

  hide(): void {
    this.shown = false;
  }
}
