// The character rig (docs/CHARACTERS.md): validation of shared/characters/characters.json, composing
// a character (its parts, a variant's accessories, a held item) into a flat list of parts on bones,
// and sampling and layering animation clips. Dependency-free, so it runs in Node (gen.mjs, the
// tests) and, pasted in by gen.mjs, in the Character Lab page (docs/prototypes/character-lab.html).
// A bad entry throws an Error whose message names it.

// ---- The engine's vocabulary: fixed in code (CHARACTERS.md §3) --------------------------------------
/** Part shapes. Every kind is scaled to fill its `size` box (metres, x y z), axis along y. */
export const PART_KINDS = ['box', 'round_box', 'ellipsoid', 'cylinder', 'capsule', 'cone'];
/** Joints: `ball` turns about x, y and z; `hinge` about its one `axis`; `fixed` never turns. */
export const JOINTS = ['ball', 'hinge', 'fixed'];
export const AXES = ['x', 'y', 'z'];
/** How a clip moves between its keys. */
export const EASES = ['linear', 'smooth', 'spline', 'step'];
/** Animation layers, lowest first: a higher layer replaces the bones it keys (at its weight). */
export const LAYERS = ['base', 'hold', 'action'];
/** Where a clip plays: on the full body, or on the first-person arms (in view space). */
export const SPACES = ['body', 'arms'];
/** Which view draws a part: both, the full body only, or the first-person arms only. */
export const VIEWS = ['both', 'body', 'arms'];
/** Materials the game supplies at run time: the held block's own look. */
export const RUNTIME_MATERIALS = ['$block'];
/** Where an accessory sits; a look may wear one accessory per place. */
export const PLACES = ['head', 'hair', 'face', 'neck', 'shoulders', 'back', 'back_top', 'waist', 'hands', 'wrists', 'feet', 'antlers'];

const NAME = /^[a-z][a-z0-9_]*$/;
const ID = /^[a-z0-9_]+:[a-z0-9_]+$/;
const TOP_KEYS = ['$comment', 'format', 'rigs', 'characters', 'accessories', 'items', 'clips'];
const BONE_KEYS = ['name', 'parent', 'joint', 'axis', 'limits'];
const CHARACTER_KEYS = [
  '$comment', 'name', 'label', 'rig', 'height', 'bones', 'sockets', 'slots', 'parts', 'accessories',
  'variants', 'clips', 'firstPerson',
];
const PART_KEYS = ['id', 'on', 'kind', 'size', 'at', 'rot', 'material', 'round', 'tip', 'mirror', 'view'];
const ACCESSORY_KEYS = ['$comment', 'label', 'place', 'fits', 'slots', 'hides', 'parts'];
const ITEM_KEYS = ['$comment', 'label', 'on', 'parts'];
const VARIANT_KEYS = ['label', 'accessories', 'slots'];
const CLIP_KEYS = ['$comment', 'rig', 'space', 'layer', 'duration', 'loop', 'ease', 'cycle', 'keys'];
const KEY_KEYS = ['t', 'bones', 'root'];
const FP_KEYS = ['bones', 'anchor', 'fov', 'clips'];
/** Rest pose: the body without accessories must reach `height` within this, feet on the ground. */
export const HEIGHT_TOLERANCE = 0.03;
const MAX_SIZE = 4;

const fail = (where, message) => {
  throw new Error(`shared/characters: ${where}: ${message}`);
};
const isObject = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);
const isNum = (v) => typeof v === 'number' && Number.isFinite(v);
const vec3 = (v, where, lo = -MAX_SIZE, hi = MAX_SIZE) => {
  if (!Array.isArray(v) || v.length !== 3 || !v.every((x) => isNum(x) && x >= lo && x <= hi))
    fail(where, `must be three numbers ${lo}–${hi}`);
  return v;
};
const keysOf = (obj, allowed, where) => {
  if (!isObject(obj)) fail(where, 'must be an object');
  for (const k of Object.keys(obj)) if (!allowed.includes(k)) fail(where, `unknown entry "${k}"`);
};
const oneOf = (v, list, where) => {
  if (!list.includes(v)) fail(where, `"${v}" is not one of ${list.join(', ')}`);
  return v;
};

