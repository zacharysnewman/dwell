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

/**
 * Resolves `files` (parsed shared/music/*.json) against the audio files in `dir`: the tracks in
 * order (by namespace, then as listed), each with its published file name (`name-<hash>.ext`).
 */
export function resolveMusic(files, dir) {
  const tracks = [];
  const sorted = [...files].sort((a, b) =>
    a.namespace < b.namespace ? -1 : 1,
  );
  for (const file of sorted) {
    if (!isObject(file)) fail("file", "must be an object");
    for (const k of Object.keys(file))
      if (!["$comment", "namespace", "tracks"].includes(k))
        fail(`namespace ${file.namespace}`, `unknown field "${k}"`);
    if (typeof file.namespace !== "string" || !NAME.test(file.namespace))
      fail("namespace", `bad namespace ${file.namespace}`);
    const ns = file.namespace;
    if (!isObject(file.tracks)) fail(ns, "tracks must be an object");
    for (const [name, t] of Object.entries(file.tracks)) {
      const id = `${ns}:${name}`;
      const where = `track ${id}`;
      if (!NAME.test(name)) fail(where, "bad name");
      if (tracks.some((x) => x.id === id)) fail(where, "defined twice");
      if (!isObject(t)) fail(where, "must be an object");
      for (const k of Object.keys(t))
        if (!TRACK_KEYS.includes(k)) fail(where, `unknown field "${k}"`);
      for (const k of TRACK_KEYS)
        if (!(k in t)) fail(where, `missing "${k}"`);
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
      });
    }
  }
  if (tracks.length === 0) fail("tracks", "none");
  return { tracks };
}
