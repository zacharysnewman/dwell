#!/usr/bin/env node
// Validates shared/models/models.json (docs/CHARACTERS.md §11, ADR 0070) against its GLBs and the
// shared palette, and writes the game's table: crates/dwell-models/src/models.gen.rs (each model's
// GLB path, scale, yaw and role → clip map, and the player model's GLB bytes); and the same from
// shared/models/fixtures/models.json as fixture.gen.rs (tests only: a model added by data alone).
// CI fails on any diff. A bad entry fails with a message naming it.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { applyCoat, validateCoat } from "./coat.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");
const CRATE = join(root, "crates/dwell-models/src");

/** The most joints a model's skin may have (the shader's joint block: dwell_models::MAX_JOINTS). */
export const MAX_JOINTS = 64;
/** The roles a model may map (dwell_characters::animator::body_role's names). */
export const ROLES = [
  "idle",
  "walk",
  "sprint",
  "jump",
  "fall",
  "land",
  "crouch",
  "crouch_walk",
  "slide",
  "swim",
  "climb",
  "fly",
];

/** Every palette stop a remap may snap to: `[{ name, hex }]`, ramps first, `exclude`d ramps left out. */
export function paletteStops(palette, exclude = []) {
  const out = [];
  for (const [ramp, stops] of Object.entries(palette.ramps)) {
    if (exclude.includes(ramp)) continue;
    for (const [stop, hex] of Object.entries(stops))
      out.push({ name: `${ramp}.${stop}`, hex });
  }
  return out;
}

/** A palette reference's hex: `ramp.stop`, a bare ramp (its base), or a swatch. */
export function resolveColor(palette, ref) {
  const [ramp, stop = "base"] = ref.split(".");
  const hex =
    palette.ramps[ramp]?.[stop] ??
    (ref in (palette.swatches ?? {}) ? palette.swatches[ref] : undefined);
  if (!hex) throw new Error(`no palette colour ${ref}`);
  return hex;
}

/** A GLB's JSON chunk. */
export function glbJson(bytes) {
  const b = Buffer.from(bytes);
  if (b.length < 20 || b.readUInt32LE(0) !== 0x46546c67)
    throw new Error("not a GLB");
  const len = b.readUInt32LE(12);
  if (b.readUInt32LE(16) !== 0x4e4f534a)
    throw new Error("GLB: first chunk is not JSON");
  return JSON.parse(b.subarray(20, 20 + len).toString("utf8"));
}

/** What the game reads from a GLB: its clip names and its skin's joint count. */
export function glbSummary(bytes) {
  const g = glbJson(bytes);
  return {
    clips: (g.animations ?? []).map((a) => a.name),
    joints: g.skins?.[0]?.joints.length ?? 0,
    jointNames: (g.skins?.[0]?.joints ?? []).map((j) => g.nodes?.[j]?.name),
    meshes: (g.meshes ?? []).length,
    meshNames: (g.meshes ?? []).map((m) => m.name ?? ""),
  };
}

/** The one level a models.json without `detail` has: the full mesh. */
const ONE_LEVEL = {
  levels: [{ id: "full", label: "100 %", ratio: 1 }],
  default: "full",
};

/**
 * The model-detail levels (ADR 0070 §10): `{ levels: [{ id, label, ratio }], default }`, the
 * first the full mesh (ratio 1), each after it fewer faces. Throws naming the bad entry.
 */
export function validateDetail(data) {
  const fail = (msg) => {
    throw new Error(`models.json: detail: ${msg}`);
  };
  const d = data.detail ?? ONE_LEVEL;
  if (!Array.isArray(d.levels) || !d.levels.length) fail("no levels");
  const ids = new Set();
  d.levels.forEach((l, k) => {
    if (!/^[a-z][a-z0-9_]*$/.test(l.id ?? ""))
      fail(`level ${k}: ids are lower_snake_case`);
    if (ids.has(l.id)) fail(`level ${l.id} twice`);
    ids.add(l.id);
    if (typeof l.label !== "string" || !l.label)
      fail(`level ${l.id}: no label`);
    if (!(l.ratio > 0 && l.ratio <= 1))
      fail(`level ${l.id}: ratio must be in (0, 1]`);
    if (k === 0 && l.ratio !== 1)
      fail(`level ${l.id}: the first level is the full mesh (ratio 1)`);
    if (k > 0 && !(l.ratio < d.levels[k - 1].ratio))
      fail(`level ${l.id}: ratios fall level by level`);
  });
  if (!ids.has(d.default)) fail(`default ${d.default} is not a level`);
  return d;
}