/** "_l" → "_r" on a sided name; undefined when the name isn't sided. */
export const mirrorName = (n) => (n.endsWith('_l') ? `${n.slice(0, -2)}_r` : undefined);

// ---- Materials: names from the shared style (shared/textures, ADR 0040) ------------------------------
/** What a material reference names: a material, a palette ramp (or one stop, `ramp.light`), a swatch,
 * or a run-time material. Undefined when it names nothing. */
export function lookupMaterial(style, ref) {
  if (typeof ref !== 'string') return undefined;
  if (RUNTIME_MATERIALS.includes(ref)) return { kind: 'runtime', name: ref };
  if (style.materials?.[ref]) {
    const m = style.materials[ref];
    const ramp = style.ramps?.[m.ramp];
    return ramp ? { kind: 'material', name: ref, ramp, overlay: m.overlay ?? 0 } : undefined;
  }
  if (style.swatches?.[ref]) {
    const c = style.swatches[ref];
    return { kind: 'swatch', name: ref, ramp: { shadow: c, base: c, light: c }, overlay: 0 };
  }
  const [rampName, stop] = ref.split('.');
  const ramp = style.ramps?.[rampName];
  if (!ramp) return undefined;
  if (stop === undefined) return { kind: 'ramp', name: ref, ramp, overlay: 0 };
  if (!['shadow', 'base', 'light'].includes(stop)) return undefined;
  const c = ramp[stop];
  return { kind: 'ramp', name: ref, ramp: { shadow: c, base: c, light: c }, overlay: 0 };
}

// ---- Validation -----------------------------------------------------------------------------------
function validateRig(name, rig) {
  const where = `rigs.${name}`;
  keysOf(rig, ['$comment', 'bones'], where);
  if (!Array.isArray(rig.bones) || rig.bones.length === 0) fail(`${where}.bones`, 'must be a non-empty list');
  const seen = new Map();
  rig.bones.forEach((b, i) => {
    const w = `${where}.bones[${i}]`;
    keysOf(b, BONE_KEYS, w);
    if (typeof b.name !== 'string' || !NAME.test(b.name)) fail(w, `name "${b.name}" must be lower_snake_case`);
    if (seen.has(b.name)) fail(w, `bone "${b.name}" is repeated`);
    if (i === 0) {
      if (b.parent !== null && b.parent !== undefined) fail(w, 'the first bone is the root and has no parent');
    } else if (!seen.has(b.parent)) fail(w, `parent "${b.parent}" of "${b.name}" is not an earlier bone`);
    oneOf(b.joint, JOINTS, `${w}.joint`);
    if (b.joint === 'hinge') oneOf(b.axis, AXES, `${w}.axis`);
    else if (b.axis !== undefined) fail(w, `only a hinge takes an axis`);
    if (b.limits !== undefined) {
      const checkRange = (r, ww) => {
        if (!Array.isArray(r) || r.length !== 2 || !r.every(isNum) || r[0] > r[1] || r[0] < -180 || r[1] > 180)
          fail(ww, 'must be [min, max] degrees within −180–180');
      };
      if (b.joint === 'hinge') checkRange(b.limits, `${w}.limits`);
      else if (b.joint === 'ball') {
        keysOf(b.limits, AXES, `${w}.limits`);
        for (const a of Object.keys(b.limits)) checkRange(b.limits[a], `${w}.limits.${a}`);
      } else fail(w, 'a fixed bone takes no limits');
    }
    seen.set(b.name, b);
  });
  return seen;
}

