import { invoke } from "@tauri-apps/api/core";
import type { GamepadConfig, Layout, PadState, PlaySetup, Snapshot } from "../controller";
import { setAttr, setHidden, setText } from "../dom";
import { addHelp, helpIcon } from "../help";
import type { Tab } from "./tab";
import { ICONS } from "./icons";
import { LayoutEditor, PresetBar, notesHtml, playBarHtml, renderChips, renderNotes, renderPlayBar, wirePlayBar } from "./layout-editor";

addHelp({
  "pad-live": `What games see: a wired <b>Xbox 360 Controller</b>. Keys and mouse buttons the layout sends as well
    (grips to keys, gyro mouse) show below it, with the player number Windows gave the pad and the rumble the game asks for.`,
});

/** What the built-in layouts are for, by name (`builtin_gamepad` in src-tauri/src/layout.rs). */
const LAYOUT_NOTES: Record<string, string> = {
  Standard: "Every button where an Xbox controller has it; the Steam button switches Desktop ⇄ Game. Grips and trackpads do nothing.",
  "Back grips as face buttons": "Standard, with the grips as A, B, X and Y so your thumbs stay on the sticks.",
  "Gyro aim (stick)": "Standard, plus aiming with the right stick by moving the controller while your thumb rests on the right stick.",
  "Gyro aim (mouse)": "Like gyro aim, but it moves the mouse: more precise, for games that take mouse and controller at once.",
  "Racing (tilt to steer)": "Lean the controller like a wheel to steer; the triggers are the pedals, the grips shift.",
  "Trackpads as D-pad and mouse": "Click the left pad for the D-pad; the right pad is a mouse with trackball glide.",
};

const OS = navigator.userAgent.includes("Windows") ? "windows" : navigator.userAgent.includes("Mac") ? "mac" : "linux";

const UDEV_RULE = `KERNEL=="uinput", SUBSYSTEM=="misc", OPTIONS+="static_node=uinput", TAG+="uaccess"`;
const UDEV_INSTALL = `echo '${UDEV_RULE}' | sudo tee /etc/udev/rules.d/60-steam-controller-toolkit-uinput.rules && sudo udevadm control --reload && sudo udevadm trigger`;

const WINDOWS_DRIVER = `
  <h3>The gamepad needs a driver</h3>
  <div class="driver-cols">
    <section><h4>Why</h4>
      <p>Windows has no built-in way for an app to add a game controller. We tried Windows' own input injection: it
      accepts a gamepad, but games never see it. Every tool that makes a virtual controller on Windows uses a driver.</p>
      <p class="muted">Keyboard &amp; mouse works without one.</p></section>
    <section><h4>What it is</h4>
      <p><b>ViGEmBus 1.22</b> by Nefarius Software Solutions: open source (BSD-3), signed by its author, and the driver
      DS4Windows, Parsec and many others use. Its installer comes with this app; nothing is downloaded.</p></section>
    <section><h4>What comes with it</h4><ul>
      <li>Windows asks for administrator permission, once.</li>
      <li>It's installed for the whole PC, until you remove it in Settings → Apps → <b>ViGEm Bus Driver</b>.</li>
      <li>Games see an <b>Xbox 360 Controller</b>, and their rumble is passed on to your controller.</li>
      <li>The project was retired in 2023: it works, but gets no more fixes.</li>
      <li>Some games' anti-cheat blocks virtual controllers.</li></ul></section>
  </div>
  <div class="driver-actions">
    <button class="gbtn primary" data-install>Install driver</button>
    <button class="gbtn" data-use-km>Use Keyboard &amp; mouse instead</button>
    <span class="muted" data-install-status></span>
  </div>`;

const LINUX_DRIVER = `
  <h3>The gamepad needs access to /dev/uinput</h3>
  <div class="driver-cols">
    <section><h4>Why</h4>
      <p>The virtual controller (and keyboard &amp; mouse) are made with Linux' <b>uinput</b>, which apps may only use
      with permission. The .deb and .rpm packages install a rule that gives it to the signed-in user, as Steam's own
      rules do.</p></section>
    <section class="wide"><h4>Install the rule</h4>
      <p>For the AppImage, or if it's still missing, run this once in a terminal:</p>
      <pre class="udev" data-copy="${UDEV_INSTALL.replace(/"/g, "&quot;")}" title="Click to copy">${UDEV_INSTALL.replace(/&/g, "&amp;").replace(/</g, "&lt;")}</pre>
      <p class="muted">Then check again. If it still says no permission, run <code>sudo modprobe uinput</code>, or sign out and in.</p></section>
  </div>
  <div class="driver-actions">
    <button class="gbtn primary" data-check>Check again</button>
    <button class="gbtn" data-use-km>Use Keyboard &amp; mouse instead</button>
    <span class="muted" data-install-status></span>
  </div>`;

const UNSUPPORTED = `<h3>Not available on this system</h3>
  <p>The gamepad output works on Windows and Linux. On macOS, apps can't add a game controller without a system extension.</p>`;