/** The characters rigs' bone names (shared/characters/characters.json `rigs`): `{ rig: [bone] }`. */
export function rigBones(characters) {
  return Object.fromEntries(
    Object.entries(characters.rigs ?? {}).map(([r, v]) => [
      r,
      (v.bones ?? []).map((b) => b.name),
    ]),
  );
}

/** The build's characters rigs (shared/characters/characters.json). */
export function buildRigs() {
  return rigBones(
    JSON.parse(
      readFileSync(join(root, "shared/characters/characters.json"), "utf8"),
    ),
  );
}

/**
 * A model's retarget map (docs/CHARACTERS.md §11.9): `{ rig, root, bones: { bone: { joint, rest } } }`,
 * each bone one of the characters rig `rig`'s, each joint one of the skin's, `rest` [x, y, z]
 * degrees. Returns it resolved (joints as skin indices). Throws naming the bad entry.
 */
export function validateRetarget(r, jointNames, rigs, at) {
  const fail = (msg) => {
    throw new Error(`models.json: ${at}: ${msg}`);
  };
  const bones = rigs[r.rig];
  if (!bones) fail(`no characters rig ${r.rig}`);
  const entries = Object.entries(r.bones ?? {});
  if (!entries.length) fail("no bones");
  const used = new Set();
  const out = entries.map(([bone, b]) => {
    if (!bones.includes(bone)) fail(`no bone ${bone} in rig ${r.rig}`);
    const joint = jointNames.indexOf(b.joint);
    if (joint < 0) fail(`bone ${bone}: no joint ${b.joint} in the skin`);
    if (used.has(joint)) fail(`bone ${bone}: joint ${b.joint} mapped twice`);
    used.add(joint);
    const rest = b.rest ?? [0, 0, 0];
    if (!(
      Array.isArray(rest) &&
      rest.length === 3 &&
      rest.every(Number.isFinite)
    ))
      fail(`bone ${bone}: rest is [x, y, z] degrees`);
    return { bone, joint, rest };
  });
  if (!r.bones[r.root]) fail(`root ${r.root} is not a mapped bone`);
  return { rig: r.rig, root: r.root, bones: out };
}

