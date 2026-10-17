// Extract the Steam Controller from the Sketchfab "Steam Machine + Controller" GLB, split its
// inputs into separate named parts, and write a small textured GLB for the 3D view.
//
//   node tools/convert-model.mjs <steam_machine__controller.glb> public/models/steam-controller.glb
//
// Source: "Steam Machine + Controller" by dark_igorek,
// https://sketchfab.com/3d-models/steam-machine-controller-c1fcf77d71984c11818a5e2412816705
// (CC BY 4.0). The controller's mesh-local frame already matches the app's: +x right,
// +y out of the face, +z towards the player.
//
// Output: node "steam_controller" with children "body", "details" (screws, port, LED) and one
// "part_<name>" node per input (names in PARTS). Positions are in cm, centred on the body.
// The D-pad is cut into four arms by Blender (tools/split-dpad.py): "part_dpad_up", "_down",
// "_left" and "_right". Blender is found via $BLENDER, the usual install paths, or PATH.

import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { Document, NodeIO } from "@gltf-transform/core";
import { ALL_EXTENSIONS, EXTMeshoptCompression } from "@gltf-transform/extensions";
import { compactPrimitive, prune, quantize, reorder, textureCompress } from "@gltf-transform/functions";
import { MeshoptEncoder } from "meshoptimizer";
import sharp from "sharp";

const [input, output] = process.argv.slice(2);
if (!input || !output) throw new Error("usage: node tools/convert-model.mjs <in.glb> <out.glb>");

/** Real controller width (Valve's CAD: 158.8 mm), used to scale the model to cm. */
const WIDTH_CM = 15.88;

/**
 * Expected centre of each input's island in the source mesh's local units (x right, y up,
 * z towards the player), from a connected-component analysis of the details mesh.
 */
const PARTS = {
  dpad: [-0.687, 0.179, -0.264],
  a: [0.68, 0.212, -0.146],
  b: [0.8, 0.187, -0.267],
  x: [0.559, 0.221, -0.267],
  y: [0.68, 0.201, -0.387],
  view: [-0.417, 0.212, -0.42],
  menu: [0.416, 0.212, -0.42],
  steam: [0, 0.224, -0.272],
  qam: [0, 0.161, 0.336],
  stick_l: [-0.342, 0.269, -0.109],
  stick_r: [0.341, 0.269, -0.109],
  pad_l: [-0.448, 0.152, 0.341],
  pad_r: [0.447, 0.152, 0.341],
  bumper_l: [-0.654, 0.018, -0.437],
  bumper_r: [0.652, 0.018, -0.437],
  trigger_l: [-0.662, -0.243, -0.429],
  trigger_r: [0.66, -0.243, -0.429],
  back_l4: [-0.591, -0.202, 0.359],
  back_r4: [0.59, -0.202, 0.359],
  back_l5: [-0.641, -0.231, 0.663],
  back_r5: [0.639, -0.231, 0.663],
};
const MATCH_TOLERANCE = 0.03; // source units; islands farther from every PARTS centre stay in "details"

const io = new NodeIO().registerExtensions(ALL_EXTENSIONS).registerDependencies({ "meshopt.encoder": MeshoptEncoder });
const doc = await io.read(input);
const root = doc.getRoot();

const controller = root.listNodes().find((n) => n.getName().startsWith("Steam_controller"));
if (!controller) throw new Error("no Steam_controller node in input");
const prims = controller.listChildren().flatMap((n) => n.getMesh()?.listPrimitives() ?? []);
const bodyPrim = prims.find((p) => p.getMaterial()?.getName() === "sc_body_mat");
const detailsPrim = prims.find((p) => p.getMaterial()?.getName() === "sc_details_mat");
if (!bodyPrim || !detailsPrim) throw new Error("controller materials not found");

// --- scale and centre on the body's bounding box -------------------------------------------
const bodyPos = bodyPrim.getAttribute("POSITION").getArray();
const min = [Infinity, Infinity, Infinity];
const max = [-Infinity, -Infinity, -Infinity];
for (let i = 0; i < bodyPos.length; i += 3) {
  for (let a = 0; a < 3; a++) {
    min[a] = Math.min(min[a], bodyPos[i + a]);
    max[a] = Math.max(max[a], bodyPos[i + a]);
  }
}
const centre = min.map((v, a) => (v + max[a]) / 2);
const scale = WIDTH_CM / (max[0] - min[0]);

