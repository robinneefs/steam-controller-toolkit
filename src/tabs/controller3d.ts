import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { MeshoptDecoder } from "three/examples/jsm/libs/meshopt_decoder.module.js";
import { toCreasedNormals } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import { pressed, type ButtonName, type ControllerState } from "../controller";

/** Valve's shell model, converted by tools/convert-model.mjs (CC BY-NC-SA 4.0, (c) Valve). */
const MODEL_URL = "/models/steam-controller.glb";

const QUAT_SCALE = 32767;
const AXIS = 32768;

const COLORS = {
  shell: 0x2b3038,
  on: 0x3fa9f5,
  touch: 0x4cc38a,
  finger: 0xf5a63f,
};

/**
 * Control positions in model space (cm; +x right, +y out of the face, +z towards the player,
 * origin at the bounding-box centre). Measured from Valve's 2:1 reference drawing
 * (steam_controller_reference_20260429.pdf): x/z from the top and bottom views, y from the
 * front view. At load each point is projected onto the shell by a ray cast along `from`.
 */
type Probe = { x: number; y?: number; z: number; from: "top" | "bottom" | "front"; r: number };
const BUTTONS: Partial<Record<ButtonName, Probe>> = {
  DpadUp: { x: -5.48, z: -3.6, from: "top", r: 0.3 },
  DpadDown: { x: -5.48, z: -2.56, from: "top", r: 0.3 },
  DpadLeft: { x: -6.0, z: -3.08, from: "top", r: 0.3 },
  DpadRight: { x: -4.96, z: -3.08, from: "top", r: 0.3 },
  Y: { x: 4.79, z: -4.03, from: "top", r: 0.38 },
  X: { x: 3.81, z: -3.08, from: "top", r: 0.38 },
  B: { x: 5.7, z: -3.08, from: "top", r: 0.38 },
  A: { x: 4.79, z: -2.14, from: "top", r: 0.38 },
  View: { x: -2.95, z: -4.25, from: "top", r: 0.3 },
  Menu: { x: 2.84, z: -4.25, from: "top", r: 0.3 },
  Steam: { x: -0.19, z: -2.99, from: "top", r: 0.45 },
  QAM: { x: 0, z: 1.08, from: "top", r: 0.3 },
  L: { x: -4.0, y: -1.07, z: 0, from: "front", r: 0.55 },
  R: { x: 4.0, y: -1.07, z: 0, from: "front", r: 0.55 },
  L4: { x: -4.19, z: 1.08, from: "bottom", r: 0.5 },
  L5: { x: -4.73, z: 3.28, from: "bottom", r: 0.5 },
  R4: { x: 4.22, z: 1.08, from: "bottom", r: 0.5 },
  R5: { x: 4.98, z: 3.28, from: "bottom", r: 0.5 },
};
const STICKS = { left: { x: -2.52, z: -1.92 }, right: { x: 2.39, z: -2.08 } };
const STICK_RADIUS = 0.6;
const PADS = { left: { x: -3.21, z: 1.08 }, right: { x: 3.18, z: 1.08 } };
const PAD_HALF = 1.45;
const TRIGGERS = { left: { x: -4.0, y: -2.65 }, right: { x: 4.0, y: -2.65 } };

