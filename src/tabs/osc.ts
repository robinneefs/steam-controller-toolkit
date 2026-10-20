import { invoke } from "@tauri-apps/api/core";
import type { OscArg, OscDestination, OscDestinationStatus, OscGroups, OscMapping, OscPreset, Snapshot } from "../controller";
import { addHelp, helpIcon } from "../help";
import type { Tab } from "./tab";
import { ICONS } from "./icons";
import { powerHtml } from "./power";
import { BUTTONS, SOURCE_LABELS, SOURCES } from "./sources";

addHelp({
  "osc-dests": `<b>Destinations</b> are the apps OSC goes to, each at a <b>host</b> and <b>port</b>, with its own preset.
    Several can be on at once.<ul>
    <li><b>Host</b>: <code>127.0.0.1</code> for an app on this computer, or the other computer's IP address.</li>
    <li><b>Port</b>: the port the app listens on. Picking an app's preset fills in its default.</li></ul>
    The dot turns green once the destination is on and its host is known. Click a row to edit its preset below.`,
  "osc-preset": `A <b>preset</b> is what a destination sends: the <b>stream</b>, <b>mappings</b>, or both. There is one for
    each app the <a data-open-osc-guide>Setup guide</a> covers. Changes apply right away; <b>Save as…</b> keeps them,
    <b>Folder</b> shows the .json files so you can share them.<ul>
    <li><b>Rate</b>: most messages per second for sticks, triggers and motion. Button presses always go out at once.</li>
    <li><b>Bundles</b>: each report's messages in one packet. Turn off for apps that don't read bundles.</li></ul>`,
  "osc-stream": `The <b>stream</b> sends every input under the prefix, ready for apps you program yourself
    (TouchDesigner, Max, Pure Data, Processing…). Only values that change are sent.<ul>
    <li><b>Grouped</b>: <code>/sc/left_stick x y</code>. <b>Split</b>: <code>/sc/left_stick/x</code> and
      <code>/sc/left_stick/y</code>, one value each.</li>
    <li>Sticks and pads −1..1, triggers and pressure 0..1, buttons 1/0, tilt in degrees, gyro in °/s, accel in g,
      quat w x y z. Pads send a position only while touched.</li>
    <li><code>/sc/connected</code> goes 0 when the controller is lost or a replay starts or stops; held buttons go back to 0.</li></ul>
    The <b>Addresses</b> list shows every address and its value.`,
  "osc-maps": `Each <b>mapping</b> sends one input to an address the app already knows, e.g. a layer's opacity in
    Resolume or <code>/go</code> in QLab.<ul>
    <li><b>Input</b>: what you move or press. Click <b>◎</b> (Learn), then move the control.</li>
    <li><b>Address</b>: where the value goes.</li>
    <li><b>Type</b>: float, int or bool (true/false).</li>
    <li><b>Sends</b>: <b>On change</b> sends the value whenever it changes. <b>Button</b> sends the top of the output
      range on press and the bottom on release. <b>Trigger</b> sends just the address once per press.</li>
    <li><b>⋯</b>: values sent before the value (e.g. a track number), the output range, and the shaping (range,
      deadzone, curve, smoothing, invert, toggle, only while held).</li></ul>`,
  "osc-values": `Every address the selected destination sends, with the value it last carried. Click one to copy its
    address, e.g. into TouchDesigner or Max.`,
  "osc-shaping": `<ul><li><b>Before</b>: values sent ahead of the controller's value, separated by spaces: <code>0</code> is an int,
    <code>0.5</code> a float, anything else a string (quote it to keep spaces).</li>
    <li><b>Output</b>: what the input's full travel maps to, e.g. −1 to 1, or 0 to 127.</li>
    <li><b>Range</b>: the part of the input's travel that sweeps the whole output.</li>
    <li><b>Deadzone</b>: travel at rest that is ignored. <b>Curve</b>: left reacts fast, right starts slow.</li>
    <li><b>Smooth</b>: evens out shaky input like tilt. <b>Invert</b> flips it. <b>Toggle</b>: each press flips on/off.</li>
    <li><b>Active</b>: only send while a button is held.</li></ul>`,
});

const GUIDE_SEEN = "osc-guide-seen";
function seenGuide(): boolean {
  try {
    return localStorage.getItem(GUIDE_SEEN) === "1";
  } catch {
    return false;
  }
}
function markGuideSeen(): void {
  try {
    localStorage.setItem(GUIDE_SEEN, "1");
  } catch {
    // Not remembered; the guide just opens again next time.
  }
}