/** Validates the data (`dir`: where its GLB paths are relative to). Throws naming the bad entry. */
export function validate(
  data,
  palette,
  dir,
  read = (p) => readFileSync(p),
  rigs = buildRigs(),
) {
  const fail = (msg) => {
    throw new Error(`models.json: ${msg}`);
  };
  if (!data.models || typeof data.models !== "object") fail("no models");
  for (const r of data.palette?.exclude ?? [])
    if (!palette.ramps[r]) fail(`palette.exclude: no ramp ${r}`);
  if (data.player && !data.models[data.player])
    fail(`player: no model ${data.player}`);
  const detail = validateDetail(data);
  const out = {};
  for (const [name, m] of Object.entries(data.models)) {
    const at = `model ${name}`;
    if (!/^[a-z][a-z0-9_]*$/.test(name))
      fail(`${at}: names are lower_snake_case`);
    if (typeof m.glb !== "string") fail(`${at}: no glb`);
    const path = join(dir, m.glb);
    if (!existsSync(path)) fail(`${at}: missing GLB ${m.glb}`);
    let s;
    try {
      s = glbSummary(read(path));
    } catch (e) {
      fail(`${at}: ${m.glb}: ${e.message}`);
    }
    // One mesh a detail level, detail_0.. (a model of one level: its one mesh, any name).
    const want = detail.levels.map((_, k) => `detail_${k}`);
    if (
      detail.levels.length === 1
        ? s.meshes !== 1
        : want.some((n) => !s.meshNames.includes(n)) || s.meshes !== want.length
    )
      fail(
        `${at}: ${m.glb} has meshes [${s.meshNames.join(", ")}], not one a detail level (${want.join(", ")}): re-import it`,
      );
    if (m.source?.remove !== undefined) {
      const r = m.source.remove;
      if (!s.jointNames.includes(r.joint))
        fail(`${at}: source.remove: no joint ${r.joint} in ${m.glb}'s skin`);
      if (!Number.isFinite(r.above))
        fail(`${at}: source.remove: above is a height (model units)`);
    }
    if (s.joints < 1 || s.joints > MAX_JOINTS)
      fail(`${at}: ${s.joints} joints (1..${MAX_JOINTS})`);
    if (!(m.scale > 0)) fail(`${at}: scale must be > 0`);
    if (typeof (m.yaw ?? 0) !== "number") fail(`${at}: yaw is degrees`);
    for (const [src, to] of Object.entries(m.remap ?? {})) {
      if (!/^#[0-9a-f]{6}$/i.test(src))
        fail(`${at}: remap key ${src} is not #rrggbb`);
      try {
        resolveColor(palette, to);
      } catch (e) {
        fail(`${at}: remap ${src}: ${e.message}`);
      }
    }
    if (m.coat !== undefined) {
      try {
        validateCoat(m.coat, palette);
      } catch (e) {
        fail(`${at}: ${e.message}`);
      }
      const bytes = read(path);
      if (
        !Buffer.from(applyCoat(bytes, m.coat, palette)).equals(
          Buffer.from(bytes),
        )
      )
        fail(
          `${at}: ${m.glb} is not painted by its coat: run node shared/models/import.mjs --coat ${name}`,
        );
    }
    const roles = Object.entries(m.roles ?? {});
    if (!roles.length) fail(`${at}: no roles`);
    for (const [role, r] of roles) {
      if (!ROLES.includes(role))
        fail(`${at}: unknown role ${role} (${ROLES.join(", ")})`);
      if (!s.clips.includes(r.clip))
        fail(
          `${at}: role ${role}: no clip ${r.clip} in ${m.glb} (${s.clips.join(", ")})`,
        );
      if (r.speed !== undefined && !(r.speed > 0))
        fail(`${at}: role ${role}: speed must be > 0`);
    }
    if (!m.roles[m.fallback])
      fail(`${at}: fallback ${m.fallback} is not one of its roles`);
    let hand;
    if (m.hand !== undefined) {
      const h = m.hand;
      const joint = s.jointNames.indexOf(h.joint);
      if (joint < 0)
        fail(`${at}: hand: no joint ${h.joint} in ${m.glb}'s skin`);
      if (typeof h.bone !== "string" || !h.bone)
        fail(`${at}: hand: bone names the characters rig's hand bone`);
      for (const k of ["at", "rot"])
        if (
          h[k] !== undefined &&
          !(
            Array.isArray(h[k]) &&
            h[k].length === 3 &&
            h[k].every(Number.isFinite)
          )
        )
          fail(`${at}: hand: ${k} is [x, y, z]`);
      hand = {
        joint,
        bone: h.bone,
        at: h.at ?? [0, 0, 0],
        rot: h.rot ?? [0, 0, 0],
      };
    }
    const retarget =
      m.retarget === undefined
        ? undefined
        : validateRetarget(m.retarget, s.jointNames, rigs, `${at}: retarget`);
    out[name] = { ...m, clips: s.clips, joints: s.joints, hand, retarget };
  }
  return out;
}

/** A GLB accessor's values as numbers (normalized integers read as 0..1), and its width. */
export function glbAccessor(bytes, index) {
  const b = Buffer.from(bytes);
  const g = glbJson(b);
  const jsonLen = b.readUInt32LE(12);
  const binAt = 20 + jsonLen + 8;
  const a = g.accessors[index];
  const v = g.bufferViews[a.bufferView];
  const width = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 }[a.type];
  const [size, readFn, max] = {
    5126: [4, "readFloatLE", 1],
    5123: [2, "readUInt16LE", 65535],
    5121: [1, "readUInt8", 255],
    5125: [4, "readUInt32LE", 1],
  }[a.componentType];
  const stride = v.byteStride ?? size * width;
  const at = binAt + (v.byteOffset ?? 0) + (a.byteOffset ?? 0);
  const out = [];
  const scale = a.normalized ? max : 1;
  for (let i = 0; i < a.count; i++)
    for (let k = 0; k < width; k++)
      out.push(b[readFn](at + i * stride + k * size) / scale);
  return { values: out, width };
}