type Side = "left" | "right";

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
  private ready = false;

  private buttons = new Map<ButtonName, THREE.Mesh>();
  // Filled in after the model loads; a control whose ray missed the shell stays absent.
  private sticks: Partial<Record<Side, { ring: THREE.Mesh; dot: THREE.Mesh; base: THREE.Vector3 }>> = {};
  private pads: Partial<Record<Side, { area: THREE.Mesh; finger: THREE.Mesh }>> = {};
  private triggers: Partial<Record<Side, THREE.Mesh>> = {};

  constructor(private container: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    this.renderer.setPixelRatio(window.devicePixelRatio);
    container.appendChild(this.renderer.domElement);

    this.camera.position.set(0, 13, 21);
    this.camera.lookAt(0, -1, 0);
    this.scene.add(new THREE.HemisphereLight(0xdde6f0, 0x20242a, 2.0));
    const key = new THREE.DirectionalLight(0xffffff, 2.2);
    key.position.set(5, 12, 9);
    const rim = new THREE.DirectionalLight(0x9fc4ff, 1.2);
    rim.position.set(-8, 4, -10);
    this.scene.add(key, rim);

    const grid = new THREE.GridHelper(30, 30, 0x2c333d, 0x1e232a);
    grid.position.y = -5.5;
    this.scene.add(grid);
    this.scene.add(this.model);

    new ResizeObserver(() => this.resize()).observe(container);
    this.resize();
    this.load().catch((e) => console.error("failed to load controller model", e));
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
    this.applyCalibrationView();

    if (this.ready) {
      // Calibration aid: open the page with #show-controls to light every highlight.
      const all = location.hash.startsWith("#show-controls") ? 0.6 : 0;
      for (const [name, mesh] of this.buttons) setLit(mesh, pressed(s, name) ? 0.9 : all, COLORS.on);

      for (const side of ["left", "right"] as const) {
        const [sx, sy] = side === "left" ? s.left_stick : s.right_stick;
        const st = this.sticks[side];
        if (st) {
          const touched = pressed(s, side === "left" ? "LeftStickTouch" : "RightStickTouch");
          const clicked = pressed(s, side === "left" ? "L3" : "R3");
          setLit(st.ring, clicked || touched ? 0.9 : 0.15, clicked ? COLORS.on : touched ? COLORS.touch : COLORS.on);
          st.dot.position.set(st.base.x + (sx / AXIS) * STICK_RADIUS, st.base.y + 0.05, st.base.z - (sy / AXIS) * STICK_RADIUS);
        }

        const pad = side === "left" ? s.left_pad : s.right_pad;
        const p = this.pads[side];
        if (p) {
          setLit(p.area, pad.clicked ? 0.55 : pad.touched ? 0.3 : all, pad.clicked ? COLORS.on : COLORS.touch);
          p.finger.visible = pad.touched;
          // Pad overlay lies in its local XY plane; +y local is towards the triggers.
          p.finger.position.set((pad.x / AXIS) * PAD_HALF, (pad.y / AXIS) * PAD_HALF, 0.06);
          p.finger.scale.setScalar(1 + pad.pressure / 12000);
        }

        const trig = (side === "left" ? s.trigger_left : s.trigger_right) / 32767;
        const trigClick = pressed(s, side === "left" ? "LeftTriggerClick" : "RightTriggerClick");
        setLit(this.triggers[side], trig > 0.02 ? 0.25 + trig * 0.65 : all, trigClick ? COLORS.on : COLORS.touch);
      }
    }

    this.renderer.render(this.scene, this.camera);
  }

  /**
   * Calibration aid for the BUTTONS/STICKS/PADS positions: `#show-controls` lights every
   * highlight; adding `-top` or `-bottom` shows the unrotated model straight from above/below
   * to compare with Valve's reference drawing.
   */
  private applyCalibrationView(): void {
    const view = location.hash.match(/^#show-controls-(top|bottom)$/)?.[1];
    if (!view) {
      this.camera.position.set(0, 13, 21);
      this.camera.lookAt(0, -1, 0);
      return;
    }
    this.model.quaternion.identity();
    this.camera.position.set(0, view === "top" ? 24 : -24, 0.001);
    this.camera.lookAt(0, 0, 0);
  }

  private resize(): void {
    const { clientWidth: w, clientHeight: h } = this.container;
    if (!w || !h) return;
    this.renderer.setSize(w, h);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.renderer.render(this.scene, this.camera);
  }

  private async load(): Promise<void> {
    const loader = new GLTFLoader().setMeshoptDecoder(MeshoptDecoder);
    const gltf = await loader.loadAsync(MODEL_URL);
    const src = gltf.scene.getObjectByProperty("isMesh", true) as THREE.Mesh;
    src.updateWorldMatrix(true, false);

    // Positions are quantized (normalized int16 in -1..1, scaled by the node transform). Copy
    // them to floats before baking the transform, or applyMatrix4 clamps them back to -1..1.
    const q = src.geometry.getAttribute("position");
    const floats = new Float32Array(q.count * 3);
    for (let i = 0; i < q.count; i++) {
      floats[i * 3] = q.getX(i);
      floats[i * 3 + 1] = q.getY(i);
      floats[i * 3 + 2] = q.getZ(i);
    }
    const base = new THREE.BufferGeometry();
    base.setAttribute("position", new THREE.BufferAttribute(floats, 3));
    base.setIndex(src.geometry.getIndex());
    base.applyMatrix4(src.matrixWorld);
    // Smooth normals except across sharp edges.
    const geo = toCreasedNormals(base, THREE.MathUtils.degToRad(35));
    const shell = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color: COLORS.shell, roughness: 0.72, metalness: 0.05 }));
    this.model.add(shell);
    // Ray hits come back in world space; place everything with the model unrotated so world
    // equals model space. update() re-applies the orientation on the next snapshot.
    this.model.quaternion.identity();
    this.model.updateMatrixWorld(true);

    const ray = new THREE.Raycaster();
    const hit = (origin: THREE.Vector3, dir: THREE.Vector3) => {
      ray.set(origin, dir);
      return ray.intersectObject(shell, false)[0];
    };
    const surface = (p: Probe) => {
      const h =
        p.from === "top" ? hit(new THREE.Vector3(p.x, 20, p.z), new THREE.Vector3(0, -1, 0))
        : p.from === "bottom" ? hit(new THREE.Vector3(p.x, -20, p.z), new THREE.Vector3(0, 1, 0))
        : hit(new THREE.Vector3(p.x, p.y ?? 0, -20), new THREE.Vector3(0, 0, 1));
      if (!h?.face) return undefined;
      return { point: h.point, normal: h.face.normal.clone().normalize() };
    };

    for (const [name, probe] of Object.entries(BUTTONS) as [ButtonName, Probe][]) {
      const s = surface(probe);
      if (!s) continue;
      const disc = decal(new THREE.CircleGeometry(probe.r, 32), s.point, s.normal);
      this.buttons.set(name, disc);
      this.model.add(disc);
    }

    for (const side of ["left", "right"] as const) {
      const st = surface({ ...STICKS[side], from: "top", r: 0 });
      if (st) {
        const ring = decal(new THREE.RingGeometry(STICK_RADIUS + 0.12, STICK_RADIUS + 0.28, 48), st.point, new THREE.Vector3(0, 1, 0));
        const dot = new THREE.Mesh(new THREE.SphereGeometry(0.16, 16, 12), new THREE.MeshBasicMaterial({ color: COLORS.finger }));
        dot.position.copy(st.point);
        this.sticks[side] = { ring, dot, base: st.point.clone() };
        this.model.add(ring, dot);
      }

      // Fit the pad plane from three surface points, then lay a square overlay on it.
      const c = surface({ ...PADS[side], from: "top", r: 0 });
      const px = surface({ x: PADS[side].x + 1, z: PADS[side].z, from: "top", r: 0 });
      const pz = surface({ x: PADS[side].x, z: PADS[side].z - 1, from: "top", r: 0 });
      if (c && px && pz) {
        const ax = px.point.clone().sub(c.point).normalize();
        const ay = pz.point.clone().sub(c.point).normalize(); // towards the triggers
        const n = new THREE.Vector3().crossVectors(ax, ay).normalize();
        const frame = new THREE.Group();
        frame.matrix.makeBasis(ax, ay, n).setPosition(c.point.clone().addScaledVector(n, 0.03));
        frame.matrixAutoUpdate = false;
        const area = new THREE.Mesh(new THREE.PlaneGeometry(PAD_HALF * 2, PAD_HALF * 2), overlayMaterial());
        const finger = new THREE.Mesh(new THREE.SphereGeometry(0.2, 16, 12), new THREE.MeshBasicMaterial({ color: COLORS.finger }));
        finger.visible = false;
        frame.add(area, finger);
        this.pads[side] = { area, finger };
        this.model.add(frame);
      }

      const tr = surface({ ...TRIGGERS[side], z: 0, from: "front", r: 0 });
      if (tr) {
        this.triggers[side] = decal(new THREE.CircleGeometry(0.6, 32), tr.point, tr.normal);
        this.model.add(this.triggers[side]);
      }
    }

    this.ready = true;
    this.renderer.render(this.scene, this.camera);
  }
}

/** A flat highlight lying on the shell at `point`, facing along `normal`. Invisible until lit. */
function decal(geo: THREE.BufferGeometry, point: THREE.Vector3, normal: THREE.Vector3): THREE.Mesh {
  const mesh = new THREE.Mesh(geo, overlayMaterial());
  mesh.position.copy(point).addScaledVector(normal, 0.03);
  mesh.lookAt(point.clone().addScaledVector(normal, 1));
  return mesh;
}

function overlayMaterial(): THREE.MeshBasicMaterial {
  return new THREE.MeshBasicMaterial({ color: COLORS.on, transparent: true, opacity: 0, depthWrite: false, side: THREE.DoubleSide });
}

function setLit(mesh: THREE.Mesh | undefined, opacity: number, color: number): void {
  if (!mesh) return;
  const m = mesh.material as THREE.MeshBasicMaterial;
  m.opacity = opacity;
  m.color.setHex(color);
}

/** Heading of a three.js orientation: rotation about +Y of its forward (-Z) axis. */
function headingOf(q: THREE.Quaternion): number {
  const f = new THREE.Vector3(0, 0, -1).applyQuaternion(q);
  return Math.atan2(-f.x, -f.z);
}
