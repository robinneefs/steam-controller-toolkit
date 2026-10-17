import { pressed, type ButtonName, type ControllerState, type Snapshot, type Trackpad } from "../controller";
import { Controller3D } from "./controller3d";
import { LiveGraph } from "./graph";
import type { Tab } from "./tab";

const AXIS = 32768;
const ACCEL_PER_G = 16384;
const GYRO_PER_DPS = 32768 / 2000;
const SVG = "http://www.w3.org/2000/svg";

const TOUCH_SENSORS: [ButtonName, string][] = [
  ["LeftGripTouch", "L grip"], ["RightGripTouch", "R grip"],
  ["LeftStickTouch", "L stick"], ["RightStickTouch", "R stick"],
  ["LeftPadTouch", "L pad"], ["RightPadTouch", "R pad"],
];

const ARROW = {
  up: "M6 2l4 6H2z", left: "M2 6l6-4v8z", right: "M10 6L4 2v8z", down: "M6 10L2 4h8z",
};

function arm(btn: ButtonName, dir: keyof typeof ARROW): string {
  return `<span class="arm" data-btn="${btn}"><svg viewBox="0 0 12 12" fill="currentColor"><path d="${ARROW[dir]}"/></svg></span>`;
}

function shoulders(s: "L" | "R"): string {
  return `<div class="shoulders">
      <span class="cap bumper" data-btn="${s}">${s}</span>
      <div class="grips"><span class="cap grip" data-btn="${s}4" title="Upper back button">${s}4</span><span class="cap grip" data-btn="${s}5" title="Lower back button">${s}5</span></div>
    </div>`;
}

const GRAPHS = [
  ["gyro", "Gyro"], ["accel", "Accel"], ["triggers", "Triggers"], ["pressure", "Pad pressure"],
] as const;
type GraphId = (typeof GRAPHS)[number][0];

