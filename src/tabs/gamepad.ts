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
  Standard: "Every button where an Xbox controller has it; grips and trackpads do nothing. With Keys & mouse on too, the Steam button switches between them.",
  "Back grips as face buttons": "Standard, with the grips as A, B, X and Y so your thumbs stay on the sticks.",
  "Gyro aim (stick)": "Standard, plus aiming with the right stick by moving the controller while your thumb rests on the right stick.",
  "Gyro aim (mouse)": "Like gyro aim, but it moves the mouse: more precise, for games that take mouse and controller at once.",
  "Racing (tilt to steer)": "Lean the controller like a wheel to steer; the triggers are the pedals, the grips shift.",
  "Trackpads as D-pad and mouse": "Click the left pad for the D-pad; the right pad is a mouse.",
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

/** An Xbox 360 controller, drawn from its state: the bumpers wrap the shoulders, the triggers fill
 * from the bottom as they're pulled (TRIGGER_TOP..TRIGGER_BOTTOM). */
const PAD_SVG = `<svg class="xpad" viewBox="0 0 320 228" aria-hidden="true">
  <defs><clipPath id="xpad-lt"><path d="M108,32 C106,20 92,11 72,12 C42,13 22,36 14,60 C22,48 32,40 45,34 C64,27 90,27 108,32 Z"/></clipPath><clipPath id="xpad-rt"><path d="M212,32 C214,20 228,11 248,12 C278,13 298,36 306,60 C298,48 288,40 275,34 C256,27 230,27 212,32 Z"/></clipPath></defs>
  <path class="xtrig" d="M108,32 C106,20 92,11 72,12 C42,13 22,36 14,60 C22,48 32,40 45,34 C64,27 90,27 108,32 Z"/><rect class="trig" data-lt clip-path="url(#xpad-lt)" x="10" y="61" width="100" height="0"/>
  <path class="xtrig" d="M212,32 C214,20 228,11 248,12 C278,13 298,36 306,60 C298,48 288,40 275,34 C256,27 230,27 212,32 Z"/><rect class="trig" data-rt clip-path="url(#xpad-rt)" x="210" y="61" width="100" height="0"/>
  <path class="xbody" d="M160,50 C126,50 96,44 72,47 C42,51 22,70 15,96 C8,122 0,160 3,186 C6,212 28,226 48,218 C66,211 78,190 92,174 C104,162 118,158 136,158 L184,157 C202,158 216,162 228,174 C242,190 254,211 272,218 C292,226 314,212 317,186 C320,160 312,122 305,96 C298,70 278,51 248,47 C224,44 194,50 160,50 Z"/>
  <path class="xb bumper" data-b="256" d="M112,36 C92,31 66,31 47,38 C30,45 16,59 8,80 C6,87 14,91 17,85 C24,67 36,55 52,49 C70,43 91,43 110,46 C118,47 119,37 112,36 Z"/><path class="xb bumper" data-b="512" d="M208,36 C228,31 254,31 273,38 C290,45 304,59 312,80 C314,87 306,91 303,85 C296,67 284,55 268,49 C250,43 229,43 210,46 C202,47 201,37 208,36 Z"/>
  <circle class="well" cx="70" cy="94" r="23"/><circle class="xstick" data-ls data-b="64" cx="70" cy="94" r="15"/>
  <circle class="well" cx="200" cy="132" r="23"/><circle class="xstick" data-rs data-b="128" cx="200" cy="132" r="15"/>
  <circle class="well" cx="120" cy="132" r="22"/>
  <g class="dpad"><path class="xb" data-b="1" d="M114,114 h12 v12 l-6,6 l-6,-6 Z"/><path class="xb" data-b="2" d="M114,150 h12 v-12 l-6,-6 l-6,6 Z"/>
    <path class="xb" data-b="4" d="M102,126 v12 h12 l6,-6 l-6,-6 Z"/><path class="xb" data-b="8" d="M138,126 v12 h-12 l-6,-6 l6,-6 Z"/></g>
  <circle class="xb small" data-b="32" cx="126" cy="98" r="6.5"/><circle class="xb small" data-b="16" cx="194" cy="98" r="6.5"/>
  <circle class="guide-ring" cx="160" cy="78" r="17"/><circle class="xb guide" data-b="1024" cx="160" cy="78" r="12"/>
  <g class="face"><circle class="xb a" data-b="4096" cx="252" cy="114" r="11"/><text x="252" y="118">A</text>
    <circle class="xb b" data-b="8192" cx="275" cy="92" r="11"/><text x="275" y="96">B</text>
    <circle class="xb x" data-b="16384" cx="229" cy="92" r="11"/><text x="229" y="96">X</text>
    <circle class="xb y" data-b="32768" cx="252" cy="70" r="11"/><text x="252" y="74">Y</text></g>
</svg>`;
const LS = [70, 94];
const RS = [200, 132];
const TRIGGER_TOP = 11;
const TRIGGER_BOTTOM = 61;

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
    setAttr(this.q("[data-ls]"), "cx", String(LS[0] + lx));
    setAttr(this.q("[data-ls]"), "cy", String(LS[1] + ly));
    setAttr(this.q("[data-rs]"), "cx", String(RS[0] + rx));
    setAttr(this.q("[data-rs]"), "cy", String(RS[1] + ry));
    for (const [attr, v] of [["[data-lt]", s.lt], ["[data-rt]", s.rt]] as const) {
      const h = Math.round((v / 255) * (TRIGGER_BOTTOM - TRIGGER_TOP));
      setAttr(this.q(attr), "y", String(TRIGGER_BOTTOM - h));
      setAttr(this.q(attr), "height", String(h));
    }
  }

  hide(): void {
    this.shown = false;
  }
}
