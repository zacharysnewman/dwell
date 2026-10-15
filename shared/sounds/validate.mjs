// Validation and resolution of the sound data (docs/AUDIO.md §2): shared by shared/sounds/gen.mjs,
// shared/blocks/gen.mjs (the blocks' `sound` references) and the client's Vitest suites. A bad
// entry throws an Error whose message names it.

const NAME = /^[a-z0-9_]+$/;
const ID = /^[a-z0-9_]+:[a-z0-9_]+$/;

/** The vocabulary the synth understands (client/src/audio/synth.ts). */
export const SOURCES = ['noise', 'osc', 'modal'];
export const NOISE_COLORS = ['white', 'pink', 'brown'];
export const WAVES = ['sine', 'triangle', 'square', 'sawtooth'];
export const FILTER_TYPES = ['lowpass', 'highpass', 'bandpass', 'peaking', 'notch'];
/** Events a block's sound set can name; the first five are required. */
export const SET_EVENTS = ['step', 'land', 'break', 'place', 'hit', 'enter', 'exit', 'swim'];
export const REQUIRED_SET_EVENTS = ['step', 'land', 'break', 'place', 'hit'];
/** Footstep gaits, in the order the Rust game numbers them. */
export const GAITS = ['walk', 'run', 'crouch', 'climb', 'swim'];
/** The menu vocabulary (AUDIO.md §6). */
export const UI_KINDS = [
  'hover',
  'click',
  'back',
  'tab',
  'toggle_on',
  'toggle_off',
  'slider',
  'error',
  'success',
];

const LAYER_KEYS = [
  'source',
  'color',
  'wave',
  'freq',
  'to',
  'partials',
  'filters',
  'env',
  'gain',
  'at',
  'pan',
  'chance',
  'grains',
  'alts',
];
const SOUND_KEYS = ['gain', 'reverb', 'variation', 'intensity', 'layers'];
const TOP_KEYS = [
  '$comment',
  'namespace',
  'fallback',
  'gaits',
  'events',
  'sets',
  'actions',
  'ui',
  'controls',
  'sounds',
];

function fail(where, message) {
  throw new Error(`shared/sounds: ${where}: ${message}`);
}

const isObject = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);

function keysIn(where, value, allowed) {
  if (!isObject(value)) fail(where, 'must be an object');
  for (const k of Object.keys(value)) {
    if (!allowed.includes(k)) fail(where, `unknown field "${k}"`);
  }
}

function number(where, v, min, max, { optional = false, integer = false } = {}) {
  if (v === undefined && optional) return;
  if (typeof v !== 'number' || !Number.isFinite(v)) fail(where, 'must be a number');
  if (integer && !Number.isInteger(v)) fail(where, 'must be a whole number');
  if (v < min || v > max) fail(where, `${v} is outside [${min}, ${max}]`);
}

function pair(where, v, min, max, { ordered = false, integer = false } = {}) {
  if (!Array.isArray(v) || v.length !== 2) fail(where, 'must be a [low, high] pair');
  number(`${where}[0]`, v[0], min, max, { integer });
  number(`${where}[1]`, v[1], min, max, { integer });
  if (ordered && v[0] > v[1]) fail(where, 'low is above high');
}

function oneOf(where, v, values) {
  if (!values.includes(v)) fail(where, `"${v}" is not one of ${values.join(', ')}`);
}

function validateEnv(where, env) {
  keysIn(where, env, ['attack', 'hold', 'decay']);
  number(`${where}.attack`, env.attack, 0, 2);
  number(`${where}.hold`, env.hold, 0, 4, { optional: true });
  number(`${where}.decay`, env.decay, 0.001, 8);
}

function validateFilter(where, f) {
  keysIn(where, f, ['type', 'freq', 'q', 'gain', 'to']);
  oneOf(`${where}.type`, f.type, FILTER_TYPES);
  number(`${where}.freq`, f.freq, 20, 20000);
  number(`${where}.to`, f.to, 20, 20000, { optional: true });
  number(`${where}.q`, f.q, 0.0001, 100, { optional: true });
  number(`${where}.gain`, f.gain, -40, 40, { optional: true });
  if (f.gain !== undefined && f.type !== 'peaking') fail(where, 'gain is for peaking filters');
}

