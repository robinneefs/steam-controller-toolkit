import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { MeshoptDecoder } from "three/examples/jsm/libs/meshopt_decoder.module.js";
import { Buttons, pressed, type ButtonName, type ControllerState } from "../controller";

/**
 * "Steam Machine + Controller" by dark_igorek (CC BY 4.0), controller extracted and split into
 * parts by tools/convert-model.mjs. Model space: cm, +x right, +y out of the face, +z towards
 * the player, origin at the body's centre.
 */
const MODEL_URL = "/models/steam-controller.glb";

const QUAT_SCALE = 32767;
const AXIS = 32768;
const MAX_PIXEL_RATIO = 1.5;

const GLOW = { press: new THREE.Color(0x1a9fff), touch: new THREE.Color(0x5fe39a) };
const FINGER_COLOR = 0xffb547;

/** Resting pose: face towards the camera, triggers on top. */
const FRONT = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), Math.PI / 2);
/** Right-trigger pull that starts / stops free rotation (hysteresis avoids flicker). */
const ROTATE_ON = 0.3;
const ROTATE_OFF = 0.15;
/** Fraction of the remaining rotation covered per snapshot, for smooth grab and release. */
const EASE = 0.25;
/** Controller size facing the camera (cm), and how much of the view it may fill. */
const MODEL_W = 16;
const MODEL_H = 11;
const FILL_W = 0.72;
const FILL_H = 0.55;
/** Camera direction from the model: straight on, slightly above. */
const CAMERA_DIR = new THREE.Vector3(0, 4, 24).normalize();

/** How far buttons travel when pressed (cm) and in which model-space direction. */
const PRESS_DEPTH = 0.1;
const DOWN = new THREE.Vector3(0, -1, 0);
const UP = new THREE.Vector3(0, 1, 0);
const BUMPER_IN = new THREE.Vector3(0, -0.6, 0.8).normalize();

type Side = "left" | "right";

/** Parts that simply move in and glow while their button is held. */
const PRESS_PARTS: [part: string, button: ButtonName, dir: THREE.Vector3][] = [
  ["a", "A", DOWN], ["b", "B", DOWN], ["x", "X", DOWN], ["y", "Y", DOWN],
  ["view", "View", DOWN], ["menu", "Menu", DOWN], ["steam", "Steam", DOWN], ["qam", "QAM", DOWN],
  ["bumper_l", "L", BUMPER_IN], ["bumper_r", "R", BUMPER_IN],
  ["back_l4", "L4", UP], ["back_l5", "L5", UP], ["back_r4", "R4", UP], ["back_r5", "R5", UP],
];

type DpadArm = "up" | "down" | "left" | "right";
const DPAD_ARMS: [arm: DpadArm, button: ButtonName][] = [
  ["up", "DpadUp"], ["down", "DpadDown"], ["left", "DpadLeft"], ["right", "DpadRight"],
];

const STICK_TILT = 0.15; // rad at full deflection
const DPAD_TILT = 0.12;
const TRIGGER_SWING = 0.35;
const PAD_HALF = 1.45; // half the trackpad's edge, cm

interface Part {
  pivot: THREE.Group;
  material: THREE.MeshStandardMaterial;
  rest: THREE.Vector3;
}

/**
 * 3D model of the controller. It rests facing the viewer; while the right trigger is held it
 * follows how the controller has turned since the trigger went down, and eases back on release.
 *
 * Controller sensor frame: +X right, +Y forward (towards the triggers), +Z out of the face.
 * Its quaternion rotates sensor vectors into a world frame with +Z up (verified: rotating the
 * accelerometer reading gives ~(0, 0, 1 g)). Three.js uses +Y up and +Z towards the viewer, so
 * we map (x, y, z) -> (x, z, -y) for both. Yaw has no absolute reference (no magnetometer), so
 * `recenter()` zeroes it for the angle readout; tilt stays absolute because it is anchored to gravity.
 *
 * Rendering is on demand: a frame is drawn only when the orientation or an input changed, or
 * the view was resized, which keeps GPU use near zero while the controller sits still.
 */