function validatePart(p, where, ctx) {
  keysOf(p, PART_KEYS, where);
  if (typeof p.id !== 'string' || !NAME.test(p.id)) fail(where, `id "${p.id}" must be lower_snake_case`);
  const w = `${where} (${p.id})`;
  if (!ctx.attach(p.on)) fail(w, `on "${p.on}" is not a bone${ctx.sockets ? ' or socket' : ''} of ${ctx.of}`);
  oneOf(p.kind, PART_KINDS, `${w}.kind`);
  vec3(p.size, `${w}.size`, 0.005, MAX_SIZE);
  if (p.at !== undefined) vec3(p.at, `${w}.at`);
  if (p.rot !== undefined) vec3(p.rot, `${w}.rot`, -360, 360);
  if (p.round !== undefined) {
    if (p.kind !== 'round_box') fail(w, 'only a round_box takes "round"');
    if (!isNum(p.round) || p.round < 0 || p.round > 0.5) fail(`${w}.round`, 'must be 0–0.5 (of the smallest side)');
  }
  if (p.tip !== undefined) {
    if (p.kind !== 'cone') fail(w, 'only a cone takes "tip"');
    if (!isNum(p.tip) || p.tip < 0 || p.tip > 1) fail(`${w}.tip`, 'must be 0–1 (the top radius as a fraction)');
  }
  if (p.view !== undefined) oneOf(p.view, VIEWS, `${w}.view`);
  if (p.mirror !== undefined) {
    if (typeof p.mirror !== 'boolean') fail(`${w}.mirror`, 'must be true or false');
    if (p.mirror) {
      if (!mirrorName(p.id)) fail(w, 'a mirrored part\'s id ends in "_l" (its copy gets "_r")');
      const other = mirrorName(p.on);
      if (other && !ctx.attach(other)) fail(w, `its mirror needs "${other}" on ${ctx.of}`);
    }
  }
  if (typeof p.material !== 'string' || !ctx.material(p.material))
    fail(w, `material "${p.material}" is not a slot of ${ctx.of}, a shared material, ramp or swatch`);
}

/** The ids a part list makes, mirrored copies included. */
const partIds = (parts) => parts.flatMap((p) => (p.mirror ? [p.id, mirrorName(p.id)] : [p.id]));

function validateClip(id, c, rigs) {
  const where = `clips.${id}`;
  keysOf(c, CLIP_KEYS, where);
  const bones = rigs.get(c.rig);
  if (!bones) fail(where, `rig "${c.rig}" is not defined`);
  if (c.space !== undefined) oneOf(c.space, SPACES, `${where}.space`);
  oneOf(c.layer, LAYERS, `${where}.layer`);
  if (!isNum(c.duration) || c.duration <= 0 || c.duration > 60) fail(`${where}.duration`, 'must be seconds, 0–60');
  if (typeof c.loop !== 'boolean') fail(`${where}.loop`, 'must be true or false');
  oneOf(c.ease, EASES, `${where}.ease`);
  if (c.cycle !== undefined && (!isNum(c.cycle) || c.cycle <= 0 || c.cycle > 20))
    fail(`${where}.cycle`, 'must be metres travelled per loop, 0–20');
  if (!Array.isArray(c.keys) || c.keys.length === 0) fail(`${where}.keys`, 'must be a non-empty list');
  let last = -1;
  c.keys.forEach((k, i) => {
    const w = `${where}.keys[${i}]`;
    keysOf(k, KEY_KEYS, w);
    if (!isNum(k.t) || k.t < 0 || k.t > c.duration) fail(`${w}.t`, `must be 0–${c.duration} (the duration)`);
    if (k.t <= last) fail(`${w}.t`, 'keys must be in increasing time');
    if (c.loop && k.t >= c.duration) fail(`${w}.t`, 'a loop\'s keys come before its duration (it wraps to the first)');
    last = k.t;
    if (k.root !== undefined) vec3(k.root, `${w}.root`, -2, 2);
    if (k.bones !== undefined) {
      if (!isObject(k.bones)) fail(`${w}.bones`, 'must be an object of bone → [x, y, z] degrees');
      for (const [b, r] of Object.entries(k.bones)) {
        const bone = bones.get(b);
        if (!bone) fail(`${w}.bones`, `"${b}" is not a bone of rig "${c.rig}"`);
        vec3(r, `${w}.bones.${b}`, -360, 360);
        if (bone.joint === 'fixed' && r.some((x) => x !== 0)) fail(`${w}.bones.${b}`, `"${b}" is a fixed joint`);
        if (bone.joint === 'hinge') {
          const ai = AXES.indexOf(bone.axis);
          if (r.some((x, j) => j !== ai && x !== 0)) fail(`${w}.bones.${b}`, `"${b}" is a hinge about ${bone.axis} only`);
        }
        const lim = bone.limits;
        if (lim) {
          AXES.forEach((a, j) => {
            const range = bone.joint === 'hinge' ? (a === bone.axis ? lim : undefined) : lim[a];
            if (range && (r[j] < range[0] || r[j] > range[1]))
              fail(`${w}.bones.${b}`, `${a} ${r[j]}° is outside "${b}"'s limits ${range[0]}–${range[1]}°`);
          });
        }
      }
    }
  });
  return { ...c, space: c.space ?? 'body' };
}

