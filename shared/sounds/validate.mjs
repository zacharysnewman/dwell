// Validation and resolution of the sound data (docs/AUDIO.md §2): shared by shared/sounds/gen.mjs,
// shared/blocks/gen.mjs (the blocks' `sound` references) and the client's Vitest suites. A bad
// entry throws an Error whose message names it.
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** Where a sound's recorded `samples` live (AUDIO.md §2b): beside the data, in `samples/`. */
export const SAMPLES_DIR = join(
  dirname(fileURLToPath(import.meta.url)),
  "samples",
);
/** A sample file: lower-case words and dashes, a format every browser decodes (AAC in MP4). */
const SAMPLE_FILE = /^[a-z0-9][a-z0-9-]*\.(m4a|mp3)$/;
/** Every sample shipped, together, may be at most this large (owner, 2026-11-10: ~5 MB). */
export const SAMPLES_MAX_BYTES = 5_000_000;
/** How a sound with samples plays: the recordings, or its procedural layers. */
export const PLAY_MODES = ["samples", "procedural"];

const NAME = /^[a-z0-9_]+$/;
const ID = /^[a-z0-9_]+:[a-z0-9_]+$/;

/** The vocabulary the synth understands (client/src/audio/synth.ts). */
export const SOURCES = ["noise", "osc", "modal"];
/** `crackle`: sparse decaying pops (a fire's crackle, a flame's spit). */
export const NOISE_COLORS = ["white", "pink", "brown", "crackle"];
export const WAVES = ["sine", "triangle", "square", "sawtooth"];
export const FILTER_TYPES = [
  "lowpass",
  "highpass",
  "bandpass",
  "peaking",
  "notch",
];
/** Events a block's sound set can name; the first five are required. */
export const SET_EVENTS = [
  "step",
  "land",
  "break",
  "place",
  "hit",
  "enter",
  "exit",
  "swim",
  "slide",
  "slide_stop",
  "burn",
  "explode",
  "impact",
  "brush",
];
/** An event's part in a continuous (looped) sound: start a voice, update it, stop it. */
export const LOOP_PARTS = ["start", "update", "stop"];
export const REQUIRED_SET_EVENTS = ["step", "land", "break", "place", "hit"];
/** Footstep gaits, in the order the Rust game numbers them. */
export const GAITS = ["walk", "run", "crouch", "climb", "swim"];
/** The menu vocabulary (AUDIO.md §6). */
export const UI_KINDS = [
  "hover",
  "click",
  "back",
  "tab",
  "toggle_on",
  "toggle_off",
  "slider",
  "error",
  "success",
];

const LAYER_KEYS = [
  "source",
  "color",
  "wave",
  "freq",
  "to",
  "partials",
  "filters",
  "env",
  "gain",
  "at",
  "pan",
  "chance",
  "grains",
  "tremolo",
  "alts",
];
const SOUND_KEYS = [
  "$comment",
  "gain",
  "voices",
  "minGapS",
  "reverb",
  "variation",
  "intensity",
  "loop",
  "layers",
  "samples",
  "play",
];
const INTENSITY_KEYS = ["gain", "cutoff", "time", "pitch"];
const TOP_KEYS = [
  "$comment",
  "namespace",
  "fallback",
  "gaits",
  "events",
  "sets",
  "actions",
  "ui",
  "controls",
  "ambient",
  "ambience",
  "creatures",
  "brush",
  "sounds",
];
/** A creature event's part (AUDIO.md §2d): which of the creature's sounds it plays. */
export const CREATURE_PARTS = ["step", "startle", "call"];
/** The surroundings an ambience bed's conditions read (crates/dwell-game ambience.rs `INPUTS`). */
export const AMBIENCE_INPUTS = [
  "altitude",
  "height",
  "open",
  "waterShare",
  "waterM",
  "flow",
  "trees",
  "day",
  "fireM",
];

function fail(where, message) {
  throw new Error(`shared/sounds: ${where}: ${message}`);
}

