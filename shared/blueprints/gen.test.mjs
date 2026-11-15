// node --test shared/blueprints/gen.test.mjs: the blueprints' data (docs/BLUEPRINTS.md).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  generate,
  loadCreatureNames,
  loadStructureNames,
  rust,
  validateBlueprints,
} from "./gen.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const data = () =>
  JSON.parse(readFileSync(join(here, "blueprints.json"), "utf8"));
const structures = loadStructureNames();
const creatures = loadCreatureNames();

test("the data validates and the generated table is current", () => {
  validateBlueprints(data(), structures, creatures);
  for (const [path, text] of generate())
    assert.equal(
      readFileSync(path, "utf8"),
      text,
      `${path} is stale: run node shared/blueprints/gen.mjs`,
    );
});

test("the catalog holds the test blueprints, the storehouse last", () => {
  assert.deepEqual(data().catalog, [
    "dwell:bp_hut",
    "dwell:bp_wall",
    "dwell:bp_well",
    "dwell:bp_watchtower",
    "dwell:bp_storehouse",
  ]);
});

test("a fixture catalog entry, builder pace and town plan are added by data alone", () => {
  const d = JSON.parse(
    readFileSync(join(here, "fixtures/fixture.json"), "utf8"),
  );
  const v = validateBlueprints(d, structures, creatures);
  assert.deepEqual(v.catalog, ["test:ladder_post"]);
  assert.match(rust(v), /pace_s: 0\.5/);
  // Site clearing: the margin and its pace by data alone.
  assert.match(rust(v), /clear_margin_m: 2, clear_pace_s: 0\.25 \}/);
  assert.match(rust(v), /catalog: &\["test:ladder_post"\]/);
  assert.match(
    rust(v),
    /TownPlan \{ name: "test:camp", weight: 2, blueprints: &\[PlanBlueprint \{ structure: "test:ladder_post", at: \[-1, 1\], turn: 0, role: Role::Storehouse \}, PlanBlueprint \{ structure: "test:ladder_post", at: \[1, -1\], turn: 2, role: Role::Building \}\] \}/,
  );
  // B5: a job, a storehouse and a carried look by data alone.
  assert.match(
    rust(v),
    /jobs: &\[JobRules \{ name: "test:mason", kind: JobKind::Build, carry_max: 4, mixed_loads: false, wait_s: 1\.0 \}\]/,
  );
  assert.match(rust(v), /job: "test:mason"/);
  assert.match(
    rust(v),
    /storehouse: StorehouseRules \{ stock: StockMode::Unlimited, range_m: 32\.0, reach_m: 2\.0 \}/,
  );
  assert.match(
    rust(v),
    /kinds: &\[CarryKind \{ block: "dwell:stone", piece_m: 0\.15, per_layer: 1 \}\]/,
  );
});

test("bad entries fail naming them", () => {
  const bad = (edit, re) => {
    const d = data();
    edit(d);
    assert.throws(() => validateBlueprints(d, structures, creatures), re);
  };
  bad(
    (d) => (d.catalog = ["dwell:nope"]),
    /catalog\[0\]: unknown structure "dwell:nope"/,
  );
  bad((d) => d.catalog.push(d.catalog[0]), /catalog\[5\]: dwell:bp_hut twice/);
  // B5: jobs, the storehouse, the carried look, a plan's roles.
  bad(
    (d) => (d.jobs[0].kind = "haul"),
    /jobs\[0\].kind "haul" is a later phase/,
  );
  bad((d) => (d.jobs[0].kind = "dance"), /jobs\[0\].kind must be one of build/);
  bad(
    (d) => (d.jobs[0].carryMax = 0),
    /jobs\[0\].carryMax must be an integer in 1..255/,
  );
  bad(
    (d) => (d.jobs[0].mixedLoads = "yes"),
    /jobs\[0\].mixedLoads must be true or false/,
  );
  bad((d) => d.jobs.push({ ...d.jobs[0] }), /jobs\[1\]: dwell:builder twice/);
  bad(
    (d) => (d.builder.job = "dwell:nobody"),
    /builder.job: unknown job "dwell:nobody"/,
  );
  bad(
    (d) => (d.storehouse.stock = "counted"),
    /storehouse.stock must be one of unlimited/,
  );
  bad(
    (d) => (d.storehouse.rangeM = 1),
    /storehouse.rangeM must be a number in 4..1024/,
  );
  bad(
    (d) => (d.carryLook.item = "sack"),
    /carryLook.item: unknown item "sack"/,
  );
  bad(
    (d) => (d.carryLook.kinds[0].block = "dwell:unobtainium"),
    /carryLook.kinds\[0\].block: unknown block/,
  );
  bad(
    (d) => (d.carryLook.perLayer = 0),
    /carryLook.perLayer must be an integer in 1..8/,
  );
  bad(
    (d) => (d.towns.plans[0].blueprints[0].role = "barracks"),
    /blueprints\[0\].role must be one of building, storehouse/,
  );
  bad(
    (d) => (d.towns.plans[0].blueprints[2].role = "storehouse"),
    /blueprints\[2\]: the storehouse must be the plan's first blueprint/,
  );
  bad(
    (d) => (d.builder.creature = "dwell:ghost"),
    /builder.creature: unknown creature "dwell:ghost"/,
  );
  bad(
    (d) => (d.player.paceMs = 0),
    /player.paceMs must be a number in 16..5000/,
  );
  bad((d) => (d.limits.perWorld = 1.5), /limits.perWorld must be an integer/);
  bad((d) => (d.builder.speed = 1), /builder: unknown field "speed"/);
  bad(
    (d) => (d.builder.clearMarginM = 1.5),
    /builder.clearMarginM must be an integer in 0..8/,
  );
  bad((d) => (d.extra = 1), /unknown field "extra"/);
  bad(
    (d) => (d.towns.map.icon = "castle"),
    /towns.map.icon "castle" is not an icon/,
  );
  bad(
    (d) => (d.towns.map.buildingColor = "gold"),
    /towns.map.buildingColor must be #rrggbb/,
  );
  bad((d) => (d.towns.map.size = 3), /towns.map: unknown field "size"/);
  bad(
    (d) => (d.towns.plans[0].blueprints[0].structure = "dwell:nope"),
    /towns.plans\[0\].blueprints\[0\]: unknown structure/,
  );
  bad(
    (d) => (d.towns.plans[0].blueprints[1].turn = 4),
    /blueprints\[1\].turn must be 0..3/,
  );
  bad(
    (d) => (d.towns.plans[0].blueprints[0].at = [999, 0]),
    /within footprintM/,
  );
  bad((d) => (d.towns.clearM = -1), /towns.clearM must be an integer/);
  bad(
    (d) => (d.towns.spacingM = 10),
    /towns.spacingM must be an integer in 64/,
  );
  bad((d) => (d.towns.heightM = [5, 1]), /towns.heightM/);
  bad((d) => (d.towns.plans = []), /towns.plans must be a non-empty list/);
});