/**
 * Validates the whole file against the shared style (`{ ramps, swatches, materials }`). Returns it
 * unchanged on success; throws naming the first bad entry otherwise.
 */
export function validate(data, style) {
  keysOf(data, TOP_KEYS, 'file');
  if (data.format !== 1) fail('format', 'must be 1');
  for (const k of ['rigs', 'characters', 'accessories', 'items', 'clips'])
    if (!isObject(data[k])) fail(k, 'must be an object');
  const rigs = new Map();
  for (const [name, rig] of Object.entries(data.rigs)) {
    if (!NAME.test(name)) fail(`rigs.${name}`, 'rig names are lower_snake_case');
    rigs.set(name, validateRig(name, rig));
  }
  const clips = new Map();
  for (const [id, c] of Object.entries(data.clips)) {
    if (!NAME.test(id)) fail(`clips.${id}`, 'clip names are lower_snake_case');
    clips.set(id, validateClip(id, c, rigs));
  }
  const material = (ref) => lookupMaterial(style, ref) !== undefined;

  // Accessories and items first (their parts name sockets; checked per character below).
  for (const [id, a] of Object.entries(data.accessories)) {
    const w = `accessories.${id}`;
    if (!NAME.test(id)) fail(w, 'accessory names are lower_snake_case');
    keysOf(a, ACCESSORY_KEYS, w);
    oneOf(a.place, PLACES, `${w}.place`);
    if (!Array.isArray(a.fits) || a.fits.length === 0 || !a.fits.every((r) => rigs.has(r)))
      fail(`${w}.fits`, 'must list the rigs it fits');
    if (a.slots !== undefined) {
      keysOf(a.slots, Object.keys(a.slots), `${w}.slots`);
      for (const [s, m] of Object.entries(a.slots)) {
        if (!NAME.test(s)) fail(`${w}.slots`, `slot "${s}" must be lower_snake_case`);
        if (lookupMaterial(style, s)) fail(`${w}.slots.${s}`, 'a slot can\'t share a shared material\'s name');
        if (!material(m)) fail(`${w}.slots.${s}`, `"${m}" is not a shared material, ramp or swatch`);
      }
    }
    if (a.hides !== undefined && (!Array.isArray(a.hides) || !a.hides.every((h) => typeof h === 'string')))
      fail(`${w}.hides`, 'must list part ids');
    if (!Array.isArray(a.parts) || a.parts.length === 0) fail(`${w}.parts`, 'must be a non-empty list');
  }
  for (const [id, it] of Object.entries(data.items)) {
    const w = `items.${id}`;
    if (!NAME.test(id)) fail(w, 'item names are lower_snake_case');
    keysOf(it, ITEM_KEYS, w);
    if (typeof it.on !== 'string') fail(`${w}.on`, 'must name a socket');
    if (!Array.isArray(it.parts) || it.parts.length === 0) fail(`${w}.parts`, 'must be a non-empty list');
  }

  for (const [id, ch] of Object.entries(data.characters)) {
    const w = `characters.${id}`;
    if (!NAME.test(id)) fail(w, 'character keys are lower_snake_case');
    keysOf(ch, CHARACTER_KEYS, w);
    if (typeof ch.name !== 'string' || !ID.test(ch.name)) fail(`${w}.name`, 'must be namespaced, e.g. "dwell:player"');
    const bones = rigs.get(ch.rig);
    if (!bones) fail(`${w}.rig`, `rig "${ch.rig}" is not defined`);
    if (!isNum(ch.height) || ch.height <= 0 || ch.height > MAX_SIZE) fail(`${w}.height`, `must be metres, 0–${MAX_SIZE}`);
    keysOf(ch.bones, [...bones.keys()], `${w}.bones`);
    for (const b of bones.keys()) {
      if (ch.bones[b] === undefined) fail(`${w}.bones`, `missing the rest offset of bone "${b}"`);
      vec3(ch.bones[b], `${w}.bones.${b}`);
    }
    const sockets = ch.sockets ?? {};
    keysOf(sockets, Object.keys(sockets), `${w}.sockets`);
    for (const [s, so] of Object.entries(sockets)) {
      const sw = `${w}.sockets.${s}`;
      if (!NAME.test(s)) fail(sw, 'socket names are lower_snake_case');
      if (bones.has(s)) fail(sw, 'a socket can\'t share a bone\'s name');
      keysOf(so, ['bone', 'at'], sw);
      if (!bones.has(so.bone)) fail(sw, `bone "${so.bone}" is not a bone of rig "${ch.rig}"`);
      vec3(so.at, `${sw}.at`);
    }
    const slots = ch.slots ?? {};
    keysOf(slots, Object.keys(slots), `${w}.slots`);
    for (const [s, m] of Object.entries(slots)) {
      if (!NAME.test(s)) fail(`${w}.slots`, `slot "${s}" must be lower_snake_case`);
      if (lookupMaterial(style, s)) fail(`${w}.slots.${s}`, 'a slot can\'t share a shared material\'s name');
      if (!material(m)) fail(`${w}.slots.${s}`, `"${m}" is not a shared material, ramp or swatch`);
    }
    const attach = (on) => bones.has(on) || Object.hasOwn(sockets, on);
    if (!Array.isArray(ch.parts) || ch.parts.length === 0) fail(`${w}.parts`, 'must be a non-empty list');
    const own = { of: `character "${id}"`, sockets: true, attach, material: (m) => Object.hasOwn(slots, m) || material(m) };
    ch.parts.forEach((p, i) => validatePart(p, `${w}.parts[${i}]`, own));
    const ids = partIds(ch.parts);
    const dup = ids.find((x, i) => ids.indexOf(x) !== i);
    if (dup) fail(`${w}.parts`, `part id "${dup}" is repeated`);

    // Every look this character can wear: the default and each variant.
    const looks = [['accessories', ch.accessories ?? [], {}]];
    const variants = ch.variants ?? {};
    keysOf(variants, Object.keys(variants), `${w}.variants`);
    for (const [v, va] of Object.entries(variants)) {
      if (!NAME.test(v)) fail(`${w}.variants`, `variant "${v}" must be lower_snake_case`);
      keysOf(va, VARIANT_KEYS, `${w}.variants.${v}`);
      for (const [s, m] of Object.entries(va.slots ?? {})) if (!material(m)) fail(`${w}.variants.${v}.slots.${s}`, `"${m}" is not a shared material, ramp or swatch`);
      looks.push([`variants.${v}`, va.accessories ?? ch.accessories ?? [], va.slots ?? {}]);
    }
    for (const [lw, list, overrides] of looks) {
      const where = `${w}.${lw}`;
      if (!Array.isArray(list)) fail(where, 'must list accessory names');
      for (const s of Object.keys(overrides)) {
        const declared = Object.hasOwn(slots, s) || list.some((a) => Object.hasOwn(data.accessories[a]?.slots ?? {}, s));
        if (!declared) fail(`${where}.slots.${s}`, `no slot "${s}" on the character or its accessories`);
      }
      const places = new Map();
      for (const a of list) {
        const acc = data.accessories[a];
        if (!acc) fail(where, `accessory "${a}" is not defined`);
        if (!acc.fits.includes(ch.rig)) fail(where, `accessory "${a}" doesn't fit rig "${ch.rig}"`);
        if (places.has(acc.place)) fail(where, `"${a}" and "${places.get(acc.place)}" both take the ${acc.place}`);
        places.set(acc.place, a);
        for (const h of acc.hides ?? []) if (!ids.includes(h)) fail(`accessories.${a}.hides`, `"${h}" is not a part of character "${id}"`);
        const accSlots = { ...(acc.slots ?? {}) };
        const ctx = {
          of: `character "${id}" (worn as "${a}")`,
          sockets: true,
          attach,
          material: (m) => Object.hasOwn(slots, m) || Object.hasOwn(accSlots, m) || material(m),
        };
        acc.parts.forEach((p, i) => validatePart(p, `accessories.${a}.parts[${i}]`, ctx));
      }
    }

    const roles = ch.clips ?? {};
    keysOf(roles, Object.keys(roles), `${w}.clips`);
    for (const [role, c] of Object.entries(roles)) {
      const clip = clips.get(c);
      if (!clip) fail(`${w}.clips.${role}`, `clip "${c}" is not defined`);
      if (clip.rig !== ch.rig) fail(`${w}.clips.${role}`, `clip "${c}" is for rig "${clip.rig}", not "${ch.rig}"`);
      if (clip.space !== 'body') fail(`${w}.clips.${role}`, `clip "${c}" plays on the arms; list it under firstPerson.clips`);
    }

    if (ch.firstPerson !== undefined) {
      const fw = `${w}.firstPerson`;
      const fp = ch.firstPerson;
      keysOf(fp, FP_KEYS, fw);
      if (!Array.isArray(fp.bones) || fp.bones.length === 0) fail(`${fw}.bones`, 'must list the arm chain\'s bones');
      fp.bones.forEach((b, i) => {
        if (!bones.has(b)) fail(`${fw}.bones`, `"${b}" is not a bone of rig "${ch.rig}"`);
        if (i > 0 && !fp.bones.includes(bones.get(b).parent)) fail(`${fw}.bones`, `"${b}"'s parent is not in the chain`);
      });
      vec3(fp.anchor, `${fw}.anchor`, -1, 1);
      if (!isNum(fp.fov) || fp.fov < 30 || fp.fov > 110) fail(`${fw}.fov`, 'must be degrees, 30–110');
      for (const [role, c] of Object.entries(fp.clips ?? {})) {
        const clip = clips.get(c);
        if (!clip) fail(`${fw}.clips.${role}`, `clip "${c}" is not defined`);
        if (clip.space !== 'arms') fail(`${fw}.clips.${role}`, `clip "${c}" plays on the full body, not the arms`);
        if (clip.rig !== ch.rig) fail(`${fw}.clips.${role}`, `clip "${c}" is for rig "${clip.rig}"`);
        for (const k of clips.get(c).keys)
          for (const b of Object.keys(k.bones ?? {}))
            if (!fp.bones.includes(b)) fail(`${fw}.clips.${role}`, `clip "${c}" keys "${b}", which is not in the arm chain`);
      }
    }

    // Held items attach to a socket of a character that has it.
    for (const [iid, it] of Object.entries(data.items)) {
      if (!Object.hasOwn(sockets, it.on)) continue;
      const ctx = { of: `item "${iid}" on character "${id}"`, sockets: true, attach, material: (m) => Object.hasOwn(slots, m) || material(m) };
      it.parts.forEach((p, i) => validatePart(p, `items.${iid}.parts[${i}]`, ctx));
    }

    // The rest pose: feet on the ground, the top at the declared height.
    const b = bounds(compose(data, style, id, { accessories: [] }), 'body');
    if (Math.abs(b.min[1]) > 0.01) fail(w, `the rest pose's lowest point is at ${b.min[1].toFixed(3)} m, not on the ground (0)`);
    if (Math.abs(b.max[1] - ch.height) > HEIGHT_TOLERANCE)
      fail(`${w}.height`, `the rest pose without accessories is ${b.max[1].toFixed(3)} m tall, not ${ch.height} m (±${HEIGHT_TOLERANCE})`);
  }
  return data;
}

