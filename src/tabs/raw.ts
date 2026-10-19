import {
  Buttons, CHARGE_STATES, REPORT_NAMES, describeSlot, hex, pressed,
  type ButtonName, type ControllerState, type Snapshot, type Trackpad,
} from "../controller";
import type { Tab } from "./tab";
import { ICONS } from "./icons";

/** How long a changed byte stays highlighted in the byte map. */
const BYTE_HIGHLIGHT_MS = 400;
const AXIS = 32768;
const ACCEL_PER_G = 16384;
const GYRO_PER_DPS = 32768 / 2000;
const QUAT_SCALE = 32767;

/** Byte fields of a state report, mirroring `parse_state` in crates/sc-core/src/protocol.rs. */
const FIELDS = {
  id: "Report id", seq: "Sequence", buttons: "Buttons", triggers: "Triggers", sticks: "Sticks",
  padtime: "Pad timestamp", pads: "Trackpads", imutime: "IMU timestamp", accel: "Accel", gyro: "Gyro",
  quat: "Orientation", rest: "Not decoded",
} as const;
type Field = keyof typeof FIELDS;

function fieldMap(id: number, len: number): Field[] {
  // 0x47 inserts a u16 trackpad timestamp before the pads and shrinks the IMU timestamp to u16.
  const pads = id === 0x47 ? 20 : 18;
  const imu = pads + 12;
  const imuData = imu + (id === 0x47 ? 2 : 4);
  const ranges: [Field, number, number][] = [
    ["id", 0, 1], ["seq", 1, 2], ["buttons", 2, 6], ["triggers", 6, 10], ["sticks", 10, 18],
    ["padtime", 18, pads], ["pads", pads, pads + 12], ["imutime", imu, imuData],
    ["accel", imuData, imuData + 6], ["gyro", imuData + 6, imuData + 12],
  ];
  if (id === 0x42) ranges.push(["quat", imuData + 12, imuData + 20]);
  const out: Field[] = Array(len).fill("rest");
  for (const [f, a, b] of ranges) for (let i = a; i < b && i < len; i++) out[i] = f;
  return out;
}

const BUTTON_ORDER: ButtonName[] = [
  "A", "B", "X", "Y", "DpadUp", "DpadDown", "DpadLeft", "DpadRight", "L", "R", "L4", "L5", "R4", "R5",
  "L3", "R3", "View", "Menu", "Steam", "QAM", "LeftTriggerClick", "RightTriggerClick", "LeftPadClick", "RightPadClick",
];
const TOUCH_ORDER: ButtonName[] = ["LeftGripTouch", "RightGripTouch", "LeftStickTouch", "RightStickTouch", "LeftPadTouch", "RightPadTouch"];
const LABELS: Partial<Record<ButtonName, string>> = {
  DpadUp: "D-pad ↑", DpadDown: "D-pad ↓", DpadLeft: "D-pad ←", DpadRight: "D-pad →", QAM: "···",
  LeftTriggerClick: "LT click", RightTriggerClick: "RT click", LeftPadClick: "L pad click", RightPadClick: "R pad click",
  LeftGripTouch: "L grip", RightGripTouch: "R grip", LeftStickTouch: "L stick", RightStickTouch: "R stick",
  LeftPadTouch: "L pad", RightPadTouch: "R pad",
};
const label = (n: ButtonName) => LABELS[n] ?? n;

/** Device facts, every decoded value next to its raw counts, and the report's bytes, for protocol work. */
export class RawTab implements Tab {
  id = "raw";
  title = "Raw data";
  group = "input" as const;
  icon = ICONS.raw;
  private root!: HTMLElement;
  private cells = new Map<string, HTMLElement>();
  private byteChangedAt: number[] = [];
  private lastRaw: number[] = [];
  /** One element per byte, rebuilt only when the report's layout changes (hover needs stable nodes). */
  private byteEls: HTMLElement[] = [];
  private layoutKey = "";
  private hoveredByte: number | null = null;

