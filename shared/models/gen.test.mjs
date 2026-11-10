// node --test shared/models/gen.test.mjs: the skinned models' data (docs/CHARACTERS.md §11).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  generate,
  glbSummary,
  lookModels,
  propSummary,
  rustProps,
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
  const out = rust(f, validate(f, palette, fixtures), "../../../shared/models/fixtures");
  assert.match(out, /DetailLevel \{ id: "coarse", label: "Coarse", ratio: 0\.5 \}/);
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
  bad((d) => d.levels.push({ id: "tiny", label: "Tiny", ratio: 0.1 }), /not one a detail level/);
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

test("a prop is added by data alone: the fixture validates and lands in the table", () => {
  const out = validateProps(fixtureProps(), fixtures);
  assert.equal(out.plate.category, "dishes");
  const rs = rustProps(out, "../../../shared/models/fixtures", ["plate"]);
  assert.match(
    rs,
    /PropData \{ name: "plate", category: "dishes", glb: "plate\.glb", length: 0\.25/,
  );
  assert.match(rs, /grip: \[0\.0, 0\.01, 0\.0\]/);
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
