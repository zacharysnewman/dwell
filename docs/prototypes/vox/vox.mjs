#!/usr/bin/env node
// Voxel-model prototype (docs/prototypes/vox/README.md): reads shared/characters/characters.json,
// poses each character at its rest pose, voxelizes every part's shape and writes one MagicaVoxel
// `.vox` per character and resolution, each part its own named model inside a group per bone, plus an
// isometric preview PNG. Then reads every file back and checks it gives the same voxels.
//
//   node docs/prototypes/vox/vox.mjs [outDir ...]   (default: this folder; more dirs get copies of the PNGs)
//
// Prototype only: nothing in the game reads these files yet. Dependency-free (Node's zlib for the PNG).
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';
import { compose, lookupMaterial, pivots, rotate, validate } from '../../../shared/characters/rig.mjs';
import { loadData, loadStyle } from '../../../shared/characters/gen.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const CHARACTERS = ['player', 'wolf', 'deer'];
const RESOLUTIONS = [16, 32]; // voxels per metre

// ---- Shapes: the rig's part kinds, in the part's own frame (centre 0, axis along y) ----------------
const D = Math.PI / 180;
function unrotate(v, r) {
  let [x, y, z] = v;
  const [a, b, c] = [-r[0] * D, -r[1] * D, -r[2] * D];
  [y, z] = [y * Math.cos(a) - z * Math.sin(a), y * Math.sin(a) + z * Math.cos(a)];
  [x, z] = [x * Math.cos(b) + z * Math.sin(b), -x * Math.sin(b) + z * Math.cos(b)];
  [x, y] = [x * Math.cos(c) - y * Math.sin(c), x * Math.sin(c) + y * Math.cos(c)];
  return [x, y, z];
}
/** Is a part-local point inside the part? `h` is the half size (already grown to a voxel at least). */
function inside(p, h, [x, y, z]) {
  const ax = Math.abs(x), ay = Math.abs(y), az = Math.abs(z);
  switch (p.kind) {
    case 'box':
      return ax <= h[0] && ay <= h[1] && az <= h[2];
    case 'round_box': {
      // As the Character Lab's RoundedBoxGeometry: corner radius = round × the smallest side.
      const r = Math.min(p.round * 2 * Math.min(...h), Math.min(...h));
      const q = [ax - (h[0] - r), ay - (h[1] - r), az - (h[2] - r)].map((v) => Math.max(v, 0));
      return ax <= h[0] && ay <= h[1] && az <= h[2] && Math.hypot(...q) <= r;
    }
    case 'ellipsoid':
      return (x / h[0]) ** 2 + (y / h[1]) ** 2 + (z / h[2]) ** 2 <= 1;
    case 'cylinder':
      return ay <= h[1] && (x / h[0]) ** 2 + (z / h[2]) ** 2 <= 1;
    case 'cone': {
      // Radius 1 at the bottom (y = −h), `tip` at the top.
      const t = (y + h[1]) / (2 * h[1]);
      const s = 1 + (p.tip - 1) * t;
      return ay <= h[1] && (x / h[0]) ** 2 + (z / h[2]) ** 2 <= s * s;
    }
    case 'capsule': {
      // As the Lab: radius x/2, the caps' centres at ±(y − x)/2, z scaled by z/x.
      const r = h[0];
      const zz = z * (h[0] / h[2]);
      const cy = Math.max(0, h[1] - r);
      const dy = Math.max(0, ay - cy);
      return x * x + zz * zz + dy * dy <= r * r;
    }
    default:
      throw new Error(`unknown part kind ${p.kind}`);
  }
}

// ---- Voxelizing a character ---------------------------------------------------------------------
// The game's frame is +y up, the character facing +z (its left at +x). MagicaVoxel is +z up, so a
// game point (x, y, z) goes to (x, −z, y): a right-handed turn about x. The character faces −y.
const toVox = ([x, y, z]) => [x, -z, y];
const fromVox = ([x, y, z]) => [x, z, -y];

/**
 * One character at one resolution: `{ parts: [{ id, bone, material, voxels: [[x, y, z, colour]] }],
 * bones: [{ name, parent, pivot }], palette }` in whole voxels of the vox frame.
 * Every voxel belongs to one part: where parts overlap, the smaller part wins, so details (eyes,
 * straps, buckles) carve the bulk they sit on instead of hiding inside it.
 */
