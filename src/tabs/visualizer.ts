import { pressed, type ButtonName, type ControllerState, type Snapshot, type Trackpad } from "../controller";
import { setAttr, setHtml, setText } from "../dom";
import { Controller3D } from "./controller3d";
import { LiveGraph } from "./graph";
import type { Tab } from "./tab";
import { ICONS } from "./icons";

const AXIS = 32768;
const ACCEL_PER_G = 16384;
const GYRO_PER_DPS = 32768 / 2000;
const SVG = "http://www.w3.org/2000/svg";
/** Smallest stick dot move worth drawing, in gauge units (92 across, ~1 px each). */
const MIN_MOVE = 0.5;

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
      <div class="bumpers ${s}"><span class="cap trigger" data-btn="${s === "L" ? "LeftTriggerClick" : "RightTriggerClick"}" title="Trigger click">${s}T</span><span class="cap bumper" data-btn="${s}">${s}</span></div>
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
  group = "input" as const;
  icon = ICONS.visualizer;
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
                  <span class="cap" data-btn="Menu" title="Menu"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><path d="M5 7h14M5 12h14M5 17h14"/></svg></span>
                  <span class="cap steam" data-btn="Steam" title="Steam"><svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.004.105.004.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.727L.436 15.27C1.862 20.307 6.486 24 11.979 24c6.627 0 11.999-5.373 11.999-12S18.605 0 11.979 0zM7.54 18.21l-1.473-.61c.262.543.714.999 1.314 1.25 1.297.539 2.793-.076 3.332-1.375.263-.63.264-1.319.005-1.949s-.75-1.121-1.377-1.383c-.624-.26-1.29-.249-1.878-.03l1.523.63c.956.4 1.409 1.5 1.009 2.455-.397.957-1.497 1.41-2.454 1.012H7.54zm11.415-9.303c0-1.662-1.353-3.015-3.015-3.015-1.665 0-3.015 1.353-3.015 3.015 0 1.665 1.35 3.015 3.015 3.015 1.663 0 3.015-1.35 3.015-3.015zm-5.273-.005c0-1.252 1.013-2.266 2.265-2.266 1.249 0 2.266 1.014 2.266 2.266 0 1.251-1.017 2.265-2.266 2.265-1.253 0-2.265-1.014-2.265-2.265z"/></svg></span>
                  <span class="cap" data-btn="QAM" title="Quick access menu">···</span>
                  <span class="cap" data-btn="View" title="View"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="5" width="12" height="10" rx="2"/><rect x="9" y="9" width="12" height="10" rx="2"/></svg></span>
                </div>
                <div class="sys stick-clicks"><span class="cap" data-btn="L3" title="Left stick click">L3</span><span class="cap" data-btn="R3" title="Right stick click">R3</span></div>
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
        // The full u16 range, square-root scaled so light touches still show.
        title: "Pad pressure", unit: "raw", range: [0, 65535], scale: "sqrt", ticks: [1000, 10000],
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
    setText(this.q("[data-conn]"), snap.tape.playback ? "Replaying recording" : snap.state ? "Connected" : "Searching for controller…");
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
    setText(this.q("[data-lock-text]"), rotating ? "Rotating with controller" : "Front view locked");
    this.q("[data-rt-fill]").style.width = `${(s.trigger_right / 32767) * 100}%`;

    const a = this.view.angles();
    for (const k of ["pitch", "roll", "yaw"] as const) {
      setHtml(this.q(`[data-angle="${k}"]`), s.imu.quat ? `${Math.round(a[k]) || 0}<small>°</small>` : "–");
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
    setText(this.q("[data-held]"), held ? `${held} held` : "");
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
  private cx = 46;
  private cy = 46;

  constructor(fig: HTMLElement) {
    const el = gaugeSvg(fig);
    this.well = svg(el, "circle", { cx: 46, cy: 46, r: 42, class: "well" });
    svg(el, "circle", { cx: 46, cy: 46, r: 21, class: "guide" });
    svg(el, "path", { d: "M4 46H88M46 4V88", class: "guide" });
    this.line = svg(el, "line", { x1: 46, y1: 46, x2: 46, y2: 46, class: "stick-line" });
    this.dot = svg(el, "circle", { cx: 46, cy: 46, r: 9, class: "dot" });
  }

  set([x, y]: [number, number], touched: boolean, click: boolean): void {
    // A resting stick jitters by ~0.4% (0.1 px here); move the dot only for more than that.
    const cx = 46 + (x / AXIS) * 34;
    const cy = 46 - (y / AXIS) * 34;
    if (Math.abs(cx - this.cx) >= MIN_MOVE || Math.abs(cy - this.cy) >= MIN_MOVE) {
      [this.cx, this.cy] = [cx, cy];
      setAttr(this.dot, "cx", cx.toFixed(1));
      setAttr(this.dot, "cy", cy.toFixed(1));
      setAttr(this.line, "x2", cx.toFixed(1));
      setAttr(this.line, "y2", cy.toFixed(1));
    }
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
    if (!p.touched) return void this.dot.classList.toggle("click", false);
    setAttr(this.dot, "cx", (46 + (p.x / AXIS) * 36).toFixed(1));
    setAttr(this.dot, "cy", (46 - (p.y / AXIS) * 36).toFixed(1));
    setAttr(this.dot, "r", (7 + Math.min(p.pressure, 30000) / 3000).toFixed(1));
    this.dot.classList.toggle("click", p.clicked);
  }
}
