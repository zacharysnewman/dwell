#!/usr/bin/env node
// Imports the skinned models of shared/models/models.json from their packs' FBX into the checked-in
// GLBs (docs/CHARACTERS.md §11, ADR 0070), with Blender headless (shared/models/blender_import.py):
// one mesh, every face one colour of the shared palette (shared/textures/palette.json), no texture,
// the clips named by their last word.
//
//   node shared/models/import.mjs --from <dir of unpacked Unity packages> [--blender <path>] [name ...]
//
// The packs are not in the repo (Unity Asset Store EULA): each package is unpacked as GUID folders
// holding `pathname` and `asset`, and a model's `source` names its pack and the paths inside it.
// The GLBs are the source of truth after import; run `node shared/models/gen.mjs` after.
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { paletteStops, resolveColor } from "./gen.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(name);
  if (i < 0) return fallback;
  const [, v] = args.splice(i, 2);
  return v;
};
const from = opt("--from");
const blender = opt("--blender", "/opt/homebrew/bin/blender");
if (!from) {
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
const names = args.length ? args : Object.keys(data.models);
const tmp = mkdtempSync(join(tmpdir(), "dwell-models-"));
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
      out.split("\n").find((l) => l.startsWith("IMPORTED")) ?? out.slice(-2000),
    );
  }
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