function voxelize(model, style, res) {
  const piv = pivots(model, {});
  const parts = model.parts.filter((p) => p.view !== 'arms' && p.material !== '$block');
  // The palette: each material's ramp (shadow, base, light), in the order first used. Index 0 is empty.
  const palette = [null];
  const colourOf = new Map();
  for (const p of parts) {
    if (colourOf.has(p.material)) continue;
    const m = lookupMaterial(style, p.material);
    const stops = m.ramp.shadow === m.ramp.base && m.ramp.base === m.ramp.light ? ['base'] : ['shadow', 'base', 'light'];
    const idx = {};
    for (const s of stops) {
      idx[s] = palette.length;
      palette.push({ hex: m.ramp[s], material: p.material, stop: s });
    }
    if (!idx.shadow) idx.shadow = idx.light = idx.base;
    colourOf.set(p.material, idx);
  }
  if (palette.length > 256) throw new Error(`${model.id}: ${palette.length - 1} colours (MagicaVoxel holds 255)`);

  const owner = new Map(); // "x,y,z" (vox voxel index) → part index
  const volume = (p) => p.size[0] * p.size[1] * p.size[2];
  const cell = 1 / res;
  const centres = [];
  parts.forEach((p, pi) => {
    const b = piv.get(p.bone);
    // Grow every side to at least one voxel, so a thin part (an eye, a brow) still makes one.
    const h = p.size.map((s) => Math.max(s, cell) / 2);
    const local = (w) => {
      let v = [w[0] - b.at[0], w[1] - b.at[1], w[2] - b.at[2]];
      for (let i = b.chain.length - 1; i >= 0; i--) v = unrotate(v, b.chain[i]);
      return unrotate([v[0] - p.at[0], v[1] - p.at[1], v[2] - p.at[2]], p.rot);
    };
    const world = (v) => {
      let w = rotate(v, p.rot);
      w = [w[0] + p.at[0], w[1] + p.at[1], w[2] + p.at[2]];
      w = b.chain.reduce((acc, e) => rotate(acc, e), w);
      return [b.at[0] + w[0], b.at[1] + w[1], b.at[2] + w[2]];
    };
    // The part's box in the vox frame.
    const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
    for (let i = 0; i < 8; i++) {
      const c = toVox(world([i & 1 ? h[0] : -h[0], i & 2 ? h[1] : -h[1], i & 4 ? h[2] : -h[2]]));
      for (let k = 0; k < 3; k++) { lo[k] = Math.min(lo[k], c[k]); hi[k] = Math.max(hi[k], c[k]); }
    }
    let any = false;
    const claim = (key) => {
      const cur = owner.get(key);
      if (cur === undefined || volume(parts[cur]) > volume(p)) owner.set(key, pi);
      any = true;
    };
    for (let x = Math.floor(lo[0] * res); x <= Math.floor(hi[0] * res); x++)
      for (let y = Math.floor(lo[1] * res); y <= Math.floor(hi[1] * res); y++)
        for (let z = Math.floor(lo[2] * res); z <= Math.floor(hi[2] * res); z++) {
          const centre = fromVox([(x + 0.5) * cell, (y + 0.5) * cell, (z + 0.5) * cell]);
          if (inside(p, h, local(centre))) claim(`${x},${y},${z}`);
        }
    if (!any) {
      const c = toVox(world([0, 0, 0])).map((v) => Math.floor(v * res));
      claim(c.join(','));
    }
    centres.push(toVox(world([0, 0, 0])).map((v) => Math.floor(v * res)).join(','));
  });
  // Every part keeps at least one voxel: one outvoted everywhere (an eye under a smaller brow, an ear
  // under its inner ear) takes back the voxel at its centre, unless that would empty another part
  // listed before it: of two parts sharing their one voxel, the one listed first (the eye, the ear) stays.
  const counts = () => {
    const n = parts.map(() => 0);
    for (const pi of owner.values()) n[pi]++;
    return n;
  };
  let n = counts();
  parts.forEach((_, pi) => {
    if (n[pi] > 0) return;
    const cur = owner.get(centres[pi]);
    if (cur === undefined || n[cur] > 1 || cur > pi) {
      owner.set(centres[pi], pi);
      n = counts();
    }
  });

  // Shading: a voxel whose top is open to the air takes the ramp's light, one open below its shadow.
  const out = parts.map((p) => ({ id: p.id, bone: p.bone, material: p.material, voxels: [] }));
  for (const [key, pi] of owner) {
    const [x, y, z] = key.split(',').map(Number);
    const idx = colourOf.get(parts[pi].material);
    const up = owner.has(`${x},${y},${z + 1}`), down = owner.has(`${x},${y},${z - 1}`);
    const c = !up ? idx.light : !down ? idx.shadow : idx.base;
    out[pi].voxels.push([x, y, z, c]);
  }
  const bones = model.bones.map((b) => ({
    name: b.name,
    parent: b.parent,
    pivot: toVox(piv.get(b.name).at).map((v) => Math.round(v * res)),
  }));
  return { parts: out.filter((p) => p.voxels.length > 0), bones, palette };
}