// ---- Composing a character ------------------------------------------------------------------------
const mirrorPart = (p) => ({
  ...p,
  id: mirrorName(p.id),
  on: mirrorName(p.on) ?? p.on,
  at: p.at ? [-p.at[0], p.at[1], p.at[2]] : undefined,
  rot: p.rot ? [p.rot[0], -p.rot[1], -p.rot[2]] : undefined,
  mirror: false,
});
const expand = (parts) => parts.flatMap((p) => (p.mirror ? [{ ...p, mirror: false }, mirrorPart(p)] : [p]));

/**
 * One character as drawn: `{ id, name, rig, height, bones: [{ name, parent, joint, axis, limits, rest }],
 * parts: [{ id, bone, kind, size, at, rot, round, tip, view, material, from }], firstPerson }`. A part's
 * `bone` is resolved through its socket and `at` is in the bone's frame; `material` is a shared style
 * name (or `$block`). `opts`: `variant`, `accessories` (replaces the look's list), `slots` (overrides),
 * `item` (a held item's name).
 */
export function compose(data, style, id, opts = {}) {
  const ch = data.characters[id];
  if (!ch) fail(`characters.${id}`, 'is not defined');
  const rig = data.rigs[ch.rig];
  const variant = opts.variant ? ch.variants?.[opts.variant] : undefined;
  if (opts.variant && !variant) fail(`characters.${id}.variants.${opts.variant}`, 'is not defined');
  const worn = opts.accessories ?? variant?.accessories ?? ch.accessories ?? [];
  const sockets = ch.sockets ?? {};
  const slots = {};
  for (const a of worn) Object.assign(slots, data.accessories[a]?.slots ?? {});
  Object.assign(slots, ch.slots ?? {}, variant?.slots ?? {}, opts.slots ?? {});
  const resolve = (on, at) => {
    const so = sockets[on];
    const a = at ?? [0, 0, 0];
    return so ? { bone: so.bone, at: [so.at[0] + a[0], so.at[1] + a[1], so.at[2] + a[2]] } : { bone: on, at: a };
  };
  const hidden = new Set();
  for (const a of worn) for (const h of data.accessories[a]?.hides ?? []) hidden.add(h);
  const out = [];
  const add = (parts, from) => {
    for (const p of expand(parts)) {
      if (from === 'body' && hidden.has(p.id)) continue;
      const { bone, at } = resolve(p.on, p.at);
      out.push({
        id: p.id,
        bone,
        kind: p.kind,
        size: p.size,
        at,
        rot: p.rot ?? [0, 0, 0],
        round: p.round ?? (p.kind === 'round_box' ? 0.25 : 0),
        tip: p.tip ?? 0,
        view: p.view ?? 'both',
        material: slots[p.material] ?? p.material,
        from,
      });
    }
  };
  add(ch.parts, 'body');
  for (const a of worn) {
    if (!data.accessories[a]) fail(`characters.${id}`, `accessory "${a}" is not defined`);
    add(data.accessories[a].parts, a);
  }
  if (opts.item) {
    const it = data.items[opts.item];
    if (!it) fail(`items.${opts.item}`, 'is not defined');
    add(it.parts.map((p) => ({ ...p, on: p.on ?? it.on })), `item:${opts.item}`);
  }
  for (const p of out) if (!lookupMaterial(style, p.material)) fail(`characters.${id}`, `part "${p.id}" has no material "${p.material}"`);
  return {
    id,
    name: ch.name,
    rig: ch.rig,
    height: ch.height,
    bones: rig.bones.map((b) => ({ ...b, parent: b.parent ?? null, rest: ch.bones[b.name] })),
    parts: out,
    firstPerson: ch.firstPerson,
  };
}