export class Controller3D {
  private renderer: THREE.WebGLRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(35, 1, 0.1, 100);
  private model = new THREE.Group();
  private yawRef = new THREE.Quaternion();
  private current = new THREE.Quaternion();
  /** Controller orientation with yaw recentred; what `angles()` reports. */
  private oriented = new THREE.Quaternion();
  /** Controller orientation when the right trigger went down, or null while the view is locked. */
  private grab: THREE.Quaternion | null = null;
  private target = FRONT.clone();
  private lastDrawnQuat = new THREE.Quaternion(NaN, NaN, NaN, NaN);
  private lastInputs: number[] = [];
  private needsRecenter = true;
  private needsDraw = true;
  private ready = false;

  private parts = new Map<string, Part>();
  private fingers: Partial<Record<Side, THREE.Mesh>> = {};

  constructor(private container: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, MAX_PIXEL_RATIO));
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    container.appendChild(this.renderer.domElement);

    this.camera.position.copy(CAMERA_DIR).multiplyScalar(30);
    this.camera.lookAt(0, 0, 0);
    this.scene.add(new THREE.HemisphereLight(0xe8eef5, 0x1a1d22, 2.2));
    const key = new THREE.DirectionalLight(0xffffff, 2.4);
    key.position.set(5, 12, 9);
    const rim = new THREE.DirectionalLight(0x9fc4ff, 1.4);
    rim.position.set(-8, 4, -10);
    this.scene.add(key, rim);

    this.model.quaternion.copy(FRONT);
    this.scene.add(this.model);

    new ResizeObserver(() => this.resize()).observe(container);
    this.resize();
    this.load().catch((e) => console.error("failed to load controller model", e));
  }

  /** True while the right trigger is held and the model follows the controller. */
  get rotating(): boolean {
    return this.grab !== null;
  }

  /** Make the current heading the new "straight ahead" for `angles()`. */
  recenter(): void {
    this.needsRecenter = true;
  }

  /** Orientation in degrees (pitch: nose up, roll: right side down, yaw: turned left), after recentering. */
  angles(): { pitch: number; roll: number; yaw: number } {
    const e = new THREE.Euler().setFromQuaternion(this.oriented, "YXZ");
    const deg = THREE.MathUtils.radToDeg;
    return { pitch: deg(e.x), roll: -deg(e.z), yaw: deg(e.y) };
  }

  update(s: ControllerState): void {
    if (location.hash === "#demo") s = demoState(performance.now() / 1000, s);

    if (s.imu.quat) {
      const [w, x, y, z] = s.imu.quat.map((v) => v / QUAT_SCALE);
      this.current.set(x, z, -y, w).normalize();
      if (this.needsRecenter) {
        this.yawRef.setFromAxisAngle(new THREE.Vector3(0, 1, 0), headingOf(this.current));
        this.needsRecenter = false;
      }
      this.oriented.copy(this.yawRef).invert().multiply(this.current);
    }

    const pull = s.trigger_right / 32767;
    if (!this.grab && pull > ROTATE_ON && s.imu.quat) {
      this.grab = this.current.clone();
    } else if (this.grab && (pull < ROTATE_OFF || !s.imu.quat)) {
      this.grab = null;
    }
    if (this.grab) {
      // The pose held when RT was pulled becomes the resting pose (FRONT), and the model copies
      // every turn since then about the controller's own axes: grab⁻¹ * current is that turn in
      // the controller's body frame. As if the model were glued to the controller, so it is
      // right however the controller is held (flat, tilted or upright) and needs no heading,
      // which the IMU cannot give (no magnetometer).
      this.target.copy(FRONT).multiply(this.grab.clone().invert()).multiply(this.current);
    } else {
      this.target.copy(FRONT);
    }
    this.model.quaternion.slerp(this.target, EASE);

    // Draw only when something visible changed. Sensor noise moves the quaternion by ~1e-5 and
    // the sticks by ~0.4%; the thresholds (about 0.1°, 1%) ignore that but track real motion.
    const inputs = inputValues(s);
    const moved = Math.abs(this.model.quaternion.dot(this.lastDrawnQuat)) < 0.9999995;
    if (!moved && !inputsMoved(inputs, this.lastInputs) && !this.needsDraw) return;
    this.lastDrawnQuat.copy(this.model.quaternion);
    this.lastInputs = inputs;
    this.needsDraw = false;

    if (this.ready) this.poseParts(s);
    this.renderer.render(this.scene, this.camera);
  }

  private poseParts(s: ControllerState): void {
    for (const [name, button, dir] of PRESS_PARTS) {
      const on = pressed(s, button);
      this.press(name, dir, on ? PRESS_DEPTH : 0);
      this.glow(name, on ? GLOW.press : null, 0.6);
    }

    // The D-pad is one rocker cut into four arms: all arms tilt together about the cross's centre
    // so the pressed arm dips, and only the pressed arm lights up.
    // Arms point -z (up), +z (down), -x (left), +x (right).
    const arm = Object.fromEntries(DPAD_ARMS.map(([a, button]) => [a, pressed(s, button)])) as Record<DpadArm, boolean>;
    const tiltX = ((arm.down ? 1 : 0) - (arm.up ? 1 : 0)) * DPAD_TILT;
    const tiltZ = ((arm.left ? 1 : 0) - (arm.right ? 1 : 0)) * DPAD_TILT;
    for (const [a] of DPAD_ARMS) {
      this.parts.get(`dpad_${a}`)?.pivot.rotation.set(tiltX, 0, tiltZ);
      this.glow(`dpad_${a}`, arm[a] ? GLOW.press : null, 0.6);
    }

    for (const side of ["left", "right"] as const) {
      const sfx = side === "left" ? "l" : "r";

      const stick = this.parts.get(`stick_${sfx}`);
      if (stick) {
        const [sx, sy] = side === "left" ? s.left_stick : s.right_stick;
        const clicked = pressed(s, side === "left" ? "L3" : "R3");
        const touched = pressed(s, side === "left" ? "LeftStickTouch" : "RightStickTouch");
        stick.pivot.rotation.set((-sy / AXIS) * STICK_TILT, 0, (-sx / AXIS) * STICK_TILT);
        stick.pivot.position.copy(stick.rest).addScaledVector(DOWN, clicked ? PRESS_DEPTH : 0);
        this.glow(`stick_${sfx}`, clicked ? GLOW.press : touched ? GLOW.touch : null, clicked ? 0.6 : 0.35);
      }

      const pad = side === "left" ? s.left_pad : s.right_pad;
      const padPart = this.parts.get(`pad_${sfx}`);
      if (padPart) {
        padPart.pivot.position.copy(padPart.rest).addScaledVector(DOWN, pad.clicked ? PRESS_DEPTH * 0.5 : 0);
        this.glow(`pad_${sfx}`, pad.clicked ? GLOW.press : pad.touched ? GLOW.touch : null, pad.clicked ? 0.5 : 0.3);
      }
      const finger = this.fingers[side];
      if (finger) {
        finger.visible = pad.touched;
        // Finger frame: local x right, local y towards the triggers, local z out of the pad.
        finger.position.set((pad.x / AXIS) * PAD_HALF, (pad.y / AXIS) * PAD_HALF, 0.08);
        finger.scale.setScalar(1 + pad.pressure / 12000);
      }

      const trigger = this.parts.get(`trigger_${sfx}`);
      if (trigger) {
        const pull = (side === "left" ? s.trigger_left : s.trigger_right) / 32767;
        const clicked = pressed(s, side === "left" ? "LeftTriggerClick" : "RightTriggerClick");
        trigger.pivot.rotation.x = -pull * TRIGGER_SWING;
        this.glow(`trigger_${sfx}`, clicked ? GLOW.press : pull > 0.02 ? GLOW.touch : null, 0.25 + pull * 0.35);
      }
    }
  }

  private press(name: string, dir: THREE.Vector3, depth: number): void {
    const p = this.parts.get(name);
    if (p) p.pivot.position.copy(p.rest).addScaledVector(dir, depth);
  }

  private glow(name: string, color: THREE.Color | null, intensity: number): void {
    const p = this.parts.get(name);
    if (!p) return;
    p.material.emissive.copy(color ?? BLACK);
    p.material.emissiveIntensity = color ? intensity : 0;
  }

  private resize(): void {
    const { clientWidth: w, clientHeight: h } = this.container;
    if (!w || !h) return;
    this.renderer.setSize(w, h);
    this.camera.aspect = w / h;
    // Back off far enough that the controller fits both ways, leaving room for the overlays.
    const halfFov = Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2)) * 2;
    const dist = Math.max(MODEL_H / FILL_H, MODEL_W / FILL_W / this.camera.aspect) / halfFov;
    this.camera.position.copy(CAMERA_DIR).multiplyScalar(dist);
    this.camera.updateProjectionMatrix();
    this.renderer.render(this.scene, this.camera);
    this.needsDraw = true;
  }

  private async load(): Promise<void> {
    const gltf = await new GLTFLoader().setMeshoptDecoder(MeshoptDecoder).loadAsync(MODEL_URL);
    // Build everything with the model unrotated, so world space equals model space while we
    // measure parts, then put it back.
    const pose = this.model.quaternion.clone();
    this.model.quaternion.identity();
    this.model.add(gltf.scene);
    this.model.updateMatrixWorld(true);

    // Collect first: re-parenting while traversing would revisit moved meshes.
    const partMeshes: THREE.Mesh[] = [];
    gltf.scene.traverse((o) => {
      if ((o as THREE.Mesh).isMesh && o.name.startsWith("part_")) partMeshes.push(o as THREE.Mesh);
    });
    // The D-pad's arms share one pivot, the centre of the whole cross.
    const dpadBox = new THREE.Box3();
    for (const mesh of partMeshes) if (mesh.name.startsWith("part_dpad_")) dpadBox.expandByObject(mesh);
    for (const mesh of partMeshes) {
      const name = mesh.name.slice("part_".length);
      // Own material per part so its glow does not light up the rest of the details.
      const material = (mesh.material as THREE.MeshStandardMaterial).clone();
      mesh.material = material;
      const box = new THREE.Box3().setFromObject(mesh);
      const pivot = new THREE.Group();
      pivot.position.copy(name.startsWith("dpad_") ? dpadBox.getCenter(new THREE.Vector3()) : pivotFor(name, box));
      mesh.parent!.add(pivot);
      pivot.updateMatrixWorld(true);
      pivot.attach(mesh); // keeps the mesh where it is, now rotating about the pivot
      this.parts.set(name, { pivot, material, rest: pivot.position.clone() });
    }

    for (const side of ["left", "right"] as const) {
      const pad = this.parts.get(`pad_${side === "left" ? "l" : "r"}`);
      if (pad) this.fingers[side] = this.addFinger(pad.pivot);
    }

    this.model.quaternion.copy(pose);
    this.ready = true;
    this.needsDraw = true;
    this.renderer.render(this.scene, this.camera);
  }

  /** A dot that slides over a trackpad's surface. The frame is fitted to the pad by ray casts. */
  private addFinger(padPivot: THREE.Group): THREE.Mesh | undefined {
    const mesh = padPivot.children[0] as THREE.Mesh;
    const box = new THREE.Box3().setFromObject(mesh);
    const c = box.getCenter(new THREE.Vector3());
    const ray = new THREE.Raycaster();
    const surface = (x: number, z: number) => {
      ray.set(new THREE.Vector3(x, box.max.y + 5, z), DOWN);
      return ray.intersectObject(mesh, false)[0]?.point;
    };
    const o = surface(c.x, c.z);
    const px = surface(c.x + 1, c.z);
    const pz = surface(c.x, c.z - 1);
    if (!o || !px || !pz) return undefined;
    const ax = px.clone().sub(o).normalize();
    const ay = pz.clone().sub(o).normalize();
    const n = new THREE.Vector3().crossVectors(ax, ay).normalize();
    const frame = new THREE.Group();
    frame.matrix.makeBasis(ax, ay, n).setPosition(padPivot.worldToLocal(o.clone()));
    frame.matrixAutoUpdate = false;
    const finger = new THREE.Mesh(new THREE.SphereGeometry(0.22, 16, 12), new THREE.MeshBasicMaterial({ color: FINGER_COLOR }));
    finger.visible = false;
    frame.add(finger);
    padPivot.add(frame);
    return finger;
  }
}

