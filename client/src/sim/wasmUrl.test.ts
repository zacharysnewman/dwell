import { describe, expect, it } from 'vitest';
import { buildInfo } from '../buildInfo';
import { dwellWorldgenUrl } from '../worldgen/generator';
import { dwellCoreUrl } from './module';
import { versionedLocateFile, wasmUrl } from './wasmUrl';

describe('WASM module URLs', () => {
  it('carry the build version, so a deploy never mixes a cached loader or .wasm with new code', () => {
    // Regression (iOS Safari): the core's fixed URLs were served from cache after a deploy, and a
    // stale loader/.wasm pair hung local worlds on "Joining…" until the cache was cleared.
    const v = `?v=${encodeURIComponent(buildInfo.sha)}`;
    expect(dwellCoreUrl()).toMatch(new RegExp(`wasm/dwell_core\\.js\\${v}$`));
    expect(dwellWorldgenUrl()).toMatch(new RegExp(`wasm/dwell_worldgen\\.js\\${v}$`));
    expect(versionedLocateFile('abc123')('dwell_core.wasm', 'https://x.test/dwell/wasm/')).toBe(
      'https://x.test/dwell/wasm/dwell_core.wasm?v=abc123',
    );
    expect(wasmUrl('dwell_core.js', 'a b')).toMatch(/dwell_core\.js\?v=a%20b$/);
  });
});