// ---- Math: Euler XYZ (three.js's default order), degrees --------------------------------------------
const D = Math.PI / 180;
/** v rotated by Rx·Ry·Rz (the order three.js applies an 'XYZ' Euler). */
export function rotate(v, r) {
  let [x, y, z] = v;
  const [a, b, c] = [r[0] * D, r[1] * D, r[2] * D];
  [x, y] = [x * Math.cos(c) - y * Math.sin(c), x * Math.sin(c) + y * Math.cos(c)];
  [x, z] = [x * Math.cos(b) + z * Math.sin(b), -x * Math.sin(b) + z * Math.cos(b)];
  [y, z] = [y * Math.cos(a) - z * Math.sin(a), y * Math.sin(a) + z * Math.cos(a)];
  return [x, y, z];
}

/** Bone pivots in the character's frame for a pose (`{ bones: { name: [x, y, z] degrees }, root }`),
 * each with its accumulated rotation as a list of Eulers applied innermost first. */
export function pivots(model, pose = {}) {
  const out = new Map();
  for (const b of model.bones) {
    const r = pose.bones?.[b.name] ?? [0, 0, 0];
    if (b.parent === null) {
      const root = pose.root ?? [0, 0, 0];
      out.set(b.name, { at: [b.rest[0] + root[0], b.rest[1] + root[1], b.rest[2] + root[2]], chain: [r] });
    } else {
      const p = out.get(b.parent);
      const off = p.chain.reduce((v, e) => rotate(v, e), b.rest);
      out.set(b.name, { at: [p.at[0] + off[0], p.at[1] + off[1], p.at[2] + off[2]], chain: [r, ...p.chain] });
    }
  }
  return out;
}

