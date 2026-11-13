// Validation and resolution of the music data (docs/AUDIO.md §2a): shared by shared/music/gen.mjs
// and the client's Vitest suites. A bad entry throws an Error whose message names it.
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const NAME = /^[a-z0-9_]+$/;
/** A file beside the data: lower-case words and dashes, one of the formats every browser plays. */
const FILE = /^[a-z0-9][a-z0-9-]*\.(mp3|m4a)$/;
/** Where a track plays: the vocabulary the shell understands (client/src/audio/music.ts). */
export const WHEN = ["always"];
/** A track's file may be at most this large (it ships once per site, streamed). */
export const MAX_BYTES = 20_000_000;
/** How the playlist picks the next track (client/src/audio/playlist.ts). */
export const ORDER = ["shuffle"];
/** The times of day a track may play at (a track's `times`, default both). */
export const TIMES = ["day", "night"];
/** Where the playlist plays, each with its own wait before the first track (client/src/audio/page.ts). */
export const CONTEXTS = ["menu", "game"];
/** The longest gap or first wait (s). */
export const MAX_WAIT_S = 3600;
const PLAYLIST_KEYS = [
  "order",
  "gapMinS",
  "gapMaxS",
  "crossfadeS",
  "dayAt",
  "contexts",
];
const CONTEXT_KEYS = ["firstDelayMinS", "firstDelayMaxS"];
const TRACK_KEYS = [
  "title",
  "author",
  "file",
  "volume",
  "loop",
  "fadeInS",
  "fadeOutS",
  "when",
];
/** Fields a track may leave out. */
const TRACK_OPTIONAL = ["times"];

function fail(where, message) {
  throw new Error(`shared/music: ${where}: ${message}`);
}

const isObject = (v) =>
  typeof v === "object" && v !== null && !Array.isArray(v);

function text(where, v) {
  if (typeof v !== "string" || v.trim() === "")
    fail(where, "must be a non-empty string");
}

function number(where, v, min, max) {
  if (typeof v !== "number" || !Number.isFinite(v))
    fail(where, "must be a number");
  if (v < min || v > max) fail(where, `${v} is outside [${min}, ${max}]`);
}

/** The file's content hash (8 hex digits): its published name changes with its bytes. */
export function contentHash(bytes) {
  return createHash("sha256").update(bytes).digest("hex").slice(0, 8);
}

/** The playlist: the order and the silence (or overlap) between tracks. */
function resolvePlaylist(where, p) {
  if (!isObject(p)) fail(where, "must be an object");
  for (const k of Object.keys(p))
    if (!PLAYLIST_KEYS.includes(k)) fail(where, `unknown field "${k}"`);
  for (const k of PLAYLIST_KEYS) if (!(k in p)) fail(where, `missing "${k}"`);
  if (!ORDER.includes(p.order))
    fail(`${where}.order`, `"${p.order}" is not one of ${ORDER.join(", ")}`);
  number(`${where}.gapMinS`, p.gapMinS, 0, MAX_WAIT_S);
  number(`${where}.gapMaxS`, p.gapMaxS, 0, MAX_WAIT_S);
  if (p.gapMaxS < p.gapMinS)
    fail(`${where}.gapMaxS`, `${p.gapMaxS} is below gapMinS ${p.gapMinS}`);
  number(`${where}.crossfadeS`, p.crossfadeS, 0, 30);
  number(`${where}.dayAt`, p.dayAt, 0, 1);
  if (!isObject(p.contexts)) fail(`${where}.contexts`, "must be an object");
  for (const k of Object.keys(p.contexts))
    if (!CONTEXTS.includes(k))
      fail(
        `${where}.contexts`,
        `"${k}" is not one of ${CONTEXTS.join(", ")}`,
      );
  const contexts = {};
  for (const name of CONTEXTS) {
    const w = `${where}.contexts.${name}`;
    const c = p.contexts[name];
    if (!(name in p.contexts)) fail(`${where}.contexts`, `missing "${name}"`);
    if (!isObject(c)) fail(w, "must be an object");
    for (const k of Object.keys(c))
      if (!CONTEXT_KEYS.includes(k)) fail(w, `unknown field "${k}"`);
    for (const k of CONTEXT_KEYS) if (!(k in c)) fail(w, `missing "${k}"`);
    number(`${w}.firstDelayMinS`, c.firstDelayMinS, 0, MAX_WAIT_S);
    number(`${w}.firstDelayMaxS`, c.firstDelayMaxS, 0, MAX_WAIT_S);
    if (c.firstDelayMaxS < c.firstDelayMinS)
      fail(
        `${w}.firstDelayMaxS`,
        `${c.firstDelayMaxS} is below firstDelayMinS ${c.firstDelayMinS}`,
      );
    contexts[name] = {
      firstDelayMinS: c.firstDelayMinS,
      firstDelayMaxS: c.firstDelayMaxS,
    };
  }
  return {
    order: p.order,
    gapMinS: p.gapMinS,
    gapMaxS: p.gapMaxS,
    crossfadeS: p.crossfadeS,
    dayAt: p.dayAt,
    contexts,
  };
}

