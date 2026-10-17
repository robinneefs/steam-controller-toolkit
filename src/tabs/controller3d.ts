import * as THREE from "three";
import { pressed, type ButtonName, type ControllerState } from "../controller";

const QUAT_SCALE = 32767;
const AXIS = 32768;

const COLORS = {
  body: 0x2a2f36,
  part: 0x3b424c,
  on: 0x3fa9f5,
  touch: 0x4cc38a,
  finger: 0xf5a63f,
};

/**
 * 3D model of the controller, oriented by the quaternion the controller computes itself.
 *
 * Controller sensor frame: +X right, +Y forward (towards the triggers), +Z out of the face.
 * Its quaternion rotates sensor vectors into a world frame with +Z up (verified: rotating the
 * accelerometer reading gives ~(0, 0, 1 g)). Three.js uses +Y up and +Z towards the viewer, so
 * we map (x, y, z) -> (x, z, -y) for both. Yaw has no absolute reference (no magnetometer), so
 * `recenter()` zeroes it; tilt stays absolute because it is anchored to gravity.
 */
export class Controller3D {
  private renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(35, 1, 0.1, 100);
  private model = new THREE.Group();
  private yawRef = new THREE.Quaternion();
  private current = new THREE.Quaternion();
  private needsRecenter = true;
  private buttons = new Map<ButtonName, THREE.Mesh>();
  private sticks: Record<"left" | "right", THREE.Group> = { left: new THREE.Group(), right: new THREE.Group() };
  private pads: Record<"left" | "right", { pad: THREE.Mesh; finger: THREE.Mesh }>;
  private triggers: Record<"left" | "right", THREE.Group> = { left: new THREE.Group(), right: new THREE.Group() };

  constructor(private container: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setPixelRatio(window.devicePixelRatio);
    container.appendChild(this.renderer.domElement);

    this.camera.position.set(0, 11, 22);
    this.camera.lookAt(0, 0, 0);
    this.scene.add(new THREE.HemisphereLight(0xdde6f0, 0x20242a, 2.2));
    const sun = new THREE.DirectionalLight(0xffffff, 2);
    sun.position.set(4, 10, 8);
    this.scene.add(sun);

    const grid = new THREE.GridHelper(30, 30, 0x2c333d, 0x1e232a);
    grid.position.y = -4;
    this.scene.add(grid);

    this.pads = { left: this.buildPad(-4.4), right: this.buildPad(4.4) };
    this.buildBody();
    this.scene.add(this.model);

    new ResizeObserver(() => this.resize()).observe(container);
    this.resize();
  }

  /** Make the current heading the new "straight ahead". */
  recenter(): void {
    this.needsRecenter = true;
  }

  /** Orientation in degrees (pitch: nose up, roll: right side down, yaw: turned left), after recentering. */
  angles(): { pitch: number; roll: number; yaw: number } {
    const e = new THREE.Euler().setFromQuaternion(this.model.quaternion, "YXZ");
    const deg = THREE.MathUtils.radToDeg;
    return { pitch: deg(e.x), roll: -deg(e.z), yaw: deg(e.y) };
  }