const isObject = (v) =>
  typeof v === "object" && v !== null && !Array.isArray(v);

function keysIn(where, value, allowed) {
  if (!isObject(value)) fail(where, "must be an object");
  for (const k of Object.keys(value)) {
    if (!allowed.includes(k)) fail(where, `unknown field "${k}"`);
  }
}

function number(
  where,
  v,
  min,
  max,
  { optional = false, integer = false } = {},
) {
  if (v === undefined && optional) return;
  if (typeof v !== "number" || !Number.isFinite(v))
    fail(where, "must be a number");
  if (integer && !Number.isInteger(v)) fail(where, "must be a whole number");
  if (v < min || v > max) fail(where, `${v} is outside [${min}, ${max}]`);
}

function pair(where, v, min, max, { ordered = false, integer = false } = {}) {
  if (!Array.isArray(v) || v.length !== 2)
    fail(where, "must be a [low, high] pair");
  number(`${where}[0]`, v[0], min, max, { integer });
  number(`${where}[1]`, v[1], min, max, { integer });
  if (ordered && v[0] > v[1]) fail(where, "low is above high");
}

function oneOf(where, v, values) {
  if (!values.includes(v))
    fail(where, `"${v}" is not one of ${values.join(", ")}`);
}

function validateEnv(where, env) {
  keysIn(where, env, ["attack", "hold", "decay"]);
  number(`${where}.attack`, env.attack, 0, 2);
  number(`${where}.hold`, env.hold, 0, 4, { optional: true });
  number(`${where}.decay`, env.decay, 0.001, 8);
}

function validateFilter(where, f) {
  keysIn(where, f, ["type", "freq", "q", "gain", "to"]);
  oneOf(`${where}.type`, f.type, FILTER_TYPES);
  number(`${where}.freq`, f.freq, 20, 20000);
  number(`${where}.to`, f.to, 20, 20000, { optional: true });
  number(`${where}.q`, f.q, 0.0001, 100, { optional: true });
  number(`${where}.gain`, f.gain, -40, 40, { optional: true });
  if (f.gain !== undefined && f.type !== "peaking")
    fail(where, "gain is for peaking filters");
}