// ---- The .vox writer (MagicaVoxel format 150: SIZE/XYZI per model, nTRN/nGRP/nSHP, LAYR, RGBA) -------
class Bytes {
  constructor() { this.parts = []; }
  i32(v) { const b = Buffer.alloc(4); b.writeInt32LE(v); this.parts.push(b); return this; }
  u8s(a) { this.parts.push(Buffer.from(a)); return this; }
  str(s) { const b = Buffer.from(s, 'utf8'); this.i32(b.length); this.parts.push(b); return this; }
  dict(d) { const e = Object.entries(d); this.i32(e.length); for (const [k, v] of e) this.str(k).str(String(v)); return this; }
  buf() { return Buffer.concat(this.parts); }
}
const chunk = (id, content, children = Buffer.alloc(0)) => {
  const h = Buffer.alloc(12);
  h.write(id, 0, 'ascii');
  h.writeInt32LE(content.length, 4);
  h.writeInt32LE(children.length, 8);
  return Buffer.concat([h, content, children]);
};
/** A model's size and the translation that puts its voxel (0,0,0) at `min` (MagicaVoxel centres a
 * model at its translation: voxel i sits at t + i − floor(size / 2)). */
function placement(voxels) {
  const min = [0, 1, 2].map((k) => Math.min(...voxels.map((v) => v[k])));
  const max = [0, 1, 2].map((k) => Math.max(...voxels.map((v) => v[k])));
  const size = [0, 1, 2].map((k) => max[k] - min[k] + 1);
  if (size.some((s) => s > 256)) throw new Error('a model is over 256 voxels on a side');
  return { min, size, centre: [0, 1, 2].map((k) => min[k] + Math.floor(size[k] / 2)) };
}

/**
 * The scene: root nTRN → nGRP, then per bone (rig order, parents first) an nTRN named by the bone at
 * its pivot relative to its parent bone, holding an nGRP with the bone's parts (nTRN named by the
 * part id → nSHP → model) and its child bones. Bones with no parts and no parts below are left out.
 */
