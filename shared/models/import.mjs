#!/usr/bin/env node
// Imports the skinned models of shared/models/models.json from their packs' FBX into the checked-in
// GLBs (docs/CHARACTERS.md §11, ADR 0070), with Blender headless (shared/models/blender_import.py):
// one mesh, every face one colour of the shared palette (shared/textures/palette.json), no texture,
// the clips named by their last word.
//
//   node shared/models/import.mjs --from <dir of unpacked Unity packages> [--blender <path>] [name ...]
//   node shared/models/import.mjs --props --from <dir> [name ...]   (the static props, props.json:
//   one Blender run, shared/models/blender_props.py)
//   node shared/models/import.mjs --coat [name ...]   (paints the models' `coat`s over the
//   checked-in GLBs again, no packs or Blender needed: shared/models/coat.mjs)
//
// The packs are not in the repo (Unity Asset Store EULA): each package is unpacked as GUID folders
// holding `pathname` and `asset`, and a model's `source` names its pack and the paths inside it.
// The GLBs are the source of truth after import; run `node shared/models/gen.mjs` after.
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { applyCoat, applyRepaint } from "./coat.mjs";
import { addPieces, addedVertices, applyDecor, decorBase } from "./decor.mjs";
import { paletteStops, resolveColor } from "./gen.mjs";
import {
  allLevelParts,
  readMesh,
  rows,
  seedParts,
  validateParts,
} from "./parts.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(name);
  if (i < 0) return fallback;
  const [, v] = args.splice(i, 2);
  return v;
};
// --colours prints each source colour seen and the palette colour it became (to write a `remap`).
const verbose = args.includes("--colours");
if (verbose) args.splice(args.indexOf("--colours"), 1);
const props = args.includes("--props");
if (props) args.splice(args.indexOf("--props"), 1);
const coatOnly = args.includes("--coat");
if (coatOnly) args.splice(args.indexOf("--coat"), 1);
const from = opt("--from");
const blender = opt("--blender", "/opt/homebrew/bin/blender");
if (!from && !coatOnly) {
  console.error(
    "usage: node shared/models/import.mjs --from <unpacked packs> [--blender <path>] [name ...]",
  );
  process.exit(2);
}

/** The asset file in pack `pack` whose Unity pathname is `path`. */
function assetIn(pack, path) {
  const dir = join(from, pack);
  for (const guid of readdirSync(dir)) {
    const p = join(dir, guid, "pathname");
    if (
      existsSync(p) &&
      readFileSync(p, "utf8").split("\n")[0].trim() === path
    ) {
      return join(dir, guid, "asset");
    }
  }
  throw new Error(`${pack}: no asset at ${path}`);
}