function validateLayerBody(where, layer) {
  keysIn(where, layer, LAYER_KEYS);
  oneOf(`${where}.source`, layer.source, SOURCES);
  if (layer.source === 'noise') {
    oneOf(`${where}.color`, layer.color, NOISE_COLORS);
    if (!layer.env) fail(where, 'a noise layer needs an env');
  } else if (layer.color !== undefined) fail(where, 'color is for noise layers');
  if (layer.source === 'osc') {
    oneOf(`${where}.wave`, layer.wave, WAVES);
    if (!layer.env) fail(where, 'an osc layer needs an env');
  } else if (layer.wave !== undefined) fail(where, 'wave is for osc layers');
  if (layer.source === 'osc' || layer.source === 'modal') {
    number(`${where}.freq`, layer.freq, 20, 20000);
    number(`${where}.to`, layer.to, 20, 20000, { optional: true });
  } else if (layer.freq !== undefined || layer.to !== undefined) {
    fail(where, 'freq is for osc and modal layers');
  }
  if (layer.source === 'modal') {
    if (!Array.isArray(layer.partials) || layer.partials.length === 0 || layer.partials.length > 16)
      fail(`${where}.partials`, 'must list 1 to 16 [ratio, gain, decay] partials');
    layer.partials.forEach((p, i) => {
      const w = `${where}.partials[${i}]`;
      if (!Array.isArray(p) || p.length !== 3) fail(w, 'must be [ratio, gain, decay]');
      number(`${w} ratio`, p[0], 0.01, 64);
      number(`${w} gain`, p[1], 0, 1);
      number(`${w} decay`, p[2], 0.001, 8);
    });
  } else if (layer.partials !== undefined) fail(where, 'partials are for modal layers');
  if (layer.env !== undefined) validateEnv(`${where}.env`, layer.env);
  if (layer.filters !== undefined) {
    if (!Array.isArray(layer.filters) || layer.filters.length > 6)
      fail(`${where}.filters`, 'must be a list of at most 6 filters');
    layer.filters.forEach((f, i) => validateFilter(`${where}.filters[${i}]`, f));
  }
  number(`${where}.gain`, layer.gain, 0, 2);
  number(`${where}.at`, layer.at, 0, 4, { optional: true });
  number(`${where}.pan`, layer.pan, -1, 1, { optional: true });
  number(`${where}.chance`, layer.chance, 0, 1, { optional: true });
  if (layer.grains !== undefined) {
    const g = layer.grains;
    keysIn(`${where}.grains`, g, ['count', 'spread', 'gain', 'pitch']);
    pair(`${where}.grains.count`, g.count, 1, 64, { ordered: true, integer: true });
    number(`${where}.grains.spread`, g.spread, 0, 4);
    pair(`${where}.grains.gain`, g.gain, 0, 2, { ordered: true });
    number(`${where}.grains.pitch`, g.pitch, 0, 24, { optional: true });
  }
}

function validateLayer(where, layer) {
  validateLayerBody(where, layer);
  if (layer.alts !== undefined) {
    if (!Array.isArray(layer.alts) || layer.alts.length < 2 || layer.alts.length > 8)
      fail(`${where}.alts`, 'must list 2 to 8 alternatives');
    layer.alts.forEach((alt, i) => {
      const w = `${where}.alts[${i}]`;
      if (!isObject(alt)) fail(w, 'must be an object');
      if ('alts' in alt || 'source' in alt) fail(w, 'an alternative cannot change source or alts');
      const { alts: _alts, ...base } = layer;
      validateLayerBody(w, { ...base, ...alt });
    });
  }
}