/** An Xbox 360 controller, drawn from its state. */
const PAD_SVG = `<svg class="xpad" viewBox="0 0 320 210" aria-hidden="true">
  <rect class="trig-track" x="42" y="6" width="70" height="9" rx="4"/><rect class="trig" data-lt x="42" y="6" width="0" height="9" rx="4"/>
  <rect class="trig-track" x="208" y="6" width="70" height="9" rx="4"/><rect class="trig" data-rt x="208" y="6" width="0" height="9" rx="4"/>
  <rect class="xb" data-b="256" x="44" y="22" width="66" height="13" rx="6"/><rect class="xb" data-b="512" x="210" y="22" width="66" height="13" rx="6"/>
  <path class="xbody" d="M78 40h164c36 0 64 46 68 104 3 44-28 66-52 40l-30-32H92l-30 32C38 210 7 188 10 144 14 86 42 40 78 40z"/>
  <circle class="well" cx="92" cy="92" r="24"/><circle class="xstick" data-ls data-b="64" cx="92" cy="92" r="14"/>
  <circle class="well" cx="206" cy="140" r="24"/><circle class="xstick" data-rs data-b="128" cx="206" cy="140" r="14"/>
  <g class="dpad"><rect class="xb" data-b="1" x="112" y="114" width="16" height="17" rx="3"/><rect class="xb" data-b="2" x="112" y="149" width="16" height="17" rx="3"/>
    <rect class="xb" data-b="4" x="94" y="132" width="17" height="16" rx="3"/><rect class="xb" data-b="8" x="129" y="132" width="17" height="16" rx="3"/></g>
  <circle class="xb" data-b="32" cx="138" cy="92" r="7"/><circle class="xb" data-b="16" cx="182" cy="92" r="7"/>
  <circle class="xb guide" data-b="1024" cx="160" cy="66" r="12"/>
  <g class="face"><circle class="xb a" data-b="4096" cx="236" cy="112" r="11"/><text x="236" y="116">A</text>
    <circle class="xb b" data-b="8192" cx="258" cy="90" r="11"/><text x="258" y="94">B</text>
    <circle class="xb x" data-b="16384" cx="214" cy="90" r="11"/><text x="214" y="94">X</text>
    <circle class="xb y" data-b="32768" cx="236" cy="68" r="11"/><text x="236" y="72">Y</text></g>
</svg>`;

