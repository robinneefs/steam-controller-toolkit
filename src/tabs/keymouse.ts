import { invoke } from "@tauri-apps/api/core";
import type { KeyMouseConfig, Layout, PlaySetup, Snapshot } from "../controller";
import { setAttr, setHidden, setText } from "../dom";
import { addHelp, helpIcon } from "../help";
import type { Tab } from "./tab";
import { ICONS } from "./icons";
import { LayoutEditor, PresetBar, notesHtml, playBarHtml, renderChips, renderNotes, renderPlayBar, wirePlayBar } from "./layout-editor";

addHelp({
  "km-slots": `Keyboard &amp; mouse has a layout for each mode: <b>Desktop</b> for using the computer, <b>Game</b> for
    games that want keys and a mouse. Pick which one to edit here; the bar above shows which one plays.`,
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

/** The controller as keyboard and mouse, for the desktop and for games, with nothing to install. */
export class KeyMouseTab implements Tab {
  id = "keymouse";
  title = "Keyboard & mouse";
  navTitle = "Keys & mouse";
  group = "output" as const;
  icon = ICONS.keymouse;
  private root!: HTMLElement;
  private cfg: KeyMouseConfig | null = null;
  private slot: "desktop" | "game" = "desktop";
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
              <button data-slot="desktop">Desktop layout</button><button data-slot="game">Game layout</button></span>${helpIcon("km-slots")}</span>
            ${PresetBar.html("Layout")}
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
      if (b) this.select(b.dataset.slot as "desktop" | "game");
    });
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
        this.cfg[this.slot] = l;
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
      await this.presets.init({ desktop: this.cfg.desktop.name, game: this.cfg.game.name });
      this.select(this.slot);
    } catch (e) {
      this.flash(String(e));
    }
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector<T>(sel)!;
  }

  private layout(): Layout | null {
    return this.cfg?.[this.slot] ?? null;
  }

  private select(slot: "desktop" | "game"): void {
    this.slot = slot;
    this.root.querySelectorAll<HTMLElement>("[data-slot]").forEach((b) => b.classList.toggle("on", b.dataset.slot === slot));
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
    setText(this.q("[data-live-slot]"), this.slot === "desktop" ? "Desktop layout" : "Game layout");
    this.editor.showLit(live ? v.lit : 0, live && v.gyro_active);
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