/** Checks one sound definition; `where` names it in messages. */
export function validateSound(where, s) {
  keysIn(where, s, SOUND_KEYS);
  number(`${where}.gain`, s.gain, 0, 4);
  number(`${where}.reverb`, s.reverb, 0, 1, { optional: true });
  if (s.variation !== 'none') {
    const v = s.variation;
    if (!isObject(v)) fail(`${where}.variation`, 'must be "none" or { pitch, gain, cutoff, time }');
    keysIn(`${where}.variation`, v, ['pitch', 'gain', 'cutoff', 'time']);
    number(`${where}.variation.pitch`, v.pitch, 0, 12, { optional: true });
    number(`${where}.variation.gain`, v.gain, 0, 12, { optional: true });
    number(`${where}.variation.cutoff`, v.cutoff, 0, 0.9, { optional: true });
    number(`${where}.variation.time`, v.time, 0, 0.9, { optional: true });
  }
  if (s.intensity !== undefined) {
    keysIn(`${where}.intensity`, s.intensity, ['gain', 'cutoff', 'time']);
    for (const k of ['gain', 'cutoff', 'time']) {
      if (s.intensity[k] !== undefined) pair(`${where}.intensity.${k}`, s.intensity[k], 0, 4);
    }
  }
  if (!Array.isArray(s.layers) || s.layers.length === 0 || s.layers.length > 12)
    fail(`${where}.layers`, 'must list 1 to 12 layers');
  s.layers.forEach((l, i) => validateLayer(`${where}.layers[${i}]`, l));
}

/** `local` or `ns:local` → `ns:local`. */
function qualify(ns, name) {
  return name.includes(':') ? name : `${ns}:${name}`;
}

/**
 * Validates every namespace's file and merges them into the data the client plays
 * (`client/src/audio/sounds.gen.ts`): names fully qualified, events in id order.
 */