function validateLayerBody(where, layer, loop) {
  keysIn(where, layer, LAYER_KEYS);
  oneOf(`${where}.source`, layer.source, SOURCES);
  if (loop) {
    // A loop sounds until it is stopped: sustained sources, levels moved by intensity.
    if (layer.source === "modal")
      fail(where, "a loop sound has noise and osc layers only");
    for (const k of ["env", "grains", "at", "chance", "to"])
      if (layer[k] !== undefined) fail(where, `${k} is not for loop sounds`);
    for (const f of layer.filters ?? [])
      if (f.to !== undefined)
        fail(where, "filter sweeps (to) are not for loop sounds");
  } else if (layer.tremolo !== undefined)
    fail(where, "tremolo is for loop sounds");
  if (layer.tremolo !== undefined) {
    keysIn(`${where}.tremolo`, layer.tremolo, ["rate", "depth"]);
    number(`${where}.tremolo.rate`, layer.tremolo.rate, 0.1, 60);
    number(`${where}.tremolo.depth`, layer.tremolo.depth, 0, 1);
  }
  if (layer.source === "noise") {
    oneOf(`${where}.color`, layer.color, NOISE_COLORS);
    if (!layer.env && !loop) fail(where, "a noise layer needs an env");
  } else if (layer.color !== undefined)
    fail(where, "color is for noise layers");
  if (layer.source === "osc") {
    oneOf(`${where}.wave`, layer.wave, WAVES);
    if (!layer.env && !loop) fail(where, "an osc layer needs an env");
  } else if (layer.wave !== undefined) fail(where, "wave is for osc layers");
  if (layer.source === "osc" || layer.source === "modal") {
    number(`${where}.freq`, layer.freq, 20, 20000);
    number(`${where}.to`, layer.to, 20, 20000, { optional: true });
  } else if (layer.freq !== undefined || layer.to !== undefined) {
    fail(where, "freq is for osc and modal layers");
  }
  if (layer.source === "modal") {
    if (
      !Array.isArray(layer.partials) ||
      layer.partials.length === 0 ||
      layer.partials.length > 16
    )
      fail(
        `${where}.partials`,
        "must list 1 to 16 [ratio, gain, decay] partials",
      );
    layer.partials.forEach((p, i) => {
      const w = `${where}.partials[${i}]`;
      if (!Array.isArray(p) || p.length !== 3)
        fail(w, "must be [ratio, gain, decay]");
      number(`${w} ratio`, p[0], 0.01, 64);
      number(`${w} gain`, p[1], 0, 1);
      number(`${w} decay`, p[2], 0.001, 8);
    });
  } else if (layer.partials !== undefined)
    fail(where, "partials are for modal layers");
  if (layer.env !== undefined) validateEnv(`${where}.env`, layer.env);
  if (layer.filters !== undefined) {
    if (!Array.isArray(layer.filters) || layer.filters.length > 6)
      fail(`${where}.filters`, "must be a list of at most 6 filters");
    layer.filters.forEach((f, i) =>
      validateFilter(`${where}.filters[${i}]`, f),
    );
  }
  number(`${where}.gain`, layer.gain, 0, 2);
  number(`${where}.at`, layer.at, 0, 4, { optional: true });
  number(`${where}.pan`, layer.pan, -1, 1, { optional: true });
  number(`${where}.chance`, layer.chance, 0, 1, { optional: true });
  if (layer.grains !== undefined) {
    const g = layer.grains;
    keysIn(`${where}.grains`, g, ["count", "spread", "gain", "pitch"]);
    pair(`${where}.grains.count`, g.count, 1, 64, {
      ordered: true,
      integer: true,
    });
    number(`${where}.grains.spread`, g.spread, 0, 4);
    pair(`${where}.grains.gain`, g.gain, 0, 2, { ordered: true });
    number(`${where}.grains.pitch`, g.pitch, 0, 24, { optional: true });
  }
}

function validateLayer(where, layer, loop) {
  validateLayerBody(where, layer, loop);
  if (layer.alts !== undefined) {
    if (
      !Array.isArray(layer.alts) ||
      layer.alts.length < 2 ||
      layer.alts.length > 8
    )
      fail(`${where}.alts`, "must list 2 to 8 alternatives");
    layer.alts.forEach((alt, i) => {
      const w = `${where}.alts[${i}]`;
      if (!isObject(alt)) fail(w, "must be an object");
      if ("alts" in alt || "source" in alt)
        fail(w, "an alternative cannot change source or alts");
      const { alts: _alts, ...base } = layer;
      validateLayerBody(w, { ...base, ...alt }, loop);
    });
  }
}

