#!/usr/bin/env node
// Tree species (docs/TREE_GROWTH.md §11, ADR 0079): validates shared/trees/*.json (and the test
// fixtures in shared/trees/fixtures) and emits crates/dwell-worldgen/src/species.gen.rs, the table
// the tree builder (dwell-worldgen `species.rs`) grows trees from. CI fails on stale output.
// `node shared/trees/gen.mjs --check` exits 1 if the generated file is stale.

import { readFileSync, readdirSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const OUT = join(root, "crates/dwell-worldgen/src/species.gen.rs");
const NAME = /^[a-z0-9_]+:[a-z0-9_]+$/;
const isNum = (v) => typeof v === "number" && Number.isFinite(v);

/** The build's blocks by id (`ns:name`), from shared/blocks/*.json. */
export function registryBlocks() {
  const dir = join(root, "shared/blocks");
  const out = new Set();
  for (const f of readdirSync(dir).filter((f) => f.endsWith(".json"))) {
    const ns = JSON.parse(readFileSync(join(dir, f), "utf8"));
    for (const b of ns.blocks ?? []) out.add(`${ns.namespace}:${b.name}`);
  }
  return out;
}

const SHAPES = {
  lumpy: ["shape", "centre", "radius", "lumps", "roughness", "scale"],
  cone: ["shape", "base", "top", "radius", "taper", "tier", "roughness", "scale"],
  fronds: ["shape", "fronds", "length", "lift", "droop", "scale"],
};
const BRANCHES = {
  none: ["kind"],
  heavy: ["kind", "count", "from", "reach", "rise", "emergeAt"],
  whorl: ["kind", "perWhorl", "reach", "emergeAt"],
};

/** Checks one species against `blocks` (a set of block ids); throws naming the species and field. */
export function validateSpecies(s, blocks, file = "(species)") {
  const at = `${file}: ${s?.name ?? "(unnamed)"}`;
  const fail = (m) => {
    throw new Error(`${at}: ${m}`);
  };
  const only = (o, keys, where) => {
    if (typeof o !== "object" || o === null || Array.isArray(o)) fail(`${where} must be an object`);
    const extra = Object.keys(o).filter((k) => k !== "$comment" && !keys.includes(k));
    if (extra.length) fail(`${where}: unknown ${extra.join(", ")}`);
    for (const k of keys) if (o[k] === undefined) fail(`${where}.${k} is missing`);
  };
  const num = (v, lo, hi, where) => {
    if (!(isNum(v) && v >= lo && v <= hi)) fail(`${where} must be a number in [${lo}, ${hi}]`);
    return v;
  };
  const range = (v, lo, hi, where) => {
    if (!(Array.isArray(v) && v.length === 2)) fail(`${where} must be [min, max]`);
    num(v[0], lo, hi, `${where}[0]`);
    num(v[1], lo, hi, `${where}[1]`);
    if (v[0] > v[1]) fail(`${where}: min above max`);
    return v;
  };
  only(s, ["name", "label", "blocks", "trunk", "branches", "crown", "variety", "growth", ...(s?.sapling === undefined ? [] : ["sapling"])], "species");
  if (s.sapling !== undefined) {
    // The item a player plants (ADR 0079 S4): a block of its own name, the ground it takes on, and
    // the chance a broken leaf of one of its trees drops it.
    const p = s.sapling;
    only(p, ["item", "plantOn", "dropChance"], "sapling");
    if (!blocks.has(p.item)) fail(`sapling.item: ${p.item} is not a block (a sapling is placed as its block)`);
    if (!(Array.isArray(p.plantOn) && p.plantOn.length > 0)) fail("sapling.plantOn must list the blocks it takes on");
    for (const g of p.plantOn) if (!blocks.has(g)) fail(`sapling.plantOn: ${g} is not a block`);
    num(p.dropChance, 0, 1, "sapling.dropChance");
  }
  if (!(typeof s.name === "string" && NAME.test(s.name))) fail("name must be ns:name");
  if (typeof s.label !== "string" || !s.label) fail("label must be a string");
  only(s.blocks, ["log", "leaf"], "blocks");
  for (const k of ["log", "leaf"]) if (!blocks.has(s.blocks[k])) fail(`blocks.${k}: ${s.blocks[k]} is not a block`);
  const t = s.trunk;
  only(t, t.lean === undefined ? ["height", "heightStep", "width"] : ["height", "heightStep", "width", "lean"], "trunk");
  only(t.height, ["sapling", "mature"], "trunk.height");
  num(t.height.sapling, 0.5, 40, "trunk.height.sapling");
  range(t.height.mature, 0.5, 40, "trunk.height.mature");
  if (t.height.mature[0] < t.height.sapling) fail("trunk.height.mature below the sapling's");
  num(t.heightStep, 0.5, 4, "trunk.heightStep");
  if (Math.abs(t.heightStep * 2 - Math.round(t.heightStep * 2)) > 1e-9) fail("trunk.heightStep must be whole ½ m cells");
  only(t.width, ["sapling", "mature", "step"], "trunk.width");
  num(t.width.sapling, 0.25, 1, "trunk.width.sapling");
  num(t.width.mature, t.width.sapling, 1, "trunk.width.mature");
  num(t.width.step, 0.25, 1, "trunk.width.step");
  for (const k of ["sapling", "mature", "step"])
    if (Math.abs(t.width[k] * 4 - Math.round(t.width[k] * 4)) > 1e-9) fail(`trunk.width.${k} must be whole ¼ m (the log's widths)`);
  if (t.lean !== undefined) range(t.lean, 0, 8, "trunk.lean");
  const b = s.branches;
  if (!BRANCHES[b?.kind]) fail(`branches.kind must be one of ${Object.keys(BRANCHES).join(", ")}`);
  only(b, BRANCHES[b.kind], "branches");
  if (b.kind === "heavy") {
    range(b.count, 0, 12, "branches.count");
    range(b.from, 0, 1, "branches.from");
    range(b.reach, 0, 8, "branches.reach");
    range(b.rise, -4, 8, "branches.rise");
  }
  if (b.kind === "whorl") {
    num(b.perWhorl, 1, 4, "branches.perWhorl");
    num(b.reach, 0, 1, "branches.reach");
  }
  if (b.kind !== "none") num(b.emergeAt, 0, 1, "branches.emergeAt");
  const c = s.crown;
  if (!SHAPES[c?.shape]) fail(`crown.shape must be one of ${Object.keys(SHAPES).join(", ")}`);
  only(c, SHAPES[c.shape], "crown");
  range(c.scale, 0.05, 2, "crown.scale");
  if (c.shape === "lumpy") {
    num(c.centre, -4, 8, "crown.centre");
    if (!(Array.isArray(c.radius) && c.radius.length === 3)) fail("crown.radius must be [x, y, z]");
    c.radius.forEach((r, i) => num(r, 0.25, 8, `crown.radius[${i}]`));
    only(c.lumps, ["radius", "height"], "crown.lumps");
    range(c.lumps.radius, 0.1, 6, "crown.lumps.radius");
    range(c.lumps.height, 0.1, 3, "crown.lumps.height");
    num(c.roughness, 0, 1, "crown.roughness");
    if (b.kind === "whorl") fail("a lumpy crown takes heavy branches or none");
  }
  if (c.shape === "cone") {
    num(c.base, 0, 1, "crown.base");
    num(c.top, 0, 6, "crown.top");
    num(c.radius, 0.25, 8, "crown.radius");
    num(c.taper, 0.1, 4, "crown.taper");
    only(c.tier, ["period", "upper"], "crown.tier");
    num(c.tier.period, 1, 8, "crown.tier.period");
    num(c.tier.upper, 0, 1, "crown.tier.upper");
    num(c.roughness, 0, 1, "crown.roughness");
    if (b.kind === "heavy") fail("a cone crown takes whorl branches or none");
  }
  if (c.shape === "fronds") {
    range(c.fronds, 1, 24, "crown.fronds");
    range(c.length, 0.5, 8, "crown.length");
    num(c.lift, 0, 4, "crown.lift");
    num(c.droop, 0, 4, "crown.droop");
    if (b.kind !== "none") fail("a fronds crown takes no branches");
  }
  only(s.variety, ["crown", "lightness", "hue"], "variety");
  range(s.variety.crown, 0.25, 2, "variety.crown");
  range(s.variety.lightness, 0.25, 2, "variety.lightness");
  range(s.variety.hue, -0.5, 0.5, "variety.hue");
  const g = s.growth;
  only(g, ["stages", "daysPerStage", "wild", "variants"], "growth");
  if (!(Number.isInteger(g.stages) && g.stages >= 2 && g.stages <= 16)) fail("growth.stages must be a whole number in [2, 16]");
  num(g.daysPerStage, 0.01, 1000, "growth.daysPerStage");
  if (!(Array.isArray(g.wild) && g.wild.length === g.stages)) fail("growth.wild must weigh each stage");
  g.wild.forEach((w, i) => {
    if (!(Number.isInteger(w) && w >= 0 && w <= 1000)) fail(`growth.wild[${i}] must be a whole number in [0, 1000]`);
  });
  if (!g.wild.some((w) => w > 0)) fail("growth.wild must weigh some stage above 0");
  if (!(Number.isInteger(g.variants) && g.variants >= 1 && g.variants <= 32)) fail("growth.variants must be a whole number in [1, 32]");
  return s;
}

/** Loads and validates every species in `dir`; names must be unique. */
export function loadSpecies(dir, blocks = registryBlocks()) {
  const out = [];
  for (const f of readdirSync(dir).filter((f) => f.endsWith(".json")).sort()) {
    const s = validateSpecies(JSON.parse(readFileSync(join(dir, f), "utf8")), blocks, f);
    if (out.some((o) => o.name === s.name)) throw new Error(`${f}: duplicate species ${s.name}`);
    if (s.sapling && out.some((o) => o.sapling?.item === s.sapling.item))
      throw new Error(`${f}: ${s.name}: sapling.item ${s.sapling.item} is another species' sapling`);
    out.push(s);
  }
  return out;
}

const f = (v) => (Number.isInteger(v) ? `${v}.0` : `${v}`);
const pair = (v) => `(${f(v[0])}, ${f(v[1])})`;

function speciesRust(s) {
  const b = s.branches;
  const branches =
    b.kind === "heavy"
      ? `Branches::Heavy { count: ${pair(b.count)}, from: ${pair(b.from)}, reach: ${pair(b.reach)}, rise: ${pair(b.rise)}, emerge_at: ${f(b.emergeAt)} }`
      : b.kind === "whorl"
        ? `Branches::Whorl { per_whorl: ${b.perWhorl}, reach: ${f(b.reach)}, emerge_at: ${f(b.emergeAt)} }`
        : "Branches::None";
  const c = s.crown;
  const crown =
    c.shape === "lumpy"
      ? `Crown::Lumpy { centre: ${f(c.centre)}, radius: [${c.radius.map(f).join(", ")}], lump_radius: ${pair(c.lumps.radius)}, lump_height: ${pair(c.lumps.height)}, roughness: ${f(c.roughness)} }`
      : c.shape === "cone"
        ? `Crown::Cone { base: ${f(c.base)}, top: ${f(c.top)}, radius: ${f(c.radius)}, taper: ${f(c.taper)}, tier_period: ${f(c.tier.period)}, tier_upper: ${f(c.tier.upper)}, roughness: ${f(c.roughness)} }`
        : `Crown::Fronds { fronds: ${pair(c.fronds)}, length: ${pair(c.length)}, lift: ${f(c.lift)}, droop: ${f(c.droop)} }`;
  return [
    "    Species {",
    `        name: ${JSON.stringify(s.name)},`,
    `        log: ${JSON.stringify(s.blocks.log)},`,
    `        leaf: ${JSON.stringify(s.blocks.leaf)},`,
    `        sapling_height: ${f(s.trunk.height.sapling)},`,
    `        mature_height: ${pair(s.trunk.height.mature)},`,
    `        height_step: ${f(s.trunk.heightStep)},`,
    `        width: (${f(s.trunk.width.sapling)}, ${f(s.trunk.width.mature)}, ${f(s.trunk.width.step)}),`,
    `        lean: ${pair(s.trunk.lean ?? [0, 0])},`,
    `        branches: ${branches},`,
    `        crown: ${crown},`,
    `        crown_scale: ${pair(c.scale)},`,
    `        crown_variety: ${pair(s.variety.crown)},`,
    `        stages: ${s.growth.stages},`,
    `        days_per_stage: ${f(s.growth.daysPerStage)},`,
    `        wild: &[${s.growth.wild.join(", ")}],`,
    `        variants: ${s.growth.variants},`,
    s.sapling
      ? `        sapling: Some(Sapling { item: ${JSON.stringify(s.sapling.item)}, plant_on: &[${s.sapling.plantOn.map((n) => JSON.stringify(n)).join(", ")}], drop_chance: ${f(s.sapling.dropChance)} }),`
      : "        sapling: None,",
    "    },",
  ].join("\n");
}

/** The Rust table for the species and the fixtures. */
export function rust(species, fixtures) {
  return [
    "// Generated by shared/trees/gen.mjs from shared/trees/*.json. Do not edit.",
    "",
    "use crate::species::{Branches, Crown, Sapling, Species};",
    "",
    "/// The tree species (shared/trees), by file name.",
    `pub static SPECIES: [Species; ${species.length}] = [`,
    ...species.map(speciesRust),
    "];",
    "",
    "/// The test fixtures (shared/trees/fixtures): species added by data alone.",
    "#[cfg(test)]",
    `pub static FIXTURES: [Species; ${fixtures.length}] = [`,
    ...fixtures.map(speciesRust),
    "];",
    "",
  ].join("\n");
}

function main() {
  const blocks = registryBlocks();
  const species = loadSpecies(here, blocks);
  const fixtures = loadSpecies(join(here, "fixtures"), blocks);
  const out = rust(species, fixtures);
  if (process.argv.includes("--check")) {
    if (!existsSync(OUT) || readFileSync(OUT, "utf8") !== out) {
      console.error("species.gen.rs is stale: run node shared/trees/gen.mjs");
      process.exit(1);
    }
    return;
  }
  writeFileSync(OUT, out);
  console.log(`species: ${species.map((s) => s.name).join(", ")} (+${fixtures.length} fixture)`);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) main();