export function resolveSounds(files) {
  const out = {
    fallback: '',
    gaits: {},
    events: [],
    sets: {},
    actions: {},
    ui: {},
    controls: {},
    sounds: {},
  };
  const sorted = [...files].sort((a, b) => (a.namespace < b.namespace ? -1 : 1));
  // Sounds first: everything else refers to them.
  for (const file of sorted) {
    keysIn(`namespace ${file.namespace}`, file, TOP_KEYS);
    if (typeof file.namespace !== 'string' || !NAME.test(file.namespace))
      fail('namespace', `bad namespace ${file.namespace}`);
    const ns = file.namespace;
    if (!isObject(file.sounds)) fail(ns, 'sounds must be an object');
    for (const [name, s] of Object.entries(file.sounds)) {
      if (!NAME.test(name)) fail(`sound ${name}`, 'bad name');
      const id = `${ns}:${name}`;
      if (id in out.sounds) fail(`sound ${id}`, 'defined twice');
      validateSound(`sound ${id}`, s);
      out.sounds[id] = s;
    }
  }
  const sound = (where, ns, name) => {
    if (typeof name !== 'string') fail(where, 'must name a sound');
    const id = qualify(ns, name);
    if (!(id in out.sounds)) fail(where, `unknown sound "${name}"`);
    return id;
  };
  for (const file of sorted) {
    const ns = file.namespace;
    for (const [name, set] of Object.entries(file.sets ?? {})) {
      const id = `${ns}:${name}`;
      const where = `set ${id}`;
      if (!NAME.test(name)) fail(where, 'bad name');
      if (id in out.sets) fail(where, 'defined twice');
      keysIn(where, set, SET_EVENTS);
      for (const e of REQUIRED_SET_EVENTS) if (!(e in set)) fail(where, `missing the "${e}" sound`);
      out.sets[id] = Object.fromEntries(
        Object.entries(set).map(([e, n]) => [e, sound(`${where}.${e}`, ns, n)]),
      );
    }
    for (const [name, a] of Object.entries(file.actions ?? {})) {
      if (!NAME.test(name)) fail(`action ${name}`, 'bad name');
      if (name in out.actions) fail(`action ${name}`, 'defined twice');
      out.actions[name] = sound(`action ${name}`, ns, a);
    }
    for (const [name, def] of Object.entries(file.events ?? {})) {
      const where = `event ${name}`;
      if (!NAME.test(name)) fail(where, 'bad name');
      if (out.events.some((e) => e.name === name)) fail(where, 'defined twice');
      keysIn(where, def, ['set', 'sound', 'speed']);
      if (def.set === undefined && def.sound === undefined) fail(where, 'plays no set event or sound');
      if (def.set !== undefined) oneOf(`${where}.set`, def.set, SET_EVENTS);
      if (def.speed !== undefined) pair(`${where}.speed`, def.speed, 0, 200, { ordered: true });
      out.events.push({
        name,
        ...(def.set !== undefined ? { set: def.set } : {}),
        ...(def.sound !== undefined ? { sound: sound(`${where}.sound`, ns, def.sound) } : {}),
        ...(def.speed !== undefined ? { speed: def.speed } : {}),
      });
    }
    if (file.gaits !== undefined) {
      keysIn(`${ns} gaits`, file.gaits, GAITS);
      for (const [g, def] of Object.entries(file.gaits)) {
        if (g in out.gaits) fail(`gait ${g}`, 'defined twice');
        keysIn(`gait ${g}`, def, ['stride', 'gain']);
        number(`gait ${g}.stride`, def.stride, 0.1, 20);
        number(`gait ${g}.gain`, def.gain, 0, 2);
        out.gaits[g] = def;
      }
    }
    if (file.ui !== undefined) {
      keysIn(`${ns} ui`, file.ui, UI_KINDS);
      for (const [k, n] of Object.entries(file.ui)) {
        if (k in out.ui) fail(`ui ${k}`, 'defined twice');
        const id = sound(`ui ${k}`, ns, n);
        if (out.sounds[id].variation !== 'none')
          fail(`ui ${k}`, `UI sounds do not vary: ${id} must have "variation": "none"`);
        out.ui[k] = id;
      }
    }
    if (file.fallback !== undefined) {
      if (out.fallback) fail('fallback', 'given twice');
      out.fallback = qualify(ns, file.fallback);
    }
    for (const [label, refs] of Object.entries(file.controls ?? {})) {
      out.controls[label] = refs;
    }
  }
  if (!out.fallback || !(out.fallback in out.sets)) fail('fallback', `unknown set "${out.fallback}"`);
  for (const g of GAITS) if (!(g in out.gaits)) fail('gaits', `missing the "${g}" gait`);
  for (const k of UI_KINDS) if (!(k in out.ui)) fail('ui', `missing the "${k}" sound`);
  if (out.events.length > 255) fail('events', 'too many');
  // Controls: each action's sounds must exist (or say why it is silent).
  for (const [label, refs] of Object.entries(out.controls)) {
    const where = `control "${label}"`;
    if (isObject(refs)) {
      keysIn(where, refs, ['silent']);
      if (typeof refs.silent !== 'string' || refs.silent.trim() === '')
        fail(where, 'a silent control says why');
      continue;
    }
    if (!Array.isArray(refs) || refs.length === 0)
      fail(where, 'must list its sounds or be { "silent": "why" }');
    for (const r of refs) {
      const [kind, name] = String(r).split(':');
      const known =
        (kind === 'event' && out.events.some((e) => e.name === name)) ||
        (kind === 'action' && name in out.actions) ||
        (kind === 'ui' && name in out.ui);
      if (!known) fail(where, `unknown sound reference "${r}"`);
    }
  }
  return out;
}

/** Checks blocks' `sound` references (`{ id, sound? }`) against resolved sound data. */
export function checkBlockSounds(blocks, resolved) {
  for (const b of blocks) {
    if (b.sound === undefined) continue;
    if (typeof b.sound !== 'string' || !ID.test(b.sound))
      throw new Error(`shared/blocks: ${b.id}: sound must be a set id like "dwell:stone"`);
    if (!(b.sound in resolved.sets))
      throw new Error(
        `shared/blocks: ${b.id}: unknown sound set "${b.sound}" (shared/sounds/*.json sets: ${Object.keys(resolved.sets).join(', ')})`,
      );
  }
}