/** A virtual Xbox controller for games without Steam. Needs ViGEmBus on Windows, uinput on Linux. */
export class GamepadTab implements Tab {
  id = "gamepad";
  title = "Gamepad";
  group = "output" as const;
  icon = ICONS.gamepad;
  private root!: HTMLElement;
  private cfg: GamepadConfig | null = null;
  private editor!: LayoutEditor;
  private presets!: PresetBar;
  private pushTimer: number | undefined;
  private shown = false;
  private last: PadState | null = null;
  private lastDriver = "";

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="play">
        ${playBarHtml("gamepad")}
        <section class="tile play-edit" data-edit>
          <div class="play-presetbar">${PresetBar.html("Layout")}</div>
          <p class="hint layout-note" data-note></p>
          <div data-editor></div>
        </section>
        <aside class="play-side" data-side>
          <section class="tile play-live">
            <h3><span class="h3-title">What games see ${helpIcon("pad-live")}</span><em data-player></em></h3>
            ${PAD_SVG}
            <div class="key-chips" data-keys></div>
            <dl class="facts">
              <dt>Rumble</dt><dd class="rumble"><i><b data-rumble-l></b></i><i><b data-rumble-r></b></i></dd>
              <dt>Gyro</dt><dd data-gyro>off</dd>
            </dl>
          </section>
          ${notesHtml()}
          <p class="hint error" data-msg hidden></p>
        </aside>
        <section class="tile pad-driver" data-driver hidden></section>
      </div>`;
    wirePlayBar(root, (on) => {
      if (!this.cfg) return;
      this.cfg.enabled = on;
      this.push(true);
    });
    this.editor = new LayoutEditor(this.q("[data-editor]"), {
      pad: true,
      onChange: () => {
        this.presets.edited();
        this.push(false);
      },
    });
    this.presets = new PresetBar(this.q(".play-presetbar"), "gamepad", {
      layout: () => this.cfg?.layout ?? null,
      load: (l: Layout) => {
        if (!this.cfg) return;
        this.cfg.layout = l;
        this.editor.set(l);
        this.renderNote();
        this.push(true);
      },
      flash: (t) => this.flash(t),
    });
    this.q("[data-driver]").addEventListener("click", (e) => {
      const el = e.target as Element;
      if (el.closest("[data-install]")) invoke("play_install_driver").catch((err) => this.driverStatus(String(err)));
      if (el.closest("[data-check]")) invoke("play_check_driver").catch(() => {});
      if (el.closest("[data-use-km]")) document.querySelector<HTMLElement>('.sidebar [data-tab="keymouse"]')?.click();
    });
    this.init();
  }

  private async init(): Promise<void> {
    try {
      const setup = await invoke<PlaySetup>("play_setup");
      this.cfg = setup.gamepad;
      await this.presets.init({ pad: this.cfg.layout.name });
      this.presets.select("pad");
      this.editor.set(this.cfg.layout);
      this.renderNote();
    } catch (e) {
      this.flash(String(e));
    }
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector<T>(sel)!;
  }

  private renderNote(): void {
    setText(this.q("[data-note]"), LAYOUT_NOTES[this.cfg?.layout.name ?? ""] ?? "");
  }

  private push(now: boolean): void {
    clearTimeout(this.pushTimer);
    const send = () => {
      if (this.cfg) invoke("play_set_gamepad", { config: this.cfg }).catch((e) => this.flash(String(e)));
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

  private driverStatus(text: string): void {
    const el = this.root.querySelector<HTMLElement>("[data-install-status]");
    if (el) setText(el, text);
  }

  update(snap: Snapshot): void {
    if (!this.shown) {
      this.shown = true;
      // The driver may have been installed or removed meanwhile; the check runs on a thread.
      invoke("play_check_driver").catch(() => {});
      invoke("play_set_preview", { slot: "pad" }).catch(() => {});
    }
    const p = snap.play;
    if (!p) return;
    renderPlayBar(this.root, snap, "gamepad");
    renderNotes(this.root, snap);
    const g = p.gamepad;

    // Without the driver, the explainer takes the place of the editor.
    const support = g.support?.kind ?? "ready";
    const driver = support === "ready" ? "" : support === "unsupported" ? "unsupported" : OS;
    if (driver !== this.lastDriver) {
      this.lastDriver = driver;
      const panel = this.q("[data-driver]");
      panel.innerHTML = driver === "windows" ? WINDOWS_DRIVER : driver === "linux" ? LINUX_DRIVER : driver ? UNSUPPORTED : "";
      panel.hidden = !driver;
      setHidden(this.q("[data-edit]"), !!driver);
      setHidden(this.q("[data-side]"), !!driver);
    }
    if (driver) {
      const install = this.root.querySelector<HTMLButtonElement>("[data-install]");
      if (install) {
        install.disabled = g.installing;
        setText(install, g.installing ? "Installing…" : "Install driver");
      }
      this.driverStatus(g.installing ? "Accept the administrator prompt, then follow the installer." : g.install_error ?? "");
      return;
    }

    const v = p.preview;
    const live = v.slot === "pad" && !!snap.state;
    this.editor.showLit(live ? v.lit : 0, live && v.gyro_active);
    this.renderPad(live ? v.pad : { buttons: 0, lt: 0, rt: 0, lx: 0, ly: 0, rx: 0, ry: 0 });
    renderChips(this.q("[data-keys]"), live ? v.keys : [], live ? v.mouse_buttons : 0);
    setText(this.q("[data-player]"), g.connected ? (g.player ? `Player ${g.player}` : "Connected") : g.enabled ? "" : "Off");
    const [l, r] = g.rumble;
    setAttr(this.q("[data-rumble-l]"), "style", `width:${Math.round((l / 255) * 100)}%`);
    setAttr(this.q("[data-rumble-r]"), "style", `width:${Math.round((r / 255) * 100)}%`);
    const gyro = this.cfg?.layout.gyro.output ?? "none";
    setText(this.q("[data-gyro]"), gyro === "none" ? "off" : live && v.gyro_active ? (gyro === "steer" ? "steering" : "aiming") : "waiting");
  }

  private renderPad(s: PadState): void {
    const prev = this.last;
    // Sticks and triggers in whole steps of the drawing, so jitter at rest writes nothing.
    const stick = (x: number, y: number) => [Math.round((x / 32767) * 10), Math.round((-y / 32767) * 10)];
    const key = (p: PadState) => [p.buttons, Math.round(p.lt / 8), Math.round(p.rt / 8), ...stick(p.lx, p.ly), ...stick(p.rx, p.ry)].join();
    if (prev && key(prev) === key(s)) return;
    this.last = s;
    this.root.querySelectorAll<SVGElement>(".xpad [data-b]").forEach((el) => {
      el.classList.toggle("on", (s.buttons & Number(el.dataset.b)) !== 0);
    });
    const [lx, ly] = stick(s.lx, s.ly);
    const [rx, ry] = stick(s.rx, s.ry);
    setAttr(this.q("[data-ls]"), "cx", String(92 + lx));
    setAttr(this.q("[data-ls]"), "cy", String(92 + ly));
    setAttr(this.q("[data-rs]"), "cx", String(206 + rx));
    setAttr(this.q("[data-rs]"), "cy", String(140 + ry));
    setAttr(this.q("[data-lt]"), "width", String(Math.round((s.lt / 255) * 70)));
    setAttr(this.q("[data-rt]"), "width", String(Math.round((s.rt / 255) * 70)));
  }

  hide(): void {
    this.shown = false;
  }
}
