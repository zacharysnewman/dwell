// node --test shared/models/gen.test.mjs: the skinned models' data (docs/CHARACTERS.md §11).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  applyCoat,
  applyRepaint,
  coatColour,
  validateCoat,
  validateRepaint,
} from "./coat.mjs";
import {
  allLevelParts,
  islands,
  nearestRamp,
  readMesh,
  rows,
  seedParts,
  validateParts,
} from "./parts.mjs";
import {
  checkCreatureParts,
  generate,
  glbAccessor,
  glbJson,
  glbSummary,
  lookModels,
  propSummary,
  rustProps,
  validateHand,
  measureHandle,
  validateProps,
  paletteStops,
  resolveColor,
  rust,
  validate,
} from "./gen.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");
const palette = JSON.parse(
  readFileSync(join(root, "shared/textures/palette.json"), "utf8"),
);
const data = () => JSON.parse(readFileSync(join(here, "models.json"), "utf8"));
const fixtures = join(here, "fixtures");
const fixture = () =>
  JSON.parse(readFileSync(join(fixtures, "models.json"), "utf8"));

test("the data validates and the generated tables are current", () => {
  validate(data(), palette, here);
  for (const [path, text] of generate()) {
    assert.equal(
      readFileSync(path, "utf8"),
      text,
      `${path} is stale: run node shared/models/gen.mjs`,
    );
  }
});

test("every GLB holds the clips its roles name", () => {
  for (const [name, m] of Object.entries(data().models)) {
    const s = glbSummary(readFileSync(join(here, m.glb)));
    for (const [role, r] of Object.entries(m.roles))
      assert.ok(s.clips.includes(r.clip), `${name} ${role}`);
  }
});

test("a model is added by data alone: the fixture validates and lands in the table", () => {
  const f = fixture();
  const models = validate(f, palette, fixtures);
  assert.equal(models.two_bone.joints, 2);
  assert.deepEqual(models.two_bone.clips, ["rest", "bend"]);
  const out = rust(f, models, "../../../shared/models/fixtures");
  assert.match(out, /name: "two_bone"/);
  assert.match(out, /role: "sprint", clip: "bend", speed: 2\.0/);
  // Its hand by data alone: the joint named in its skin, as an index.
  assert.match(
    out,
    /hand: Some\(Hand \{ joint: 1, bone: "hand_r", at: \[0\.0, 0\.1, 0\.0\], rot: \[0\.0, 0\.0, 90\.0\] \}\)/,
  );
  assert.match(
    out,
    /include_bytes!\("\.\.\/\.\.\/\.\.\/shared\/models\/fixtures\/two_bone\.glb"\)/,
  );
});

test("the detail levels are data: the fixture's own level lands in the table", () => {
  const f = fixture();
  const out = rust(
    f,
    validate(f, palette, fixtures),
    "../../../shared/models/fixtures",
  );
  assert.match(
    out,
    /DetailLevel \{ id: "coarse", label: "Coarse", ratio: 0\.5 \}/,
  );
  assert.match(out, /DETAIL_DEFAULT: usize = 1;/);
  // Every game model holds one mesh a level.
  const d = data();
  for (const [name, m] of Object.entries(d.models)) {
    const s = glbSummary(readFileSync(join(here, m.glb)));
    assert.deepEqual(
      s.meshNames,
      d.detail.levels.map((_, k) => `detail_${k}`),
      name,
    );
  }
});

test("bad detail levels fail naming the problem", () => {
  const bad = (edit, re) => {
    const f = fixture();
    edit(f.detail, f);
    assert.throws(() => validate(f, palette, fixtures), re);
  };
  bad((d) => (d.default = "ultra"), /default ultra is not a level/);
  bad((d) => (d.levels[0].ratio = 0.9), /the first level is the full mesh/);
  bad((d) => (d.levels[1].ratio = 1), /ratios fall level by level/);
  bad((d) => (d.levels[1].id = "full"), /level full twice/);
  bad(
    (d) => d.levels.push({ id: "tiny", label: "Tiny", ratio: 0.1 }),
    /not one a detail level/,
  );
});