/** Checks one sound definition; `where` names it in messages. */
export function validateSound(where, s) {
  keysIn(where, s, SOUND_KEYS);
  number(`${where}.gain`, s.gain, 0, 4);
  number(`${where}.reverb`, s.reverb, 0, 1, { optional: true });
  // At most this many plays of the sound at once (more are dropped, never restarted), and no
  // second play within `minGapS` seconds of the last (BUGS.md #103).
  number(`${where}.voices`, s.voices, 1, 24, { optional: true });
  if (s.voices !== undefined && !Number.isInteger(s.voices))
    fail(`${where}.voices`, "must be a whole number");
  number(`${where}.minGapS`, s.minGapS, 0, 2, { optional: true });
  if (s.variation !== "none") {
    const v = s.variation;
    if (!isObject(v))
      fail(
        `${where}.variation`,
        'must be "none" or { pitch, gain, cutoff, time }',
      );
    keysIn(`${where}.variation`, v, ["pitch", "gain", "cutoff", "time"]);
    number(`${where}.variation.pitch`, v.pitch, 0, 12, { optional: true });
    number(`${where}.variation.gain`, v.gain, 0, 12, { optional: true });
    number(`${where}.variation.cutoff`, v.cutoff, 0, 0.9, { optional: true });
    number(`${where}.variation.time`, v.time, 0, 0.9, { optional: true });
  }
  if (s.loop !== undefined && s.loop !== true)
    fail(`${where}.loop`, "must be true or absent");
  const loop = s.loop === true;
  if (s.intensity !== undefined) {
    keysIn(`${where}.intensity`, s.intensity, INTENSITY_KEYS);
    for (const k of INTENSITY_KEYS) {
      if (s.intensity[k] !== undefined)
        pair(`${where}.intensity.${k}`, s.intensity[k], 0, 4);
    }
    if (s.intensity.pitch !== undefined && !loop)
      fail(`${where}.intensity.pitch`, "is for loop sounds");
  }
  if (!Array.isArray(s.layers) || s.layers.length === 0 || s.layers.length > 12)
    fail(`${where}.layers`, "must list 1 to 12 layers");
  s.layers.forEach((l, i) => validateLayer(`${where}.layers[${i}]`, l, loop));
  if (s.samples !== undefined) {
    const sm = s.samples;
    keysIn(`${where}.samples`, sm, ["files", "gain", "pitch", "reverb"]);
    if (loop) fail(`${where}.samples`, "loop sounds are procedural only");
    if (
      !Array.isArray(sm.files) ||
      sm.files.length === 0 ||
      sm.files.length > 16
    )
      fail(`${where}.samples.files`, "must list 1 to 16 files");
    for (const f of sm.files)
      if (typeof f !== "string" || !SAMPLE_FILE.test(f))
        fail(
          `${where}.samples.files`,
          `"${f}" is not a lower-case .m4a or .mp3 name`,
        );
    number(`${where}.samples.gain`, sm.gain, 0, 4, { optional: true });
    number(`${where}.samples.pitch`, sm.pitch, 0, 12, { optional: true });
    number(`${where}.samples.reverb`, sm.reverb, 0, 1, { optional: true });
  }
  if (s.play !== undefined) {
    oneOf(`${where}.play`, s.play, PLAY_MODES);
    if (s.samples === undefined)
      fail(`${where}.play`, "needs samples to choose between");
  }
}

/**
 * The ambience (AUDIO.md §2c): `volume` (the Ambience volume's default), `fadeS` (the beds' glide
 * time constant), `cap` (no bed louder), `probe` (`radiusM`, the `trees` blocks), and `beds`: each
 * `{ files, gain, when: [{ input, from, full, floor? }] }`, its level `gain` times every
 * condition's ramp (0 at `from`, 1 at `full`, either way round; never under `floor`).
 */
function validateAmbience(ns, a) {
  keysIn("ambience", a, ["$comment", "volume", "fadeS", "cap", "probe", "beds"]);
  number("ambience.volume", a.volume, 0, 1);
  number("ambience.fadeS", a.fadeS, 0.05, 30);
  number("ambience.cap", a.cap, 0, 1);
  keysIn("ambience.probe", a.probe, ["radiusM", "trees"]);
  number("ambience.probe.radiusM", a.probe.radiusM, 2, 64);
  if (!Array.isArray(a.probe.trees)) fail("ambience.probe.trees", "must list block ids");
  for (const t of a.probe.trees)
    if (typeof t !== "string" || !ID.test(t))
      fail("ambience.probe.trees", `"${t}" is not a block id like "dwell:leaves"`);
  if (!isObject(a.beds)) fail("ambience.beds", "must be an object");
  const beds = {};
  for (const [name, b] of Object.entries(a.beds)) {
    const where = `ambience bed ${ns}:${name}`;
    if (!NAME.test(name)) fail(where, "bad name");
    keysIn(where, b, ["files", "gain", "when"]);
    if (!Array.isArray(b.files) || b.files.length === 0 || b.files.length > 4)
      fail(`${where}.files`, "must list 1 to 4 loop files");
    for (const f of b.files)
      if (typeof f !== "string" || !SAMPLE_FILE.test(f))
        fail(`${where}.files`, `"${f}" is not a lower-case .m4a or .mp3 name`);
    number(`${where}.gain`, b.gain, 0, 1);
    if (b.gain > a.cap) fail(`${where}.gain`, `${b.gain} is over the cap ${a.cap}`);
    if (!Array.isArray(b.when)) fail(`${where}.when`, "must list conditions");
    b.when.forEach((c, i) => {
      const w = `${where}.when[${i}]`;
      keysIn(w, c, ["input", "from", "full", "floor"]);
      if (!AMBIENCE_INPUTS.includes(c.input))
        fail(`${w}.input`, `unknown input "${c.input}" (one of ${AMBIENCE_INPUTS.join(", ")})`);
      number(`${w}.from`, c.from, -1000, 1000);
      number(`${w}.full`, c.full, -1000, 1000);
      if (c.from === c.full) fail(w, "from and full must differ");
      number(`${w}.floor`, c.floor, 0, 1, { optional: true });
    });
    beds[`${ns}:${name}`] = b;
  }
  return { volume: a.volume, fadeS: a.fadeS, cap: a.cap, probe: a.probe, beds };
}