  update(s: ControllerState): void {
    if (s.imu.quat) {
      const [w, x, y, z] = s.imu.quat.map((v) => v / QUAT_SCALE);
      this.current.set(x, z, -y, w).normalize();
      if (this.needsRecenter) {
        this.yawRef.setFromAxisAngle(new THREE.Vector3(0, 1, 0), headingOf(this.current));
        this.needsRecenter = false;
      }
      this.model.quaternion.copy(this.yawRef).invert().multiply(this.current);
    }

    for (const [name, mesh] of this.buttons) {
      (mesh.material as THREE.MeshStandardMaterial).color.setHex(pressed(s, name) ? COLORS.on : COLORS.part);
    }

    for (const side of ["left", "right"] as const) {
      const [sx, sy] = side === "left" ? s.left_stick : s.right_stick;
      const stick = this.sticks[side];
      stick.rotation.set((-sy / AXIS) * 0.45, 0, (-sx / AXIS) * 0.45);
      const touched = pressed(s, side === "left" ? "LeftStickTouch" : "RightStickTouch");
      const clicked = pressed(s, side === "left" ? "L3" : "R3");
      setColor(stick.children[1] as THREE.Mesh, clicked ? COLORS.on : touched ? COLORS.touch : COLORS.part);

      const pad = side === "left" ? s.left_pad : s.right_pad;
      const p = this.pads[side];
      setColor(p.pad, pad.clicked ? COLORS.on : pad.touched ? COLORS.touch : COLORS.part);
      p.finger.visible = pad.touched;
      p.finger.position.set((pad.x / AXIS) * 1.3, 0.12, (-pad.y / AXIS) * 1.3);
      p.finger.scale.setScalar(1 + pad.pressure / 12000);

      const trig = (side === "left" ? s.trigger_left : s.trigger_right) / 32767;
      this.triggers[side].rotation.x = -trig * 0.5;
      const trigClick = pressed(s, side === "left" ? "LeftTriggerClick" : "RightTriggerClick");
      setColor(this.triggers[side].children[0] as THREE.Mesh, trigClick ? COLORS.on : trig > 0.02 ? COLORS.touch : COLORS.part);
    }

    this.renderer.render(this.scene, this.camera);
  }

  private resize(): void {
    const { clientWidth: w, clientHeight: h } = this.container;
    if (!w || !h) return;
    this.renderer.setSize(w, h);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.renderer.render(this.scene, this.camera);
  }

  // --- model construction. Units ≈ cm. Face is +Y, triggers at -Z, player at +Z. ---

  private buildBody(): void {
    const shape = roundedRect(17, 7, 2.2);
    const body = new THREE.Mesh(
      new THREE.ExtrudeGeometry(shape, { depth: 1.6, bevelEnabled: true, bevelSize: 0.3, bevelThickness: 0.3, bevelSegments: 3 }),
      material(COLORS.body),
    );
    body.rotation.x = -Math.PI / 2;
    body.position.y = -1.6;
    this.model.add(body);

    for (const side of [-1, 1]) {
      const grip = new THREE.Mesh(new THREE.CapsuleGeometry(1.5, 3.5, 6, 16), material(COLORS.body));
      grip.position.set(side * 6.6, -1.8, 2.6);
      grip.rotation.set(-Math.PI / 2.6, 0, side * 0.25);
      this.model.add(grip);
    }

    const top = 0.3;
    this.addButton("DpadUp", box(0.55, 0.3, 0.8), -7, top, -1.8);
    this.addButton("DpadDown", box(0.55, 0.3, 0.8), -7, top, -0.2);
    this.addButton("DpadLeft", box(0.8, 0.3, 0.55), -7.8, top, -1);
    this.addButton("DpadRight", box(0.8, 0.3, 0.55), -6.2, top, -1);
    this.addButton("Y", sphere(0.42), 7, top, -1.9);
    this.addButton("A", sphere(0.42), 7, top, -0.1);
    this.addButton("X", sphere(0.42), 6.1, top, -1);
    this.addButton("B", sphere(0.42), 7.9, top, -1);
    this.addButton("View", box(0.6, 0.2, 0.35), -1.2, top, -2.2);
    this.addButton("Menu", box(0.6, 0.2, 0.35), 1.2, top, -2.2);
    this.addButton("Steam", box(0.9, 0.2, 0.5), 0, top, 0.6);
    this.addButton("QAM", box(0.6, 0.2, 0.35), 0, top, 1.5);

    for (const [side, x] of [["left", -3.6], ["right", 3.6]] as const) {
      const stick = this.sticks[side];
      const shaft = new THREE.Mesh(new THREE.CylinderGeometry(0.35, 0.35, 0.8, 16), material(COLORS.part));
      shaft.position.y = 0.4;
      const cap = new THREE.Mesh(new THREE.CylinderGeometry(0.9, 0.8, 0.35, 32), material(COLORS.part));
      cap.position.y = 0.9;
      stick.add(shaft, cap);
      stick.position.set(x, 0, -1.4);
      this.model.add(stick);
    }

    // Shoulder buttons and triggers on the back edge; triggers pivot down when pulled.
    for (const [side, x] of [["left", -5.6], ["right", 5.6]] as const) {
      this.addButton(side === "left" ? "L" : "R", box(2.6, 0.5, 0.7), x, -0.4, -3.9);
      const trigger = this.triggers[side];
      const blade = new THREE.Mesh(box(2.2, 0.6, 1.4), material(COLORS.part));
      blade.position.set(0, 0, -0.7);
      trigger.add(blade);
      trigger.position.set(x, -1.3, -3.6);
      this.model.add(trigger);
    }

    // Back buttons on the underside of the grips: 4 upper, 5 lower.
    this.addButton("L4", box(0.8, 0.25, 1.6), -6.0, -2.75, 0.4);
    this.addButton("L5", box(0.8, 0.25, 1.6), -6.6, -3.45, 2.4);
    this.addButton("R4", box(0.8, 0.25, 1.6), 6.0, -2.75, 0.4);
    this.addButton("R5", box(0.8, 0.25, 1.6), 6.6, -3.45, 2.4);
  }

