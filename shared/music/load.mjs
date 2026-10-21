// Reads shared/music/*.json (docs/AUDIO.md §2a) and resolves them (validate.mjs).
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveMusic } from "./validate.mjs";

export const MUSIC_DIR = dirname(fileURLToPath(import.meta.url));

export function readMusicFiles(dir = MUSIC_DIR) {
  return readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .sort()
    .map((f) => {
      try {
        return JSON.parse(readFileSync(join(dir, f), "utf8"));
      } catch (e) {
        throw new Error(`shared/music/${f}: ${e.message}`);
      }
    });
}

export function loadMusic(dir = MUSIC_DIR) {
  return resolveMusic(readMusicFiles(dir), dir);
}