const BLACK = new THREE.Color(0x000000);

/** Point each part rotates about: sticks pivot below their cap, triggers hinge at the top front. */
function pivotFor(name: string, box: THREE.Box3): THREE.Vector3 {
  const c = box.getCenter(new THREE.Vector3());
  if (name.startsWith("stick_")) return new THREE.Vector3(c.x, box.min.y - 1.0, c.z);
  if (name.startsWith("trigger_")) return new THREE.Vector3(c.x, box.max.y, box.min.z);
  return c;
}

/** Cheap fingerprint of everything that changes what the parts look like. */
/** The inputs the model shows: buttons first, then the analog values. */
function inputValues(s: ControllerState): number[] {
  return [
    s.buttons, s.trigger_left, s.trigger_right, ...s.left_stick, ...s.right_stick,
    s.left_pad.x, s.left_pad.y, s.left_pad.pressure, s.right_pad.x, s.right_pad.y, s.right_pad.pressure,
  ];
}

/** Analog change worth a frame: ~1% of full scale (a stick tilts 0.1° for it). */
const ANALOG_STEP = 330;

/**
 * Whether the inputs moved since the last drawn frame. Analog values are compared with the
 * values last drawn, not rounded into buckets: a resting stick jitters by ~0.4%, which crossed a
 * bucket edge on most reports and redrew the view ~50 times a second while nothing moved.
 */
