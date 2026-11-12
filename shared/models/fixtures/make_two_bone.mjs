#!/usr/bin/env node
// Writes shared/models/fixtures/two_bone.glb: the tests' smallest skinned model. Two triangles on a
// two-joint chain (`root` at the origin, `tip` 1 m up it): vertex 0 at the origin and vertex 2 at
// +x on the root, vertex 1 2 m up on the tip; a second triangle (0, 3, 2) below it all on the root
// (vertex 3 at 1 m down), the part a first-person body keeps (fixtures/models.json `firstPerson`). Clips: `rest` (the tip still) and `bend` (the tip
// turning 90° about +z over a second). Run it after changing it; the GLB is checked in.
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const s = Math.SQRT1_2;
const parts = [];
const views = [];
const accessors = [];
let length = 0;
/** Appends `array` (a typed array) as a buffer view and an accessor. */
function add(array, type, componentType, extra = {}) {
  const bytes = Buffer.from(array.buffer, array.byteOffset, array.byteLength);
  const pad = (4 - (bytes.length % 4)) % 4;
  views.push({ buffer: 0, byteOffset: length, byteLength: bytes.length });
  parts.push(bytes, Buffer.alloc(pad));
  length += bytes.length + pad;
  const width = { SCALAR: 1, VEC3: 3, VEC4: 4, MAT4: 16 }[type];
  accessors.push({
    bufferView: views.length - 1,
    componentType,
    count: array.length / width,
    type,
    ...extra,
  });
  return accessors.length - 1;
}
const F = 5126;
const position = add(new Float32Array([0, 0, 0, 0, 2, 0, 1, 0, 0, 0.5, -1, 0]), "VEC3", F, {
  min: [0, -1, 0],
  max: [1, 2, 0],
});
const normal = add(new Float32Array([0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1]), "VEC3", F);
const color = add(
  new Float32Array([1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1]),
  "VEC4",
  F,
);
const joints = add(
  new Uint8Array([0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
  "VEC4",
  5121,
);
const weights = add(
  new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]),
  "VEC4",
  F,
);
const indices = add(new Uint16Array([0, 2, 1, 0, 3, 2]), "SCALAR", 5123);
const ibm = add(
  new Float32Array([
    1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0,
    0, 1, 0, 0, -1, 0, 1,
  ]),
  "MAT4",
  F,
);
const times = add(new Float32Array([0, 1]), "SCALAR", F, {
  min: [0],
  max: [1],
});
const bend = add(new Float32Array([0, 0, 0, 1, 0, 0, s, s]), "VEC4", F);
const still = add(new Float32Array([0, 0, 0, 1, 0, 0, 0, 1]), "VEC4", F);
const json = {
  asset: {
    version: "2.0",
    generator: "shared/models/fixtures/make_two_bone.mjs",
  },
  scene: 0,
  scenes: [{ nodes: [0, 2, 3] }],
  nodes: [
    { name: "root", children: [1] },
    { name: "tip", translation: [0, 1, 0] },
    { name: "mesh", mesh: 0, skin: 0 },
    // A second model-detail level (fixtures/models.json `detail`): the same triangles.
    { name: "mesh_1", mesh: 1, skin: 0 },
  ],
  meshes: [0, 1].map((k) => ({
    name: `detail_${k}`,
    primitives: [
      {
        attributes: {
          POSITION: position,
          NORMAL: normal,
          COLOR_0: color,
          JOINTS_0: joints,
          WEIGHTS_0: weights,
        },
        indices,
      },
    ],
  })),
  skins: [{ joints: [0, 1], inverseBindMatrices: ibm }],
  animations: [
    {
      name: "rest",
      samplers: [{ input: times, output: still, interpolation: "LINEAR" }],
      channels: [{ sampler: 0, target: { node: 1, path: "rotation" } }],
    },
    {
      name: "bend",
      samplers: [{ input: times, output: bend, interpolation: "LINEAR" }],
      channels: [{ sampler: 0, target: { node: 1, path: "rotation" } }],
    },
  ],
  buffers: [{ byteLength: length }],
  bufferViews: views,
  accessors,
};
let text = Buffer.from(JSON.stringify(json));
text = Buffer.concat([text, Buffer.alloc((4 - (text.length % 4)) % 4, 0x20)]);
const bin = Buffer.concat(parts);
const header = Buffer.alloc(12);
header.writeUInt32LE(0x46546c67, 0);
header.writeUInt32LE(2, 4);
header.writeUInt32LE(12 + 8 + text.length + 8 + bin.length, 8);
const chunk = (type, data) => {
  const h = Buffer.alloc(8);
  h.writeUInt32LE(data.length, 0);
  h.writeUInt32LE(type, 4);
  return Buffer.concat([h, data]);
};
writeFileSync(
  join(here, "two_bone.glb"),
  Buffer.concat([header, chunk(0x4e4f534a, text), chunk(0x004e4942, bin)]),
);
console.log("wrote two_bone.glb");