/** Setup per app, by built-in preset name (`builtin_presets` in src-tauri/src/osc.rs). */
const APPS: Record<string, string> = {
  Generic: `<p>Have your app listen for <b>OSC over UDP</b> on the port. Every input arrives under <code>/sc</code>;
    the <b>Addresses</b> list shows them all.</p>`,
  TouchDesigner: `<p>Add an <b>OSC In CHOP</b> and set its <b>Network Port</b> to the destination's port (10000 here). Each address
    becomes a channel. <b>Split</b> is on, so every value is a channel of its own.</p>`,
  "Max/MSP": `<p><code>[udpreceive 7400]</code> → <code>[route /sc/left_stick /sc/button/a]</code> → <code>[unpack f f]</code>.
    udpreceive reads bundles as well.</p>`,
  "Pure Data": `<p><code>[netreceive -u -b 9001]</code> → <code>[oscparse]</code> → <code>[list trim]</code> →
    <code>[route sc]</code> → <code>[route left_stick trigger_right]</code>. oscparse splits each address into words.</p>`,
  "Processing (oscP5)": `<p><code>oscP5 = new OscP5(this, 12000);</code> then in <code>oscEvent(OscMessage m)</code>:
    <code>if (m.checkAddrPattern("/sc/left_stick")) x = m.get(0).floatValue();</code></p>`,
  "openFrameworks (ofxOsc)": `<p><code>receiver.setup(12345);</code> then each frame, while
    <code>receiver.hasWaitingMessages()</code>: get the message and check <code>m.getAddress() == "/sc/left_stick"</code>,
    then <code>m.getArgAsFloat(0)</code>.</p>`,
  SuperCollider: `<p>sclang listens on <b>57120</b> (<code>NetAddr.langPort</code>):
    <code>OSCdef(\\stick, { |msg| msg.postln }, '/sc/left_stick');</code></p>`,
  "Sonic Pi": `<p>In <b>Prefs → IO</b>, allow incoming OSC. Sonic Pi listens on <b>4560</b>; wait for a value with
    <code>x, y = sync "/osc*/sc/left_stick"</code>. Every message is a cue, so this preset sends only triggers, sticks
    and buttons, 60 per second.</p>`,
  "Unity (OscJack)": `<p>Install <b>OscJack</b> (keijiro, Unity 2021.3+). Make an <b>OSC Connection</b> asset with port 9000, then add an
    <b>OSC Event Receiver</b> per address, e.g. <code>/sc/left_stick</code>. It reads up to 4 values per message.</p>`,
  "Unreal Engine": `<p>Turn on the <b>OSC</b> plugin (Edit → Plugins → Input Devices). In a Blueprint, <b>Create OSC Server</b> on port 8000
    with Start Listening, then bind an event to an address such as <code>/sc/left_stick</code>.</p>`,
  "Godot (godOSC)": `<p>Install <b>godOSC</b> from the Asset Library. Add an <b>OSCServer</b> node (port 4646, its default) and an
    <b>OSCReceiver</b> for each address you want, e.g. <code>/sc/left_stick</code>.</p>`,
  Chataigne: `<p>Add <b>Module → Protocol → OSC</b>; its local port is <b>12000</b>. Incoming addresses show up as values you can map.</p>`,
  "Open Stage Control": `<p>Its OSC port is the same as its HTTP port, <b>8080</b>, unless set with <code>osc-port</code>.
    Widgets whose address matches, e.g. <code>/sc/trigger_right</code>, follow the controller.</p>`,
  Isadora: `<p>Isadora listens on <b>1234</b>. Open <b>Communications → Stream Setup</b>, turn on <b>Auto-detect input</b> and move
    the controls; then use <b>OSC Listener</b> actors with the channel numbers it shows.</p>`,
  VDMX: `<p>In <b>Preferences → OSC → Inputs</b>, add a port (1235 here). Then set a control to receive from an address, e.g.
    <code>/sc/trigger_right</code>, or use its detect option and move the control.</p>`,
  "QLC+": `<p>In <b>Inputs/Outputs</b>, enable the <b>OSC</b> input for universe 1 (port <b>7700</b>; universe 2 is 7701). Then
    auto-detect controls in an input profile or a widget and move them. One value per address, 60 per second.</p>`,
  MadMapper: `<p>In <b>Preferences → OSC/MIDI</b>, the OSC input port is usually <b>8000</b>. The right trigger sets the master level
    (<code>/master/master_level</code>). For other controls, right-click one → <b>Copy OSC Address</b> and add a mapping,
    or use the stream.</p>`,
  Millumin: `<p>Open <b>Devices</b> (Cmd+K) → <b>OSC</b> and set the input port (5000 here). A, B, X and Y launch columns 1–4.</p>`,
  "Resolume Arena / Avenue": `<p>In <b>Preferences → OSC</b>, turn on <b>OSC Input</b> (port <b>7000</b>). The triggers fade layers 1 and 2; A, B, X and Y
    play clips 1–4 of layer 1. <b>Shortcuts → Edit OSC</b> shows the address of any control.</p>`,
  QLab: `<p>QLab listens on <b>53000</b>. In <b>Workspace Settings → Network</b>, allow OSC without a passcode (or the commands are
    ignored). A is <b>GO</b>, B is <b>Panic</b>, D-pad up/down move the selection.</p>`,
  Reaper: `<p><b>Preferences → Control/OSC/web → Add → OSC</b>: mode <b>Local port</b>, listen port <b>8000</b>, pattern config
    <b>Default</b>. A play, B stop, X record, D-pad ←/→ rewind and forward, left stick pans the selected track, triggers set
    FX parameters 1 and 2.</p>`,
  "Ableton Live (AbletonOSC)": `<p>Install <b>AbletonOSC</b> (github.com/ideoforms/AbletonOSC) in <i>User Library/Remote Scripts</i>, restart Live 11+,
    then pick <b>AbletonOSC</b> as a Control Surface in <b>Settings → Link, Tempo &amp; MIDI</b>. It listens on <b>11000</b>.
    A play, B stop, X/Y fire scenes 1 and 2, the triggers set track 1 and 2 volume (full is 0 dB).</p>`,
  "Bitwig (DrivenByMoss)": `<p>Install <b>DrivenByMoss</b>, then <b>Settings → Controllers → Add controller → Open Sound Control</b>. Pick any MIDI
    input it asks for. It receives on <b>8000</b> with values 0–127 (value resolution Low). A play, B stop, triggers set
    track 1 and 2 volume, right stick the first two device parameters.</p>`,
  "VCV Rack (cvOSCcv)": `<p>Add trowaSoft's <b>cvOSCcv</b>, set its in port to <b>7001</b> and enable it. Channels <code>/ch/1</code>–<code>/ch/8</code>:
    triggers 0–10 V, sticks ±5 V, A and B as gates.</p>`,
  "ETC Eos": `<p><b>Setup → System → Show Control → OSC</b>: turn on <b>OSC UDP RX</b> and set the RX port (ETC suggests <b>8000</b>).
    A presses <b>GO</b>, Y brings Sub 1 to full. Add your own from Eos' OSC dictionary, e.g. <code>/eos/key/…</code>.</p>`,
  VRChat: `<p>In the Action Menu: <b>Options → OSC → Enabled</b>. VRChat listens on <b>9000</b>. Left stick moves, right stick X turns,
    A jumps, L3 runs, Menu opens the quick menu. <code>/avatar/parameters/TriggerRight</code> is an example; rename it to a
    parameter your avatar has.</p>`,
};

const GROUPS: [keyof OscGroups, string, string][] = [
  ["triggers", "Triggers", "trigger_left/right 0..1, and their click"],
  ["sticks", "Sticks", "left_stick, right_stick: x y −1..1"],
  ["pads", "Pads", "left_pad, right_pad: x y (while touched), pressure, touch, click"],
  ["buttons", "Buttons", "button/a, button/dpad_up, button/l4… 1 or 0"],
  ["touch", "Touch", "stick and grip touch sensors, 1 or 0"],
  ["tilt", "Tilt", "tilt: pitch roll in degrees"],
  ["gyro", "Gyro", "gyro: pitch roll yaw turn speed in °/s"],
  ["accel", "Accel", "accel: x y z in g"],
  ["quat", "Quat", "quat: orientation w x y z"],
  ["status", "Status", "battery (%) and connected (1 or 0)"],
];

