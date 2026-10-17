// Convert Valve's Steam Controller STL into a small, compressed GLB for the 3D view.
//
//   node tools/convert-model.mjs <input.stl> <output.glb> [--triangles 120000] [--info]
//
// Source: https://github.com/ValveSoftware/SteamController (CC BY-NC-SA 4.0, (c) Valve).
// Steps: parse binary STL -> mm to cm -> re-orient to the app's frame (see ORIENT) -> weld
// duplicate vertices -> simplify with meshoptimizer -> quantize -> meshopt-compress.
// --info only prints the raw bounding box, to work out ORIENT for a new revision.

import { readFileSync } from "node:fs";
import { Document, NodeIO } from "@gltf-transform/core";
import { ALL_EXTENSIONS, EXTMeshoptCompression } from "@gltf-transform/extensions";
import { quantize, reorder, simplify, weld } from "@gltf-transform/functions";
import { MeshoptEncoder, MeshoptSimplifier } from "meshoptimizer";

const args = process.argv.slice(2);
const [input, output] = args;
const opt = (name, def) => {
  const i = args.indexOf(name);
  return i >= 0 ? Number(args[i + 1]) : def;
};
const targetTriangles = opt("--triangles", 120000);
const infoOnly = args.includes("--info");

/**
 * Maps a raw STL point (mm) to app space (cm): +x right, +y out of the face, +z towards the
 * player (three.js convention; matches Controller3D's mapping of the sensor frame).
 * Each entry: [source axis index, sign].
 */
const ORIENT = [
  [0, 1],
  [1, 1],
  [2, 1],
];
const SCALE = 0.1; // mm -> cm

function readStl(path) {
  const buf = readFileSync(path);
  const count = buf.readUInt32LE(80);
  if (84 + count * 50 !== buf.length) throw new Error("not a binary STL (size mismatch)");
  const pos = new Float32Array(count * 9);
  for (let t = 0; t < count; t++) {
    const base = 84 + t * 50 + 12; // skip the facet normal
    for (let k = 0; k < 9; k++) pos[t * 9 + k] = buf.readFloatLE(base + k * 4);
  }
  return pos;
}

function bbox(pos) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < pos.length; i += 3) {
    for (let a = 0; a < 3; a++) {
      min[a] = Math.min(min[a], pos[i + a]);
      max[a] = Math.max(max[a], pos[i + a]);
    }
  }
  return { min, max, size: max.map((v, a) => v - min[a]) };
}

const raw = readStl(input);
const rawBox = bbox(raw);
console.log(`STL: ${raw.length / 9} triangles`);
console.log("raw bbox (mm):", rawBox.min.map((v) => v.toFixed(1)), "..", rawBox.max.map((v) => v.toFixed(1)),
  "size", rawBox.size.map((v) => v.toFixed(1)));
if (infoOnly) process.exit(0);

// Re-orient, scale, and centre on the bounding-box centre.
const centre = rawBox.min.map((v, a) => (v + rawBox.max[a]) / 2);
const pos = new Float32Array(raw.length);
for (let i = 0; i < raw.length; i += 3) {
  for (let a = 0; a < 3; a++) {
    const [src, sign] = ORIENT[a];
    pos[i + a] = sign * (raw[i + src] - centre[src]) * SCALE;
  }
}
// Mirroring flips triangle winding; swap two vertices per triangle to keep faces outward.
const det = determinant(ORIENT);
if (det < 0) {
  for (let t = 0; t < pos.length; t += 9) {
    for (let k = 0; k < 3; k++) [pos[t + 3 + k], pos[t + 6 + k]] = [pos[t + 6 + k], pos[t + 3 + k]];
  }
}

const doc = new Document();
const buffer = doc.createBuffer();
const prim = doc
  .createPrimitive()
  .setAttribute("POSITION", doc.createAccessor().setType("VEC3").setArray(pos).setBuffer(buffer))
  .setMaterial(doc.createMaterial("shell").setBaseColorFactor([0.17, 0.19, 0.22, 1]).setRoughnessFactor(0.75));
const mesh = doc.createMesh("steam_controller").addPrimitive(prim);
doc.createScene().addChild(doc.createNode("steam_controller").setMesh(mesh));
doc.getRoot().getAsset().copyright =
  "Steam Controller shell (c) 2026 Valve Corporation, CC BY-NC-SA 4.0. Converted and simplified.";

await MeshoptSimplifier.ready;
await MeshoptEncoder.ready;
const before = raw.length / 9;
await doc.transform(
  weld(),
  simplify({ simplifier: MeshoptSimplifier, ratio: Math.min(1, targetTriangles / before), error: 0.0005 }),
  // No normals stored: gltf-transform's normals() is flat-shaded and un-shares every vertex.
  // The app computes crease-aware normals on load (toCreasedNormals in controller3d.ts).
  reorder({ encoder: MeshoptEncoder }),
  quantize(),
);
doc.createExtension(EXTMeshoptCompression).setRequired(true).setEncoderOptions({
  method: EXTMeshoptCompression.EncoderMethod.QUANTIZE,
});

const io = new NodeIO().registerExtensions(ALL_EXTENSIONS).registerDependencies({ "meshopt.encoder": MeshoptEncoder });
await io.write(output, doc);

const outPrim = doc.getRoot().listMeshes()[0].listPrimitives()[0];
const tris = outPrim.getIndices() ? outPrim.getIndices().getCount() / 3 : outPrim.getAttribute("POSITION").getCount() / 3;
const size = readFileSync(output).length;
console.log(`wrote ${output}: ${Math.round(tris)} triangles, ${(size / 1024 / 1024).toFixed(2)} MB`);

function determinant(o) {
  const m = o.map(([src, sign]) => [0, 1, 2].map((a) => (a === src ? sign : 0)));
  return (
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) -
    m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) +
    m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
  );
}