function inputsMoved(now: number[], drawn: number[]): boolean {
  if (now[0] !== drawn[0]) return true;
  for (let i = 1; i < now.length; i++) if (!(Math.abs(now[i] - drawn[i]) < ANALOG_STEP)) return true;
  return false;
}

/** Debug aid (#demo in the URL): cycles every input so the part motions can be checked. */
function demoState(t: number, s: ControllerState): ControllerState {
  const order: ButtonName[] = ["A", "B", "X", "Y", "DpadUp", "DpadRight", "DpadDown", "DpadLeft", "View", "Menu", "Steam", "QAM", "L", "R", "L4", "L5", "R4", "R5", "L3", "R3"];
  const held = order[Math.floor(t * 1.5) % order.length];
  const wave = (Math.sin(t * 2) + 1) / 2;
  const circle = (r: number): [number, number] => [Math.round(Math.cos(t * 2) * r), Math.round(Math.sin(t * 2) * r)];
  const [px, py] = circle(26000);
  const pad = { touched: true, clicked: false, x: px, y: py, pressure: Math.round(wave * 20000) };
  return {
    ...s,
    buttons: Buttons[held] | Buttons.LeftPadTouch | Buttons.RightPadTouch,
    trigger_left: Math.round(wave * 32767),
    trigger_right: Math.round((1 - wave) * 32767),
    left_stick: circle(30000),
    right_stick: circle(-30000),
    left_pad: pad,
    right_pad: { ...pad, x: -pad.x },
  };
}

/** Heading of a three.js orientation: rotation about +Y of its forward (-Z) axis. */
function headingOf(q: THREE.Quaternion): number {
  const f = new THREE.Vector3(0, 0, -1).applyQuaternion(q);
  return Math.atan2(-f.x, -f.z);
}