/** The axis-aligned box around a model's parts drawn in `view` ('body' or 'arms'), in a pose. */
export function bounds(model, view = 'body', pose = {}) {
  const piv = pivots(model, pose);
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (const p of model.parts) {
    if (p.view !== 'both' && p.view !== view) continue;
    const b = piv.get(p.bone);
    const h = p.size.map((s) => s / 2);
    for (let i = 0; i < 8; i++) {
      const c = [i & 1 ? h[0] : -h[0], i & 2 ? h[1] : -h[1], i & 4 ? h[2] : -h[2]];
      let v = rotate(c, p.rot);
      v = [v[0] + p.at[0], v[1] + p.at[1], v[2] + p.at[2]];
      v = b.chain.reduce((acc, e) => rotate(acc, e), v);
      for (let k = 0; k < 3; k++) {
        min[k] = Math.min(min[k], b.at[k] + v[k]);
        max[k] = Math.max(max[k], b.at[k] + v[k]);
      }
    }
  }
  return { min, max };
}

// ---- Animation ------------------------------------------------------------------------------------
const lerp = (a, b, u) => a + (b - a) * u;
const lerp3 = (a, b, u) => [lerp(a[0], b[0], u), lerp(a[1], b[1], u), lerp(a[2], b[2], u)];
const catmull = (p0, p1, p2, p3, u) =>
  0.5 * (2 * p1 + (-p0 + p2) * u + (2 * p0 - 5 * p1 + 4 * p2 - p3) * u * u + (-p0 + 3 * p1 - 3 * p2 + p3) * u * u * u);