// --- split details into islands and classify them ------------------------------------------
const dPos = detailsPrim.getAttribute("POSITION").getArray(); // still in source units here
const dIdx = detailsPrim.getIndices().getArray();
const islands = connectedIslands(dPos, dIdx);
const byPart = new Map();
const leftover = [];
for (const tris of islands) {
  const c = islandCentre(dPos, tris);
  let best = null;
  let bestDist = Infinity;
  for (const [name, p] of Object.entries(PARTS)) {
    const d = Math.hypot(c[0] - p[0], c[1] - p[1], c[2] - p[2]);
    if (d < bestDist) [best, bestDist] = [name, d];
  }
  if (bestDist <= MATCH_TOLERANCE && !byPart.has(best)) byPart.set(best, tris);
  else leftover.push(...tris);
}
const missing = Object.keys(PARTS).filter((n) => !byPart.has(n));
if (missing.length) throw new Error(`parts not found: ${missing.join(", ")}`);

// --- transform positions to cm, centred (normals are unaffected by uniform scale) ----------
for (const prim of [bodyPrim, detailsPrim]) {
  const acc = prim.getAttribute("POSITION");
  const arr = acc.getArray().slice();
  for (let i = 0; i < arr.length; i += 3) for (let a = 0; a < 3; a++) arr[i + a] = (arr[i + a] - centre[a]) * scale;
  acc.setArray(arr);
  // Textures only use TEXCOORD_0; tangents are recomputed by the renderer when needed.
  for (const sem of ["TEXCOORD_1", "TEXCOORD_2", "TEXCOORD_3", "TANGENT"]) prim.setAttribute(sem, null);
}

// --- build the new node tree ---------------------------------------------------------------
const buffer = root.listBuffers()[0];
const primWithIndices = (src, tris) => {
  const p = src.clone();
  p.setIndices(doc.createAccessor().setType("SCALAR").setArray(new Uint32Array(tris)).setBuffer(buffer));
  return compactPrimitive(p);
};
const top = doc.createNode("steam_controller");
top.addChild(doc.createNode("body").setMesh(doc.createMesh("body").addPrimitive(bodyPrim)));
top.addChild(doc.createNode("details").setMesh(doc.createMesh("details").addPrimitive(primWithIndices(detailsPrim, leftover))));
for (const name of Object.keys(PARTS)) {
  const prim = primWithIndices(detailsPrim, byPart.get(name));
  if (name === "dpad") {
    for (const [arm, armPrim] of await splitDpad(prim)) {
      top.addChild(doc.createNode(`part_dpad_${arm}`).setMesh(doc.createMesh(`part_dpad_${arm}`).addPrimitive(armPrim)));
    }
    prim.dispose();
    continue;
  }
  top.addChild(doc.createNode(`part_${name}`).setMesh(doc.createMesh(`part_${name}`).addPrimitive(prim)));
}
const scene = root.listScenes()[0];
for (const child of scene.listChildren()) scene.removeChild(child);
scene.addChild(top);
for (const m of [bodyPrim.getMaterial(), detailsPrim.getMaterial()]) m.setDoubleSided(false);

root.getAsset().extras = {
  title: "Steam Controller (from Steam Machine + Controller)",
  author: "dark_igorek (https://sketchfab.com/dark_igorek)",
  license: "CC-BY-4.0 (http://creativecommons.org/licenses/by/4.0/)",
  source: "https://sketchfab.com/3d-models/steam-machine-controller-c1fcf77d71984c11818a5e2412816705",
  changes: "Controller extracted, scaled to cm, inputs split into separate parts, textures re-encoded as WebP, geometry compressed.",
};

await MeshoptEncoder.ready;
await doc.transform(
  prune(),
  textureCompress({ encoder: sharp, targetFormat: "webp", slots: /^(?!normalTexture).*$/, quality: 85 }),
  textureCompress({ encoder: sharp, targetFormat: "webp", slots: /^normalTexture$/, quality: 92 }),
  reorder({ encoder: MeshoptEncoder }),
  quantize(),
);
doc.createExtension(EXTMeshoptCompression).setRequired(true).setEncoderOptions({
  method: EXTMeshoptCompression.EncoderMethod.QUANTIZE,
});
await io.write(output, doc);

const tris = root.listMeshes().reduce((n, m) => n + m.listPrimitives().reduce((k, p) => k + p.getIndices().getCount() / 3, 0), 0);
const { size } = await import("node:fs").then((fs) => fs.statSync(output));
console.log(`parts: ${byPart.size}, leftover details: ${leftover.length / 3} tris`);
console.log(`textures: ${root.listTextures().map((t) => `${t.getMimeType()} ${t.getSize()?.join("x")}`).join(", ")}`);
console.log(`wrote ${output}: ${tris} triangles, ${(size / 1024 / 1024).toFixed(2)} MB`);

// --- helpers --------------------------------------------------------------------------------

