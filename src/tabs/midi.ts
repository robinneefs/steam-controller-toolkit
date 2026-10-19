import { invoke } from "@tauri-apps/api/core";
import type { MidiMapping, MidiOutputChoice, MidiPreset, MidiScale, MidiTarget, PadGrid, Snapshot } from "../controller";
import { addHelp, helpIcon } from "../help";
import type { Tab } from "./tab";

addHelp({
  "midi-output": `<b>Output</b> is the MIDI port the controller's messages go to. In your DAW, pick the same port
    as a MIDI input. The dot turns green once messages are being sent.<br><br>
    <b>Steam Controller</b> is the app's own port: nothing to install. On macOS (CoreMIDI) and Linux (ALSA) it always
    works. On Windows it needs Windows 11 25H2 or later and Windows MIDI Services; until Microsoft's MIDI update
    (expected late November 2026) it's marked <b>experimental</b> there, because Windows makes it only once per
    restart, and a loopback port is the dependable choice. The <a data-open-guide>Setup guide</a> shows how.`,
  "midi-preset": `<b>Presets</b> are complete sets of mappings and pad grids.
    <ul><li><b>Effects + drums</b>: sticks, triggers, left pad and tilt to CC 20–29; face buttons and D-pad play drums on channel 10.</li>
    <li><b>Pad instrument</b>: both pads play notes, triggers are mod wheel and expression, bumpers are sustain.</li>
    <li><b>Effects only</b>: every analog input to its own CC.</li></ul>
    Changes apply right away. <b>Save as…</b> keeps them; <b>Folder</b> shows the .json files so you can share them.`,
  "midi-maps": `Each <b>mapping</b> turns one input into one MIDI message.
    <ul><li><b>Input</b>: what you move or press. Click <b>◎</b> (Learn), then move the control.</li>
    <li><b>Sends</b>: the message (CC, pitch bend, aftertouch, note or program change) and its number.</li>
    <li><b>Ch</b>: MIDI channel 1–16.</li>
    <li><b>⋯</b>: range, deadzone, curve, smoothing, invert, toggle and an "only while held" button.</li></ul>
    The blue bar under a row shows the value being sent. Untick a row to pause it.`,
  "midi-input": `What drives the mapping. Sticks, pads and tilt rest in the middle; triggers, pressure and buttons
    rest at 0. Pad X/Y only send while the pad is touched.<br><br>
    <b>◎ Learn</b>: click it, then move or press the control you want. Touch sensors and turn speed can't be learned; pick them from the list.`,
  "midi-sends": `<ul><li><b>CC</b> (control change): for effect knobs and faders. Use your DAW's MIDI learn / MIDI map on a knob,
    then move the control. <b>14-bit</b> gives smoother sweeps (CC 0–31) if the DAW supports it.</li>
    <li><b>Pitch bend</b>: the middle of the input is no bend.</li>
    <li><b>Aftertouch</b>: channel pressure, often vibrato or filter in synths.</li>
    <li><b>Note</b>: plays while the input is past halfway, at the velocity you set.</li>
    <li><b>Program</b>: a program change on each press, to switch sounds on instruments that support it.</li></ul>`,
  "midi-channel": `MIDI channel 1–16. An instrument track in the DAW can listen to one channel, so give each part its own
    channel (say pads on 1, drums on 10) to play different instruments at once.`,
  "midi-shaping": `<ul><li><b>Range</b>: the part of the input's travel that sweeps the whole output; e.g. 50–100% uses only stick-up.</li>
    <li><b>Deadzone</b>: travel at rest that is ignored, so a stick that doesn't centre perfectly stays still.</li>
    <li><b>Curve</b>: left reacts fast at first; right starts slow for fine control.</li>
    <li><b>Smooth</b>: evens out shaky input like tilt, at the cost of a little lag.</li>
    <li><b>Invert</b>: flips the output.</li>
    <li><b>Toggle</b>: each press flips between off and on (a latch, e.g. sustain or an effect on/off).</li>
    <li><b>Active</b>: only send while a button is held, e.g. tilt only while R4 is down, so moving the controller
      doesn't change anything by accident.</li></ul>`,
  "midi-grid": `Play a <b>trackpad as a grid of notes</b>. The grid on the left mirrors the pad: bottom-left is the root note.
    <ul><li><b>Scale</b> keeps every cell in key. <b>Row step</b> is how many scale notes each row goes up.</li>
    <li><b>Play on</b>: Touch plays as soon as your thumb lands; Click waits until you press the pad down.</li>
    <li><b>Slide</b>: sliding onto another cell plays it, like a glissando.</li>
    <li><b>Pressure</b>: how hard you press, sent as aftertouch while the note holds.</li></ul>
    The sound comes from the instrument listening to this <b>channel</b> in your DAW.`,
  "midi-activity": `The latest MIDI messages sent, newest on top, with their channel. If this list moves but the DAW does
    nothing, the port isn't enabled as a MIDI input there (see the <a data-open-guide>Setup guide</a>).
    <b>Panic</b> stops notes that keep sounding.`,
});

const GUIDE_SEEN = "midi-guide-seen";
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

/** Buttons by their names in protocol.rs `buttons::NAMES`, with friendlier labels. */
const BUTTONS: [string, string][] = [
  ["A", "A"], ["B", "B"], ["X", "X"], ["Y", "Y"],
  ["Up", "D-pad up"], ["Down", "D-pad down"], ["Left", "D-pad left"], ["Right", "D-pad right"],
  ["L", "Left bumper"], ["R", "Right bumper"], ["LTrigClick", "Left trigger click"], ["RTrigClick", "Right trigger click"],
  ["L4", "L4"], ["L5", "L5"], ["R4", "R4"], ["R5", "R5"],
  ["L3", "L3 (stick click)"], ["R3", "R3 (stick click)"], ["LPadClick", "Left pad click"], ["RPadClick", "Right pad click"],
  ["LPadTouch", "Left pad touch"], ["RPadTouch", "Right pad touch"], ["LStickTouch", "Left stick touch"],
  ["RStickTouch", "Right stick touch"], ["LGrip", "Left grip touch"], ["RGrip", "Right grip touch"],
  ["View", "View"], ["Menu", "Menu"], ["QAM", "Quick access"], ["Steam", "Steam"],
];