/** The file's content hash (8 hex digits): its published name changes with its bytes. */
function contentHash(bytes) {
  return createHash("sha256").update(bytes).digest("hex").slice(0, 8);
}

/**
 * Checks every sound's sample files against `dir` (each exists; all of them fit the budget) and
 * rewrites each to its published name (`name-<hash>.m4a`, served once per site under `media/`);
 * returns the files to ship, `{ file, source }`, and their total size.
 */
function resolveSamples(sounds, dir, beds = {}) {
  const ship = new Map();
  let bytes = 0;
  for (const [id, b] of Object.entries(beds)) {
    const files = b.files.map((f) => {
      const path = join(dir, f);
      if (!existsSync(path))
        fail(`ambience bed ${id}.files`, `no file "${f}" in shared/sounds/samples/`);
      const data = readFileSync(path);
      const published = f.replace(/[.](\w+)$/, (_, ext) => `-${contentHash(data)}.${ext}`);
      if (!ship.has(published)) {
        ship.set(published, f);
        bytes += data.length;
      }
      return published;
    });
    beds[id] = { ...b, files };
  }
  for (const [id, s] of Object.entries(sounds)) {
    if (!s.samples) continue;
    const files = s.samples.files.map((f) => {
      const path = join(dir, f);
      if (!existsSync(path))
        fail(`sound ${id}.samples`, `no file "${f}" in shared/sounds/samples/`);
      const data = readFileSync(path);
      const published = f.replace(/\.(\w+)$/, `-${contentHash(data)}.$1`);
      if (!ship.has(published)) {
        ship.set(published, f);
        bytes += data.length;
      }
      return published;
    });
    sounds[id] = { ...s, samples: { ...s.samples, files } };
  }
  if (bytes > SAMPLES_MAX_BYTES)
    fail(
      "samples",
      `${bytes} bytes is over the ${SAMPLES_MAX_BYTES}-byte budget`,
    );
  return {
    files: [...ship].map(([file, source]) => ({ file, source })),
    bytes,
  };
}

/**
 * The ambient sounds' tunables (AUDIO.md §2): `fire` — burning light sources (a placed torch, one
 * in a hand) crackle within `radiusM`, the nearest `max`; `falls` — waterfalls rush within
 * `radiusM`, the nearest `max`, at full level by their flow at `fullM3s` (docs/WATER_SURFACE_GRID.md §2e). (No wind loop: removed, owner
 * 2026-10-21; no night crickets: removed, owner 2026-10-28.)
 */
function validateAmbient(a) {
  keysIn("ambient", a, ["fire", "falls"]);
  keysIn("ambient.fire", a.fire, ["radiusM", "max"]);
  number("ambient.fire.radiusM", a.fire.radiusM, 1, 48);
  number("ambient.fire.max", a.fire.max, 0, 16, { integer: true });
  keysIn("ambient.falls", a.falls, ["radiusM", "max", "fullM3s"]);
  number("ambient.falls.radiusM", a.falls.radiusM, 1, 128);
  number("ambient.falls.max", a.falls.max, 0, 16, { integer: true });
  number("ambient.falls.fullM3s", a.falls.fullM3s, 0.01, 1000);
  return a;
}