  mount(root: HTMLElement): void {
    this.root = root;
    const row = (label: string, key: string, wrap = false) =>
      `<tr><th>${label}</th><td class="raw-val" data-k="${key}.raw">–</td><td${wrap ? ` class="wrap"` : ""} data-k="${key}">–</td></tr>`;
    const axisRow = (label: string, key: string, axes: string[]) =>
      `<tr><th>${label}</th>${axes.map((a) => `<td><b data-k="${key}.${a}">–</b><small data-k="${key}.${a}.raw"></small></td>`).join("")}</tr>`;

    root.innerHTML = `
      <div class="raw-tab">
        <section class="tile">
          <h3>Connection</h3>
          <dl class="facts">
            <dt>Device</dt><dd data-k="device">–</dd>
            <dt>Link</dt><dd data-k="link">–</dd>
            <dt>Report</dt><dd data-k="report">–</dd>
            <dt>Rate</dt><dd data-k="rate">–</dd>
            <dt>Battery</dt><dd data-k="battery">–</dd>
            <dt>Settings</dt><dd title="Re-sent every 2 s, because the controller reverts them">Lizard mode off, raw IMU</dd>
            <dt>Steam</dt><dd data-k="steam">–</dd>
          </dl>
        </section>
        <section class="tile">
          <h3>Latest state report <button class="mini" data-copy-report title="Copy all bytes as hex">Copy all</button></h3>
          <pre class="hex bytemap" data-bytes>–</pre>
          <div class="byte-info" data-byte-info>Hover a byte to see its field; click any value to copy it</div>
          <div class="legend">${(Object.keys(FIELDS) as Field[]).map((f) => `<span class="f-${f}">${FIELDS[f]}</span>`).join("")}</div>
        </section>
        <section class="tile">
          <h3>Inputs <em>raw → value</em></h3>
          <table class="raw-table">
            ${row("Buttons", "buttons", true)}
            ${row("Touch", "touch", true)}
            ${row("L trigger", "lt")}
            ${row("R trigger", "rt")}
            ${row("L stick", "ls")}
            ${row("R stick", "rs")}
            ${row("L pad", "lp")}
            ${row("R pad", "rp")}
          </table>
        </section>
        <section class="tile">
          <h3>Motion <em title="Sensor frame: +X right, +Y forward (towards the triggers), +Z out of the face">sensor frame</em></h3>
          <table class="raw-table axes">
            <tr><th></th><th>X</th><th>Y</th><th>Z</th><th>W</th></tr>
            ${axisRow("Accel (g)", "accel", ["x", "y", "z"])}
            ${axisRow("Gyro (°/s)", "gyro", ["x", "y", "z"])}
            ${axisRow("Orientation", "quat", ["x", "y", "z", "w"])}
          </table>
          <dl class="facts" style="margin-top:12px">
            <dt>IMU time</dt><dd data-k="imutime">–</dd>
            <dt title="From the accelerometer alone: a cross-check for the 3D view">Gravity tilt</dt><dd data-k="tilt">–</dd>
          </dl>
        </section>
        <section class="tile wide">
          <h3>Undecoded reports</h3>
          <div data-unknown><p class="muted">None seen yet.</p></div>
        </section>
      </div>`;
    root.querySelectorAll<HTMLElement>("[data-k]").forEach((el) => {
      this.cells.set(el.dataset.k!, el);
      el.dataset.copy = ""; // click to copy its text (see main.ts)
    });

    const bytes = this.q("[data-bytes]");
    bytes.addEventListener("mouseover", (e) => {
      const i = (e.target as HTMLElement).dataset?.byte;
      this.hoveredByte = i === undefined ? null : Number(i);
      this.renderByteInfo();
    });
    bytes.addEventListener("mouseleave", () => {
      this.hoveredByte = null;
      this.renderByteInfo();
    });
    this.q("[data-copy-report]").dataset.copy = "";
  }

  update(snap: Snapshot): void {
    const s = snap.state;
    const b = snap.battery;
    if (snap.slot) {
      const d = describeSlot(snap.slot);
      this.set("device", d.product);
      this.set("link", `${d.label} · 28de:${d.pid.toString(16).padStart(4, "0")} · interface ${d.iface}`);
    } else {
      this.set("device", "–");
      this.set("link", "Searching…");
    }
    this.set("report", s ? `0x${s.report_id.toString(16)} ${REPORT_NAMES[s.report_id] ?? ""} · ${snap.raw.length} B · seq ${s.seq}` : "–");
    this.set("rate", s ? `${snap.report_rate} reports/s` : "–");
    this.set("battery", b ? `${b.level}% · ${CHARGE_STATES[b.charge_state] ?? `state ${b.charge_state}`}` : "–");
    this.set("steam", snap.steam_running ? "Running" : "Not running");

    if (s) {
      this.renderInputs(s);
      this.renderMotion(s);
    }
    if (snap.raw.length) {
      this.renderBytes(snap.raw);
      this.q("[data-copy-report]").dataset.copy = hex(snap.raw);
      this.renderByteInfo();
    }
    if (snap.unknown_reports.length) {
      this.q("[data-unknown]").innerHTML = snap.unknown_reports
        .map((u) => `<div class="unknown-head">0x${u.id.toString(16)} <em>×${u.count} · ${u.data.length} bytes</em></div><pre class="hex" data-copy="${hex(u.data)}">${lines(hex(u.data).split(" "))}</pre>`)
        .join("");
    }
  }

