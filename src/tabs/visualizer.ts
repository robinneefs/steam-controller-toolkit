import { CHARGE_STATES, hex, pressed, type ButtonName, type ControllerState, type Snapshot, type Trackpad } from "../controller";
import { Controller3D } from "./controller3d";
import { LiveGraph } from "./graph";
import type { Tab } from "./tab";

const AXIS = 32768;
const ACCEL_PER_G = 16384;
const GYRO_PER_DPS = 32768 / 2000;
/** How long a changed byte stays highlighted in the raw view. */
const BYTE_HIGHLIGHT_MS = 400;

type Side = "left" | "right";

function sideHtml(side: Side): string {
  const s = side === "left" ? "L" : "R";
  return `
    <section class="panel side side-${side}">
      <div class="row-buttons">
        <span class="btn" data-btn="${s}">${s}</span>
        <span class="btn" data-btn="${s}4" title="Upper back button">${s}4</span>
        <span class="btn" data-btn="${s}5" title="Lower back button">${s}5</span>
      </div>
      <div class="meter" title="${side} trigger">
        <div class="meter-fill" data-trigger="${side}"></div>
        <span class="meter-label" data-trigger-label="${side}"></span>
      </div>
      <div class="pair">
        <figure>
          <div class="square-box"><div class="stick" data-stick-area="${side}"><div class="dot" data-stick="${side}"></div></div></div>
          <figcaption data-stick-label="${side}"></figcaption>
        </figure>
        <figure>
          <div class="square-box"><div class="pad" data-pad-area="${side}"><div class="dot" data-pad="${side}"></div></div></div>
          <figcaption data-pad-label="${side}"></figcaption>
        </figure>
      </div>
    </section>`;
}

const TOUCH_SENSORS: [ButtonName, string][] = [
  ["LeftGripTouch", "L grip"], ["RightGripTouch", "R grip"],
  ["LeftStickTouch", "L stick"], ["RightStickTouch", "R stick"],
  ["LeftPadTouch", "L pad"], ["RightPadTouch", "R pad"],
];