/** What a prop's GLB holds: its size (m, x y z), its colours (distinct, rounded) and its meshes. */
export function propSummary(bytes) {
  const g = glbJson(bytes);
  const prims = g.meshes?.[0]?.primitives ?? [];
  const lo = [Infinity, Infinity, Infinity];
  const hi = [-Infinity, -Infinity, -Infinity];
  const colours = new Set();
  let colour0 = prims.length > 0;
  for (const p of prims) {
    const a = g.accessors[p.attributes.POSITION];
    for (let k = 0; k < 3; k++) {
      lo[k] = Math.min(lo[k], a.min[k]);
      hi[k] = Math.max(hi[k], a.max[k]);
    }
    if (p.attributes.COLOR_0 === undefined) {
      colour0 = false;
      continue;
    }
    const { values, width } = glbAccessor(bytes, p.attributes.COLOR_0);
    for (let i = 0; i < values.length; i += width)
      colours.add(
        values
          .slice(i, i + 3)
          .map((c) => Math.round(c * 255))
          .join(","),
      );
  }
  return {
    meshes: (g.meshes ?? []).length,
    skins: (g.skins ?? []).length,
    clips: (g.animations ?? []).length,
    colour0,
    colours: colours.size,
    /** Its colours, each "r,g,b" (COLOR_0 × 255, rounded). */
    palette: [...colours].sort(),
    size: hi.map((h, k) => h - lo[k]),
    centre: hi.map((h, k) => (h + lo[k]) / 2),
  };
}

/** Validates props.json (`dir`: where its GLB paths are relative to). Throws naming the bad entry. */
export function validateProps(data, dir, read = (p) => readFileSync(p)) {
  const fail = (msg) => {
    throw new Error(`props.json: ${msg}`);
  };
  const cats = data.categories ?? {};
  for (const [c, v] of Object.entries(cats))
    if (!(
      v.min > 0 &&
      v.max >= v.min &&
      v.length >= v.min &&
      v.length <= v.max
    ))
      fail(`category ${c}: needs 0 < min <= length <= max`);
  const out = {};
  for (const [name, p] of Object.entries(data.props ?? {})) {
    const at = `prop ${name}`;
    if (!/^[a-z][a-z0-9_]*$/.test(name))
      fail(`${at}: names are lower_snake_case`);
    const cat = cats[p.category];
    if (!cat)
      fail(
        `${at}: unknown category ${p.category} (${Object.keys(cats).join(", ")})`,
      );
    if (typeof p.glb !== "string") fail(`${at}: no glb`);
    const path = join(dir, p.glb);
    if (!existsSync(path)) fail(`${at}: missing GLB ${p.glb}`);
    let s;
    try {
      s = propSummary(read(path));
    } catch (e) {
      fail(`${at}: ${p.glb}: ${e.message}`);
    }
    if (s.meshes !== 1) fail(`${at}: ${p.glb} has ${s.meshes} meshes, not one`);
    if (s.skins || s.clips)
      fail(`${at}: ${p.glb} has a skin or clips (a prop is static)`);
    if (!s.colour0) fail(`${at}: ${p.glb} has no COLOR_0 (re-import it)`);
    const length = p.length ?? cat.length;
    if (!(length >= cat.min && length <= cat.max))
      fail(
        `${at}: length ${length} m is outside ${p.category}'s ${cat.min}..${cat.max} m`,
      );
    const longest = Math.max(...s.size);
    if (Math.abs(longest - length) > 0.02 * length + 0.005)
      fail(
        `${at}: ${p.glb} is ${longest.toFixed(3)} m long, not its length ${length} m (re-import it)`,
      );
    if (
      p.grip !== undefined &&
      !(
        Array.isArray(p.grip) &&
        p.grip.length === 3 &&
        p.grip.every(Number.isFinite)
      )
    )
      fail(`${at}: grip is [x, y, z] metres`);
    const handle =
      p.handle === undefined ? null : measureHandle(read(path), p.handle, at);
    const hold = validateHold(p.hold, at);
    if (
      p.objects !== undefined &&
      !(
        Array.isArray(p.objects) &&
        p.objects.length > 0 &&
        p.objects.every((o) => typeof o === "string" && o.length > 0) &&
        new Set(p.objects).size === p.objects.length
      )
    )
      fail(
        `${at}: objects is a list of its FBX's mesh object names, each once`,
      );
    out[name] = {
      ...p,
      length,
      size: s.size,
      colours: s.colours,
      // With a handle, the grip is the point on its measured axis at the grip's height.
      grip: handle ? handle.at(p.grip?.[1] ?? 0) : (p.grip ?? [0, 0, 0]),
      axis: handle ? handle.axis : [0, 1, 0],
      hold,
    };
  }
  return out;
}