function writeVox({ parts, bones, palette }) {
  const models = [];
  for (const p of parts) {
    const pl = placement(p.voxels);
    const xyzi = new Bytes().i32(p.voxels.length);
    for (const [x, y, z, c] of p.voxels) xyzi.u8s([x - pl.min[0], y - pl.min[1], z - pl.min[2], c]);
    models.push(chunk('SIZE', new Bytes().i32(pl.size[0]).i32(pl.size[1]).i32(pl.size[2]).buf()));
    models.push(chunk('XYZI', xyzi.buf()));
    p.place = pl;
  }
  const byBone = new Map(bones.map((b) => [b.name, { ...b, kids: [], parts: [] }]));
  parts.forEach((p, i) => byBone.get(p.bone).parts.push(i));
  let rootBone;
  for (const b of byBone.values()) (b.parent === null ? (rootBone = b) : byBone.get(b.parent).kids.push(b));
  const used = (b) => b.parts.length > 0 || b.kids.some(used);

  const nodes = [];
  let next = 0;
  const id = () => next++;
  const trn = (nodeId, attrs, child, layer, t) =>
    nodes.push(chunk('nTRN', new Bytes().i32(nodeId).dict(attrs).i32(child).i32(-1).i32(layer).i32(1)
      .dict(t ? { _t: t.join(' ') } : {}).buf()));
  const grp = (nodeId, attrs, kids) => {
    const b = new Bytes().i32(nodeId).dict(attrs).i32(kids.length);
    for (const k of kids) b.i32(k);
    nodes.push(chunk('nGRP', b.buf()));
  };
  // Depth-first: a node's id is taken before its children's, and every chunk is written after its
  // id is known, so ids are assigned first and the chunks collected in id order at the end.
  const emit = new Map();
  const bone = (b, parentPivot) => {
    const t = id(), g = id();
    const kids = [];
    for (const pi of b.parts) {
      const p = parts[pi];
      const pt = id(), ps = id();
      const rel = p.place.centre.map((v, k) => v - b.pivot[k]);
      emit.set(pt, () => trn(pt, { _name: p.id }, ps, 0, rel));
      emit.set(ps, () => nodes.push(chunk('nSHP', new Bytes().i32(ps).dict({}).i32(1).i32(pi).dict({}).buf())));
      kids.push(pt);
    }
    for (const k of b.kids) if (used(k)) kids.push(bone(k, b.pivot));
    const rel = b.pivot.map((v, k) => v - parentPivot[k]);
    emit.set(t, () => trn(t, { _name: b.name }, g, 0, rel));
    emit.set(g, () => grp(g, {}, kids));
    return t;
  };
  const top = id(), topGroup = id();
  const first = bone(rootBone, [0, 0, 0]);
  emit.set(top, () => trn(top, {}, topGroup, -1, null));
  emit.set(topGroup, () => grp(topGroup, {}, [first]));
  for (let i = 0; i < next; i++) emit.get(i)();

  const layr = chunk('LAYR', new Bytes().i32(0).dict({ _name: 'body' }).i32(-1).buf());
  const rgba = Buffer.alloc(1024);
  for (let i = 1; i < palette.length; i++) {
    const hex = palette[i].hex.replace('#', '');
    rgba.writeUInt8(parseInt(hex.slice(0, 2), 16), (i - 1) * 4);
    rgba.writeUInt8(parseInt(hex.slice(2, 4), 16), (i - 1) * 4 + 1);
    rgba.writeUInt8(parseInt(hex.slice(4, 6), 16), (i - 1) * 4 + 2);
    rgba.writeUInt8(255, (i - 1) * 4 + 3);
  }
  const main = chunk('MAIN', Buffer.alloc(0), Buffer.concat([...models, ...nodes, layr, chunk('RGBA', rgba)]));
  const head = Buffer.alloc(8);
  head.write('VOX ', 0, 'ascii');
  head.writeInt32LE(150, 4);
  return Buffer.concat([head, main]);
}

// ---- The .vox reader: what a future importer does (names → parts, groups → bones) -----------------
/** `{ palette: [[r,g,b,a]] (index 1..255), parts: [{ name, bone, voxels: [[x,y,z,c]] in the scene }] }`. */
export function readVox(buf) {
  if (buf.toString('ascii', 0, 4) !== 'VOX ') throw new Error('not a .vox file');
  let o = 8;
  const rd = () => { const v = buf.readInt32LE(o); o += 4; return v; };
  const rs = () => { const n = rd(); const s = buf.toString('utf8', o, o + n); o += n; return s; };
  const rdict = () => { const d = {}; for (let n = rd(); n > 0; n--) { const k = rs(); d[k] = rs(); } return d; };
  if (buf.toString('ascii', o, o + 4) !== 'MAIN') throw new Error('no MAIN chunk');
  o += 12;
  const sizes = [], models = [], nodes = new Map();
  const palette = [null];
  while (o < buf.length) {
    const tag = buf.toString('ascii', o, o + 4);
    const n = buf.readInt32LE(o + 4), m = buf.readInt32LE(o + 8);
    o += 12;
    const end = o + n;
    if (tag === 'SIZE') sizes.push([rd(), rd(), rd()]);
    else if (tag === 'XYZI') {
      const v = [];
      for (let k = rd(); k > 0; k--) { v.push([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]); o += 4; }
      models.push(v);
    } else if (tag === 'nTRN') {
      const nid = rd(), attrs = rdict(), child = rd(); rd(); rd();
      const frames = [];
      for (let k = rd(); k > 0; k--) frames.push(rdict());
      nodes.set(nid, { kind: 'trn', name: attrs._name, child, t: (frames[0]?._t ?? '0 0 0').split(' ').map(Number) });
    } else if (tag === 'nGRP') {
      const nid = rd(); rdict();
      const kids = [];
      for (let k = rd(); k > 0; k--) kids.push(rd());
      nodes.set(nid, { kind: 'grp', kids });
    } else if (tag === 'nSHP') {
      const nid = rd(); rdict();
      const ms = [];
      for (let k = rd(); k > 0; k--) { ms.push(rd()); rdict(); }
      nodes.set(nid, { kind: 'shp', models: ms });
    } else if (tag === 'RGBA') {
      for (let i = 0; i < 255; i++) palette.push([buf[o + i * 4], buf[o + i * 4 + 1], buf[o + i * 4 + 2], buf[o + i * 4 + 3]]);
    }
    o = end; // no chunk but MAIN has children in these files
    if (m) throw new Error(`unexpected children in ${tag}`);
  }
  const parts = [];
  const walk = (nid, at, bone) => {
    const nd = nodes.get(nid);
    if (nd.kind === 'trn') {
      const t = [at[0] + nd.t[0], at[1] + nd.t[1], at[2] + nd.t[2]];
      const child = nodes.get(nd.child);
      // A transform over a group is a bone; over a shape, a part.
      if (child.kind === 'grp') walk(nd.child, t, nd.name ?? bone);
      else for (const mi of child.models) {
        const s = sizes[mi];
        const voxels = models[mi].map(([x, y, z, c]) => [t[0] + x - Math.floor(s[0] / 2), t[1] + y - Math.floor(s[1] / 2), t[2] + z - Math.floor(s[2] / 2), c]);
        parts.push({ name: nd.name, bone, voxels });
      }
    } else if (nd.kind === 'grp') for (const k of nd.kids) walk(k, at, bone);
  };
  walk(0, [0, 0, 0], undefined);
  return { palette, parts };
}