/** Cut the D-pad primitive into its four arms with Blender; returns [arm, primitive] pairs. */
async function splitDpad(prim) {
  const dir = mkdtempSync(join(tmpdir(), "dpad-"));
  try {
    const out = new Document();
    const outBuffer = out.createBuffer();
    const outPrim = out.createPrimitive().setIndices(out.createAccessor().setType("SCALAR").setArray(prim.getIndices().getArray()).setBuffer(outBuffer));
    for (const sem of ["POSITION", "NORMAL", "TEXCOORD_0"]) {
      const a = prim.getAttribute(sem);
      outPrim.setAttribute(sem, out.createAccessor().setType(a.getType()).setArray(a.getArray()).setNormalized(a.getNormalized()).setBuffer(outBuffer));
    }
    out.createScene().addChild(out.createNode("dpad").setMesh(out.createMesh("dpad").addPrimitive(outPrim)));
    const inPath = join(dir, "dpad.glb");
    const armsPath = join(dir, "arms.glb");
    await io.write(inPath, out);

    const script = fileURLToPath(new URL("./split-dpad.py", import.meta.url));
    execFileSync(findBlender(), ["-b", "--factory-startup", "--python", script, "--", inPath, armsPath], { stdio: ["ignore", "ignore", "inherit"] });

    const arms = await io.read(armsPath);
    const result = [];
    for (const node of arms.getRoot().listNodes()) {
      const armPrim = node.getMesh()?.listPrimitives()[0];
      if (!armPrim) continue;
      const p = doc.createPrimitive().setMaterial(prim.getMaterial());
      p.setIndices(doc.createAccessor().setType("SCALAR").setArray(new Uint32Array(armPrim.getIndices().getArray())).setBuffer(buffer));
      for (const sem of ["POSITION", "NORMAL", "TEXCOORD_0"]) {
        const a = armPrim.getAttribute(sem);
        if (!a) throw new Error(`Blender output lacks ${sem}`);
        p.setAttribute(sem, doc.createAccessor().setType(a.getType()).setArray(a.getArray().slice()).setBuffer(buffer));
      }
      result.push([node.getName().replace(/^dpad_/, ""), p]);
    }
    const names = result.map(([n]) => n).sort().join(",");
    if (names !== "down,left,right,up") throw new Error(`unexpected D-pad arms from Blender: ${names}`);
    return result;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function findBlender() {
  if (process.env.BLENDER) return process.env.BLENDER;
  const candidates = [];
  const win = "C:/Program Files/Blender Foundation";
  if (existsSync(win)) {
    const versions = readdirSync(win).filter((d) => d.startsWith("Blender")).sort().reverse();
    candidates.push(...versions.map((d) => `${win}/${d}/blender.exe`));
  }
  candidates.push("/Applications/Blender.app/Contents/MacOS/Blender");
  return candidates.find((c) => existsSync(c)) ?? "blender";
}

/** Triangle index lists of each connected component, welding vertices by position. */
function connectedIslands(pos, idx) {
  const n = pos.length / 3;
  const keys = new Map();
  const canon = new Int32Array(n);
  for (let i = 0; i < n; i++) {
    const k = `${Math.round(pos[i * 3] * 1e5)},${Math.round(pos[i * 3 + 1] * 1e5)},${Math.round(pos[i * 3 + 2] * 1e5)}`;
    if (!keys.has(k)) keys.set(k, i);
    canon[i] = keys.get(k);
  }
  const parent = Int32Array.from({ length: n }, (_, i) => i);
  const find = (x) => {
    while (parent[x] !== x) x = parent[x] = parent[parent[x]];
    return x;
  };
  for (let t = 0; t < idx.length; t += 3) {
    const a = find(canon[idx[t]]);
    for (const k of [1, 2]) {
      const b = find(canon[idx[t + k]]);
      if (a !== b) parent[b] = a;
    }
  }
  const groups = new Map();
  for (let t = 0; t < idx.length; t += 3) {
    const r = find(canon[idx[t]]);
    if (!groups.has(r)) groups.set(r, []);
    groups.get(r).push(idx[t], idx[t + 1], idx[t + 2]);
  }
  return [...groups.values()];
}

function islandCentre(pos, tris) {
  const lo = [Infinity, Infinity, Infinity];
  const hi = [-Infinity, -Infinity, -Infinity];
  for (const v of tris) {
    for (let a = 0; a < 3; a++) {
      lo[a] = Math.min(lo[a], pos[v * 3 + a]);
      hi[a] = Math.max(hi[a], pos[v * 3 + a]);
    }
  }
  return lo.map((v, a) => (v + hi[a]) / 2);
}