/**
 * A prop's handle axis measured from its GLB (`handle` [from, to]: the bare handle's span along the
 * GLB's +y, `from` the end away from its head): the centre of the bounds of the vertices in each
 * half of the span, the axis the line through them pointing toward `to`. Returns the unit axis and
 * the point on it at a height. Throws when a half holds no vertex or the axis is not along +y.
 */
export function measureHandle(bytes, handle, at) {
  const fail = (msg) => {
    throw new Error(`props.json: ${at}: ${msg}`);
  };
  if (!(
    Array.isArray(handle) &&
    handle.length === 2 &&
    handle.every(Number.isFinite) &&
    Math.abs(handle[1] - handle[0]) >= 0.05
  ))
    fail("handle is [from, to] metres along its length, at least 0.05 m apart");
  const g = glbJson(bytes);
  // Its surface sampled every centimetre along each triangle's edges (a long handle's only
  // vertices may be at its ends).
  const pts = [];
  for (const p of g.meshes?.[0]?.primitives ?? []) {
    const { values } = glbAccessor(bytes, p.attributes.POSITION);
    const v = (i) => values.slice(i * 3, i * 3 + 3);
    const idx =
      p.indices === undefined
        ? [...Array(values.length / 3).keys()]
        : glbAccessor(bytes, p.indices).values;
    for (let t = 0; t + 2 < idx.length; t += 3)
      for (const [i, j] of [
        [idx[t], idx[t + 1]],
        [idx[t + 1], idx[t + 2]],
        [idx[t + 2], idx[t]],
      ]) {
        const [u, w] = [v(i), v(j)];
        const n = Math.max(
          1,
          Math.ceil(Math.hypot(...u.map((x, k) => w[k] - x)) / 0.01),
        );
        for (let s = 0; s <= n; s++)
          pts.push(u.map((x, k) => x + ((w[k] - x) * s) / n));
      }
  }
  const [a, b] = handle;
  const mid = (a + b) / 2;
  const centre = (lo, hi) => {
    const s = pts.filter((p) => p[1] >= lo && p[1] <= hi);
    if (s.length === 0)
      fail(`handle: no vertex between ${lo} and ${hi} m (measure its span)`);
    const c = [0, 1, 2].map(
      (k) =>
        (Math.min(...s.map((p) => p[k])) + Math.max(...s.map((p) => p[k]))) / 2,
    );
    return c;
  };
  const [lo, hi] = [Math.min(a, mid), Math.max(a, mid)];
  const p0 = centre(lo, hi);
  const p1 = centre(Math.min(b, mid), Math.max(b, mid));
  const d = [0, 1, 2].map((k) => p1[k] - p0[k]);
  const l = Math.hypot(...d);
  const axis = d.map((v) => v / l);
  if (Math.abs(axis[1]) < Math.cos((30 * Math.PI) / 180))
    fail(
      `handle: its measured axis [${axis.map((v) => v.toFixed(3))}] is not along its length`,
    );
  return {
    axis,
    at: (y) => {
      const t = (y - p0[1]) / d[1];
      return [0, 1, 2].map((k) => p0[k] + d[k] * t);
    },
  };
}

/**
 * The hand that holds props (props.json `hand`, in the characters rig's hand bone's frame): `at`
 * the fist's centre (m), `axis` the line through the closed fist a handle lies along, `edge` the
 * way across it a tool's working side faces (perpendicular to `axis`).
 */