  private renderInputs(s: ControllerState): void {
    const held = BUTTON_ORDER.filter((n) => pressed(s, n));
    this.set("buttons.raw", `0x${s.buttons.toString(16).padStart(8, "0")}`);
    this.set("buttons", held.length ? held.map(label).join(", ") : "none");
    const touched = TOUCH_ORDER.filter((n) => pressed(s, n));
    this.set("touch.raw", `0x${(s.buttons & touchMask).toString(16).padStart(8, "0")}`);
    this.set("touch", touched.length ? touched.map(label).join(", ") : "none");

    const trig = (k: string, v: number, click: boolean) => {
      this.set(`${k}.raw`, `${v}`);
      this.set(k, `${((v / 32767) * 100).toFixed(1)}%${click ? " · click" : ""}`);
    };
    trig("lt", s.trigger_left, pressed(s, "LeftTriggerClick"));
    trig("rt", s.trigger_right, pressed(s, "RightTriggerClick"));

    const stick = (k: string, [x, y]: [number, number], touch: boolean, click: boolean) => {
      this.set(`${k}.raw`, `${x}, ${y}`);
      this.set(k, `${signed(x / AXIS)}, ${signed(y / AXIS)}${touch ? " · touch" : ""}${click ? " · click" : ""}`);
    };
    stick("ls", s.left_stick, pressed(s, "LeftStickTouch"), pressed(s, "L3"));
    stick("rs", s.right_stick, pressed(s, "RightStickTouch"), pressed(s, "R3"));

    const pad = (k: string, p: Trackpad) => {
      this.set(`${k}.raw`, `${p.x}, ${p.y} p${p.pressure}`);
      this.set(k, p.touched ? `${signed(p.x / AXIS)}, ${signed(p.y / AXIS)}${p.clicked ? " · click" : ""}` : "not touched");
    };
    pad("lp", s.left_pad);
    pad("rp", s.right_pad);
  }

  private renderMotion(s: ControllerState): void {
    const axes = ["x", "y", "z"] as const;
    axes.forEach((a, i) => {
      this.set(`accel.${a}`, signed(s.imu.accel[i] / ACCEL_PER_G, 3));
      this.set(`accel.${a}.raw`, `${s.imu.accel[i]}`);
      this.set(`gyro.${a}`, signed(s.imu.gyro[i] / GYRO_PER_DPS, 1));
      this.set(`gyro.${a}.raw`, `${s.imu.gyro[i]}`);
    });
    const q = s.imu.quat;
    // Wire order is w, x, y, z.
    (["w", "x", "y", "z"] as const).forEach((a, i) => {
      this.set(`quat.${a}`, q ? signed(q[i] / QUAT_SCALE, 3) : "–");
      this.set(`quat.${a}.raw`, q ? `${q[i]}` : "");
    });
    this.set("imutime", `${(s.imu.timestamp / 1e6).toFixed(3)} s`);

    // Tilt from gravity alone: works without the quaternion and is a check on its sign conventions.
    const [ax, ay, az] = s.imu.accel;
    const deg = 180 / Math.PI;
    const pitch = Math.atan2(ay, Math.hypot(ax, az)) * deg;
    const roll = Math.atan2(-ax, az) * deg;
    this.set("tilt", `pitch ${pitch.toFixed(0)}° · roll ${roll.toFixed(0)}°`);
  }

  private set(key: string, text: string): void {
    const el = this.cells.get(key);
    if (el && el.textContent !== text) el.textContent = text;
  }

  private q(sel: string): HTMLElement {
    return this.root.querySelector(sel) as HTMLElement;
  }

  /** Hex dump, 8 bytes per line, each byte tinted by its field; recently changed bytes light up. */
  private renderBytes(raw: number[]): void {
    const now = performance.now();
    raw.forEach((v, i) => {
      if (this.lastRaw[i] !== v) this.byteChangedAt[i] = now;
    });
    this.lastRaw = raw;

    const key = `${raw[0]}:${raw.length}`;
    if (key !== this.layoutKey) {
      this.layoutKey = key;
      const map = fieldMap(raw[0], raw.length);
      const cells = raw.map((_, i) => `<span class="f-${map[i]}" data-byte="${i}" data-field="${map[i]}" data-copy></span>`);
      this.q("[data-bytes]").innerHTML = lines(cells);
      this.byteEls = [...this.root.querySelectorAll<HTMLElement>("[data-byte]")];
    }
    raw.forEach((v, i) => {
      const el = this.byteEls[i];
      const h = v.toString(16).padStart(2, "0");
      if (el.textContent !== h) el.textContent = h;
      el.classList.toggle("chg", now - (this.byteChangedAt[i] ?? 0) < BYTE_HIGHLIGHT_MS);
    });
  }

  private renderByteInfo(): void {
    const i = this.hoveredByte;
    const el = i === null ? undefined : this.byteEls[i];
    const info = this.q("[data-byte-info]");
    if (!el || i === null) {
      info.textContent = "Hover a byte to see its field; click any value to copy it";
      info.className = "byte-info";
      return;
    }
    const v = this.lastRaw[i] ?? 0;
    const field = el.dataset.field as Field;
    info.className = `byte-info f-${field}`;
    info.textContent = `Byte ${i} · ${FIELDS[field]} · 0x${v.toString(16).padStart(2, "0")} = ${v} = 0b${v.toString(2).padStart(8, "0")}`;
  }
}

const touchMask = TOUCH_ORDER.reduce((m, n) => m | Buttons[n], 0);

function signed(v: number, decimals = 2): string {
  const t = v.toFixed(decimals);
  return Number(t) === 0 ? (0).toFixed(decimals) : (v > 0 ? "+" : "") + t;
}

function lines(cells: string[]): string {
  const out: string[] = [];
  for (let i = 0; i < cells.length; i += 8) {
    out.push(`<span class="off">${i.toString().padStart(2, "0")}</span> ${cells.slice(i, i + 8).join(" ")}`);
  }
  return out.join("\n");
}