/** Every input a mapping can read. Axis keys mirror `AXES` in src-tauri/src/midi.rs. */
const SOURCES: [string, [string, string][]][] = [
  ["Triggers", [["trigger_left", "Left trigger"], ["trigger_right", "Right trigger"]]],
  ["Sticks", [
    ["left_stick_x", "Left stick X"], ["left_stick_y", "Left stick Y"],
    ["right_stick_x", "Right stick X"], ["right_stick_y", "Right stick Y"],
  ]],
  ["Trackpads", [
    ["left_pad_x", "Left pad X"], ["left_pad_y", "Left pad Y"], ["left_pad_pressure", "Left pad pressure"],
    ["right_pad_x", "Right pad X"], ["right_pad_y", "Right pad Y"], ["right_pad_pressure", "Right pad pressure"],
  ]],
  ["Motion", [
    ["tilt_pitch", "Tilt forward / back"], ["tilt_roll", "Tilt left / right"],
    ["gyro_pitch", "Turn speed: pitch"], ["gyro_roll", "Turn speed: roll"], ["gyro_yaw", "Turn speed: yaw"],
  ]],
  ["Buttons", BUTTONS.map(([n, l]) => [`button:${n}`, l])],
];
const SOURCE_LABELS = new Map(SOURCES.flatMap(([, list]) => list));

const KINDS: [MidiTarget["kind"], string, string][] = [
  ["cc", "CC", "Control change: knobs and faders in the DAW (MIDI-learn it there)"],
  ["pitch_bend", "Pitch bend", "Pitch bend; the middle of the input is no bend"],
  ["channel_pressure", "Aftertouch", "Channel pressure (aftertouch)"],
  ["note", "Note", "Note on while the input is past half, note off below"],
  ["program", "Program", "Program change each time the input goes past half"],
];

const SCALES: [MidiScale, string][] = [
  ["major_pentatonic", "Major pentatonic"], ["minor_pentatonic", "Minor pentatonic"],
  ["major", "Major"], ["minor", "Minor"], ["dorian", "Dorian"], ["harmonic_minor", "Harmonic minor"],
  ["blues", "Blues"], ["chromatic", "Chromatic"],
];
const SCALE_STEPS: Record<MidiScale, number[]> = {
  chromatic: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
  major: [0, 2, 4, 5, 7, 9, 11],
  minor: [0, 2, 3, 5, 7, 8, 10],
  major_pentatonic: [0, 2, 4, 7, 9],
  minor_pentatonic: [0, 3, 5, 7, 10],
  blues: [0, 3, 5, 6, 7, 10],
  dorian: [0, 2, 3, 5, 7, 9, 10],
  harmonic_minor: [0, 2, 3, 5, 7, 8, 11],
};

const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
/** Note name with middle C (60) as C4. DAWs differ: Ableton calls 60 C3. */
const noteName = (n: number) => `${NOTE_NAMES[n % 12]}${Math.floor(n / 12) - 1}`;
const NOTE_OPTIONS = Array.from({ length: 128 }, (_, n) => `<option value="${n}">${noteName(n)} · ${n}</option>`).join("");
const CHANNEL_OPTIONS = Array.from({ length: 16 }, (_, i) => `<option value="${i + 1}">${i + 1}</option>`).join("");

/** Note of a pad grid cell; mirrors `PadGrid::note` in midi.rs. */
function gridNote(g: PadGrid, cell: number): number | null {
  const cols = Math.max(1, g.cols);
  const degree = Math.floor(cell / cols) * g.row_step + (cell % cols);
  const steps = SCALE_STEPS[g.scale];
  const n = g.root + 12 * Math.floor(degree / steps.length) + steps[degree % steps.length];
  return n <= 127 ? n : null;
}