  private buildPad(x: number): { pad: THREE.Mesh; finger: THREE.Mesh } {
    const pad = new THREE.Mesh(
      new THREE.ExtrudeGeometry(roundedRect(3, 3, 0.6), { depth: 0.12, bevelEnabled: false }),
      material(COLORS.part),
    );
    pad.rotation.x = -Math.PI / 2;
    const group = new THREE.Group();
    group.position.set(x, 0.32, 1.7);
    group.add(pad);
    const finger = new THREE.Mesh(new THREE.SphereGeometry(0.25, 16, 12), material(COLORS.finger));
    finger.visible = false;
    group.add(finger);
    this.model.add(group);
    return { pad, finger };
  }

  private addButton(name: ButtonName, geo: THREE.BufferGeometry, x: number, y: number, z: number): void {
    const mesh = new THREE.Mesh(geo, material(COLORS.part));
    mesh.position.set(x, y, z);
    this.model.add(mesh);
    this.buttons.set(name, mesh);
  }
}

/** Heading of a three.js orientation: rotation about +Y of its forward (-Z) axis. */
function headingOf(q: THREE.Quaternion): number {
  const f = new THREE.Vector3(0, 0, -1).applyQuaternion(q);
  return Math.atan2(-f.x, -f.z);
}

function material(color: number): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color, roughness: 0.7, metalness: 0.05 });
}

function setColor(mesh: THREE.Mesh, color: number): void {
  (mesh.material as THREE.MeshStandardMaterial).color.setHex(color);
}

function box(w: number, h: number, d: number): THREE.BufferGeometry {
  return new THREE.BoxGeometry(w, h, d);
}

function sphere(r: number): THREE.BufferGeometry {
  return new THREE.SphereGeometry(r, 20, 14);
}

/** Rounded rectangle centred on the origin, in the XY plane. */
function roundedRect(w: number, h: number, r: number): THREE.Shape {
  const x = -w / 2;
  const y = -h / 2;
  const s = new THREE.Shape();
  s.moveTo(x + r, y);
  s.lineTo(x + w - r, y);
  s.quadraticCurveTo(x + w, y, x + w, y + r);
  s.lineTo(x + w, y + h - r);
  s.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  s.lineTo(x + r, y + h);
  s.quadraticCurveTo(x, y + h, x, y + h - r);
  s.lineTo(x, y + r);
  s.quadraticCurveTo(x, y, x + r, y);
  return s;
}
