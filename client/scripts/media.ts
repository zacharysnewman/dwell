// The music's files (docs/AUDIO.md §6a, RELEASES.md §4): each track ships once per site, under
// `media/` at the site's root (`/dwell/media/<name>-<hash>.mp3`), not in every version directory:
// a release build references them there, the build and the launcher emit them into their own
// `media/`, and the site's assembly moves the builds' copies to the root (`site.ts media`). Names
// carry the file's content hash, so a file there never changes.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Plugin } from 'vite';
import { MUSIC_DATA } from '../src/audio/music.gen.ts';

/** The directory the music's sources live in (shared/music), from the client directory. */
const MUSIC_DIR = fileURLToPath(new URL('../../shared/music/', import.meta.url));

/**
 * Where a build loads the music from, given its base: a version directory's
 * (`/dwell/v/<version>/`) is the site root's `media/` (`/dwell/media/`); any other base its own.
 */
export function mediaBase(base: string): string {
  const m = /^(.*\/)v\/[^/]+\/$/.exec(base);
  return `${m?.[1] ?? base}media/`;
}

/** Emits every track into the build's `media/`, and serves them there in the dev server. */
export function musicFiles(base: string): Plugin {
  const files = new Map(MUSIC_DATA.tracks.map((t) => [t.file, t.source]));
  const prefix = mediaBase(base);
  return {
    name: 'dwell-music-files',
    generateBundle() {
      for (const [file, source] of files) {
        this.emitFile({
          type: 'asset',
          fileName: `media/${file}`,
          source: readFileSync(join(MUSIC_DIR, source)),
        });
      }
    },
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const path = (req.url ?? '').split('?')[0] ?? '';
        const source = path.startsWith(prefix) ? files.get(path.slice(prefix.length)) : undefined;
        if (!source) {
          next();
          return;
        }
        res.setHeader('Content-Type', 'audio/mpeg');
        res.end(readFileSync(join(MUSIC_DIR, source)));
      });
    },
  };
}