/** `local` or `ns:local` → `ns:local`. */
function qualify(ns, name) {
  return name.includes(":") ? name : `${ns}:${name}`;
}

/**
 * Validates every namespace's file and merges them into the data the client plays
 * (`client/src/audio/sounds.gen.ts`): names fully qualified, events in id order.
 */
export function resolveSounds(files, samplesDir = SAMPLES_DIR) {
  const out = {
    fallback: "",
    gaits: {},
    events: [],
    sets: {},
    actions: {},
    ui: {},
    controls: {},
    ambient: undefined,
    ambience: undefined,
    creatures: undefined,
    brush: undefined,
    sounds: {},
  };
  const sorted = [...files].sort((a, b) =>
    a.namespace < b.namespace ? -1 : 1,
  );
  // Sounds first: everything else refers to them.
  for (const file of sorted) {
    keysIn(`namespace ${file.namespace}`, file, TOP_KEYS);
    if (typeof file.namespace !== "string" || !NAME.test(file.namespace))
      fail("namespace", `bad namespace ${file.namespace}`);
    const ns = file.namespace;
    if (!isObject(file.sounds)) fail(ns, "sounds must be an object");
    for (const [name, s] of Object.entries(file.sounds)) {
      if (!NAME.test(name)) fail(`sound ${name}`, "bad name");
      const id = `${ns}:${name}`;
      if (id in out.sounds) fail(`sound ${id}`, "defined twice");
      validateSound(`sound ${id}`, s);
      out.sounds[id] = s;
    }
    if (file.ambience !== undefined) {
      if (out.ambience) fail("ambience", "given twice");
      out.ambience = validateAmbience(ns, file.ambience);
    }
  }
  if (!out.ambience) fail("ambience", "missing (the ambience beds)");
  const samples = resolveSamples(out.sounds, samplesDir, out.ambience.beds);
  const sound = (where, ns, name) => {
    if (typeof name !== "string") fail(where, "must name a sound");
    const id = qualify(ns, name);
    if (!(id in out.sounds)) fail(where, `unknown sound "${name}"`);
    return id;
  };
  for (const file of sorted) {
    const ns = file.namespace;
    for (const [name, set] of Object.entries(file.sets ?? {})) {
      const id = `${ns}:${name}`;
      const where = `set ${id}`;
      if (!NAME.test(name)) fail(where, "bad name");
      if (id in out.sets) fail(where, "defined twice");
      keysIn(where, set, SET_EVENTS);
      for (const e of REQUIRED_SET_EVENTS)
        if (!(e in set)) fail(where, `missing the "${e}" sound`);
      out.sets[id] = Object.fromEntries(
        Object.entries(set).map(([e, n]) => [e, sound(`${where}.${e}`, ns, n)]),
      );
    }
    for (const [name, a] of Object.entries(file.actions ?? {})) {
      if (!NAME.test(name)) fail(`action ${name}`, "bad name");
      if (name in out.actions) fail(`action ${name}`, "defined twice");
      out.actions[name] = sound(`action ${name}`, ns, a);
    }
    for (const [name, def] of Object.entries(file.events ?? {})) {
      const where = `event ${name}`;
      if (!NAME.test(name)) fail(where, "bad name");
      if (out.events.some((e) => e.name === name)) fail(where, "defined twice");
      keysIn(where, def, ["set", "sound", "speed", "loop", "fallback", "creature"]);
      if (def.creature !== undefined) oneOf(`${where}.creature`, def.creature, CREATURE_PARTS);
      if (def.loop !== undefined) oneOf(`${where}.loop`, def.loop, LOOP_PARTS);
      const plays = def.set !== undefined || def.sound !== undefined || def.creature !== undefined;
      if (def.loop === "update") {
        if (plays) fail(where, "a loop update plays nothing of its own");
      } else if (!plays && def.loop !== "stop")
        fail(where, "plays no set event or sound");
      if (def.fallback !== undefined) {
        if (def.fallback !== false)
          fail(`${where}.fallback`, "must be false or absent");
        if (def.set === undefined)
          fail(`${where}.fallback`, "is for set events");
      }
      if (def.set !== undefined) oneOf(`${where}.set`, def.set, SET_EVENTS);
      if (def.speed !== undefined)
        pair(`${where}.speed`, def.speed, 0, 200, { ordered: true });
      const id =
        def.sound !== undefined
          ? sound(`${where}.sound`, ns, def.sound)
          : undefined;
      if (
        id !== undefined &&
        (out.sounds[id].loop === true) !== (def.loop === "start")
      )
        fail(
          `${where}.sound`,
          def.loop === "start"
            ? `${id} must be a loop sound`
            : `${id} is a loop sound`,
        );
      out.events.push({
        name,
        ...(def.set !== undefined ? { set: def.set } : {}),
        ...(id !== undefined ? { sound: id } : {}),
        ...(def.speed !== undefined ? { speed: def.speed } : {}),
        ...(def.loop !== undefined ? { loop: def.loop } : {}),
        ...(def.fallback === false ? { fallback: false } : {}),
        ...(def.creature !== undefined ? { creature: def.creature } : {}),
      });
    }
    if (file.gaits !== undefined) {
      keysIn(`${ns} gaits`, file.gaits, GAITS);
      for (const [g, def] of Object.entries(file.gaits)) {
        if (g in out.gaits) fail(`gait ${g}`, "defined twice");
        keysIn(`gait ${g}`, def, ["stride", "gain"]);
        number(`gait ${g}.stride`, def.stride, 0.1, 20);
        number(`gait ${g}.gain`, def.gain, 0, 2);
        out.gaits[g] = def;
      }
    }
    if (file.ui !== undefined) {
      keysIn(`${ns} ui`, file.ui, UI_KINDS);
      for (const [k, n] of Object.entries(file.ui)) {
        if (k in out.ui) fail(`ui ${k}`, "defined twice");
        const id = sound(`ui ${k}`, ns, n);
        if (out.sounds[id].variation !== "none")
          fail(
            `ui ${k}`,
            `UI sounds do not vary: ${id} must have "variation": "none"`,
          );
        out.ui[k] = id;
      }
    }
    if (file.fallback !== undefined) {
      if (out.fallback) fail("fallback", "given twice");
      out.fallback = qualify(ns, file.fallback);
    }
    for (const [label, refs] of Object.entries(file.controls ?? {})) {
      out.controls[label] = refs;
    }
    if (file.ambient !== undefined) {
      if (out.ambient) fail("ambient", "given twice");
      out.ambient = validateAmbient(file.ambient);
    }
  }
  if (!out.ambient) fail("ambient", "missing (the fire and night tunables)");
  // Creatures (AUDIO.md §2d): each model's stride and sounds.
  for (const file of sorted) {
    const c = file.creatures;
    if (c === undefined) continue;
    if (out.creatures) fail("creatures", "given twice");
    keysIn("creatures", c, ["$comment", "radiusM", "models"]);
    number("creatures.radiusM", c.radiusM, 1, 128);
    if (!isObject(c.models)) fail("creatures.models", "must be an object");
    const models = {};
    for (const [m, d] of Object.entries(c.models)) {
      const where = `creature ${m}`;
      if (!NAME.test(m)) fail(where, "bad model name");
      keysIn(where, d, ["stride", ...CREATURE_PARTS, "callEveryS"]);
      number(`${where}.stride`, d.stride, 0.1, 20);
      const r = { stride: d.stride };
      for (const part of CREATURE_PARTS)
        if (d[part] !== undefined) r[part] = sound(`${where}.${part}`, file.namespace, d[part]);
      if (d.callEveryS !== undefined) {
        pair(`${where}.callEveryS`, d.callEveryS, 1, 3600, { ordered: true });
        if (r.call === undefined) fail(`${where}.callEveryS`, "needs a call");
        r.callEveryS = d.callEveryS;
      }
      models[m] = r;
    }
    out.creatures = { radiusM: c.radiusM, models };
  }
  if (!out.creatures) out.creatures = { radiusM: 32, models: {} };
  // Brushing through foliage (AUDIO.md §5 `brush`, BUGS.md #103).
  for (const file of sorted) {
    const b = file.brush;
    if (b === undefined) continue;
    if (out.brush) fail("brush", "given twice");
    keysIn("brush", b, ["$comment", "everyM", "minS"]);
    number("brush.everyM", b.everyM, 0.1, 20);
    number("brush.minS", b.minS, 0, 5);
    out.brush = { everyM: b.everyM, minS: b.minS };
  }
  if (!out.brush) fail("brush", "missing (the foliage rustle's rate)");
  if (!out.fallback || !(out.fallback in out.sets))
    fail("fallback", `unknown set "${out.fallback}"`);
  // A loop's start plays loop sounds, every other event one-shots; the fallback set has every
  // loop event's set sound, so a set without one still sounds (a loop without a sound would
  // leave its voice silent, and its stop with nothing to play).
  for (const e of out.events) {
    if (e.set === undefined) continue;
    if (
      e.loop !== undefined &&
      e.fallback !== false &&
      !(e.set in out.sets[out.fallback])
    )
      fail(
        `event ${e.name}`,
        `the fallback set ${out.fallback} has no "${e.set}" sound`,
      );
    for (const [set, events] of Object.entries(out.sets)) {
      const id = events[e.set];
      if (id === undefined) continue;
      if ((out.sounds[id].loop === true) !== (e.loop === "start"))
        fail(
          `set ${set}.${e.set}`,
          e.loop === "start"
            ? `${id} must be a loop sound (event ${e.name} starts a loop)`
            : `${id} is a loop sound (event ${e.name} plays once)`,
        );
    }
  }
  for (const g of GAITS)
    if (!(g in out.gaits)) fail("gaits", `missing the "${g}" gait`);
  for (const k of UI_KINDS)
    if (!(k in out.ui)) fail("ui", `missing the "${k}" sound`);
  if (out.events.length > 255) fail("events", "too many");
  // Controls: each action's sounds must exist (or say why it is silent).
  for (const [label, refs] of Object.entries(out.controls)) {
    const where = `control "${label}"`;
    if (isObject(refs)) {
      keysIn(where, refs, ["silent"]);
      if (typeof refs.silent !== "string" || refs.silent.trim() === "")
        fail(where, "a silent control says why");
      continue;
    }
    if (!Array.isArray(refs) || refs.length === 0)
      fail(where, 'must list its sounds or be { "silent": "why" }');
    for (const r of refs) {
      const [kind, name] = String(r).split(":");
      const known =
        (kind === "event" && out.events.some((e) => e.name === name)) ||
        (kind === "action" && name in out.actions) ||
        (kind === "ui" && name in out.ui);
      if (!known) fail(where, `unknown sound reference "${r}"`);
    }
  }
  out.sampleFiles = samples.files;
  out.sampleBytes = samples.bytes;
  return out;
}

/** Checks blocks' `sound` references (`{ id, sound? }`) against resolved sound data. */
export function checkBlockSounds(blocks, resolved) {
  for (const b of blocks) {
    if (b.sound === undefined) continue;
    if (typeof b.sound !== "string" || !ID.test(b.sound))
      throw new Error(
        `shared/blocks: ${b.id}: sound must be a set id like "dwell:stone"`,
      );
    if (!(b.sound in resolved.sets))
      throw new Error(
        `shared/blocks: ${b.id}: unknown sound set "${b.sound}" (shared/sounds/*.json sets: ${Object.keys(resolved.sets).join(", ")})`,
      );
  }
}