test("a retarget map is data: the fixture's lands in the table, bad names fail naming them", () => {
  const f = fixture();
  const models = validate(f, palette, fixtures);
  assert.deepEqual(models.two_bone.retarget.bones, [
    { bone: "hips", joint: 0, rest: [0, 0, 0] },
    { bone: "arm_r", joint: 1, rest: [0, 0, 180] },
  ]);
  assert.match(
    rust(f, models, "x"),
    /retarget: Some\(Retarget \{ rig: "biped", root: "hips", bones: &\[RetargetBone \{ bone: "hips", joint: 0/,
  );
  for (const [edit, re] of [
    [(r) => (r.rig = "octopus"), /retarget: no characters rig octopus/],
    [
      (r) => (r.bones.tail = { joint: "tip" }),
      /retarget: no bone tail in rig biped/,
    ],
    [
      (r) => (r.bones.arm_r.joint = "elbow"),
      /retarget: bone arm_r: no joint elbow/,
    ],
    [(r) => (r.bones.arm_l = { joint: "tip" }), /joint tip mapped twice/],
    [
      (r) => (r.bones.arm_r.rest = [1, 2]),
      /bone arm_r: rest is \[x, y, z\] degrees/,
    ],
    [(r) => (r.root = "chest"), /root chest is not a mapped bone/],
  ]) {
    const g = fixture();
    edit(g.models.two_bone.retarget);
    assert.throws(() => validate(g, palette, fixtures), re);
  }
});

test("your own body in first person is data: its cut validates and lands in the table", () => {
  const f = fixture();
  const out = rust(f, validate(f, palette, fixtures), "../../../shared/models/fixtures");
  assert.match(out, /body: Some\(FirstPersonBody \{ hide: &\[\], weight: 0\.5 \}\)/);
  for (const [edit, re] of [
    [(b) => (b.hide = ["neck"]), /firstPerson: body: no joint neck/],
    [(b) => (b.hide = ["tip"]), /firstPerson: body: a joint is listed twice/],
    [(b) => (b.weight = 0), /firstPerson: body: weight is in \(0, 1\]/],
    [(b) => delete b.hide, /firstPerson: body: hide names the skin joints/],
  ]) {
    const g = fixture();
    edit(g.models.two_bone.firstPerson.body);
    assert.throws(() => validate(g, palette, fixtures), re);
  }
});

test("a role naming a clip the GLB lacks fails naming it", () => {
  const f = fixture();
  f.models.two_bone.roles.walk.clip = "cartwheel";
  assert.throws(
    () => validate(f, palette, fixtures),
    /model two_bone: role walk: no clip cartwheel/,
  );
});

test("a missing GLB fails naming it", () => {
  const f = fixture();
  f.models.two_bone.glb = "nope.glb";
  assert.throws(
    () => validate(f, palette, fixtures),
    /model two_bone: missing GLB nope\.glb/,
  );
});

test("unknown roles, bad fallbacks, scales and remaps fail", () => {
  const bad = (edit, re) => {
    const f = fixture();
    edit(f.models.two_bone, f);
    assert.throws(() => validate(f, palette, fixtures), re);
  };
  bad((m) => (m.roles.dance = { clip: "rest" }), /unknown role dance/);
  bad((m) => (m.fallback = "swim"), /fallback swim/);
  bad((m) => (m.scale = 0), /scale must be > 0/);
  bad(
    (m) => (m.remap = { "#123456": "nowhere.base" }),
    /no palette colour nowhere\.base/,
  );
  bad((m) => (m.remap = { red: "soil" }), /not #rrggbb/);
  bad((_, f) => (f.player = "ghost"), /player: no model ghost/);
  bad((m) => (m.roles.walk.speed = -1), /speed must be > 0/);
  bad((m) => (m.hand.joint = "paw"), /hand: no joint paw/);
  bad((m) => (m.hand.bone = ""), /hand: bone names/);
  bad((m) => (m.hand.rot = [0, 0]), /hand: rot is \[x, y, z\]/);
});

const props = () => JSON.parse(readFileSync(join(here, "props.json"), "utf8"));
const fixtureProps = () =>
  JSON.parse(readFileSync(join(fixtures, "props.json"), "utf8"));

test("every prop GLB loads: one static mesh, palette colours in COLOR_0, its length", () => {
  const out = validateProps(props(), here);
  assert.ok(Object.keys(out).length >= 53);
  for (const [name, p] of Object.entries(out)) {
    assert.ok(p.colours > 1, `${name}: ${p.colours} colour(s)`);
    assert.ok(Math.max(...p.size) <= 1.8 && Math.max(...p.size) >= 0.05, name);
    const s = propSummary(readFileSync(join(here, p.glb)));
    assert.equal(s.skins + s.clips, 0, name);
    // Centred on its origin (a grip is measured from there).
    for (const c of s.centre)
      assert.ok(Math.abs(c) < 0.01, `${name}: ${s.centre}`);
  }
});

test("a prop of a stacked FBX keeps only its named objects: the metal axe has no fire-axe red", () => {
  // The axes FBX stacks SingleHeadedAxe (the red fire axe), DoubleHeadedAxe and the MetalAxe
  // handle; the metal axe is the double-headed head on the handle (owner, 2026-11-10).
  const p = props().props.metal_axe;
  assert.deepEqual(p.objects, ["DoubleHeadedAxe", "MetalAxe"]);
  const red = (c) => {
    const [r, g, b] = c.split(",").map(Number);
    return r > 60 && r > 3 * g && r > 3 * b;
  };
  const s = propSummary(readFileSync(join(here, p.glb)));
  assert.deepEqual(s.palette.filter(red), [], `metal_axe: ${s.palette}`);
  // The other tools are whole files of one tool, or a head and its handle.
  for (const n of [
    "stone_axes",
    "stone_pickaxes",
    "metal_pickaxes",
    "wooden_shovel",
    "metal_shovel",
  ])
    assert.ok(props().props[n], n);
});

test("a prop's objects are a list of names, each once", () => {
  for (const objects of [[], "DoubleHeadedAxe", [""], ["A", "A"], [1]]) {
    const f = fixtureProps();
    f.props.plate.objects = objects;
    assert.throws(
      () => validateProps(f, fixtures),
      /prop plate: objects is a list of its FBX's mesh object names, each once/,
    );
  }
  const f = fixtureProps();
  f.props.plate.objects = ["Plate"];
  assert.equal(validateProps(f, fixtures).plate.objects[0], "Plate");
});

test("a prop is added by data alone: the fixture validates and lands in the table", () => {
  const out = validateProps(fixtureProps(), fixtures);
  assert.equal(out.plate.category, "dishes");
  const rs = rustProps(
    out,
    "../../../shared/models/fixtures",
    ["plate"],
    validateHand(fixtureProps().hand),
  );
  assert.match(
    rs,
    /PropData \{ name: "plate", category: "dishes", glb: "plate\.glb", length: 0\.25/,
  );
  assert.match(rs, /grip: \[0\.0, 0\.01, 0\.0\]/);
  // Its hold pose by data alone; a prop naming none is held as modelled.
  assert.match(
    rs,
    /axis: \[0\.0, 1\.0, 0\.0\], hold: PropHold \{ scale: 0\.5, roll: 90\.0 \}/,
  );
  const plain = fixtureProps();
  delete plain.props.plate.hold;
  assert.deepEqual(validateProps(plain, fixtures).plate.hold, {
    scale: 1,
    roll: 0,
  });
  // The hand that holds it, by data alone.
  assert.match(
    rs,
    /HAND_GRIP: HandGrip = HandGrip \{ bone: "hand_r", at: \[0\.0, -0\.08, 0\.0\], axis: \[0\.0, 0\.0, 1\.0\], edge: \[0\.0, -1\.0, 0\.0\] \}/,
  );
  assert.match(
    rs,
    /\("plate", include_bytes!\("\.\.\/\.\.\/\.\.\/shared\/models\/fixtures\/plate\.glb"\)\)/,
  );
});

test("bad props fail naming the entry and the problem", () => {
  const bad = (edit, re) => {
    const f = fixtureProps();
    edit(f.props.plate, f);
    assert.throws(() => validateProps(f, fixtures), re);
  };
  bad((p) => (p.glb = "nope.glb"), /prop plate: missing GLB nope\.glb/);
  bad((p) => (p.category = "food"), /prop plate: unknown category food/);
  bad((p) => (p.length = 2), /length 2 m is outside dishes's/);
  bad((p) => (p.length = 0.4), /is 0\.250 m long, not its length 0\.4 m/);
  bad((p) => (p.grip = [0, 1]), /grip is \[x, y, z\]/);
  bad((p) => (p.hold = [1]), /prop plate: hold is \{ scale, roll \}/);
  bad((p) => (p.hold = { size: 1 }), /prop plate: hold has unknown field size/);
  bad(
    (p) => (p.hold = { scale: 0 }),
    /prop plate: hold\.scale 0 is outside 0\.1\.\.2/,
  );
  bad(
    (p) => (p.hold = { rotation: [0, 90, 0] }),
    /prop plate: hold has unknown field rotation \(scale, roll\)/,
  );
  bad(
    (p) => (p.hold = { roll: 270 }),
    /prop plate: hold\.roll 270 is outside ±180 degrees/,
  );
  bad(
    (p) => (p.handle = [0, 0.01]),
    /prop plate: handle is \[from, to\] metres along its length/,
  );
  bad((p) => (p.handle = [0.4, 0.6]), /prop plate: handle: no vertex between/);
  assert.throws(
    () => validateHand({ ...fixtureProps().hand, edge: [0, 0, 1] }),
    /hand: edge is not perpendicular to axis/,
  );
  bad((p) => (p.glb = "two_bone.glb"), /a skin or clips|meshes, not one/);
  bad((_, f) => (f.categories.dishes.min = 0), /category dishes: needs/);
  // The game's item looks may only name a prop.
  assert.deepEqual(
    lookModels({
      items: [{ look: { model: "a" } }, { look: { shape: "rod" } }, {}],
    }),
    ["a"],
  );
});

test("palette references resolve; excluded ramps are left out of the snap", () => {
  assert.equal(resolveColor(palette, "soil"), palette.ramps.soil.base);
  assert.equal(resolveColor(palette, "soil.light"), palette.ramps.soil.light);
  assert.equal(resolveColor(palette, "ink"), palette.swatches.ink);
  const names = paletteStops(palette, ["chalk"]).map((s) => s.name);
  assert.ok(names.includes("soil.shadow"));
  assert.ok(!names.some((n) => n.startsWith("chalk.")));
});

// The coat (shared/models/coat.mjs, ADR 0070 §11): a model's colour as a few key colours by body
// region, blended softly, instead of a colour a face.
const hex = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
const srgb = (c) =>
  c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055;
const coatFixture = () => ({
  head: "+z",
  base: "sand",
  keep: [],
  patches: [{ colour: "bedrock.shadow", height: [0.5, 1], blend: 0.2 }],
});

/** A mesh's faces' mean sRGB colours (faces with a `skip`ped corner left out) and the pairs sharing an edge. */
function faceColours(bytes, level = 0, skip = () => false) {
  const g = glbJson(bytes);
  const p = g.meshes[level].primitives[0];
  const pos = glbAccessor(bytes, p.attributes.POSITION).values;
  const col = glbAccessor(bytes, p.attributes.COLOR_0);
  const idx = glbAccessor(bytes, p.indices).values;
  const key = (i) => [0, 1, 2].map((k) => pos[i * 3 + k].toFixed(4)).join(",");
  const faces = [];
  const edges = new Map();
  for (let f = 0; f < idx.length / 3; f++) {
    const vs = [idx[f * 3], idx[f * 3 + 1], idx[f * 3 + 2]];
    const cs = vs.map((v) =>
      [0, 1, 2].map((k) => srgb(col.values[v * col.width + k])),
    );
    if (cs.some(skip)) continue;
    faces.push(cs.reduce((a, c) => a.map((x, k) => x + c[k] / 3), [0, 0, 0]));
    for (let e = 0; e < 3; e++) {
      const ek = [key(vs[e]), key(vs[(e + 1) % 3])].sort().join("|");
      edges.set(ek, [...(edges.get(ek) ?? []), faces.length - 1]);
    }
  }
  return { faces, pairs: [...edges.values()].filter((fs) => fs.length === 2) };
}

/** The largest colour step (one channel, sRGB 0..1) between two faces sharing an edge. */
function maxNeighbourStep(bytes, level, skip) {
  const { faces, pairs } = faceColours(bytes, level, skip);
  let most = 0;
  for (const [a, b] of pairs)
    for (let k = 0; k < 3; k++)
      most = Math.max(most, Math.abs(faces[a][k] - faces[b][k]));
  return most;
}

// The owner's bar (2026-11-10): no "harsh color changes" between neighbouring faces. The per-face
// wolf (0.52.4) stepped by up to 0.62 from face to face; a blended coat's steps stay small.
const STEP = 0.2;

test("the wolf's coat is smooth on every detail level, with no red or blue left", () => {
  const m = data().models.wolf;
  const bytes = readFileSync(join(here, m.glb));
  const keep = (m.coat.keep ?? []).flatMap((r) =>
    Object.values(palette.ramps[r]).map(hex),
  );
  const kept = (c) =>
    keep.some((k) => k.every((v, i) => Math.abs(v - c[i]) < 2 / 255));
  for (let level = 0; level < glbJson(bytes).meshes.length; level++) {
    const step = maxNeighbourStep(bytes, level, kept);
    assert.ok(
      step < STEP,
      `level ${level}: neighbouring faces step by ${step.toFixed(3)}`,
    );
    for (const c of faceColours(bytes, level).faces) {
      assert.ok(!(c[0] > c[1] + 0.15 && c[0] > c[2] + 0.15), `a red face ${c}`);
      assert.ok(!(c[2] > c[0] + 0.15), `a blue face ${c}`);
    }
  }
});

test("a coat is added by data alone: the fixture's coat paints its GLB", () => {
  const coat = coatFixture();
  const raw = readFileSync(join(fixtures, "two_bone.glb"));
  const painted = applyCoat(raw, coat, palette);
  // Painting again changes nothing (the check gen.mjs runs over a coated model's GLB).
  assert.ok(Buffer.from(applyCoat(painted, coat, palette)).equals(painted));
  const g = glbJson(painted);
  const p = g.meshes[0].primitives[0];
  const pos = glbAccessor(painted, p.attributes.POSITION).values;
  const col = glbAccessor(painted, p.attributes.COLOR_0);
  const ys = pos.filter((_, i) => i % 3 === 1);
  const [lo, hi] = [Math.min(...ys), Math.max(...ys)];
  const at = (i) => [0, 1, 2].map((k) => srgb(col.values[i * col.width + k]));
  const near = (a, b) => a.every((v, k) => Math.abs(v - b[k]) < 2 / 255);
  for (let i = 0; i < ys.length; i++) {
    const h = (ys[i] - lo) / (hi - lo);
    if (h < 0.35)
      assert.ok(near(at(i), hex(palette.ramps.sand.base)), `low vertex ${i}`);
    if (h > 0.65)
      assert.ok(
        near(at(i), hex(palette.ramps.bedrock.shadow)),
        `high vertex ${i}`,
      );
  }
  // Through the data: the fixture model with the coat validates over its painted GLB, and fails
  // naming the fix over the unpainted one.
  const f = fixture();
  f.models.two_bone.coat = coat;
  const read = (path) =>
    path.endsWith("two_bone.glb") ? painted : readFileSync(path);
  validate(f, palette, fixtures, read);
  assert.throws(
    () => validate(f, palette, fixtures),
    /model two_bone: two_bone\.glb is not painted by its coat: run node shared\/models\/import\.mjs --coat two_bone/,
  );
  // A patch's blend, amount and covered middle (coatColour at the body point).
  const q = { along: 0.5, height: 0.9, side: 0 };
  assert.deepEqual(
    coatColour(coat, palette, q).map((v) => Math.round(v * 255)),
    hex(palette.ramps.bedrock.shadow).map((v) => Math.round(v * 255)),
  );
  coat.patches[0].amount = 0.5;
  const half = coatColour(coat, palette, q);
  const [a, b] = [
    hex(palette.ramps.sand.base),
    hex(palette.ramps.bedrock.shadow),
  ];
  assert.ok(half.every((v, k) => Math.abs(v - (a[k] + b[k]) / 2) < 1e-9));
});

test("bad coats fail naming the model and the problem", () => {
  const bad = (edit, re) => {
    const coat = coatFixture();
    edit(coat);
    assert.throws(() => validateCoat(coat, palette, "model x: coat"), re);
  };
  bad((c) => (c.head = "up"), /model x: coat: head is an axis/);
  bad((c) => (c.base = "nope"), /base: no palette colour nope/);
  bad((c) => (c.keep = ["nope"]), /keep: no ramp nope/);
  bad((c) => (c.fur = 1), /unknown key fur/);
  bad(
    (c) => (c.patches[0].colour = "wolf.dark"),
    /patch 0: no palette colour wolf\.dark/,
  );
  bad(
    (c) => (c.patches[0].height = [0.6, 0.2]),
    /patch 0: height is \[from, to\]/,
  );
  bad(
    (c) => (c.patches[0].height = [0, 1.5]),
    /patch 0: height is \[from, to\]/,
  );
  bad((c) => delete c.patches[0].height, /patch 0: no range/);
  bad((c) => (c.patches[0].blend = 0), /patch 0: blend must be in/);
  bad((c) => (c.patches[0].amount = 2), /patch 0: amount must be in/);
  bad((c) => (c.patches[0].width = 2), /patch 0: unknown key width/);
  bad((c) => {
    c.keep = ["hair_black"];
    c.patches[0].colour = "hair_black.light";
  }, /patch 0: hair_black\.light is a kept ramp's colour/);
  // Through the data: a model's bad coat fails validate, naming the model.
  const f = fixture();
  f.models.two_bone.coat = { ...coatFixture(), head: "y" };
  assert.throws(
    () => validate(f, palette, fixtures),
    /model two_bone: coat: head is an axis/,
  );
});

test("a handle's axis is measured from its GLB: the props' tools", () => {
  const props = validateProps(
    JSON.parse(readFileSync(join(here, "props.json"), "utf8")),
    here,
  );
  // The stone axe's handle runs 3.5 cm off its origin, along its length.
  const a = props.stone_axes;
  assert.ok(Math.abs(a.grip[0] + 0.035) < 0.003, `${a.grip}`);
  assert.ok(a.axis[1] > 0.999, `${a.axis}`);
  // A shovel's handle is named from its top, so its axis points down its length to the blade.
  assert.ok(props.metal_shovel.axis[1] < -0.999, `${props.metal_shovel.axis}`);
  assert.ok(Math.abs(props.metal_shovel.grip[2] + 0.056) < 0.003);
  assert.equal(typeof measureHandle, "function");
});

test("the sword is one sword out of the joined katanas prop, held by its handle", () => {
  // The katanas FBX is one blade and five colour variants of one handle, stacked (WORK_QUEUE
  // item 35): the sword is the blade on one handle, not the whole stack.
  const data = props();
  const p = data.props.sword;
  assert.deepEqual(p.objects, ["KatanaBlade.001", "KatanaHandle.001"]);
  const out = validateProps(data, here).sword;
  assert.equal(out.length, 1);
  assert.equal(propSummary(readFileSync(join(here, p.glb))).meshes, 1);
  // The grip is on the bare handle, toward the pommel end, on its measured axis.
  const [lo, hi] = [Math.min(...p.handle), Math.max(...p.handle)];
  assert.ok(out.grip[1] >= lo && out.grip[1] <= hi, `${out.grip}`);
  assert.ok(out.grip[1] < -0.25, "the handle is the sword's lower quarter");
  assert.ok(out.axis[1] > 0.99, `${out.axis}`);
  // A grip off the handle (on the blade) fails with a message.
  const bad = props();
  bad.props.sword.grip = [0, 0.2, 0];
  assert.throws(
    () => validateProps(bad, here),
    /prop sword: grip 0\.2 m is outside its handle -0\.47\.\.-0\.25 m/,
  );
});

test("idle poses are data: the fixture's lie by data alone, bad entries fail by name", () => {
  // WORK_QUEUE item 39: an animal's graze, sit or lie as joint turns on its idle clip.
  const f = fixture();
  const m = validate(f, palette, fixtures).two_bone;
  assert.deepEqual(m.poses, [
    { name: "lie", turns: [{ joint: 1, rot: [90, 0, 0] }], drop: 0 },
  ]);
  assert.match(
    rust(f, validate(f, palette, fixtures), "../../../shared/models/fixtures"),
    /poses: &\[Grip \{ name: "lie", turns: &\[\(1, \[90\.0, 0\.0, 0\.0\]\)\], drop: 0\.0 \}\], pose_blend_s: 1\.0/,
  );
  const bad = (edit, re) => {
    const g = fixture();
    edit(g.models.two_bone);
    assert.throws(() => validate(g, palette, fixtures), re);
  };
  bad(
    (t) => (t.poses = { dance: { tip: [1, 0, 0] } }),
    /poses: dance is not a pose/,
  );
  bad(
    (t) => (t.poses = { lie: { nose: [1, 0, 0] } }),
    /poses: lie: no joint nose/,
  );
  bad(
    (t) => (t.poses = { lie: { tip: [1, 0] } }),
    /poses: lie: tip is \[x, y, z\]/,
  );
  bad((t) => (t.poses = { lie: {} }), /poses: lie turns no joint/);
  bad((t) => (t.poseBlendS = 0), /poseBlendS 0 is outside/);
  // The shipped animals' poses validate against their skins.
  const shipped = validate(data(), palette, here);
  assert.ok(shipped.deer.poses.some((p) => p.name === "graze"));
  assert.ok(shipped.wolf.poses.some((p) => p.name === "lie"));
});

test("a pose's $drop lowers the body: the wolf sits and lies down on the ground, a bad drop fails", () => {
  const rs = rust(data(), validate(data(), palette, here));
  assert.match(rs, /name: "sit", turns: [^\n]*?drop: 0\.3 \}/);
  assert.match(rs, /name: "lie", turns: [^\n]*?drop: 0\.5 \}/);
  const d = data();
  d.models.wolf.poses.sit.$drop = -1;
  assert.throws(() => validate(d, palette, here), /\$drop -1 is outside/);
});

test("the Peasant's hat has a ramp of its own (repaint by part): the hat is wool_yellow, the hair hair_brown", () => {
  // Owner, 2026-11-12: "Hat gets its own colours"; BUGS.md #96: the repaint by height had caught
  // the hair's fringe (wool_yellow on the hair, so a villager's hat colour recoloured it).
  const m = data().models.peasant;
  const bytes = readFileSync(join(here, m.glb));
  const file = JSON.parse(readFileSync(join(here, m.parts), "utf8"));
  const base = Uint8Array.from(file.vertices.join(""), Number);
  const parts = { names: file.parts, levels: allLevelParts(bytes, base) };
  assert.ok(
    Buffer.from(applyRepaint(bytes, m.repaint, palette, parts)).equals(bytes),
    "applied (idempotent)",
  );
  const hat = file.parts.indexOf("hat") + 1;
  const hair = file.parts.indexOf("hair") + 1;
  parts.levels.forEach((ps, k) => {
    const mesh = readMesh(bytes, `detail_${k}`);
    let hatYellow = 0;
    for (let i = 0; i < mesh.count; i++) {
      const ramp = nearestRamp(palette, mesh.colour(i));
      if (ps[i] === hair)
        assert.notEqual(ramp, "wool_yellow", `detail_${k}: hair vertex ${i} is the hat's ramp`);
      if (ps[i] === hat && ramp === "wool_yellow") hatYellow++;
    }
    assert.ok(hatYellow > 20, `detail_${k}: the hat repainted: ${hatYellow}`);
  });
  assert.throws(
    () => validateRepaint([{ from: "hair_brown", to: "nope", part: "hat" }], palette),
    /no palette ramp nope/,
  );
  assert.throws(
    () => validateRepaint([{ from: "hair_brown", to: "leather", part: "cape" }], palette, "r", ["hat"]),
    /no part cape/,
  );
  assert.throws(
    () => validateRepaint([{ from: "hair_brown", to: "leather", minY: 1 }], palette),
    /unknown entry minY/,
  );
});

test("a part map is checked: its names, every vertex a part, as many as the full mesh", () => {
  const ok = { parts: ["skin", "hair"], vertices: ["0120", "21"] };
  assert.equal(validateParts(ok, "p", 6), "012021");
  assert.throws(() => validateParts(ok, "p", 7), /6 vertices, but the full mesh has 7/);
  assert.throws(() => validateParts({ ...ok, vertices: ["013"] }, "p"), /vertex 2 is "3", not 0..2/);
  assert.throws(() => validateParts({ ...ok, parts: ["skin", "skin"] }, "p"), /part skin twice/);
  assert.throws(() => validateParts({ ...ok, parts: ["Skin"] }, "p"), /not a lower_snake_case/);
  assert.throws(
    () => validateParts({ ...ok, parts: ["a", "b", "c", "d", "e", "f", "g"] }, "p"),
    /7 parts, at most 6/,
  );
  assert.throws(() => validateParts({ ...ok, seed: [{ part: "cape", y: [0, 1] }] }, "p"), /no part cape/);
  assert.throws(() => validateParts({ ...ok, seed: [{ part: "skin" }] }, "p"), /no test/);
  assert.throws(() => validateParts({ ...ok, wat: 1 }, "p"), /unknown key wat/);
});

test("a part map is added by data alone: the fixture's seeds to its checked-in vertices", () => {
  const f = fixture();
  const m = f.models.two_bone;
  const bytes = readFileSync(join(fixtures, m.glb));
  const file = JSON.parse(readFileSync(join(fixtures, m.parts), "utf8"));
  const mesh = readMesh(bytes, "detail_0");
  assert.equal(rows(seedParts(mesh, file, palette)).join(""), file.vertices.join(""));
  const models = validate(f, palette, fixtures);
  assert.deepEqual(models.two_bone.partMap.names, ["tip", "base"]);
  assert.match(generate()[1][1], /parts: Some\(Parts \{ names: &\["tip", "base"\], levels: &\["2122", /);
  // A part map of the wrong length fails naming the model.
  const read = (p) =>
    p.endsWith("two_bone.json")
      ? Buffer.from(JSON.stringify({ ...file, vertices: ["21"] }))
      : readFileSync(p);
  assert.throws(() => validate(f, palette, fixtures, read), /model two_bone: .*2 vertices, but the full mesh has 4/);
});

test("the Peasant's parts: the hat and the hair are their islands, no head vertex is clothes", () => {
  const m = data().models.peasant;
  const bytes = readFileSync(join(here, m.glb));
  const file = JSON.parse(readFileSync(join(here, m.parts), "utf8"));
  const mesh = readMesh(bytes, "detail_0");
  const ps = Uint8Array.from(file.vertices.join(""), Number);
  const isl = islands(mesh);
  const num = (n) => file.parts.indexOf(n) + 1;
  const head = ["spine.005", "spine.006"].map((j) => mesh.jointNames.indexOf(j));
  const onHead = (i) =>
    mesh.joints(i).reduce((a, j, k) => a + (head.includes(j) ? mesh.weights(i)[k] : 0), 0) >= 0.5;
  const islandOf = (part) => new Set([...ps.keys()].filter((i) => ps[i] === num(part)).map((i) => isl[i]));
  assert.equal(islandOf("hat").size, 1, "the hat is one island");
  assert.equal(islandOf("hair").size, 1, "the hair is one island");
  for (let i = 0; i < mesh.count; i++) {
    if (onHead(i)) assert.ok(![num("shirt"), num("trousers")].includes(ps[i]), `head vertex ${i}`);
    if (ps[i] === num("hat")) assert.ok(mesh.position(i)[1] >= 1.44, `hat vertex ${i} below the brim`);
  }
});

test("every NPC colour part is a part of its model", () => {
  const models = validate(data(), palette, here);
  const creatures = JSON.parse(readFileSync(join(root, "shared/creatures/creatures.json"), "utf8"));
  checkCreatureParts(creatures, models);
  const bad = structuredClone(creatures);
  bad.creatures.find((c) => c.npc).npc.colours[0].part = "cape";
  assert.throws(() => checkCreatureParts(bad, models), /colour part cape is not a part of model peasant/);
  const none = structuredClone(creatures);
  none.creatures.find((c) => c.npc).model = "wolf";
  assert.throws(() => checkCreatureParts(none, models), /need a part map on model wolf/);
});