export class VisualizerTab implements Tab {
  id = "visualizer";
  title = "Visualizer";
  private root!: HTMLElement;
  private view!: Controller3D;
  private graphs!: { gyro: LiveGraph; accel: LiveGraph; triggers: LiveGraph; pressure: LiveGraph };
  private byteChangedAt: number[] = [];
  private lastRaw: number[] = [];

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="visualizer">
        <section class="panel view3d">
          <div class="view3d-canvas" data-view3d></div>
          <div class="view3d-overlay">
            <span data-angles></span>
            <button data-recenter title="Make the current heading straight ahead">Re-center</button>
          </div>
          <div class="view3d-credit" title="Adapted from sketchfab.com/3d-models/steam-machine-controller-c1fcf77d71984c11818a5e2412816705: controller extracted, split into moving parts, compressed.">
            3D model: “Steam Machine + Controller” by dark_igorek · CC BY 4.0 · adapted
          </div>
        </section>
        <section class="panel graphs">
          <div data-graph="gyro"></div>
          <div data-graph="accel"></div>
          <div data-graph="triggers"></div>
          <div data-graph="pressure"></div>
        </section>
        ${sideHtml("left")}
        <section class="panel center">
          <div class="clusters">
            <div class="cluster">
              <span class="btn" data-btn="DpadUp">↑</span>
              <span class="btn" data-btn="DpadLeft">←</span>
              <span class="btn" data-btn="DpadRight">→</span>
              <span class="btn" data-btn="DpadDown">↓</span>
            </div>
            <div class="cluster face">
              <span class="btn" data-btn="Y">Y</span>
              <span class="btn" data-btn="X">X</span>
              <span class="btn" data-btn="B">B</span>
              <span class="btn" data-btn="A">A</span>
            </div>
          </div>
          <div class="row-buttons">
            <span class="btn" data-btn="View">View</span>
            <span class="btn" data-btn="Steam">Steam</span>
            <span class="btn" data-btn="QAM" title="Quick access menu">···</span>
            <span class="btn" data-btn="Menu">Menu</span>
          </div>
          <div class="section-label">Touch sensors</div>
          <div class="chips">
            ${TOUCH_SENSORS.map(([b, label]) => `<span class="chip" data-touch="${b}">${label}</span>`).join("")}
          </div>
        </section>
        ${sideHtml("right")}
        <section class="panel details">
          <dl class="facts">
            <dt>Battery</dt><dd data-fact="battery">–</dd>
            <dt>Slot</dt><dd data-fact="slot">–</dd>
            <dt>Report</dt><dd data-fact="report">–</dd>
            <dt>IMU time</dt><dd data-fact="imu">–</dd>
          </dl>
          <div class="section-label">Latest state report <small>(changing bytes highlighted)</small></div>
          <pre class="hex" data-raw></pre>
          <div data-unknown></div>
        </section>
      </div>`;

    this.view = new Controller3D(this.q("[data-view3d]"));
    this.q("[data-recenter]").addEventListener("click", () => this.view.recenter());
    this.graphs = {
      gyro: new LiveGraph(this.q('[data-graph="gyro"]'), {
        title: "Gyro", unit: "°/s", minSpan: 50,
        series: [{ name: "pitch", color: "--series-1" }, { name: "roll", color: "--series-2" }, { name: "yaw", color: "--series-3" }],
      }),
      accel: new LiveGraph(this.q('[data-graph="accel"]'), {
        title: "Accel", unit: "g", range: [-2, 2], decimals: 2,
        series: [{ name: "x", color: "--series-1" }, { name: "y", color: "--series-2" }, { name: "z", color: "--series-3" }],
      }),
      triggers: new LiveGraph(this.q('[data-graph="triggers"]'), {
        title: "Triggers", unit: "%", range: [0, 100],
        series: [{ name: "L", color: "--series-1" }, { name: "R", color: "--series-2" }],
      }),
      pressure: new LiveGraph(this.q('[data-graph="pressure"]'), {
        title: "Pad pressure", unit: "raw", minSpan: 1000, positive: true,
        series: [{ name: "L", color: "--series-1" }, { name: "R", color: "--series-2" }],
      }),
    };
  }

  update(snap: Snapshot): void {
    this.root.classList.toggle("disconnected", !snap.state);
    this.renderDetails(snap);
    if (snap.state) this.render(snap.state);
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector(sel) as T;
  }

  private render(s: ControllerState): void {
    this.view.update(s);
    const a = this.view.angles();
    this.q("[data-angles]").textContent = s.imu.quat
      ? `pitch ${a.pitch.toFixed(0)}° · roll ${a.roll.toFixed(0)}° · yaw ${a.yaw.toFixed(0)}°`
      : `report 0x${s.report_id.toString(16)} has no orientation data`;

    this.graphs.gyro.push(s.imu.gyro.map((v) => v / GYRO_PER_DPS));
    this.graphs.accel.push(s.imu.accel.map((v) => v / ACCEL_PER_G));
    this.graphs.triggers.push([(s.trigger_left / 32767) * 100, (s.trigger_right / 32767) * 100]);
    this.graphs.pressure.push([s.left_pad.pressure, s.right_pad.pressure]);

    this.root.querySelectorAll<HTMLElement>("[data-btn]").forEach((el) => {
      el.classList.toggle("on", pressed(s, el.dataset.btn as ButtonName));
    });
    this.root.querySelectorAll<HTMLElement>("[data-touch]").forEach((el) => {
      el.classList.toggle("on", pressed(s, el.dataset.touch as ButtonName));
    });

    for (const side of ["left", "right"] as const) {
      const trig = side === "left" ? s.trigger_left : s.trigger_right;
      const click = pressed(s, side === "left" ? "LeftTriggerClick" : "RightTriggerClick");
      const fill = this.q(`[data-trigger="${side}"]`);
      fill.style.width = `${(trig / 32767) * 100}%`;
      fill.classList.toggle("on", click);
      this.q(`[data-trigger-label="${side}"]`).textContent =
        `${side === "left" ? "L" : "R"} trigger ${Math.round((trig / 32767) * 100)}%${click ? " · click" : ""}`;

      const [sx, sy] = side === "left" ? s.left_stick : s.right_stick;
      const stickTouch = pressed(s, side === "left" ? "LeftStickTouch" : "RightStickTouch");
      const stickClick = pressed(s, side === "left" ? "L3" : "R3");
      place(this.q(`[data-stick="${side}"]`), sx, sy);
      this.q(`[data-stick-area="${side}"]`).classList.toggle("touched", stickTouch);
      this.q(`[data-stick="${side}"]`).classList.toggle("on", stickClick);
      this.q(`[data-stick-label="${side}"]`).textContent = `stick ${sx}, ${sy}${stickClick ? " · click" : ""}`;

      const pad: Trackpad = side === "left" ? s.left_pad : s.right_pad;
      const dot = this.q(`[data-pad="${side}"]`);
      dot.style.visibility = pad.touched ? "visible" : "hidden";
      dot.classList.toggle("on", pad.clicked);
      dot.style.transform = `translate(-50%, -50%) scale(${1 + pad.pressure / 8000})`;
      place(dot, pad.x, pad.y);
      this.q(`[data-pad-area="${side}"]`).classList.toggle("touched", pad.touched);
      this.q(`[data-pad-label="${side}"]`).textContent = pad.touched
        ? `pad ${pad.x}, ${pad.y}${pad.clicked ? " · click" : ""}`
        : "pad –";
    }
  }

  private renderDetails(snap: Snapshot): void {
    const s = snap.state;
    const b = snap.battery;
    this.q('[data-fact="battery"]').textContent = b ? `${b.level}% · ${CHARGE_STATES[b.charge_state] ?? `state ${b.charge_state}`}` : "–";
    this.q('[data-fact="slot"]').textContent = snap.slot ? `${snap.slot} · ${snap.report_rate} reports/s` : "–";
    this.q('[data-fact="report"]').textContent = s
      ? `0x${s.report_id.toString(16)} · seq ${s.seq} · buttons 0x${s.buttons.toString(16).padStart(8, "0")}`
      : "–";
    this.q('[data-fact="imu"]').textContent = s ? `${s.imu.timestamp} µs` : "–";

    if (snap.raw.length) this.q("[data-raw]").innerHTML = this.rawHtml(snap.raw);
    this.q("[data-unknown]").innerHTML = snap.unknown_reports
      .map((u) => `<div class="section-label">Undecoded report 0x${u.id.toString(16)} <small>×${u.count}</small></div>
                   <pre class="hex">${hexLines(u.data)}</pre>`)
      .join("");
  }

  /** Hex dump, 8 bytes per line, with recently changed bytes wrapped in <mark>. */
  private rawHtml(raw: number[]): string {
    const now = performance.now();
    raw.forEach((v, i) => {
      if (this.lastRaw[i] !== v) this.byteChangedAt[i] = now;
    });
    this.lastRaw = raw;
    const cells = raw.map((v, i) => {
      const h = v.toString(16).padStart(2, "0");
      return now - (this.byteChangedAt[i] ?? 0) < BYTE_HIGHLIGHT_MS ? `<mark>${h}</mark>` : h;
    });
    return chunk(cells, 8).map((line, i) => `<span class="off">${(i * 8).toString().padStart(2, "0")}</span> ${line.join(" ")}`).join("\n");
  }
}

function hexLines(bytes: number[]): string {
  return chunk(hex(bytes).split(" "), 8).map((line, i) => `<span class="off">${(i * 8).toString().padStart(2, "0")}</span> ${line.join(" ")}`).join("\n");
}

function chunk<T>(items: T[], size: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < items.length; i += size) out.push(items.slice(i, i + size));
  return out;
}

/** Position a dot inside its square container from a signed axis pair (+y up). */
function place(dot: HTMLElement, x: number, y: number): void {
  dot.style.left = `${(x / AXIS / 2 + 0.5) * 100}%`;
  dot.style.top = `${(0.5 - y / AXIS / 2) * 100}%`;
}
