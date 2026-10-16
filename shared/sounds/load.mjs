// Reads shared/sounds/*.json (docs/AUDIO.md §2) and resolves them (validate.mjs).
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveSounds } from './validate.mjs';

export const SOUNDS_DIR = dirname(fileURLToPath(import.meta.url));

export function readSoundFiles(dir = SOUNDS_DIR) {
  return readdirSync(dir)
    .filter((f) => f.endsWith('.json'))
    .sort()
    .map((f) => {
      try {
        return JSON.parse(readFileSync(join(dir, f), 'utf8'));
      } catch (e) {
        throw new Error(`shared/sounds/${f}: ${e.message}`);
      }
    });
}

export function loadSounds(dir = SOUNDS_DIR) {
  return resolveSounds(readSoundFiles(dir));
}