export class VisualizerTab implements Tab {
  id = "visualizer";
  title = "Visualizer";
  private root!: HTMLElement;
  private view!: Controller3D;
  private graphs!: Record<GraphId, LiveGraph>;
  private gauges!: { lt: Ring; rt: Ring; ls: Stick; rs: Stick; lp: Pad; rp: Pad };

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="visualizer">
        <section class="hero">
          <div class="view3d-canvas" data-view3d></div>
          <div class="hero-title"><small data-conn>Searching</small><b>Steam Controller</b></div>
          <div class="lock" data-lock>
            <span class="lock-icon"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg></span>
            <span data-lock-text>Front view locked</span>
            <span class="lock-hint">· hold <span class="rt-chip"><i><b data-rt-fill></b></i>RT</span> to rotate</span>
          </div>
          <div class="angles">
            <div><span>Pitch</span><b class="num" data-angle="pitch" data-copy>–</b></div>
            <div><span>Roll</span><b class="num" data-angle="roll" data-copy>–</b></div>
            <div><span>Yaw</span><b class="num" data-angle="yaw" data-copy>–</b></div>
          </div>
          <button class="gbtn hero-action" data-recenter title="Make the current heading yaw 0°">Re-center yaw</button>
        </section>
        <div class="tiles">
          <section class="tile analog">
            <h3>Analog</h3>
            <div class="gauges">
              <figure data-gauge="lt"><figcaption>LT</figcaption></figure>
              <figure data-gauge="rt"><figcaption>RT</figcaption></figure>
              <figure data-gauge="ls"><figcaption>L stick</figcaption></figure>
              <figure data-gauge="rs"><figcaption>R stick</figcaption></figure>
              <figure data-gauge="lp"><figcaption>L pad</figcaption></figure>
              <figure data-gauge="rp"><figcaption>R pad</figcaption></figure>
            </div>
          </section>
          <section class="tile motion">
            <h3>Motion
              <span class="segmented">${GRAPHS.map(([id, label]) => `<button data-pick="${id}">${label}</button>`).join("")}</span>
            </h3>
            ${GRAPHS.map(([id]) => `<div class="graph-slot" data-graph="${id}"></div>`).join("")}
          </section>
          <section class="tile buttons">
            <h3>Buttons <em data-held></em></h3>
            <div class="pad-cluster">
              <div class="side-c">
                <div class="diamond">${arm("DpadUp", "up")}${arm("DpadLeft", "left")}${arm("DpadRight", "right")}${arm("DpadDown", "down")}</div>
                ${shoulders("L")}
              </div>
              <div class="center-c">
                <div class="sys">
                  <span class="cap" data-btn="View" title="View"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="5" width="12" height="10" rx="2"/><rect x="9" y="9" width="12" height="10" rx="2"/></svg></span>
                  <span class="cap steam" data-btn="Steam" title="Steam"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="8"/><circle cx="15" cy="10" r="2.5"/><path d="M4.5 14.5l6 2.5"/></svg></span>
                  <span class="cap" data-btn="QAM" title="Quick access menu">···</span>
                  <span class="cap" data-btn="Menu" title="Menu"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><path d="M5 7h14M5 12h14M5 17h14"/></svg></span>
                </div>
                <div class="touch">${TOUCH_SENSORS.map(([b, label]) => `<span data-touch="${b}">${label}</span>`).join("")}</div>
              </div>
              <div class="side-c">
                <div class="diamond">
                  <span class="face" data-btn="Y">Y</span><span class="face" data-btn="X">X</span><span class="face" data-btn="B">B</span><span class="face" data-btn="A">A</span>
                </div>
                ${shoulders("R")}
              </div>
            </div>
          </section>
        </div>
      </div>`;

    this.view = new Controller3D(this.q("[data-view3d]"));
    this.q("[data-recenter]").addEventListener("click", () => this.view.recenter());

    const g = (id: GraphId) => this.q(`[data-graph="${id}"]`);
    this.graphs = {
      gyro: new LiveGraph(g("gyro"), {
        // Fixed at the sensor's full scale: an auto range would rescale (and so redraw) past data.
        title: "Gyro", unit: "°/s", range: [-2000, 2000], scale: "sqrt", ticks: [500],
        series: [{ name: "pitch", color: "--series-1" }, { name: "roll", color: "--series-2" }, { name: "yaw", color: "--series-3" }],
      }),
      accel: new LiveGraph(g("accel"), {
        title: "Accel", unit: "g", range: [-2, 2], decimals: 2,
        series: [{ name: "x", color: "--series-1" }, { name: "y", color: "--series-2" }, { name: "z", color: "--series-3" }],
      }),
      triggers: new LiveGraph(g("triggers"), {
        title: "Triggers", unit: "%", range: [0, 100],
        series: [{ name: "L", color: "--series-1" }, { name: "R", color: "--series-2" }],
      }),
      pressure: new LiveGraph(g("pressure"), {
        title: "Pad pressure", unit: "raw", minSpan: 1000, positive: true,
        series: [{ name: "L", color: "--series-1" }, { name: "R", color: "--series-2" }],
      }),
    };
    this.root.querySelectorAll<HTMLElement>("[data-pick]").forEach((b) =>
      b.addEventListener("click", () => this.pickGraph(b.dataset.pick as GraphId)),
    );
    this.pickGraph("gyro");

    const fig = (id: string) => this.q(`[data-gauge="${id}"]`);
    this.gauges = {
      lt: new Ring(fig("lt")), rt: new Ring(fig("rt")),
      ls: new Stick(fig("ls")), rs: new Stick(fig("rs")),
      lp: new Pad(fig("lp")), rp: new Pad(fig("rp")),
    };
  }

  update(snap: Snapshot): void {
    this.root.classList.toggle("disconnected", !snap.state);
    this.q("[data-conn]").textContent = snap.state ? "Connected" : "Searching for controller…";
    if (snap.state) this.render(snap.state);
  }

  private pickGraph(id: GraphId): void {
    this.root.querySelectorAll<HTMLElement>("[data-pick]").forEach((b) => b.classList.toggle("on", b.dataset.pick === id));
    this.root.querySelectorAll<HTMLElement>("[data-graph]").forEach((el) => (el.hidden = el.dataset.graph !== id));
  }

  private q<T extends HTMLElement = HTMLElement>(sel: string): T {
    return this.root.querySelector(sel) as T;
  }

  private render(s: ControllerState): void {
    this.view.update(s);

    const lock = this.q("[data-lock]");
    const rotating = this.view.rotating;
    lock.classList.toggle("free", rotating);
    this.q("[data-lock-text]").textContent = rotating ? "Rotating with controller" : "Front view locked";
    this.q("[data-rt-fill]").style.width = `${(s.trigger_right / 32767) * 100}%`;

    const a = this.view.angles();
    for (const k of ["pitch", "roll", "yaw"] as const) {
      this.q(`[data-angle="${k}"]`).innerHTML = s.imu.quat ? `${Math.round(a[k]) || 0}<small>°</small>` : "–";
    }

    this.graphs.gyro.push(s.imu.gyro.map((v) => v / GYRO_PER_DPS));
    this.graphs.accel.push(s.imu.accel.map((v) => v / ACCEL_PER_G));
    this.graphs.triggers.push([(s.trigger_left / 32767) * 100, (s.trigger_right / 32767) * 100]);
    this.graphs.pressure.push([s.left_pad.pressure, s.right_pad.pressure]);

    let held = 0;
    this.root.querySelectorAll<HTMLElement>("[data-btn]").forEach((el) => {
      const on = pressed(s, el.dataset.btn as ButtonName);
      el.classList.toggle("on", on);
      if (on) held++;
    });
    this.q("[data-held]").textContent = held ? `${held} held` : "";
    this.root.querySelectorAll<HTMLElement>("[data-touch]").forEach((el) => {
      el.classList.toggle("on", pressed(s, el.dataset.touch as ButtonName));
    });

    const g = this.gauges;
    g.lt.set(s.trigger_left / 32767, pressed(s, "LeftTriggerClick"));
    g.rt.set(s.trigger_right / 32767, pressed(s, "RightTriggerClick"));
    g.ls.set(s.left_stick, pressed(s, "LeftStickTouch"), pressed(s, "L3"));
    g.rs.set(s.right_stick, pressed(s, "RightStickTouch"), pressed(s, "R3"));
    g.lp.set(s.left_pad);
    g.rp.set(s.right_pad);
  }
}

function svg(parent: Element, tag: string, attrs: Record<string, string | number>): SVGElement {
  const el = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  parent.appendChild(el);
  return el;
}

function gaugeSvg(fig: HTMLElement): SVGSVGElement {
  const el = document.createElementNS(SVG, "svg");
  el.setAttribute("viewBox", "0 0 92 92");
  fig.prepend(el);
  return el;
}

/** Trigger pull as a ring that fills clockwise; turns warm when the trigger clicks. */
class Ring {
  private static R = 38;
  private arc: SVGElement;
  private label: SVGElement;
  private last = "";

  constructor(fig: HTMLElement) {
    const el = gaugeSvg(fig);
    svg(el, "circle", { cx: 46, cy: 46, r: Ring.R, class: "ring-track" });
    // Inline style, not the attribute: a stylesheet stroke-dasharray would override an attribute.
    this.arc = svg(el, "circle", { cx: 46, cy: 46, r: Ring.R, class: "ring-fill", transform: "rotate(-90 46 46)" });
    this.arc.style.strokeDasharray = `0 ${2 * Math.PI * Ring.R}`;
    this.arc.style.visibility = "hidden";
    this.label = svg(el, "text", { x: 46, y: 53, "text-anchor": "middle", class: "ring-label" });
  }

  set(pull: number, click: boolean): void {
    const pct = Math.round(pull * 100);
    const key = `${pct}${click}`;
    if (key === this.last) return;
    this.last = key;
    const c = 2 * Math.PI * Ring.R;
    // A round cap draws a dot even at length 0; hide the arc entirely when released.
    this.arc.style.strokeDasharray = `${(c * pct) / 100} ${c}`;
    this.arc.style.visibility = pct > 0 ? "visible" : "hidden";
    this.arc.classList.toggle("click", click);
    this.label.textContent = `${pct}%`;
  }
}

/** Stick position in a round well; outlined when touched, warm when clicked. */
class Stick {
  private well: SVGElement;
  private line: SVGElement;
  private dot: SVGElement;

  constructor(fig: HTMLElement) {
    const el = gaugeSvg(fig);
    this.well = svg(el, "circle", { cx: 46, cy: 46, r: 42, class: "well" });
    svg(el, "circle", { cx: 46, cy: 46, r: 21, class: "guide" });
    svg(el, "path", { d: "M4 46H88M46 4V88", class: "guide" });
    this.line = svg(el, "line", { x1: 46, y1: 46, x2: 46, y2: 46, class: "stick-line" });
    this.dot = svg(el, "circle", { cx: 46, cy: 46, r: 9, class: "dot" });
  }

  set([x, y]: [number, number], touched: boolean, click: boolean): void {
    const cx = 46 + (x / AXIS) * 34;
    const cy = 46 - (y / AXIS) * 34;
    this.dot.setAttribute("cx", cx.toFixed(1));
    this.dot.setAttribute("cy", cy.toFixed(1));
    this.line.setAttribute("x2", cx.toFixed(1));
    this.line.setAttribute("y2", cy.toFixed(1));
    this.well.classList.toggle("touched", touched);
    this.dot.classList.toggle("click", click);
  }
}

/** Trackpad touch point; grows with pressure, warm when clicked. */
class Pad {
  private area: SVGElement;
  private dot: SVGElement;

  constructor(fig: HTMLElement) {
    const el = gaugeSvg(fig);
    this.area = svg(el, "rect", { x: 5, y: 5, width: 82, height: 82, rx: 16, class: "well" });
    svg(el, "path", { d: "M5 46H87M46 5V87", class: "guide" });
    this.dot = svg(el, "circle", { cx: 46, cy: 46, r: 8, class: "dot" });
  }

  set(p: Trackpad): void {
    this.area.classList.toggle("touched", p.touched);
    this.dot.style.visibility = p.touched ? "visible" : "hidden";
    this.dot.setAttribute("cx", (46 + (p.x / AXIS) * 36).toFixed(1));
    this.dot.setAttribute("cy", (46 - (p.y / AXIS) * 36).toFixed(1));
    this.dot.setAttribute("r", (7 + Math.min(p.pressure, 30000) / 3000).toFixed(1));
    this.dot.classList.toggle("click", p.clicked);
  }
}