export function validateHand(h) {
  const fail = (msg) => {
    throw new Error(`props.json: hand: ${msg}`);
  };
  const v3 = (v, name) => {
    if (!(Array.isArray(v) && v.length === 3 && v.every(Number.isFinite)))
      fail(`${name} is [x, y, z]`);
    return v;
  };
  if (!h || typeof h !== "object") fail("is { bone, at, axis, edge }");
  if (typeof h.bone !== "string") fail("bone names the rig's hand bone");
  const at = v3(h.at, "at");
  const unit = (v, name) => {
    const l = Math.hypot(...v3(v, name));
    if (l < 1e-6) fail(`${name} is zero`);
    return v.map((x) => x / l);
  };
  const axis = unit(h.axis, "axis");
  const edge = unit(h.edge, "edge");
  if (Math.abs(axis.reduce((s, x, k) => s + x * edge[k], 0)) > 1e-3)
    fail("edge is not perpendicular to axis");
  return { bone: h.bone, at, axis, edge };
}

/** The pose a prop is held in when it names none: as modelled, at its grip. */
export const DEFAULT_HOLD = { scale: 1, roll: 0 };

/**
 * A prop's hold (`hold`, optional): `scale` (0.1..2, about the grip) and `roll` degrees (within
 * ±180) about its handle, from its +x side facing the hand's `edge`; missing fields take
 * DEFAULT_HOLD's. The handle lies along the hand's grip axis: a lean comes from the hand's pose.
 */
export function validateHold(h, at) {
  const fail = (msg) => {
    throw new Error(`props.json: ${msg}`);
  };
  if (h === undefined) return { ...DEFAULT_HOLD };
  if (h === null || typeof h !== "object" || Array.isArray(h))
    fail(`${at}: hold is { scale, roll }`);
  for (const k of Object.keys(h))
    if (!["scale", "roll"].includes(k))
      fail(`${at}: hold has unknown field ${k} (scale, roll)`);
  const scale = h.scale ?? DEFAULT_HOLD.scale;
  if (!(Number.isFinite(scale) && scale >= 0.1 && scale <= 2))
    fail(`${at}: hold.scale ${scale} is outside 0.1..2`);
  const roll = h.roll ?? DEFAULT_HOLD.roll;
  if (!(Number.isFinite(roll) && Math.abs(roll) <= 180))
    fail(`${at}: hold.roll ${roll} is outside ±180 degrees`);
  return { scale, roll };
}

/** The props an item's look names (shared/inventory `look.model`): the ones built into the game. */
export function lookModels(inventory) {
  return (inventory.items ?? []).map((i) => i.look?.model).filter(Boolean);
}

/** The props' Rust table; `built`: the props whose GLB bytes are built in. */
export function rustProps(props, glbRoot, built, hand) {
  const v3 = (v) => `[${v.map((x) => f32(Number(x.toFixed(4)))).join(", ")}]`;
  const lines = [
    `pub const HAND_GRIP: HandGrip = HandGrip { bone: ${JSON.stringify(hand.bone)}, at: ${v3(hand.at)}, axis: ${v3(hand.axis)}, edge: ${v3(hand.edge)} };`,
    `pub static PROP_GLBS: &[(&str, &[u8])] = &[${built
      .map(
        (n) =>
          `(${JSON.stringify(n)}, include_bytes!(${JSON.stringify(`${glbRoot}/${props[n].glb}`)}))`,
      )
      .join(", ")}];`,
    "pub static PROPS: &[PropData] = &[",
  ];
  for (const [name, p] of Object.entries(props))
    lines.push(
      `    PropData { name: ${JSON.stringify(name)}, category: ${JSON.stringify(p.category)}, glb: ${JSON.stringify(p.glb)}, length: ${f32(p.length)}, size: ${v3(p.size)}, grip: ${v3(p.grip)}, axis: ${v3(p.axis)}, hold: PropHold { scale: ${f32(p.hold.scale)}, roll: ${f32(p.hold.roll)} } },`,
    );
  lines.push("];", "");
  return lines.join("\n");
}

const f32 = (v) => (Number.isInteger(v) ? `${v}.0` : `${v}`);