const escape = (s: string) => s.replace(/[&<>"]/g, (c) => `&${{ "&": "amp", "<": "lt", ">": "gt", '"': "quot" }[c]};`);
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** One MIDI message as text, e.g. "CC 21 = 64". */
function describe(b: number[]): string {
  const [s, a, v] = b;
  switch (s & 0xf0) {
    case 0x90: return `Note on ${noteName(a)} · vel ${v}`;
    case 0x80: return `Note off ${noteName(a)}`;
    case 0xa0: return `Poly pressure ${noteName(a)} = ${v}`;
    case 0xb0: return `CC ${a} = ${v}`;
    case 0xc0: return `Program ${a}`;
    case 0xd0: return `Aftertouch = ${a}`;
    case 0xe0: return `Pitch bend ${((v << 7) | a) - 8192}`;
    default: return b.map((x) => x.toString(16).padStart(2, "0")).join(" ");
  }
}

function defaultTarget(kind: MidiTarget["kind"], cc: number): MidiTarget {
  switch (kind) {
    case "cc": return { kind, cc, hi_res: false };
    case "note": return { kind, note: 60, velocity: 100 };
    case "program": return { kind, program: 0 };
    default: return { kind };
  }
}

interface PresetInfo {
  name: string;
  builtin: boolean;
}

const encodeOutput = (o: MidiOutputChoice) => (o.kind === "port" ? `port:${o.name}` : o.kind);
function decodeOutput(v: string): MidiOutputChoice {
  return v.startsWith("port:") ? { kind: "port", name: v.slice(5) } : { kind: v as "off" | "virtual" };
}

/** Which system the app runs on, for the Setup guide. */
type Os = "windows" | "mac" | "linux";
const UA = navigator.userAgent;
const OS: Os = UA.includes("Windows") ? "windows" : UA.includes("Mac") ? "mac" : "linux";
const OS_NAMES: Record<Os, string> = { windows: "Windows", mac: "macOS", linux: "Linux" };
const EXPERIMENTAL = `Experimental on this Windows: until Microsoft's MIDI update (expected late November 2026), Windows can make this
       port only once per restart. If it says the MIDI service isn't responding, restart the PC or use a loopback port.
       It uses Windows MIDI Services, which Microsoft still calls a preview.`;

/** What this PC can do for our own port: what the backend's `MidiPorts` says. */
interface VirtualSupport {
  available: boolean;
  experimental: boolean;
  note: string | null;
}

/** DAWs and where they turn a MIDI input on, by the systems they run on. */
const DAWS: [string, Os[], string][] = [
  ["Ableton Live", ["windows", "mac"], "Settings → Link, Tempo &amp; MIDI → for the port's input, turn on <b>Track</b> (notes) and <b>Remote</b> (knob mapping)."],
  ["FL Studio", ["windows", "mac"], "Options → MIDI settings → select the port under Input and click <b>Enable</b>."],
  ["Bitwig Studio", ["windows", "mac", "linux"], "Settings → Controllers → <b>Add controller</b> → Generic → MIDI Keyboard, then pick the port."],
  ["Reaper", ["windows", "mac", "linux"], "Preferences → MIDI Inputs (MIDI Devices) → enable the port."],
  ["Logic Pro and GarageBand", ["mac"], "new MIDI inputs are used automatically."],
  ["Cubase", ["windows", "mac"], "Studio → Studio Setup → MIDI Port Setup → make sure the port is active and \"In All MIDI Inputs\"."],
  ["Ardour", ["linux", "mac"], "Window → MIDI Connections, or pick the port as the track's input."],
];

/** Step 1 per system: our own virtual port where this PC can have one, else a loopback port. */
function portStep(v: VirtualSupport, ports: string[]): string {
  if (OS === "mac") {
    return `<p>Pick <b>Steam Controller (virtual port)</b> under <b>Output</b> (it's the default). The app makes this
      CoreMIDI port itself, so there's nothing to install; DAWs list it as a MIDI input while the app runs.</p>
      <p class="muted">macOS may ask to allow <b>Input Monitoring</b> the first time; that's for reading the controller.</p>`;
  }
  if (OS === "linux") {
    return `<p>Pick <b>Steam Controller (virtual port)</b> under <b>Output</b> (it's the default). The app makes an
      <b>ALSA MIDI</b> port while it runs; nothing to install. <code>aconnect -l</code> lists it.</p>
      <p>On <b>PipeWire</b> (most current distros) it also shows up for JACK apps. On plain <b>JACK</b>, bridge ALSA MIDI
      with <code>a2jmidid -e</code> (or your JACK setup's ALSA MIDI option).</p>`;
  }
  if (v.available && !v.experimental) {
    return `<p>Pick <b>Steam Controller (virtual port)</b> under <b>Output</b> (it's the default). The app makes this port
      itself with Windows MIDI Services, so there's nothing to install; it goes away when the app closes.</p>`;
  }
  const loopback = ports.filter((p) => !/wavetable|microsoft gs/i.test(p));
  const loopbackSteps = `<ol class="port-steps">
      <li>Install the free <b>loopMIDI</b> (tobias-erichsen.de) and click <b>+</b> to add a port. Keep it running; it can start with Windows.
        <span class="muted">Or, with Windows MIDI Services: open <b>MIDI Settings</b> → <b>Loopback endpoints</b> and create one.</span></li>
      <li><button class="mini" data-guide-refresh>↻ Look for ports</button>
        ${loopback.length ? `Found: <b>${loopback.map(escape).join("</b>, <b>")}</b>.` : `<span class="warn-text">No loopback port found yet.</span>`}</li>
      <li>Pick it under <b>Output</b>, and as the MIDI input in your DAW.</li>
    </ol>
    <p class="muted">Skip <i>Microsoft GS Wavetable Synth</i>: it plays Windows' built-in sounds instead of reaching your DAW.</p>`;
  if (!v.experimental) {
    return `<p>This PC can't give the app its own MIDI port${v.note ? ` (${escape(v.note.replace(/\.$/, ""))})` : ""},
      so make a <b>loopback port</b> once. It passes MIDI from this app to your DAW:</p>${loopbackSteps}`;
  }
  return `<div class="port-options">
      <div class="port-option">
        <div class="port-option-head"><b>A. The app's own port</b><span class="badge exp">Experimental</span></div>
        <p>Nothing to install: pick <b>Steam Controller (experimental)</b> under <b>Output</b>, then pick it as the MIDI
          input in your DAW.</p>
        <div class="callout">
          <b>Why experimental?</b> It runs on Windows MIDI Services, which Microsoft still calls a preview. Until their fix,
          Windows can make this port only <b>once per restart</b>: it works the first time you open the app after starting
          the PC, but after closing and reopening the app the MIDI service may stop responding. Restarting the PC brings it back.
        </div>
        <div class="callout fix">
          <b>A fix is coming.</b> Microsoft's MIDI update (expected late November 2026) puts Windows MIDI Services in Windows
          itself. The app then uses it automatically: the port works every time, becomes the default and loses the
          experimental label.
        </div>
      </div>
      <div class="port-option">
        <div class="port-option-head"><b>B. A loopback port</b><span class="badge sure">Always works</span></div>
        <p>If the experimental port doesn't work for you (the MIDI service isn't responding, or your DAW doesn't list
          it), make a loopback port once. It passes MIDI from this app to your DAW:</p>
        ${loopbackSteps}
      </div>
    </div>`;
}

/** Problems worth a line, per system. */
const TROUBLE: Record<Os, string> = {
  windows: `<li>"The Windows MIDI service isn't responding": restart the PC, or use a loopback port (step 1).</li>`,
  mac: `<li>No controller found: allow the app under System Settings → Privacy &amp; Security → <b>Input Monitoring</b>.</li>`,
  linux: `<li>No port appears: the ALSA sequencer may be off; <code>sudo modprobe snd-seq</code> turns it on.</li>
    <li>No controller found: your user needs access to <code>hidraw</code> (Steam's udev rules give it).</li>`,
};

/** Step-by-step setup, for the system the app runs on. */
function guideHtml(v: VirtualSupport, ports: string[]): string {
  const daws = DAWS.filter(([, systems]) => systems.includes(OS))
    .map(([name, , how]) => `<li><b>${name}</b>: ${how}</li>`)
    .join("");
  return `
    <div class="guide-head">
      <h2>Connect to your DAW <span class="guide-os">${OS_NAMES[OS]}</span></h2>
      <button class="icon-btn" data-guide-close title="Close" aria-label="Close">×</button>
    </div>
    <div class="guide-steps">
      <section${OS === "windows" && v.experimental ? ` class="wide"` : ""}><h4><i>1</i>Choose where MIDI goes</h4>${portStep(v, ports)}
        <p>The dot next to the output turns <span class="ok-text">green</span> when it's connected.</p></section>
      <section><h4><i>2</i>Turn the port on in your DAW</h4>
        <p>Enable it as a MIDI <b>input</b>. Menu names vary a little between versions:</p>
        <ul>${daws}</ul></section>
      <section><h4><i>3</i>Play notes</h4>
        <p>Make an <b>instrument track</b> (synth, piano, drum rack), set its MIDI input to the port (or All inputs) and arm or
        monitor it. <b>Pad notes</b> and <b>Note</b> mappings now play it; the instrument decides the sound.</p>
        <p>To play several instruments at once, give each part its own <b>channel</b> (e.g. right pad 1, left pad 2, drums 10)
        and set each instrument track to listen to that channel.</p></section>
      <section><h4><i>4</i>Control effects</h4>
        <p>Use the DAW's <b>MIDI learn / MIDI map</b> (Ableton: Ctrl/Cmd+M; FL Studio: right-click a knob → Link to controller;
        Bitwig, Reaper: right-click → Learn). Click a knob, then move the control that is mapped to a <b>CC</b> here.</p>
        <p>Tip: the DAW learns the first CC it sees. While learning, untick the other mappings so a stick at rest
        doesn't get picked instead.</p></section>
      <section><h4><i>5</i>If nothing happens</h4>
        <ul><li><b>Activity</b> stays empty: check the controller is connected (top right) and the mapping is ticked.</li>
        <li>Activity moves but the DAW doesn't react: the port isn't enabled as an input there (step 2), or the track isn't armed.</li>
        <li>Notes keep sounding: press <b>Panic</b>.</li>
        ${TROUBLE[OS]}
        <li>Steam is running: its Desktop Layout may move your mouse too (see the warning at the bottom of the window).</li></ul></section>
    </div>`;
}

/** Send controller input to a DAW as MIDI: mappings for effects, note grids on the trackpads. */
export class MidiTab implements Tab {
  id = "midi";
  title = "MIDI";
  private root!: HTMLElement;
  private preset: MidiPreset | null = null;
  private output: MidiOutputChoice = { kind: "off" };
  private ports: string[] = [];
  private virtual: VirtualSupport = { available: false, experimental: false, note: null };
  private presets: PresetInfo[] = [];
  /** The saved or built-in preset the editor started from; `null` once it no longer matches one. */
  private loaded: PresetInfo | null = null;
  private edited = false;
  private pushTimer: number | undefined;
  private side: "left_pad" | "right_pad" = "right_pad";
  /** Row waiting for learn, and whether the backend has reported it is listening. */
  private learnRow: number | null = null;
  private learnSeen = false;
  private open = new Set<number>();
  private lastLog = -1;
  private lastStatus = "";
  private deleteArmed = false;
  private armTimer: number | undefined;

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="midi">
        <section class="tile midi-bar">
          <span class="bar-group">
            <span class="muted">Output</span>${helpIcon("midi-output")}
            <select class="field" data-output></select>
          </span>
          <button class="icon-btn" data-refresh title="Look for MIDI ports again" aria-label="Look for MIDI ports again">↻</button>
          <button class="mini guide-btn" data-guide title="How to connect the controller to your DAW">Setup guide</button>
          <span class="midi-conn" data-conn></span>
          <span class="bar-group preset-group">
            <span class="muted">Preset</span>${helpIcon("midi-preset")}
            <select class="field" data-preset></select>
            <input class="field" data-save-name placeholder="Preset name" maxlength="60" hidden />
            <button class="mini" data-save title="Save the current mappings as a preset">Save as…</button>
            <button class="mini" data-save-cancel hidden>Cancel</button>
            <button class="mini danger" data-delete title="Delete this saved preset">Delete</button>
            <button class="mini" data-folder title="Open the presets folder (share presets by copying the .json files)">Folder</button>
          </span>
          <button class="mini panic" data-panic title="Stop every sounding note on every channel">Panic</button>
        </section>

        <section class="tile midi-maps">
          <h3><span class="h3-title">Mappings ${helpIcon("midi-maps")}</span> <span class="head-actions"><em data-map-count></em><button class="mini" data-add>+ Add</button></span></h3>
          <div class="map-head"><span></span><span>Input ${helpIcon("midi-input")}</span><span></span><span>Sends ${helpIcon("midi-sends")}</span><span></span><span>Ch ${helpIcon("midi-channel")}</span><span></span><span></span></div>
          <div class="map-list" data-maps></div>
        </section>

        <div class="midi-side">
          <section class="tile midi-grid">
            <h3><span class="h3-title">Pad notes ${helpIcon("midi-grid")}</span>
              <span class="segmented" data-sides>
                <button data-side="left_pad">Left pad</button><button data-side="right_pad">Right pad</button>
              </span>
            </h3>
            <div data-grid></div>
          </section>
          <section class="tile midi-log">
            <h3><span class="h3-title">Activity ${helpIcon("midi-activity")}</span> <em class="num" data-rate></em></h3>
            <div class="log" data-log></div>
          </section>
        </div>
        <div class="midi-guide" data-guide-panel hidden></div>
      </div>`;

    this.q("[data-output]").addEventListener("change", (e) => this.setOutput(decodeOutput((e.target as HTMLSelectElement).value)));
    this.q("[data-refresh]").addEventListener("click", () => this.refreshPorts());
    this.q("[data-guide]").addEventListener("click", () => this.showGuide(this.q("[data-guide-panel]").hidden !== false));
    // "Setup guide" links in hints and help tips (the tips live on <body>, outside this tab).
    document.addEventListener("click", (e) => {
      if ((e.target as Element).closest?.("[data-open-guide]")) this.showGuide(true);
    });
    this.q("[data-guide-panel]").addEventListener("click", (e) => {
      const el = e.target as Element;
      if (el.closest("[data-guide-close]")) this.showGuide(false);
      if (el.closest("[data-guide-refresh]")) this.refreshPorts().then(() => this.showGuide(true));
    });
    this.q("[data-panic]").addEventListener("click", () => invoke("midi_panic"));
    this.q("[data-preset]").addEventListener("change", (e) => this.loadPreset((e.target as HTMLSelectElement).value));
    this.q("[data-save]").addEventListener("click", () => this.saveClicked());
    this.q("[data-save-cancel]").addEventListener("click", () => this.showSaveName(false));
    this.q<HTMLInputElement>("[data-save-name]").addEventListener("keydown", (e) => {
      if (e.key === "Enter") this.saveClicked();
      if (e.key === "Escape") this.showSaveName(false);
    });
    this.q("[data-delete]").addEventListener("click", () => this.deleteClicked());
    this.q("[data-folder]").addEventListener("click", () => invoke("midi_reveal_presets").catch((e) => this.flash(String(e))));
    this.q("[data-add]").addEventListener("click", () => this.addMapping());
    this.q("[data-sides]").addEventListener("click", (e) => {
      const b = (e.target as Element).closest<HTMLElement>("[data-side]");
      if (!b) return;
      this.side = b.dataset.side as "left_pad" | "right_pad";
      this.renderGrid();
    });

    const list = this.q("[data-maps]");
    list.addEventListener("change", (e) => this.mapFieldChanged(e.target as HTMLElement));
    list.addEventListener("input", (e) => {
      const el = e.target as HTMLInputElement;
      if (el.type === "range" || el.type === "number") this.mapFieldChanged(el);
    });
    list.addEventListener("click", (e) => this.mapClicked(e.target as Element));

    const grid = this.q("[data-grid]");
    grid.addEventListener("change", (e) => this.gridFieldChanged(e.target as HTMLElement));
    grid.addEventListener("click", (e) => {
      const b = (e.target as Element).closest<HTMLElement>("[data-trigger]");
      if (b) this.setGrid({ trigger: b.dataset.trigger as "touch" | "click" }, true);
    });

    this.init();
  }

  private async init(): Promise<void> {
    try {
      const setup = await invoke<{ preset: MidiPreset; output: MidiOutputChoice }>("midi_setup");
      this.preset = setup.preset;
      this.output = setup.output;
      await Promise.all([this.refreshPorts(), this.refreshPresets()]);
      this.loaded = this.presets.find((p) => p.name === this.preset!.name && !p.builtin) ??
        this.presets.find((p) => p.name === this.preset!.name) ?? null;
      this.renderPresets();
      this.renderMaps();
      this.renderGrid();
      // First visit with nothing set up: start with the guide.
      if (this.output.kind === "off" && !seenGuide()) this.showGuide(true);
    } catch (e) {
      this.flash(String(e));
    }
  }

  update(snap: Snapshot): void {
    const m = snap.midi;
    if (!m || !this.preset) return;
    // The backend can change the output itself (first run on a PC without our own port).
    if (encodeOutput(m.output) !== encodeOutput(this.output)) {
      this.output = m.output;
      this.renderOutput();
      this.lastLog = -1;
    }
    // Connection status.
    const text = m.connected
      ? `Sending to ${m.connected}`
      : m.connecting
        ? "Opening the port…"
        : (m.error ?? (this.output.kind === "off" ? "Not sending: pick an output" : "Not sending"));
    const conn = m.connected
      ? `<i class="ok"></i>Sending to <b>${escape(m.connected)}</b>`
      : m.connecting
        ? `<i></i>${text}`
        : m.error
          ? `<i class="warn"></i><span>${escape(m.error)}</span>`
          : `<i></i>Not sending`;
    if (conn !== this.lastStatus) {
      const el = this.q("[data-conn]");
      el.innerHTML = this.lastStatus = conn;
      // The text gets cut short in narrow windows; the full status is on hover.
      el.title = text;
    }
    const rate = m.connected ? `${m.rate} msg/s` : "";
    const rateEl = this.q("[data-rate]");
    if (rateEl.textContent !== rate) rateEl.textContent = rate;

    // Meters.
    this.root.querySelectorAll<HTMLElement>(".map-row").forEach((row) => {
      const level = m.levels[Number(row.dataset.i)];
      const bar = row.querySelector<HTMLElement>(".meter i")!;
      const w = level == null ? "0%" : `${(level * 100).toFixed(1)}%`;
      if (bar.style.width !== w) bar.style.width = w;
      row.classList.toggle("idle", level == null);
    });

    // Pad grid.
    const cell = m.cells[this.side === "left_pad" ? 0 : 1];
    this.root.querySelectorAll<HTMLElement>(".grid-cells [data-cell]").forEach((el) => {
      const on = cell?.[0] === Number(el.dataset.cell);
      el.classList.toggle("hover", on && !cell![1]);
      el.classList.toggle("on", on && cell![1]);
    });

    // Activity.
    const last = m.log.length ? m.log[m.log.length - 1][0] : 0;
    if (last !== this.lastLog) {
      this.lastLog = last;
      this.q("[data-log]").innerHTML = m.log.length
        ? m.log.slice().reverse().map(([, b]) => `<div><span class="ch">ch ${(b[0] & 0x0f) + 1}</span>${describe(b)}</div>`).join("")
        : `<p class="hint">${this.setupHint()}</p>`;
    }

    // Learn.
    if (this.learnRow !== null) {
      if (m.learning) this.learnSeen = true;
      else if (this.learnSeen && m.learned) {
        const i = this.learnRow;
        this.stopLearn(false);
        this.preset.mappings[i].source = m.learned;
        this.renderMaps();
        this.push();
      }
    }
  }

  hide(): void {
    this.stopLearn(true);
  }

  private setupHint(): string {
    if (this.output.kind === "virtual" && this.virtual.experimental) {
      return `Nothing sent yet. Move a mapped control.<br><br>${EXPERIMENTAL}`;
    }
    if (this.output.kind !== "off") return "Nothing sent yet. Move a mapped control.";
    return this.virtual.available && !this.virtual.experimental
      ? `Pick <b>Steam Controller (virtual port)</b> as the output, then choose it as a MIDI input in your DAW.
         <a data-open-guide>Setup guide</a>`
      : `This PC needs a loopback MIDI port: create one with loopMIDI (or in Windows MIDI Settings → Loopback endpoints),
         press ↻, pick it as the output here and as a MIDI input in your DAW.
         <a data-open-guide>Setup guide</a>`;
  }

  private showGuide(on: boolean): void {
    const panel = this.q("[data-guide-panel]");
    if (on) {
      panel.innerHTML = guideHtml(this.virtual, this.ports);
      markGuideSeen();
    }
    panel.hidden = !on;
    this.q("[data-guide]").classList.toggle("on", on);
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector<T>(sel)!;
  }

  private flash(text: string): void {
    const el = this.q("[data-log]");
    el.innerHTML = `<p class="hint error">${escape(text)}</p>`;
    this.lastLog = -1;
  }

  // ---------- Output ----------

  private async refreshPorts(): Promise<void> {
    try {
      const r = await invoke<{ ports: string[]; virtual_port: boolean; virtual_experimental: boolean; virtual_note: string | null }>("midi_ports");
      this.ports = r.ports;
      this.virtual = { available: r.virtual_port, experimental: r.virtual_experimental, note: r.virtual_note };
    } catch (e) {
      this.ports = [];
      this.flash(String(e));
    }
    this.renderOutput();
  }

  private renderOutput(): void {
    const opts: [string, string][] = [["off", "Off"]];
    if (this.virtual.available) {
      opts.push(["virtual", this.virtual.experimental ? "Steam Controller (experimental)" : "Steam Controller (virtual port)"]);
    }
    for (const p of this.ports) opts.push([`port:${p}`, p]);
    const current = encodeOutput(this.output);
    if (!opts.some(([v]) => v === current)) {
      opts.push([current, this.output.kind === "port" ? `${this.output.name} (not found)` : "Steam Controller (not available)"]);
    }
    const sel = this.q<HTMLSelectElement>("[data-output]");
    sel.innerHTML = opts.map(([v, l]) => `<option value="${escape(v)}">${escape(l)}</option>`).join("");
    sel.value = current;
  }

  private async setOutput(o: MidiOutputChoice): Promise<void> {
    this.output = o;
    this.lastLog = -1;
    try {
      await invoke("midi_set_output", { output: o });
    } catch (e) {
      this.flash(String(e));
    }
  }

  // ---------- Presets ----------

  private async refreshPresets(): Promise<void> {
    this.presets = await invoke<PresetInfo[]>("midi_list_presets");
  }

  private renderPresets(): void {
    const opt = (p: PresetInfo) => `<option value="${p.builtin ? "b" : "s"}:${escape(p.name)}">${escape(p.name)}</option>`;
    const builtin = this.presets.filter((p) => p.builtin);
    const saved = this.presets.filter((p) => !p.builtin);
    const sel = this.q<HTMLSelectElement>("[data-preset]");
    const unsaved = !this.loaded ? `<option value="">${escape(this.preset?.name || "Custom")} (unsaved)</option>` : "";
    sel.innerHTML = unsaved +
      `<optgroup label="Built-in">${builtin.map(opt).join("")}</optgroup>` +
      (saved.length ? `<optgroup label="Saved">${saved.map(opt).join("")}</optgroup>` : "");
    sel.value = this.loaded ? `${this.loaded.builtin ? "b" : "s"}:${this.loaded.name}` : "";
    sel.classList.toggle("edited", this.edited);
    sel.title = this.edited ? "Changed since it was loaded; Save as… to keep the changes" : "";
    this.q("[data-delete]").hidden = !this.loaded || this.loaded.builtin;
    this.disarmDelete();
  }

  private async loadPreset(value: string): Promise<void> {
    if (!value) return;
    const builtin = value.startsWith("b:");
    const name = value.slice(2);
    try {
      this.stopLearn(true);
      this.preset = await invoke<MidiPreset>("midi_load_preset", { name, builtin });
      this.loaded = { name, builtin };
      this.edited = false;
      this.open.clear();
      this.renderPresets();
      this.renderMaps();
      this.renderGrid();
    } catch (e) {
      this.flash(String(e));
      this.renderPresets();
    }
  }

  private showSaveName(on: boolean): void {
    const input = this.q<HTMLInputElement>("[data-save-name]");
    input.hidden = !on;
    this.q("[data-preset]").hidden = on;
    this.q("[data-save-cancel]").hidden = !on;
    this.q("[data-save]").textContent = on ? "Save" : "Save as…";
    this.q("[data-delete]").hidden = on || !this.loaded || this.loaded.builtin;
    this.q("[data-folder]").hidden = on;
    if (on) {
      input.value = this.loaded && !this.loaded.builtin ? this.loaded.name : "";
      input.focus();
      input.select();
    }
  }

  private async saveClicked(): Promise<void> {
    const input = this.q<HTMLInputElement>("[data-save-name]");
    if (input.hidden) return this.showSaveName(true);
    const name = input.value.trim();
    if (!name) return input.focus();
    try {
      await this.flush();
      await invoke("midi_save_preset", { name });
      this.preset!.name = name;
      this.loaded = { name, builtin: false };
      this.edited = false;
      await this.refreshPresets();
      this.showSaveName(false);
      this.renderPresets();
    } catch (e) {
      this.flash(String(e));
    }
  }

  private async deleteClicked(): Promise<void> {
    if (!this.loaded || this.loaded.builtin) return;
    const b = this.q("[data-delete]");
    if (!this.deleteArmed) {
      this.deleteArmed = true;
      b.classList.add("armed");
      b.textContent = "Click to delete";
      this.armTimer = window.setTimeout(() => this.disarmDelete(), 3000);
      return;
    }
    try {
      await invoke("midi_delete_preset", { name: this.loaded.name });
      // The mappings stay as they are; they are just not saved anywhere now.
      this.loaded = null;
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

  /** Send the edited preset to the backend soon, coalescing quick edits like slider drags. */
  private push(): void {
    if (!this.edited) {
      this.edited = true;
      this.renderPresets();
    }
    clearTimeout(this.pushTimer);
    this.pushTimer = window.setTimeout(() => this.flush(), 50);
  }

  private async flush(): Promise<void> {
    clearTimeout(this.pushTimer);
    if (this.preset) await invoke("midi_set_preset", { preset: this.preset }).catch((e) => this.flash(String(e)));
  }

  // ---------- Mappings ----------

  private renderMaps(): void {
    const maps = this.preset?.mappings ?? [];
    this.q("[data-map-count]").textContent = maps.length ? String(maps.length) : "";
    this.q("[data-maps]").innerHTML = maps.length
      ? maps.map((m, i) => this.mapRow(m, i)).join("")
      : `<p class="hint">No mappings. Add one, or load a preset.</p>`;
    maps.forEach((m, i) => {
      const row = this.root.querySelector<HTMLElement>(`.map-row[data-i="${i}"]`)!;
      row.querySelectorAll<HTMLSelectElement>("select[data-f]").forEach((s) => (s.value = this.selectValue(m, s.dataset.f!)));
    });
  }

  private selectValue(m: MidiMapping, f: string): string {
    const t = m.target as Record<string, unknown>;
    switch (f) {
      case "source": return m.source;
      case "kind": return m.target.kind;
      case "channel": return String(m.channel);
      case "hold": return m.hold ?? "";
      default: return String(t[f] ?? "");
    }
  }

  private mapRow(m: MidiMapping, i: number): string {
    const sourceOpts = SOURCES.map(([g, list]) =>
      `<optgroup label="${g}">${list.map(([k, l]) => `<option value="${k}">${l}</option>`).join("")}</optgroup>`).join("") +
      (SOURCE_LABELS.has(m.source) ? "" : `<option value="${escape(m.source)}">${escape(m.source)} (unknown)</option>`);
    const t = m.target;
    const args =
      t.kind === "cc"
        ? `<input class="field num" type="number" data-f="cc" min="0" max="127" value="${t.cc}" title="Controller number (0–127)" />
           <label class="chk" title="14-bit: also sends CC ${t.cc + 32} for finer steps (CC 0–31 only)"><input type="checkbox" data-f="hi_res" ${t.hi_res ? "checked" : ""} ${t.cc < 32 ? "" : "disabled"} />14-bit</label>`
        : t.kind === "note"
          ? `<select class="field" data-f="note" title="Note">${NOTE_OPTIONS}</select>
             <input class="field num" type="number" data-f="velocity" min="1" max="127" value="${t.velocity}" title="Velocity (1–127)" />`
          : t.kind === "program"
            ? `<input class="field num" type="number" data-f="program" min="0" max="127" value="${t.program}" title="Program number (0–127; many DAWs show it as 1–128)" />`
            : "";
    const pct = (v: number) => Math.round(v * 100);
    const holdOpts = `<option value="">Always</option>` + BUTTONS.map(([n, l]) => `<option value="${n}">While ${l} held</option>`).join("");
    const learning = this.learnRow === i;
    return `
      <div class="map-row${m.enabled ? "" : " off"}" data-i="${i}">
        <div class="map-main">
          <input type="checkbox" data-f="enabled" ${m.enabled ? "checked" : ""} title="On / off" />
          <select class="field" data-f="source" title="Input">${sourceOpts}</select>
          <button class="icon-btn${learning ? " learning" : ""}" data-learn title="${learning ? "Move a control now… (click to cancel)" : "Learn: click, then move or press a control"}">${learning ? "…" : "◎"}</button>
          <select class="field" data-f="kind" title="${KINDS.find(([k]) => k === t.kind)?.[2] ?? ""}">${KINDS.map(([k, l]) => `<option value="${k}">${l}</option>`).join("")}</select>
          <span class="args">${args}</span>
          <select class="field ch" data-f="channel" title="MIDI channel">${CHANNEL_OPTIONS}</select>
          <button class="icon-btn${this.open.has(i) ? " on" : ""}" data-more title="Shaping: range, deadzone, curve, smoothing, toggle">⋯</button>
          <button class="icon-btn" data-del title="Remove">×</button>
        </div>
        <span class="meter"><i></i></span>
        <div class="map-more" ${this.open.has(i) ? "" : "hidden"}>
          ${helpIcon("midi-shaping", "What these settings do")}
          <label title="Part of the input's travel that spans the output, in %">Range
            <input class="field num" type="number" data-f="range0" min="0" max="100" value="${pct(m.range[0])}" />–<input class="field num" type="number" data-f="range1" min="0" max="100" value="${pct(m.range[1])}" />%</label>
          <label title="Travel around the centre (or at the bottom) that counts as rest, in %">Deadzone
            <input class="field num" type="number" data-f="deadzone" min="0" max="90" value="${pct(m.deadzone)}" />%</label>
          <label title="Left: reacts fast near rest. Right: slow near rest, for fine control">Curve
            <input type="range" data-f="curve" min="-100" max="100" value="${pct(m.curve)}" /></label>
          <label title="Evens out shaky input such as tilt; more smoothing lags more">Smooth
            <input type="range" data-f="smoothing" min="0" max="95" value="${pct(m.smoothing)}" /></label>
          <label class="chk" title="Output from 1 down to 0"><input type="checkbox" data-f="invert" ${m.invert ? "checked" : ""} />Invert</label>
          <label class="chk" title="Each press flips the output on or off (a latch)"><input type="checkbox" data-f="toggle" ${m.toggle ? "checked" : ""} />Toggle</label>
          <label title="Only send while a button is held, e.g. tilt only while R4 is down">Active
            <select class="field" data-f="hold">${holdOpts}</select></label>
        </div>
      </div>`;
  }

  private mapFieldChanged(el: HTMLElement): void {
    const row = el.closest<HTMLElement>(".map-row");
    const f = el.dataset.f;
    if (!row || !f || !this.preset) return;
    const i = Number(row.dataset.i);
    const m = this.preset.mappings[i];
    const input = el as HTMLInputElement;
    const num = (lo: number, hi: number) => clamp(Math.round(Number(input.value) || 0), lo, hi);
    const t = m.target as Record<string, unknown>;
    switch (f) {
      case "enabled":
        m.enabled = input.checked;
        row.classList.toggle("off", !m.enabled);
        break;
      case "source":
        m.source = input.value;
        break;
      case "kind": {
        const cc = m.target.kind === "cc" ? m.target.cc : this.freeCc();
        m.target = defaultTarget(input.value as MidiTarget["kind"], cc);
        this.rerenderRow(i);
        break;
      }
      case "cc":
        if (input.value === "") return;
        t.cc = num(0, 127);
        if ((t.cc as number) >= 32) t.hi_res = false;
        {
          // Edit the 14-bit box in place: redrawing the row would steal focus while typing.
          const hr = row.querySelector<HTMLInputElement>('[data-f="hi_res"]')!;
          hr.disabled = (t.cc as number) >= 32;
          hr.checked = !!t.hi_res;
          hr.parentElement!.title = `14-bit: also sends CC ${(t.cc as number) + 32} for finer steps (CC 0–31 only)`;
        }
        break;
      case "hi_res":
        t.hi_res = input.checked;
        break;
      case "note":
        t.note = Number(input.value);
        break;
      case "velocity":
        if (input.value === "") return;
        t.velocity = num(1, 127);
        break;
      case "program":
        if (input.value === "") return;
        t.program = num(0, 127);
        break;
      case "channel":
        m.channel = Number(input.value);
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
    this.push();
  }

  private rerenderRow(i: number): void {
    const row = this.root.querySelector<HTMLElement>(`.map-row[data-i="${i}"]`);
    if (!row || !this.preset) return;
    const m = this.preset.mappings[i];
    row.outerHTML = this.mapRow(m, i);
    const fresh = this.root.querySelector<HTMLElement>(`.map-row[data-i="${i}"]`)!;
    fresh.querySelectorAll<HTMLSelectElement>("select[data-f]").forEach((s) => (s.value = this.selectValue(m, s.dataset.f!)));
  }

  private mapClicked(el: Element): void {
    const row = el.closest<HTMLElement>(".map-row");
    if (!row || !this.preset) return;
    const i = Number(row.dataset.i);
    if (el.closest("[data-del]")) {
      this.stopLearn(true);
      this.preset.mappings.splice(i, 1);
      // Rows below shift up by one; so do their open panels.
      this.open = new Set([...this.open].filter((n) => n !== i).map((n) => (n > i ? n - 1 : n)));
      this.renderMaps();
      this.push();
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
    await invoke("midi_learn", { on: true });
  }

  private stopLearn(cancel: boolean): void {
    if (this.learnRow === null) return;
    const i = this.learnRow;
    this.learnRow = null;
    if (cancel) invoke("midi_learn", { on: false });
    this.rerenderRow(i);
  }

  /** The lowest CC from 20 up that no mapping uses yet. */
  private freeCc(): number {
    const used = new Set(this.preset!.mappings.flatMap((m) => (m.target.kind === "cc" ? [m.target.cc] : [])));
    for (let cc = 20; cc < 120; cc++) if (!used.has(cc)) return cc;
    return 20;
  }

  private addMapping(): void {
    if (!this.preset) return;
    this.preset.mappings.push({
      enabled: true, source: "trigger_right", target: { kind: "cc", cc: this.freeCc(), hi_res: false }, channel: 1,
      range: [0, 1], invert: false, deadzone: 0, curve: 0, smoothing: 0, toggle: false, hold: null,
    });
    this.renderMaps();
    this.push();
    const list = this.q("[data-maps]");
    list.scrollTop = list.scrollHeight;
    // Learn right away: the next control moved becomes the input.
    this.startLearn(this.preset.mappings.length - 1);
  }

  // ---------- Pad grids ----------

  private renderGrid(): void {
    this.root.querySelectorAll<HTMLElement>("[data-side]").forEach((b) => b.classList.toggle("on", b.dataset.side === this.side));
    if (!this.preset) return;
    const g = this.preset[this.side];
    const cols = clamp(g.cols, 1, 8);
    const rows = clamp(g.rows, 1, 8);
    const cells: string[] = [];
    // Row 0 is at the bottom, like the pad.
    for (let r = rows - 1; r >= 0; r--) {
      for (let c = 0; c < cols; c++) {
        const cell = r * cols + c;
        const n = gridNote(g, cell);
        const root = n !== null && (n - g.root) % 12 === 0;
        cells.push(`<span data-cell="${cell}" class="${root ? "root" : ""}" title="${n === null ? "Out of range" : `${noteName(n)} · ${n}`}">${n === null ? "" : noteName(n)}</span>`);
      }
    }
    const sel = (f: string, opts: [string | number, string][], v: string | number, title = "") =>
      `<select class="field" data-g="${f}" title="${title}">${opts.map(([k, l]) => `<option value="${k}"${String(k) === String(v) ? " selected" : ""}>${l}</option>`).join("")}</select>`;
    const sizes = Array.from({ length: 8 }, (_, i) => [i + 1, String(i + 1)] as [number, string]);
    this.q("[data-grid]").innerHTML = `
      <div class="grid-wrap${g.enabled ? "" : " off"}">
        <label class="chk grid-enable"><input type="checkbox" data-g="enabled" ${g.enabled ? "checked" : ""} />Play notes on the ${this.side === "left_pad" ? "left" : "right"} pad</label>
        <div class="grid-body">
          <div class="grid-cells" style="grid-template-columns: repeat(${cols}, 1fr); grid-template-rows: repeat(${rows}, 1fr)">${cells.join("")}</div>
          <div class="grid-form">
            <label>Root ${sel("root", Array.from({ length: 128 }, (_, n) => [n, `${noteName(n)} · ${n}`]), g.root, "Note of the bottom-left cell")}</label>
            <label>Scale ${sel("scale", SCALES, g.scale)}</label>
            <label>Grid ${sel("cols", sizes, g.cols, "Columns")}<span class="muted">×</span>${sel("rows", sizes, g.rows, "Rows")}</label>
            <label title="Scale steps from one row to the next; same as the columns plays one continuous run">Row step ${sel("row_step", Array.from({ length: 12 }, (_, i) => [i + 1, String(i + 1)]), g.row_step)}</label>
            <label>Channel ${sel("channel", Array.from({ length: 16 }, (_, i) => [i + 1, String(i + 1)]), g.channel)}</label>
            <label title="What starts a note">Play on <span class="segmented"><button data-trigger="touch" class="${g.trigger === "touch" ? "on" : ""}">Touch</button><button data-trigger="click" class="${g.trigger === "click" ? "on" : ""}">Click</button></span></label>
            <label>Velocity <input class="field num" type="number" data-g="velocity" min="1" max="127" value="${g.velocity}" ${g.dynamic_velocity ? "disabled" : ""} />
              <span class="chk" title="Velocity from how hard you press when the note starts"><input type="checkbox" data-g="dynamic_velocity" ${g.dynamic_velocity ? "checked" : ""} />By pressure</span></label>
            <label title="Send pad pressure while a note is held">Pressure ${sel("pressure", [["off", "Off"], ["poly", "Poly aftertouch"], ["channel", "Channel aftertouch"]], g.pressure)}</label>
            <label class="chk" title="Sliding onto another cell while held plays it (otherwise the first note keeps playing)"><input type="checkbox" data-g="slide" ${g.slide ? "checked" : ""} />Slide between notes</label>
          </div>
        </div>
      </div>`;
  }

  private gridFieldChanged(el: HTMLElement): void {
    const f = el.dataset.g;
    if (!f || !this.preset) return;
    const input = el as HTMLInputElement;
    const g = this.preset[this.side] as unknown as Record<string, unknown>;
    if (input.type === "checkbox") g[f] = input.checked;
    else if (f === "scale" || f === "pressure") g[f] = input.value;
    else {
      if (input.value === "") return;
      g[f] = clamp(Math.round(Number(input.value) || 0), f === "root" ? 0 : 1, f === "root" || f === "velocity" ? 127 : 16);
    }
    // The cells' notes depend on most fields; redraw unless only the velocity changed.
    this.setGrid({}, f !== "velocity");
  }

  private setGrid(change: Partial<PadGrid>, redraw: boolean): void {
    if (!this.preset) return;
    Object.assign(this.preset[this.side], change);
    if (redraw) this.renderGrid();
    this.push();
  }
}
