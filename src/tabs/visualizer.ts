import { pressed, type ButtonName, type ControllerState, type Snapshot, type Trackpad } from "../controller";
import { Controller3D } from "./controller3d";
import type { Tab } from "./tab";

const AXIS = 32768;
const ACCEL_PER_G = 16384;
const GYRO_PER_DPS = 32768 / 2000;

type Side = "left" | "right";

function sideHtml(side: Side): string {
  const s = side === "left" ? "L" : "R";
  return `
    <section class="side">
      <div class="row-buttons">
        <span class="btn" data-btn="${s}">${s}</span>
        <span class="btn" data-btn="${s}4">${s}4</span>
        <span class="btn" data-btn="${s}5">${s}5</span>
        <span class="btn" data-btn="${side === "left" ? "LeftGripTouch" : "RightGripTouch"}">grip</span>
      </div>
      <div class="meter" title="${side} trigger">
        <div class="meter-fill" data-trigger="${side}"></div>
        <span class="meter-label" data-trigger-label="${side}"></span>
      </div>
      <div class="pair">
        <figure>
          <div class="stick" data-stick-area="${side}"><div class="dot" data-stick="${side}"></div></div>
          <figcaption data-stick-label="${side}"></figcaption>
        </figure>
        <figure>
          <div class="pad" data-pad-area="${side}"><div class="dot" data-pad="${side}"></div></div>
          <figcaption data-pad-label="${side}"></figcaption>
        </figure>
      </div>
    </section>`;
}

export class VisualizerTab implements Tab {
  id = "visualizer";
  title = "Visualizer";
  private root!: HTMLElement;
  private view!: Controller3D;

  mount(root: HTMLElement): void {
    this.root = root;
    root.innerHTML = `
      <div class="view3d">
        <div class="view3d-canvas" data-view3d></div>
        <div class="view3d-overlay">
          <span data-angles></span>
          <button data-recenter title="Make the current heading straight ahead">Re-center</button>
        </div>
      </div>
      <div class="visualizer">
        ${sideHtml("left")}
        <section class="center">
          <div class="cluster">
            <span class="btn" data-btn="DpadUp">↑</span>
            <span class="btn" data-btn="DpadLeft">←</span>
            <span class="btn" data-btn="DpadRight">→</span>
            <span class="btn" data-btn="DpadDown">↓</span>
          </div>
          <div class="row-buttons">
            <span class="btn" data-btn="View">View</span>
            <span class="btn" data-btn="Steam">Steam</span>
            <span class="btn" data-btn="QAM">···</span>
            <span class="btn" data-btn="Menu">Menu</span>
          </div>
          <div class="cluster face">
            <span class="btn" data-btn="Y">Y</span>
            <span class="btn" data-btn="X">X</span>
            <span class="btn" data-btn="B">B</span>
            <span class="btn" data-btn="A">A</span>
          </div>
          <table class="imu">
            <tr><th></th><th>x</th><th>y</th><th>z</th></tr>
            <tr><th>accel (g)</th><td data-accel="0"></td><td data-accel="1"></td><td data-accel="2"></td></tr>
            <tr><th>gyro (°/s)</th><td data-gyro="0"></td><td data-gyro="1"></td><td data-gyro="2"></td></tr>
          </table>
          <pre class="raw" data-raw></pre>
        </section>
        ${sideHtml("right")}
      </div>`;
    this.view = new Controller3D(this.q("[data-view3d]"));
    this.q("[data-recenter]").addEventListener("click", () => this.view.recenter());
  }

  update(snap: Snapshot): void {
    this.root.classList.toggle("disconnected", !snap.state);
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

    this.root.querySelectorAll<HTMLElement>("[data-btn]").forEach((el) => {
      el.classList.toggle("on", pressed(s, el.dataset.btn as ButtonName));
    });

    for (const side of ["left", "right"] as const) {
      const trig = side === "left" ? s.trigger_left : s.trigger_right;
      const click = pressed(s, side === "left" ? "LeftTriggerClick" : "RightTriggerClick");
      this.q(`[data-trigger="${side}"]`).style.width = `${(trig / 32767) * 100}%`;
      this.q(`[data-trigger="${side}"]`).classList.toggle("on", click);
      this.q(`[data-trigger-label="${side}"]`).textContent = `${side} trigger ${Math.round((trig / 32767) * 100)}%${click ? " · click" : ""}`;

      const [sx, sy] = side === "left" ? s.left_stick : s.right_stick;
      const stickTouch = pressed(s, side === "left" ? "LeftStickTouch" : "RightStickTouch");
      const stickClick = pressed(s, side === "left" ? "L3" : "R3");
      place(this.q(`[data-stick="${side}"]`), sx, sy);
      this.q(`[data-stick-area="${side}"]`).classList.toggle("touched", stickTouch);
      this.q(`[data-stick="${side}"]`).classList.toggle("on", stickClick);
      this.q(`[data-stick-label="${side}"]`).textContent =
        `stick ${sx}, ${sy}${stickTouch ? " · touch" : ""}${stickClick ? " · click" : ""}`;

      const pad: Trackpad = side === "left" ? s.left_pad : s.right_pad;
      const dot = this.q(`[data-pad="${side}"]`);
      dot.style.visibility = pad.touched ? "visible" : "hidden";
      dot.classList.toggle("on", pad.clicked);
      dot.style.transform = `translate(-50%, -50%) scale(${1 + pad.pressure / 8000})`;
      place(dot, pad.x, pad.y);
      this.q(`[data-pad-label="${side}"]`).textContent = pad.touched
        ? `pad ${pad.x}, ${pad.y} · pressure ${pad.pressure}${pad.clicked ? " · click" : ""}`
        : "pad —";
    }

    for (let i = 0; i < 3; i++) {
      this.q(`[data-accel="${i}"]`).textContent = (s.imu.accel[i] / ACCEL_PER_G).toFixed(2);
      this.q(`[data-gyro="${i}"]`).textContent = (s.imu.gyro[i] / GYRO_PER_DPS).toFixed(1);
    }
    this.q("[data-raw]").textContent =
      `report 0x${s.report_id.toString(16)} · seq ${s.seq} · buttons 0x${s.buttons.toString(16).padStart(8, "0")}`;
  }
}

/** Position a dot inside its square container from a signed axis pair (+y up). */
function place(dot: HTMLElement, x: number, y: number): void {
  dot.style.left = `${(x / AXIS / 2 + 0.5) * 100}%`;
  dot.style.top = `${(0.5 - y / AXIS / 2) * 100}%`;
}