const RATES: [number, string][] = [[265, "Every report"], [120, "120 / s"], [60, "60 / s"]];

const escape = (s: string) => s.replace(/[&<>"]/g, (c) => `&${{ "&": "amp", "<": "lt", ">": "gt", '"': "quot" }[c]};`);
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** An argument as text: 3, 0.25, true, "name". */
function argText(a: OscArg): string {
  if ("int" in a) return String(a.int);
  if ("float" in a) return Number.isInteger(a.float) ? a.float.toFixed(1) : String(Math.round(a.float * 1000) / 1000);
  if ("bool" in a) return a.bool ? "true" : "false";
  return JSON.stringify(a.string);
}

/** Fixed arguments as typed: ints, floats (with a dot), true/false, else strings ("quoted" keeps spaces). */
function parseArgs(text: string): OscArg[] {
  const out: OscArg[] = [];
  for (const m of text.matchAll(/"([^"]*)"|(\S+)/g)) {
    if (m[1] !== undefined) out.push({ string: m[1] });
    else if (/^-?\d+$/.test(m[2])) out.push({ int: Number(m[2]) });
    else if (/^-?(\d+\.\d*|\.\d+)$/.test(m[2])) out.push({ float: Number(m[2]) });
    else if (m[2] === "true" || m[2] === "false") out.push({ bool: m[2] === "true" });
    else out.push({ string: m[2] });
  }
  return out;
}

interface PresetInfo {
  name: string;
  builtin: boolean;
  port: number | null;
}

/** Send controller input to other apps as OSC: a stream of every input, mappings, several destinations. */
export class OscTab implements Tab {
  id = "osc";
  title = "OSC";
  group = "output" as const;
  icon = ICONS.osc;
  private root!: HTMLElement;
  private dests: OscDestination[] = [];
  /** The destination being edited. */
  private sel = 0;
  private presets: PresetInfo[] = [];
  /** Per destination: the preset its editor started from (`null` once unknown), and whether it changed since. */
  private loaded: (PresetInfo | null)[] = [];
  private edited: boolean[] = [];
  private pushTimer: number | undefined;
  private learnRow: number | null = null;
  private learnSeen = false;
  private open = new Set<number>();
  private lastValues = "";
  private lastStatus: string[] = [];
  private lastPower = "";
  private deleteArmed = false;
  private armTimer: number | undefined;

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="osc">
        <section class="tile osc-dests">
          <div class="out-bar">
            ${powerHtml("OSC", "Turn OSC on or off")}
            <span class="midi-conn" data-osc-conn><i></i>Off</span>
            <span class="head-actions">
              <span class="h3-title muted">Destinations ${helpIcon("osc-dests")}</span>
              <button class="mini guide-btn" data-guide title="How to receive OSC in your app">Setup guide</button>
              <button class="mini" data-add-dest title="Send to another app or computer as well">+ Add</button>
            </span>
          </div>
          <div class="dest-list" data-dests></div>
        </section>

        <section class="tile osc-edit">
          <div class="osc-presetbar">
            <span class="bar-group">
              <span class="muted">Preset</span>${helpIcon("osc-preset")}
              <select class="field" data-preset></select>
              <input class="field" data-save-name placeholder="Preset name" maxlength="60" hidden />
              <button class="mini" data-save title="Save this destination's preset">Save as…</button>
              <button class="mini" data-save-cancel hidden>Cancel</button>
              <button class="mini danger" data-delete title="Delete this saved preset">Delete</button>
              <button class="mini" data-folder title="Open the presets folder (share presets by copying the .json files)">Folder</button>
            </span>
            <span class="bar-group send-group">
              <label class="muted" title="Most messages per second for sticks, triggers and motion; button presses always go out at once">Rate
                <select class="field" data-pf="rate">${RATES.map(([v, l]) => `<option value="${v}">${l}</option>`).join("")}</select></label>
              <label class="chk" title="Send each report's messages in one packet (an OSC bundle)"><input type="checkbox" data-pf="bundle" />Bundles</label>
            </span>
          </div>
          <div class="osc-stream" data-stream></div>
          <h3 class="maps-h"><span class="h3-title">Mappings ${helpIcon("osc-maps")}</span>
            <span class="head-actions"><em data-map-count></em><button class="mini" data-add>+ Add</button></span></h3>
          <div class="map-head osc-map-head"><span></span><span>Input</span><span></span><span>Address</span><span>Type</span><span>Sends</span><span></span><span></span></div>
          <div class="map-list" data-maps></div>
        </section>

        <section class="tile osc-values">
          <h3><span class="h3-title">Addresses ${helpIcon("osc-values")}</span> <em class="num" data-rate></em></h3>
          <div class="addr-list" data-values></div>
        </section>
        <div class="midi-guide osc-guide" data-guide-panel hidden></div>
      </div>`;

    this.q("[data-guide]").addEventListener("click", () => this.showGuide(this.q("[data-guide-panel]").hidden !== false));
    document.addEventListener("click", (e) => {
      if ((e.target as Element).closest?.("[data-open-osc-guide]")) this.showGuide(true);
    });
    const panel = this.q("[data-guide-panel]");
    panel.addEventListener("click", (e) => {
      const el = e.target as Element;
      if (el.closest("[data-guide-close]")) this.showGuide(false);
      if (el.closest("[data-guide-use]")) {
        const name = this.q<HTMLSelectElement>("[data-guide-app]").value;
        this.loadPreset(`b:${name}`).then(() => this.showGuide(false));
      }
    });
    panel.addEventListener("change", (e) => {
      if ((e.target as Element).matches("[data-guide-app]")) this.renderGuideApp();
    });
    this.q("[data-add-dest]").addEventListener("click", () => this.addDest());
    const dests = this.q("[data-dests]");
    dests.addEventListener("change", (e) => this.destFieldChanged(e.target as HTMLInputElement));
    dests.addEventListener("click", (e) => this.destClicked(e.target as Element));

    this.q("[data-preset]").addEventListener("change", (e) => this.loadPreset((e.target as HTMLSelectElement).value));
    this.q("[data-save]").addEventListener("click", () => this.saveClicked());
    this.q("[data-save-cancel]").addEventListener("click", () => this.showSaveName(false));
    this.q<HTMLInputElement>("[data-save-name]").addEventListener("keydown", (e) => {
      if (e.key === "Enter") this.saveClicked();
      if (e.key === "Escape") this.showSaveName(false);
    });
    this.q("[data-delete]").addEventListener("click", () => this.deleteClicked());
    this.q("[data-folder]").addEventListener("click", () => invoke("osc_reveal_presets").catch((e) => this.flash(String(e))));
    this.root.querySelectorAll<HTMLElement>("[data-pf]").forEach((el) =>
      el.addEventListener("change", () => {
        const p = this.preset();
        if (!p) return;
        if (el.dataset.pf === "rate") p.rate = Number((el as HTMLSelectElement).value);
        else p.bundle = (el as HTMLInputElement).checked;
        this.push(true);
      }),
    );

    const stream = this.q("[data-stream]");
    stream.addEventListener("change", (e) => this.streamFieldChanged(e.target as HTMLInputElement));
    stream.addEventListener("click", (e) => this.streamClicked(e.target as Element));

    this.q("[data-add]").addEventListener("click", () => this.addMapping());
    const list = this.q("[data-maps]");
    list.addEventListener("change", (e) => this.mapFieldChanged(e.target as HTMLElement));
    list.addEventListener("input", (e) => {
      const el = e.target as HTMLInputElement;
      if (el.type === "range" || el.type === "number") this.mapFieldChanged(el);
    });
    list.addEventListener("click", (e) => this.mapClicked(e.target as Element));
    this.q<HTMLInputElement>("[data-on]").addEventListener("change", (e) =>
      invoke("osc_set_enabled", { on: (e.target as HTMLInputElement).checked }).catch((err) => this.flash(String(err))),
    );

    this.init();
  }

  private async init(): Promise<void> {
    try {
      const [dests] = await Promise.all([invoke<OscDestination[]>("osc_destinations"), this.refreshPresets()]);
      this.dests = dests;
      this.loaded = dests.map((d) => this.presets.find((p) => p.name === d.preset.name && !p.builtin) ?? this.presets.find((p) => p.name === d.preset.name) ?? null);
      this.edited = dests.map(() => false);
      this.renderAll();
      // First visit: start with the guide.
      if (!seenGuide()) this.showGuide(true);
    } catch (e) {
      this.flash(String(e));
    }
  }

  update(snap: Snapshot): void {
    const o = snap.osc;
    if (!o) return;
    this.renderPower(o.enabled, o.destinations);
    if (!this.dests.length) return;
    this.dests.forEach((d, i) => this.renderDestStatus(i, d, o.destinations[i], o.enabled));
    const st = o.destinations[this.sel];
    const rate = st?.address ? `${st.rate} msg/s` : "";
    const rateEl = this.q("[data-rate]");
    if (rateEl.textContent !== rate) rateEl.textContent = rate;

    // Meters.
    this.root.querySelectorAll<HTMLElement>(".osc .map-row").forEach((row) => {
      const level = st?.levels[Number(row.dataset.i)];
      const bar = row.querySelector<HTMLElement>(".meter i")!;
      const w = level == null ? "0%" : `${(level * 100).toFixed(1)}%`;
      if (bar.style.width !== w) bar.style.width = w;
      row.classList.toggle("idle", level == null);
    });

    this.renderValues(st);

    // Learn.
    if (this.learnRow !== null) {
      if (o.learning) this.learnSeen = true;
      else if (this.learnSeen && o.learned) {
        const i = this.learnRow;
        this.stopLearn(false);
        const m = this.preset()?.mappings[i];
        if (m) {
          m.source = o.learned;
          this.renderMaps();
          this.push(true);
        }
      }
    }
  }

  hide(): void {
    this.stopLearn(true);
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector<T>(sel)!;
  }

  private preset(): OscPreset | null {
    return this.dests[this.sel]?.preset ?? null;
  }

  private flash(text: string): void {
    this.q("[data-values]").innerHTML = `<p class="hint error">${escape(text)}</p>`;
    this.lastValues = "";
  }

  private renderAll(): void {
    this.sel = clamp(this.sel, 0, Math.max(0, this.dests.length - 1));
    this.renderDests();
    this.renderPresets();
    this.renderStream();
    this.renderMaps();
    this.lastValues = "";
  }

  // ---------- Destinations ----------

  private renderDests(): void {
    this.lastStatus = [];
    this.q("[data-dests]").innerHTML = this.dests.length
      ? this.dests.map((d, i) => `
        <div class="dest-row${i === this.sel ? " sel" : ""}${d.enabled ? "" : " off"}" data-d="${i}" title="Click to edit this destination's preset">
          <input type="checkbox" data-df="enabled" ${d.enabled ? "checked" : ""} title="Send to this destination" />
          <i class="dot" data-dot></i>
          <span class="dest-name" title="${escape(d.preset.name || "Custom")}">${escape(d.preset.name || "Custom")}</span>
          <input class="field host" data-df="host" value="${escape(d.host)}" spellcheck="false" title="Host: 127.0.0.1 for this computer, or another computer's IP address" />
          <span class="muted">:</span>
          <input class="field num port" type="number" data-df="port" min="1" max="65535" value="${d.port}" title="The port the app listens on" />
          <span class="dest-status" data-status></span>
          <button class="icon-btn" data-del-dest title="Remove this destination">×</button>
        </div>`).join("")
      : `<p class="hint">No destinations. <b>+ Add</b> one to send OSC.</p>`;
    this.q("[data-dests]").classList.toggle("scroll", this.dests.length > 3);
  }

  /** The switch and what OSC as a whole is doing. */
  private renderPower(on: boolean, list: OscDestinationStatus[]): void {
    const input = this.q<HTMLInputElement>("[data-on]");
    if (input.checked !== on) input.checked = on;
    const sending = list.filter((d) => d.address);
    const html = !on
      ? `<i></i>Off`
      : sending.length
        ? `<i class="ok"></i>Sending to <b>${sending.length > 1 ? `${sending.length} destinations` : escape(sending[0].address!)}</b>`
        : list.some((d) => d.error)
          ? `<i class="warn"></i><span>Not sending</span>`
          : this.dests.some((d) => d.enabled)
            ? `<i></i>Looking up the host…`
            : `<i></i>On · tick a destination to send to it`;
    if (html === this.lastPower) return;
    this.lastPower = html;
    this.q("[data-osc-conn]").innerHTML = html;
  }

  private renderDestStatus(i: number, d: OscDestination, st: OscDestinationStatus | undefined, on: boolean): void {
    const [cls, text, title] = !d.enabled
      ? ["", "Off", "Tick the box to send"]
      : !on
        ? ["", "OSC is off", "Turn OSC on to send"]
      : st?.error
        ? ["warn", escape(st.error), st.error]
        : st?.resolving || !st?.address
          ? ["", "Looking up the host…", ""]
          : ["ok", `<b>${st.rate}</b> msg/s`, `Sending to ${st.address}`];
    const key = `${cls}|${text}`;
    if (this.lastStatus[i] === key) return;
    this.lastStatus[i] = key;
    const row = this.root.querySelector<HTMLElement>(`.dest-row[data-d="${i}"]`);
    if (!row) return;
    row.querySelector<HTMLElement>("[data-dot]")!.className = `dot ${cls}`;
    const el = row.querySelector<HTMLElement>("[data-status]")!;
    el.innerHTML = text;
    el.title = title;
    el.classList.toggle("warn-text", cls === "warn");
  }

  private destFieldChanged(el: HTMLInputElement): void {
    const row = el.closest<HTMLElement>(".dest-row");
    if (!row || !el.dataset.df) return;
    const d = this.dests[Number(row.dataset.d)];
    switch (el.dataset.df) {
      case "enabled":
        d.enabled = el.checked;
        row.classList.toggle("off", !d.enabled);
        break;
      case "host":
        d.host = el.value.trim();
        break;
      case "port":
        if (el.value === "") return;
        d.port = clamp(Math.round(Number(el.value) || 0), 1, 65535);
        el.value = String(d.port);
        break;
    }
    this.push(false);
  }

  private destClicked(el: Element): void {
    const row = el.closest<HTMLElement>(".dest-row");
    if (!row) return;
    const i = Number(row.dataset.d);
    if (el.closest("[data-del-dest]")) {
      this.stopLearn(true);
      this.dests.splice(i, 1);
      this.loaded.splice(i, 1);
      this.edited.splice(i, 1);
      if (this.sel >= i && this.sel > 0) this.sel--;
      this.open.clear();
      this.renderAll();
      this.push(false);
      return;
    }
    this.select(i);
  }

  private select(i: number): void {
    if (i === this.sel) return;
    this.stopLearn(true);
    this.sel = i;
    this.open.clear();
    this.root.querySelectorAll<HTMLElement>(".dest-row").forEach((r) => r.classList.toggle("sel", Number(r.dataset.d) === i));
    this.renderPresets();
    this.renderStream();
    this.renderMaps();
    this.lastValues = "";
  }

  private async addDest(): Promise<void> {
    try {
      const preset = await invoke<OscPreset>("osc_load_preset", { name: "Generic", builtin: true });
      this.dests.push({ enabled: true, host: "127.0.0.1", port: preset.port ?? 9000, preset });
      this.loaded.push(this.presets.find((p) => p.builtin && p.name === "Generic") ?? null);
      this.edited.push(false);
      this.sel = this.dests.length - 1;
      this.open.clear();
      this.renderAll();
      this.push(false);
    } catch (e) {
      this.flash(String(e));
    }
  }

  // ---------- Presets ----------

  private async refreshPresets(): Promise<void> {
    this.presets = await invoke<PresetInfo[]>("osc_list_presets");
  }

  private renderPresets(): void {
    const p = this.preset();
    const loaded = this.loaded[this.sel] ?? null;
    const edited = !!this.edited[this.sel];
    const opt = (i: PresetInfo) => `<option value="${i.builtin ? "b" : "s"}:${escape(i.name)}">${escape(i.name)}</option>`;
    const builtin = this.presets.filter((i) => i.builtin);
    const saved = this.presets.filter((i) => !i.builtin);
    const sel = this.q<HTMLSelectElement>("[data-preset]");
    const unsaved = !loaded ? `<option value="">${escape(p?.name || "Custom")} (unsaved)</option>` : "";
    sel.innerHTML = unsaved +
      `<optgroup label="Apps">${builtin.map(opt).join("")}</optgroup>` +
      (saved.length ? `<optgroup label="Saved">${saved.map(opt).join("")}</optgroup>` : "");
    sel.value = loaded ? `${loaded.builtin ? "b" : "s"}:${loaded.name}` : "";
    sel.disabled = !p;
    sel.classList.toggle("edited", edited);
    sel.title = edited ? "Changed since it was loaded; Save as… to keep the changes" : "";
    this.q("[data-delete]").hidden = !loaded || loaded.builtin;
    this.q<HTMLSelectElement>("[data-pf='rate']").value = String(p?.rate ?? 265);
    this.q<HTMLInputElement>("[data-pf='bundle']").checked = !!p?.bundle;
    this.root.querySelectorAll<HTMLInputElement>("[data-pf], [data-add], [data-save]").forEach((el) => (el.disabled = !p));
    this.disarmDelete();
  }

  /** Load a preset into the selected destination; an app's preset brings its port. */
  private async loadPreset(value: string): Promise<void> {
    const d = this.dests[this.sel];
    if (!value || !d) return;
    const builtin = value.startsWith("b:");
    const name = value.slice(2);
    try {
      this.stopLearn(true);
      d.preset = await invoke<OscPreset>("osc_load_preset", { name, builtin });
      if (d.preset.port) d.port = d.preset.port;
      this.loaded[this.sel] = this.presets.find((p) => p.name === name && p.builtin === builtin) ?? null;
      this.edited[this.sel] = false;
      this.open.clear();
      this.renderAll();
      this.push(false);
    } catch (e) {
      this.flash(String(e));
      this.renderPresets();
    }
  }

  private showSaveName(on: boolean): void {
    const input = this.q<HTMLInputElement>("[data-save-name]");
    const loaded = this.loaded[this.sel];
    input.hidden = !on;
    this.q("[data-preset]").hidden = on;
    this.q("[data-save-cancel]").hidden = !on;
    this.q("[data-save]").textContent = on ? "Save" : "Save as…";
    this.q("[data-delete]").hidden = on || !loaded || loaded.builtin;
    this.q("[data-folder]").hidden = on;
    if (on) {
      input.value = loaded && !loaded.builtin ? loaded.name : "";
      input.focus();
      input.select();
    }
  }

  private async saveClicked(): Promise<void> {
    const input = this.q<HTMLInputElement>("[data-save-name]");
    const p = this.preset();
    if (!p) return;
    if (input.hidden) return this.showSaveName(true);
    const name = input.value.trim();
    if (!name) return input.focus();
    try {
      // Keep the port: loading the saved preset later fills it in, like an app's.
      await invoke("osc_save_preset", { name, preset: { ...p, port: this.dests[this.sel].port } });
      p.name = name;
      await this.refreshPresets();
      this.loaded[this.sel] = this.presets.find((i) => i.name === name && !i.builtin) ?? null;
      this.edited[this.sel] = false;
      this.showSaveName(false);
      this.renderPresets();
      this.renderDests();
      this.push(false);
    } catch (e) {
      this.flash(String(e));
    }
  }

  private async deleteClicked(): Promise<void> {
    const loaded = this.loaded[this.sel];
    if (!loaded || loaded.builtin) return;
    const b = this.q("[data-delete]");
    if (!this.deleteArmed) {
      this.deleteArmed = true;
      b.classList.add("armed");
      b.textContent = "Click to delete";
      this.armTimer = window.setTimeout(() => this.disarmDelete(), 3000);
      return;
    }
    try {
      await invoke("osc_delete_preset", { name: loaded.name });
      // The destination keeps the preset; it's just not saved anywhere now.
      this.loaded = this.loaded.map((l) => (l && !l.builtin && l.name === loaded.name ? null : l));
      await this.refreshPresets();
      this.renderPresets();
    } catch (e) {
      this.flash(String(e));
    }
  }

  private disarmDelete(): void {
    clearTimeout(this.armTimer);
    this.deleteArmed = false;
    const b = this.q("[data-delete]");
    b.classList.remove("armed");
    b.textContent = "Delete";
  }

  /** Send the destinations to the backend soon, coalescing quick edits like slider drags. */
  private push(presetEdit: boolean): void {
    if (presetEdit && !this.edited[this.sel]) {
      this.edited[this.sel] = true;
      this.renderPresets();
    }
    clearTimeout(this.pushTimer);
    this.pushTimer = window.setTimeout(() => this.flush(), 50);
  }

  private async flush(): Promise<void> {
    clearTimeout(this.pushTimer);
    await invoke("osc_set_destinations", { destinations: this.dests }).catch((e) => this.flash(String(e)));
  }

  // ---------- Stream ----------

  private renderStream(): void {
    const p = this.preset();
    const el = this.q("[data-stream]");
    if (!p) {
      el.innerHTML = "";
      return;
    }
    const s = p.stream;
    el.innerHTML = `
      <div class="stream-head${s.enabled ? "" : " off"}">
        <label class="chk stream-on"><input type="checkbox" data-sf="enabled" ${s.enabled ? "checked" : ""} />Stream every input</label>
        ${helpIcon("osc-stream")}
        <label class="muted" title="Put in front of every address; may be empty">Prefix
          <input class="field prefix" data-sf="prefix" value="${escape(s.prefix)}" spellcheck="false" /></label>
        <span class="segmented" title="Grouped: /sc/left_stick x y. Split: /sc/left_stick/x and /sc/left_stick/y">
          <button data-split="0" class="${s.split ? "" : "on"}">Grouped</button><button data-split="1" class="${s.split ? "on" : ""}">Split</button>
        </span>
      </div>
      <div class="group-chips${s.enabled ? "" : " off"}">
        ${GROUPS.map(([k, l, t]) => `<button class="chip${s.groups[k] ? " on" : ""}" data-group="${k}" title="${escape(t)}">${l}</button>`).join("")}
      </div>`;
  }

  private streamFieldChanged(el: HTMLInputElement): void {
    const p = this.preset();
    if (!p || !el.dataset.sf) return;
    if (el.dataset.sf === "enabled") {
      p.stream.enabled = el.checked;
      this.renderStream();
    } else {
      p.stream.prefix = el.value.trim();
    }
    this.push(true);
  }

  private streamClicked(el: Element): void {
    const p = this.preset();
    if (!p) return;
    const split = el.closest<HTMLElement>("[data-split]");
    const group = el.closest<HTMLElement>("[data-group]");
    if (split) p.stream.split = split.dataset.split === "1";
    else if (group) {
      const k = group.dataset.group as keyof OscGroups;
      p.stream.groups[k] = !p.stream.groups[k];
    } else return;
    this.renderStream();
    this.push(true);
  }

  // ---------- Addresses ----------

  private renderValues(st: OscDestinationStatus | undefined): void {
    const list = this.q("[data-values]");
    const values = st?.values ?? [];
    const key = values.map(([a]) => a).join("|");
    if (key !== this.lastValues) {
      this.lastValues = key;
      const d = this.dests[this.sel];
      list.innerHTML = values.length
        ? values.map(([a], i) => `<div class="addr-row" data-copy="${escape(a)}" title="Click to copy the address"><span class="addr">${escape(a).replace(/\//g, "<wbr>/")}</span><span class="vals num" data-v="${i}"></span></div>`).join("")
        : `<p class="hint">${!d ? "Add a destination to send OSC." : "Nothing sent yet. Move a control, or turn on the stream or a mapping."}</p>`;
    }
    list.querySelectorAll<HTMLElement>("[data-v]").forEach((el) => {
      const v = values[Number(el.dataset.v)]?.[1].map(argText).join(" ") ?? "";
      if (el.textContent !== v) el.textContent = v;
    });
  }

  // ---------- Mappings ----------

  private renderMaps(): void {
    const maps = this.preset()?.mappings ?? [];
    this.q("[data-map-count]").textContent = maps.length ? String(maps.length) : "";
    this.q("[data-maps]").innerHTML = maps.length
      ? maps.map((m, i) => this.mapRow(m, i)).join("")
      : `<p class="hint">${this.preset() ? "No mappings. The stream sends every input; add a mapping to send one to a fixed address." : ""}</p>`;
    maps.forEach((m, i) => this.setSelects(m, i));
    this.showAddressEnds();
  }

  /** Long addresses share their start (/composition/layers/1/…): show their ends, which differ. */
  private showAddressEnds(): void {
    requestAnimationFrame(() =>
      this.root.querySelectorAll<HTMLInputElement>(".osc .field.addr").forEach((el) => {
        if (document.activeElement !== el) el.scrollLeft = el.scrollWidth;
      }),
    );
  }

  private setSelects(m: OscMapping, i: number): void {
    const row = this.root.querySelector<HTMLElement>(`.osc .map-row[data-i="${i}"]`)!;
    row.querySelectorAll<HTMLSelectElement>("select[data-f]").forEach((s) => {
      const f = s.dataset.f!;
      s.value = f === "source" ? m.source : f === "value" ? m.value : f === "mode" ? m.mode : f === "hold" ? (m.hold ?? "") : "";
    });
  }

  private mapRow(m: OscMapping, i: number): string {
    const sourceOpts = SOURCES.map(([g, list]) =>
      `<optgroup label="${g}">${list.map(([k, l]) => `<option value="${k}">${l}</option>`).join("")}</optgroup>`).join("") +
      (SOURCE_LABELS.has(m.source) ? "" : `<option value="${escape(m.source)}">${escape(m.source)} (unknown)</option>`);
    const pct = (v: number) => Math.round(v * 100);
    const holdOpts = `<option value="">Always</option>` + BUTTONS.map(([n, l]) => `<option value="${n}">While ${l} held</option>`).join("");
    const learning = this.learnRow === i;
    const bad = !/^\/\S+$/.test(m.address);
    const trigger = m.mode === "trigger";
    return `
      <div class="map-row${m.enabled ? "" : " off"}" data-i="${i}">
        <div class="map-main osc-map-main">
          <input type="checkbox" data-f="enabled" ${m.enabled ? "checked" : ""} title="On / off" />
          <select class="field" data-f="source" title="Input">${sourceOpts}</select>
          <button class="icon-btn${learning ? " learning" : ""}" data-learn title="${learning ? "Move a control now… (click to cancel)" : "Learn: click, then move or press a control"}">${learning ? "…" : "◎"}</button>
          <input class="field addr${bad ? " bad" : ""}" data-f="address" value="${escape(m.address)}" spellcheck="false"
            title="${bad ? "An OSC address starts with / and has no spaces" : m.args.length ? `Sends ${escape(m.args.map(argText).join(" "))} before the value` : "OSC address"}" />
          <select class="field" data-f="value" title="Type of the value sent" ${trigger ? "disabled" : ""}>
            <option value="float">float</option><option value="int">int</option><option value="bool">bool</option></select>
          <select class="field" data-f="mode" title="When it sends">
            <option value="continuous">On change</option><option value="button">Button</option><option value="trigger">Trigger</option></select>
          <button class="icon-btn${this.open.has(i) ? " on" : ""}" data-more title="Values before, output range, shaping">⋯</button>
          <button class="icon-btn" data-del title="Remove">×</button>
        </div>
        <span class="meter"><i></i></span>
        <div class="map-more" ${this.open.has(i) ? "" : "hidden"}>
          ${helpIcon("osc-shaping", "What these settings do")}
          <label title="Sent before the value: 0 is an int, 0.5 a float, anything else a string">Before
            <input class="field args" data-f="args" value="${escape(m.args.map(argText).join(" "))}" placeholder="none" spellcheck="false" /></label>
          <label title="What the input's full travel maps to" ${trigger ? "hidden" : ""}>Output
            <input class="field num" type="number" step="any" data-f="out0" value="${m.out[0]}" />–<input class="field num" type="number" step="any" data-f="out1" value="${m.out[1]}" /></label>
          <label title="Part of the input's travel that spans the output, in %">Range
            <input class="field num" type="number" data-f="range0" min="0" max="100" value="${pct(m.range[0])}" />–<input class="field num" type="number" data-f="range1" min="0" max="100" value="${pct(m.range[1])}" />%</label>
          <label title="Travel around the centre (or at the bottom) that counts as rest, in %">Deadzone
            <input class="field num" type="number" data-f="deadzone" min="0" max="90" value="${pct(m.deadzone)}" />%</label>
          <label title="Left: reacts fast near rest. Right: slow near rest, for fine control">Curve
            <input type="range" data-f="curve" min="-100" max="100" value="${pct(m.curve)}" /></label>
          <label title="Evens out shaky input such as tilt; more smoothing lags more">Smooth
            <input type="range" data-f="smoothing" min="0" max="95" value="${pct(m.smoothing)}" /></label>
          <label class="chk" title="Output from the top down to the bottom"><input type="checkbox" data-f="invert" ${m.invert ? "checked" : ""} />Invert</label>
          <label class="chk" title="Each press flips the output on or off (a latch)"><input type="checkbox" data-f="toggle" ${m.toggle ? "checked" : ""} />Toggle</label>
          <label title="Only send while a button is held, e.g. tilt only while R4 is down">Active
            <select class="field" data-f="hold">${holdOpts}</select></label>
        </div>
      </div>`;
  }

  private mapFieldChanged(el: HTMLElement): void {
    const row = el.closest<HTMLElement>(".map-row");
    const f = el.dataset.f;
    const p = this.preset();
    if (!row || !f || !p) return;
    const i = Number(row.dataset.i);
    const m = p.mappings[i];
    const input = el as HTMLInputElement;
    const num = (lo: number, hi: number) => clamp(Math.round(Number(input.value) || 0), lo, hi);
    switch (f) {
      case "enabled":
        m.enabled = input.checked;
        row.classList.toggle("off", !m.enabled);
        break;
      case "source":
        m.source = input.value;
        break;
      case "address":
        m.address = input.value.trim();
        input.classList.toggle("bad", !/^\/\S+$/.test(m.address));
        break;
      case "value":
        m.value = input.value as OscMapping["value"];
        break;
      case "mode":
        m.mode = input.value as OscMapping["mode"];
        this.rerenderRow(i);
        break;
      case "args":
        m.args = parseArgs(input.value);
        break;
      case "out0":
      case "out1":
        if (input.value === "" || !Number.isFinite(Number(input.value))) return;
        m.out[f === "out0" ? 0 : 1] = Number(input.value);
        break;
      case "range0":
      case "range1":
        if (input.value === "") return;
        m.range[f === "range0" ? 0 : 1] = num(0, 100) / 100;
        break;
      case "deadzone":
        if (input.value === "") return;
        m.deadzone = num(0, 90) / 100;
        break;
      case "curve":
        m.curve = num(-100, 100) / 100;
        break;
      case "smoothing":
        m.smoothing = num(0, 95) / 100;
        break;
      case "invert":
        m.invert = input.checked;
        break;
      case "toggle":
        m.toggle = input.checked;
        break;
      case "hold":
        m.hold = input.value || null;
        break;
      default:
        return;
    }
    this.push(true);
  }

  private rerenderRow(i: number): void {
    const row = this.root.querySelector<HTMLElement>(`.osc .map-row[data-i="${i}"]`);
    const m = this.preset()?.mappings[i];
    if (!row || !m) return;
    row.outerHTML = this.mapRow(m, i);
    this.setSelects(m, i);
    this.showAddressEnds();
  }

  private mapClicked(el: Element): void {
    const row = el.closest<HTMLElement>(".map-row");
    const p = this.preset();
    if (!row || !p) return;
    const i = Number(row.dataset.i);
    if (el.closest("[data-del]")) {
      this.stopLearn(true);
      p.mappings.splice(i, 1);
      this.open = new Set([...this.open].filter((n) => n !== i).map((n) => (n > i ? n - 1 : n)));
      this.renderMaps();
      this.push(true);
    } else if (el.closest("[data-more]")) {
      if (this.open.has(i)) this.open.delete(i);
      else this.open.add(i);
      row.querySelector<HTMLElement>(".map-more")!.hidden = !this.open.has(i);
      row.querySelector("[data-more]")!.classList.toggle("on", this.open.has(i));
    } else if (el.closest("[data-learn]")) {
      if (this.learnRow === i) this.stopLearn(true);
      else this.startLearn(i);
    }
  }

  private async startLearn(i: number): Promise<void> {
    const prev = this.learnRow;
    this.learnRow = i;
    this.learnSeen = false;
    if (prev !== null) this.rerenderRow(prev);
    this.rerenderRow(i);
    await invoke("osc_learn", { on: true });
  }

  private stopLearn(cancel: boolean): void {
    if (this.learnRow === null) return;
    const i = this.learnRow;
    this.learnRow = null;
    if (cancel) invoke("osc_learn", { on: false });
    this.rerenderRow(i);
  }

  private addMapping(): void {
    const p = this.preset();
    if (!p) return;
    p.mappings.push({
      enabled: true, source: "trigger_right", range: [0, 1], invert: false, deadzone: 0, curve: 0, smoothing: 0,
      toggle: false, hold: null, address: `/value/${p.mappings.length + 1}`, args: [], value: "float", out: [0, 1], mode: "continuous",
    });
    this.renderMaps();
    this.push(true);
    const list = this.q("[data-maps]");
    list.scrollTop = list.scrollHeight;
    // Learn right away: the next control moved becomes the input.
    this.startLearn(p.mappings.length - 1);
  }

  // ---------- Setup guide ----------

  private showGuide(on: boolean): void {
    const panel = this.q("[data-guide-panel]");
    if (on) {
      panel.innerHTML = this.guideHtml();
      this.renderGuideApp();
      markGuideSeen();
    }
    panel.hidden = !on;
    this.q("[data-guide]").classList.toggle("on", on);
  }

  private guideHtml(): string {
    const current = this.preset()?.name ?? "";
    const apps = Object.keys(APPS);
    const app = apps.includes(current) ? current : "Generic";
    return `
      <div class="guide-head">
        <h2>Send OSC to your app</h2>
        <button class="icon-btn" data-guide-close title="Close" aria-label="Close">×</button>
      </div>
      <div class="guide-steps">
        <section><h4><i>1</i>Pick your app</h4>
          <p class="guide-app-pick"><select class="field" data-guide-app>${apps.map((a) => `<option${a === app ? " selected" : ""}>${escape(a)}</option>`).join("")}</select>
            <button class="mini guide-btn" data-guide-use title="Load this app's preset (and its port) into the selected destination">Use this preset</button></p>
          <div class="guide-app" data-guide-app-steps></div></section>
        <section><h4><i>2</i>Point a destination at it</h4>
          <p>Each <b>destination</b> is one app: its <b>host</b>, <b>port</b> and preset. Tick it to start sending; the dot
            turns <span class="ok-text">green</span>.</p>
          <ul><li>App on this computer: host <code>127.0.0.1</code>.</li>
            <li>On another computer: that computer's IP address (same network). Its firewall must let UDP in on the port.</li>
            <li>The port must match the one the app listens on. Picking an app's preset fills in its default.</li>
            <li>Send to several apps at once with <b>+ Add</b>.</li></ul></section>
        <section><h4><i>3</i>Check it arrives</h4>
          <p><b>Addresses</b> lists everything the selected destination sends, with live values; click one to copy it.
            Most apps also have an OSC monitor or "learn" that shows what comes in.</p>
          <p><b>Stream</b> presets send every input under <code>/sc</code>, for apps you program. App presets map inputs to
            addresses the app knows; change them under <b>Mappings</b>.</p></section>
        <section><h4><i>4</i>If nothing happens</h4>
          <ul><li><b>Addresses</b> stays empty: check the controller is connected (top right) and the stream or a mapping is on.</li>
            <li>Values move but the app doesn't react: the port doesn't match, the app's OSC input is off, or (another
              computer) its firewall blocks it.</li>
            <li>"Address already in use" in the app: another program has that port; pick another on both sides.</li>
            <li>The app reads single messages only: untick <b>Bundles</b>.</li>
            <li>Steam is running: its Desktop Layout may move your mouse too (see the warning at the bottom of the window).</li></ul></section>
      </div>`;
  }

  private renderGuideApp(): void {
    const name = this.q<HTMLSelectElement>("[data-guide-app]").value;
    const info = this.presets.find((p) => p.builtin && p.name === name);
    this.q("[data-guide-app-steps]").innerHTML =
      (APPS[name] ?? "") + (info?.port ? `<p class="muted">Preset port: <b>${info.port}</b></p>` : "");
  }
}
