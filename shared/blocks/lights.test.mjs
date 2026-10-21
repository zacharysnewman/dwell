// node --test shared/blocks/lights.test.mjs — light sources as block data (docs/LIGHTS.md §2).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  blockHeld,
  blockLight,
  checkLight,
  lightsRust,
  readLights,
} from "./lights.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const read = (p) => JSON.parse(readFileSync(join(here, p), "utf8"));
const rf = (v) => (Number.isInteger(v) ? `${v}.0` : `${v}`);
const torch = () => structuredClone(read("dwell.json").lights["dwell:torch"]);

test("the build has the torch light, and its torch block references it and the torch item", () => {
  const dwell = read("dwell.json");
  const lights = readLights([dwell]);
  const block = dwell.blocks.find((b) => b.name === "torch");
  assert.equal(
    lights[blockLight("dwell:torch", block, lights)].name,
    "dwell:torch",
  );
  assert.equal(blockHeld("dwell:torch", block, ["block", "torch"]), "torch");
  // Its flame, the upper cell, gives off the block light (a 1 m torch over two cells).
  const flame = dwell.blocks.find((b) => b.name === "torch_upper");
  assert.ok(
    flame.emits > 0,
    "a placed torch gives off block light from its flame",
  );
  assert.equal(
    lights[blockLight("dwell:torch_upper", flame, lights)].name,
    "dwell:torch",
  );
});

test("a light added by data alone reaches the Rust table (the fixture lantern)", () => {
  const lights = readLights([read("dwell.json"), read("fixtures/test.json")]);
  const rs = lightsRust(lights, rf).join("\n");
  assert.match(
    rs,
    /LightInfo \{ name: "test:lantern", color: 0xa0c8ff, strength: 1\.5, radius_m: 6\.0, flicker: 0\.0, flicker_speed: 1\.0, shadows: false \}/,
  );
  assert.match(rs, /pub static LIGHTS: \[LightInfo; 2\]/);
});

test("bad entries fail with their field named", () => {
  const cases = [
    [(l) => (l.radiusM = 0), /radiusM 0 is outside \(0, 32\]/],
    [(l) => (l.strength = 40), /strength 40 is outside/],
    [(l) => (l.color = "orange"), /color must be #rrggbb/],
    [(l) => (l.flicker.amount = 1), /flicker\.amount 1 is outside \[0, 1\)/],
    [(l) => (l.flicker.wobble = 1), /unknown field flicker\.wobble/],
    [(l) => delete l.shadows, /missing shadows/],
    [(l) => (l.shadows = "yes"), /shadows must be true or false/],
    [(l) => (l.glow = 1), /unknown field glow/],
  ];
  for (const [edit, re] of cases) {
    const l = torch();
    edit(l);
    assert.throws(() => checkLight("dwell:torch", l, "dwell"), re);
  }
  assert.throws(
    () => checkLight("test:torch", torch(), "dwell"),
    /must be named dwell:/,
  );
  assert.throws(
    () =>
      blockLight(
        "dwell:x",
        { light: "dwell:nope" },
        readLights([read("dwell.json")]),
      ),
    /unknown light dwell:nope/,
  );
  assert.throws(
    () => blockHeld("dwell:x", { held: "sword" }, ["block", "torch"]),
    /held must name an item/,
  );
});

test("the torch's light and the block light are warm: blue well under red (owner, 2026-10-21: warmer)", () => {
  const rgb = (hex) =>
    [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const look = JSON.parse(
    readFileSync(join(here, "../look/light.json"), "utf8"),
  );
  for (const [name, hex] of [
    ["the held torch", torch().color],
    ["block light", look.block.color],
  ]) {
    const [r, g, b] = rgb(hex);
    // Was #ffa45c (blue/red 0.36) and #ffb878 (0.47).
    assert.ok(b / r <= 0.33, `${name} ${hex}: blue/red ${(b / r).toFixed(2)}`);
    assert.ok(g / r <= 0.62, `${name} ${hex}: green/red ${(g / r).toFixed(2)}`);
  }
});