// ---- Preview: an isometric software render (front-right and back-left, side by side) -------------
function png(w, h, rgb) {
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) rgb.copy(raw, y * (w * 3 + 1) + 1, y * w * 3, (y + 1) * w * 3);
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (b) => { let c = 0xffffffff; for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const ch = (t, d) => { const l = Buffer.alloc(4); l.writeUInt32BE(d.length); const td = Buffer.concat([Buffer.from(t), d]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(td)); return Buffer.concat([l, td, c]); };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), ch('IHDR', ihdr), ch('IDAT', deflateSync(raw)), ch('IEND', Buffer.alloc(0))]);
}
function preview({ palette, parts }, scale) {
  const all = parts.flatMap((p) => p.voxels);
  const set = new Set(all.map(([x, y, z]) => `${x},${y},${z}`));
  const k = scale; // half a cube's width in pixels
  const views = [
    (v) => v, // camera at +x −y +z: front-right
    ([x, y, z, c]) => [-x, -y, z, c], // turned 180° about z: back-left
  ];
  const tiles = views.map((f) => {
    const vs = all.map(f);
    const solid = new Set(vs.map(([x, y, z]) => `${x},${y},${z}`));
    // Screen: x right = (x + y)·k, y down = (x − y)·k/2 − z·k.
    const sx = (x, y) => (x + y) * k, sy = (x, y, z) => (x - y) * k / 2 - z * k;
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const [x, y, z] of vs) {
      minX = Math.min(minX, sx(x, y + 1)); maxX = Math.max(maxX, sx(x + 1, y + 1) + 2 * k);
      minY = Math.min(minY, sy(x, y + 1, z + 1)); maxY = Math.max(maxY, sy(x + 1, y, z) + 2 * k);
    }
    return { vs, solid, sx, sy, minX, maxX, minY, maxY };
  });
  const pad = 8 * k;
  const H = Math.ceil(Math.max(...tiles.map((t) => t.maxY - t.minY))) + 2 * pad;
  const widths = tiles.map((t) => Math.ceil(t.maxX - t.minX) + 2 * pad);
  const W = widths.reduce((a, b) => a + b, 0);
  const img = Buffer.alloc(W * H * 3);
  for (let i = 0; i < W * H; i++) { img[i * 3] = 0xe8; img[i * 3 + 1] = 0xe4; img[i * 3 + 2] = 0xda; }
  let ox = 0;
  tiles.forEach((t, ti) => {
    const offX = ox + pad - t.minX, offY = pad - t.minY;
    // Far to near: larger (x − y + z) is nearer the camera.
    const order = [...t.vs].sort((a, b) => a[0] - a[1] + a[2] - (b[0] - b[1] + b[2]));
    const fill = (poly, col) => {
      const xs = poly.map((p) => p[0]), ys = poly.map((p) => p[1]);
      for (let py = Math.floor(Math.min(...ys)); py < Math.ceil(Math.max(...ys)); py++)
        for (let px = Math.floor(Math.min(...xs)); px < Math.ceil(Math.max(...xs)); px++) {
          const cx = px + 0.5, cy = py + 0.5;
          let ins = true;
          for (let i = 0; i < poly.length && ins; i++) {
            const [ax, ay] = poly[i], [bx, by] = poly[(i + 1) % poly.length];
            if ((bx - ax) * (cy - ay) - (by - ay) * (cx - ax) < 0) ins = false;
          }
          if (!ins || px < 0 || py < 0 || px >= W || py >= H) continue;
          const o = (py * W + px) * 3;
          img[o] = col[0]; img[o + 1] = col[1]; img[o + 2] = col[2];
        }
    };
    const P = (x, y, z) => [offX + t.sx(x, y), offY + t.sy(x, y, z)];
    for (const [x, y, z, c] of order) {
      const base = palette[c].slice(0, 3);
      const shade = (f) => base.map((v) => Math.round(v * f));
      const s = (dx, dy, dz) => t.solid.has(`${x + dx},${y + dy},${z + dz}`);
      // Faces seen: +z (top), +x (right-front), −y (left-front). Corners wound clockwise on screen.
      if (!s(0, 0, 1)) fill([P(x, y + 1, z + 1), P(x + 1, y + 1, z + 1), P(x + 1, y, z + 1), P(x, y, z + 1)], shade(1));
      if (!s(1, 0, 0)) fill([P(x + 1, y + 1, z + 1), P(x + 1, y + 1, z), P(x + 1, y, z), P(x + 1, y, z + 1)], shade(0.78));
      if (!s(0, -1, 0)) fill([P(x, y, z + 1), P(x + 1, y, z + 1), P(x + 1, y, z), P(x, y, z)], shade(0.62));
    }
    ox += widths[ti];
  });
  return png(W, H, img);
}