/** The Rust table (`glbRoot`: the GLB paths' directory, relative to the crate's src). */
export function rust(data, models, glbRoot) {
  const lines = [
    "// Generated by shared/models/gen.mjs from shared/models/models.json. Do not edit.",
    "use crate::{DetailLevel, Hand, ModelData, Retarget, RetargetBone, RoleClip};",
    "",
    `pub const PLAYER: &str = ${JSON.stringify(data.player ?? "")};`,
  ];
  const detail = validateDetail(data);
  lines.push(
    `pub static DETAIL_LEVELS: &[DetailLevel] = &[${detail.levels
      .map(
        (l) =>
          `DetailLevel { id: ${JSON.stringify(l.id)}, label: ${JSON.stringify(l.label)}, ratio: ${f32(l.ratio)} }`,
      )
      .join(", ")}];`,
    `pub const DETAIL_DEFAULT: usize = ${detail.levels.findIndex((l) => l.id === detail.default)};`,
  );
  const player = data.player && models[data.player];
  lines.push(
    player
      ? `pub static PLAYER_GLB: &[u8] = include_bytes!(${JSON.stringify(`${glbRoot}/${player.glb}`)});`
      : "pub static PLAYER_GLB: &[u8] = &[];",
  );
  // Every model's GLB, built in (ADR 0073: the creatures' models are drawn in the world).
  lines.push(
    `pub static GLBS: &[(&str, &[u8])] = &[${Object.entries(models)
      .map(
        ([name, m]) =>
          `(${JSON.stringify(name)}, include_bytes!(${JSON.stringify(`${glbRoot}/${m.glb}`)}))`,
      )
      .join(", ")}];`,
  );
  lines.push("pub static MODELS: &[ModelData] = &[");
  for (const [name, m] of Object.entries(models)) {
    const roles = Object.entries(m.roles)
      .map(
        ([role, r]) =>
          `RoleClip { role: ${JSON.stringify(role)}, clip: ${JSON.stringify(r.clip)}, speed: ${f32(r.speed ?? 1)} }`,
      )
      .join(", ");
    const v3 = (v) => `[${v.map(f32).join(", ")}]`;
    const hand = m.hand
      ? `Some(Hand { joint: ${m.hand.joint}, bone: ${JSON.stringify(m.hand.bone)}, at: ${v3(m.hand.at)}, rot: ${v3(m.hand.rot)} })`
      : "None";
    const retarget = m.retarget
      ? `Some(Retarget { rig: ${JSON.stringify(m.retarget.rig)}, root: ${JSON.stringify(m.retarget.root)}, bones: &[${m.retarget.bones
          .map(
            (b) =>
              `RetargetBone { bone: ${JSON.stringify(b.bone)}, joint: ${b.joint}, rest: ${v3(b.rest)} }`,
          )
          .join(", ")}] })`
      : "None";
    lines.push(
      `    ModelData { name: ${JSON.stringify(name)}, glb: ${JSON.stringify(m.glb)}, joints: ${m.joints}, scale: ${f32(m.scale)}, yaw: ${f32(m.yaw ?? 0)}, fallback: ${JSON.stringify(m.fallback)}, roles: &[${roles}], hand: ${hand}, retarget: ${retarget} },`,
    );
  }
  lines.push("];", "");
  return lines.join("\n");
}

const readJson = (p) => JSON.parse(readFileSync(p, "utf8"));

export function generate() {
  const palette = readJson(join(root, "shared/textures/palette.json"));
  const outputs = [];
  for (const [src, out] of [
    [here, "models.gen.rs"],
    [join(here, "fixtures"), "fixture.gen.rs"],
  ]) {
    const data = readJson(join(src, "models.json"));
    const models = validate(data, palette, src);
    const propsData = readJson(join(src, "props.json"));
    const props = validateProps(propsData, src);
    // The game builds in the props an item's look names; the fixture, all of its own.
    const built =
      src === here
        ? lookModels(readJson(join(root, "shared/inventory/inventory.json")))
        : Object.keys(props);
    for (const n of built)
      if (!props[n])
        throw new Error(
          `inventory.json: look.model ${n} is not a prop of props.json`,
        );
    const glbRoot = relative(CRATE, src);
    outputs.push([
      join(CRATE, out),
      rust(data, models, glbRoot).replace(
        "use crate::{DetailLevel, Hand, ModelData, Retarget, RetargetBone, RoleClip};",
        "use crate::{DetailLevel, Hand, HandGrip, ModelData, PropData, PropHold, Retarget, RetargetBone, RoleClip};",
      ) + rustProps(props, glbRoot, built, validateHand(propsData.hand)),
    ]);
  }
  return outputs;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  for (const [path, text] of generate()) writeFileSync(path, text);
  console.log("shared/models: wrote models.gen.rs, fixture.gen.rs");
}