/**
 * Resolves `files` (parsed shared/music/*.json) against the audio files in `dir`: the tracks in
 * order (by namespace, then as listed), each with its published file name (`name-<hash>.ext`).
 */
export function resolveMusic(files, dir) {
  const tracks = [];
  let playlist = null;
  const sorted = [...files].sort((a, b) =>
    a.namespace < b.namespace ? -1 : 1,
  );
  for (const file of sorted) {
    if (!isObject(file)) fail("file", "must be an object");
    for (const k of Object.keys(file))
      if (!["$comment", "namespace", "playlist", "tracks"].includes(k))
        fail(`namespace ${file.namespace}`, `unknown field "${k}"`);
    if (typeof file.namespace !== "string" || !NAME.test(file.namespace))
      fail("namespace", `bad namespace ${file.namespace}`);
    const ns = file.namespace;
    if ("playlist" in file) {
      if (playlist)
        fail(`namespace ${ns}`, "a second playlist (only one file sets it)");
      playlist = resolvePlaylist(`playlist (${ns})`, file.playlist);
    }
    if (!isObject(file.tracks)) fail(ns, "tracks must be an object");
    for (const [name, t] of Object.entries(file.tracks)) {
      const id = `${ns}:${name}`;
      const where = `track ${id}`;
      if (!NAME.test(name)) fail(where, "bad name");
      if (tracks.some((x) => x.id === id)) fail(where, "defined twice");
      if (!isObject(t)) fail(where, "must be an object");
      for (const k of Object.keys(t))
        if (!TRACK_KEYS.includes(k) && !TRACK_OPTIONAL.includes(k))
          fail(where, `unknown field "${k}"`);
      for (const k of TRACK_KEYS) if (!(k in t)) fail(where, `missing "${k}"`);
      text(`${where}.title`, t.title);
      text(`${where}.author`, t.author);
      if (typeof t.file !== "string" || !FILE.test(t.file))
        fail(
          `${where}.file`,
          `"${t.file}" must be a lower-case .mp3 or .m4a file name`,
        );
      const path = join(dir, t.file);
      if (!existsSync(path))
        fail(`${where}.file`, `${t.file} is not in shared/music/`);
      const size = statSync(path).size;
      if (size > MAX_BYTES)
        fail(
          `${where}.file`,
          `${t.file} is ${size} bytes, over ${MAX_BYTES}: encode it smaller`,
        );
      number(`${where}.volume`, t.volume, 0, 1);
      if (typeof t.loop !== "boolean")
        fail(`${where}.loop`, "must be true or false");
      number(`${where}.fadeInS`, t.fadeInS, 0, 60);
      number(`${where}.fadeOutS`, t.fadeOutS, 0, 60);
      if (!WHEN.includes(t.when))
        fail(`${where}.when`, `"${t.when}" is not one of ${WHEN.join(", ")}`);
      const times = t.times ?? TIMES;
      if (!Array.isArray(times) || times.length === 0)
        fail(`${where}.times`, `must be a non-empty list of ${TIMES.join(", ")}`);
      for (const x of times)
        if (!TIMES.includes(x))
          fail(`${where}.times`, `"${x}" is not one of ${TIMES.join(", ")}`);
      if (new Set(times).size !== times.length)
        fail(`${where}.times`, "lists a time twice");
      const dot = t.file.lastIndexOf(".");
      const published = `${t.file.slice(0, dot)}-${contentHash(readFileSync(path))}${t.file.slice(dot)}`;
      tracks.push({
        id,
        title: t.title,
        author: t.author,
        source: t.file,
        file: published,
        volume: t.volume,
        loop: t.loop,
        fadeInS: t.fadeInS,
        fadeOutS: t.fadeOutS,
        when: t.when,
        times: TIMES.filter((x) => times.includes(x)),
      });
    }
  }
  if (tracks.length === 0) fail("tracks", "none");
  if (!playlist) fail("playlist", "missing (one file sets it)");
  return { playlist, tracks };
}