/** One channel's value at time t from its keys `[{ t, v }]` (sorted). */
function channel(keys, t, clip) {
  const n = keys.length;
  if (n === 1) return keys[0].v;
  const T = clip.duration;
  let i = -1;
  for (let k = 0; k < n; k++) if (keys[k].t <= t) i = k;
  let a, b, ta, tb;
  if (i === -1) {
    if (!clip.loop) return keys[0].v;
    [a, b, ta, tb] = [n - 1, 0, keys[n - 1].t - T, keys[0].t];
  } else if (i === n - 1) {
    if (!clip.loop) return keys[n - 1].v;
    [a, b, ta, tb] = [n - 1, 0, keys[n - 1].t, keys[0].t + T];
  } else [a, b, ta, tb] = [i, i + 1, keys[i].t, keys[i + 1].t];
  const u = tb > ta ? (t - ta) / (tb - ta) : 0;
  const A = keys[a].v;
  const B = keys[b].v;
  switch (clip.ease) {
    case 'step':
      return A;
    case 'linear':
      return lerp3(A, B, u);
    case 'smooth':
      return lerp3(A, B, u * u * (3 - 2 * u));
    default: {
      const at = (k) => (clip.loop ? keys[((k % n) + n) % n].v : keys[Math.max(0, Math.min(n - 1, k))].v);
      const P0 = at(a - 1);
      const P3 = at(b + 1);
      return [0, 1, 2].map((j) => catmull(P0[j], A[j], B[j], P3[j], u));
    }
  }
}

/** A clip's pose at time t (seconds; loops wrap): `{ bones: { name: [x, y, z] }, root }`, only the
 * channels it keys. Each bone interpolates between the keys that name it. */
export function sample(clip, t) {
  const T = clip.duration;
  const tt = clip.loop ? ((t % T) + T) % T : Math.max(0, Math.min(T, t));
  const chans = new Map();
  let root;
  for (const k of clip.keys) {
    for (const [b, r] of Object.entries(k.bones ?? {})) {
      if (!chans.has(b)) chans.set(b, []);
      chans.get(b).push({ t: k.t, v: r });
    }
    if (k.root) (root ??= []).push({ t: k.t, v: k.root });
  }
  const bones = {};
  for (const [b, keys] of chans) bones[b] = channel(keys, tt, clip);
  return { bones, root: root ? channel(root, tt, clip) : undefined };
}

/** Layers poses lowest first (`[{ pose, weight }]`): each replaces the bones (and root) it keys, blended
 * by its weight (0–1) toward the layers below. */
export function layer(entries) {
  const bones = {};
  let root = [0, 0, 0];
  for (const { pose, weight = 1 } of entries) {
    for (const [b, r] of Object.entries(pose.bones)) bones[b] = lerp3(bones[b] ?? [0, 0, 0], r, weight);
    if (pose.root) root = lerp3(root, pose.root, weight);
  }
  return { bones, root };
}