// ---- Main ----------------------------------------------------------------------------------------
const outDirs = process.argv.slice(2).length ? process.argv.slice(2) : [here];
const style = loadStyle();
const data = validate(loadData(), style);
const manifest = { $comment: 'Written by docs/prototypes/vox/vox.mjs: per file, its parts (model name → bone) and palette (index → material stop).', files: {} };
for (const id of CHARACTERS) {
  const model = compose(data, style, id);
  for (const res of RESOLUTIONS) {
    const v = voxelize(model, style, res);
    const file = `${id}_${res}.vox`;
    const buf = writeVox(v);
    writeFileSync(join(here, file), buf);
    // Read it back: every part by name, on its bone, the same voxels and colours.
    const back = readVox(readFileSync(join(here, file)));
    const key = (p) => p.voxels.map((q) => q.join(',')).sort().join(';');
    for (const p of v.parts) {
      const q = back.parts.find((b) => b.name === p.id);
      if (!q) throw new Error(`${file}: part ${p.id} missing on read-back`);
      if (q.bone !== p.bone) throw new Error(`${file}: part ${p.id} read back on bone ${q.bone}, not ${p.bone}`);
      if (key(q) !== key(p)) throw new Error(`${file}: part ${p.id}'s voxels differ on read-back`);
    }
    if (back.parts.length !== v.parts.length) throw new Error(`${file}: ${back.parts.length} parts read back, ${v.parts.length} written`);
    const count = v.parts.reduce((n, p) => n + p.voxels.length, 0);
    const pic = preview(back, res === 16 ? 10 : 5);
    for (const d of outDirs) {
      mkdirSync(d, { recursive: true });
      writeFileSync(join(d, `${id}_${res}.png`), pic);
    }
    manifest.files[file] = {
      character: id,
      voxelsPerMetre: res,
      voxels: count,
      parts: Object.fromEntries(v.parts.map((p) => [p.id, { bone: p.bone, material: p.material }])),
      palette: Object.fromEntries(v.palette.slice(1).map((c, i) => [i + 1, `${c.material}.${c.stop}`])),
    };
    console.log(`${file}: ${v.parts.length} parts, ${count} voxels, ${v.palette.length - 1} colours; read back OK`);
  }
}
writeFileSync(join(here, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