const hexRgb = (hex) =>
  [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
const data = JSON.parse(readFileSync(join(here, "models.json"), "utf8"));
const palette = JSON.parse(
  readFileSync(join(root, "shared/textures/palette.json"), "utf8"),
);
// Each stop and its ramp's base: a colour takes the base of the ramp it is nearest (the painted
// shading in the packs' textures is the renderer's to do).
const stops = paletteStops(palette, data.palette?.exclude ?? []).map((s) => [
  hexRgb(s.hex),
  hexRgb(resolveColor(palette, s.name.split(".")[0])),
]);
/** Paints model `name`'s coat (if it has one) over its GLB (shared/models/coat.mjs). */
function coat(name) {
  const m = data.models[name];
  const path = join(here, m.glb);
  // Its decor's pieces first (geometry: shared/models/decor.mjs), added after the mesh's own vertices;
  // the part map gains a row of no part for each (it lists the full mesh's vertices).
  if (m?.decor) {
    const bytes = addPieces(readFileSync(path), m.decor, palette);
    writeFileSync(path, bytes);
    console.log("PIECES", path, addedVertices(bytes, "detail_0"), "vertices on the full mesh");
    if (m.parts) {
      const partsPath = join(here, m.parts);
      const file = JSON.parse(readFileSync(partsPath, "utf8"));
      const count = readMesh(bytes, "detail_0").count;
      const own = count - addedVertices(bytes, "detail_0");
      const vs = file.vertices.join("");
      if (vs.length !== count && vs.length >= own) {
        const v = Uint8Array.from(vs.slice(0, own) + "0".repeat(count - own), Number);
        writeFileSync(partsPath, `${JSON.stringify({ ...file, vertices: rows(v) }, null, 2)}\n`);
        console.log("PARTS", partsPath, `${count - own} piece vertices, no part`);
      }
    }
  }
  // Its `repaint` next (a part given a ramp of its own; shared/models/coat.mjs), by its part map
  // (shared/models/parts.mjs), seeded again if the mesh no longer matches it (a re-import).
  if (m?.repaint) {
    const bytes = readFileSync(path);
    const partsPath = join(here, m.parts);
    const file = JSON.parse(readFileSync(partsPath, "utf8"));
    const base = readMesh(bytes, "detail_0");
    let vertices;
    try {
      vertices = Uint8Array.from(validateParts(file, m.parts, base.count), Number);
    } catch {
      vertices = seedParts(base, file, palette, data.palette?.exclude ?? []);
      writeFileSync(partsPath, `${JSON.stringify({ ...file, vertices: rows(vertices) }, null, 2)}\n`);
      console.log("SEEDED", partsPath, "(check it: node shared/models/parts.mjs --render)");
    }
    const parts = { names: file.parts, levels: allLevelParts(bytes, vertices) };
    writeFileSync(path, applyRepaint(bytes, m.repaint, palette, parts));
    console.log("REPAINT", path);
  }
  if (m?.coat) {
    writeFileSync(path, applyCoat(readFileSync(path), m.coat, palette));
    console.log("COAT", path);
  }
  // Its decor's paint last (bands and marks over the repainted colours), a band's part by the part map.
  if (m?.decor) {
    const bytes = readFileSync(path);
    const file = m.parts && JSON.parse(readFileSync(join(here, m.parts), "utf8"));
    writeFileSync(path, applyDecor(bytes, m.decor, palette, file && decorBase(bytes, file)));
    console.log("DECOR", path);
  }
}

if (coatOnly) {
  for (const name of args.length ? args : Object.keys(data.models)) coat(name);
  process.exit(0);
}
const tmp = mkdtempSync(join(tmpdir(), "dwell-models-"));
if (props) {
  try {
    const pd = JSON.parse(readFileSync(join(here, "props.json"), "utf8"));
    const src = pd.source;
    const want = args.length ? args : Object.keys(pd.props);
    const job = {
      texture: assetIn(src.pack, src.root + src.texture),
      palette: stops,
      props: want.map((name) => {
        const p = pd.props[name];
        if (!p) throw new Error(`no prop ${name}`);
        const fbx = join(tmp, `${name}.fbx`);
        execFileSync("ln", ["-sf", assetIn(src.pack, src.root + p.fbx), fbx]);
        mkdirSync(dirname(join(here, p.glb)), { recursive: true });
        return {
          fbx,
          out: join(here, p.glb),
          length: p.length ?? pd.categories[p.category].length,
          objects: p.objects ?? null,
        };
      }),
    };
    const jobPath = join(tmp, "props.json");
    writeFileSync(jobPath, JSON.stringify(job));
    const out = execFileSync(
      blender,
      [
        "-b",
        "--factory-startup",
        "-P",
        join(here, "blender_props.py"),
        "--",
        jobPath,
      ],
      { encoding: "utf8", maxBuffer: 1 << 26 },
    );
    for (const l of out.split("\n")) if (l.startsWith("PROP")) console.log(l);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
  process.exit(0);
}
const names = args.length ? args : Object.keys(data.models);
try {
  for (const name of names) {
    const m = data.models[name];
    if (!m) throw new Error(`no model ${name}`);
    const job = {
      fbx: join(tmp, `${name}.fbx`),
      texture: assetIn(m.source.pack, m.source.texture),
      out: join(here, m.glb),
      palette: stops,
      remap: Object.entries(m.remap ?? {}).map(([src, to]) => [
        hexRgb(src),
        hexRgb(resolveColor(palette, to)),
      ]),
      frame_step: 2,
      levels: data.detail.levels.map((l) => l.ratio),
      remove: m.source.remove
        ? { joint: m.source.remove.joint, above: m.source.remove.above }
        : null,
    };
    // Blender's FBX importer wants the extension.
    execFileSync("ln", ["-sf", assetIn(m.source.pack, m.source.fbx), job.fbx]);
    const jobPath = join(tmp, `${name}.json`);
    writeFileSync(jobPath, JSON.stringify(job));
    const out = execFileSync(
      blender,
      [
        "-b",
        "--factory-startup",
        "-P",
        join(here, "blender_import.py"),
        "--",
        jobPath,
      ],
      {
        encoding: "utf8",
        maxBuffer: 1 << 26,
      },
    );
    console.log(
      out
        .split("\n")
        .filter(
          (l) =>
            l.startsWith("IMPORTED") || (verbose && l.startsWith("COLOUR")),
        )
        .join("\n") || out.slice(-2000),
    );
    coat(name);
  }
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
