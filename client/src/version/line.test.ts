// The compatibility line local builds carry (shared/version/line.json, docs/RELEASES.md §3): read by
// crates/dwell-sim/build.rs and vite.config.ts instead of package.json, so a patch release
// recompiles nothing. It must stay package.json's MAJOR.MINOR.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { parseVersion } from './semver';

const read = (path: string): string => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('the version line file', () => {
  it("is package.json's MAJOR.MINOR", () => {
    const pkg = parseVersion(
      (JSON.parse(read('../../package.json')) as { version: string }).version,
    );
    const { line } = JSON.parse(read('../../../shared/version/line.json')) as { line: string };
    expect(pkg).not.toBeNull();
    expect(
      line,
      'a new line raises shared/version/line.json with package.json (docs/RELEASES.md §3)',
    ).toBe(`${String(pkg?.major)}.${String(pkg?.minor)}`);
  });

  it("is what dwell-sim's build script reruns on, not package.json", () => {
    const build = read('../../../crates/dwell-sim/build.rs');
    const reruns = [...build.matchAll(/cargo:rerun-if-changed=\{?(\w+)/g)].map((m) => m[1]);
    expect(reruns).toEqual(['line_file']);
    expect(build).toContain('/../../shared/version/line.json"');
    expect(build).not.toContain('package.json"');
  });

  it('gives local builds a valid pre-release on the line', () => {
    const { line } = JSON.parse(read('../../../shared/version/line.json')) as { line: string };
    expect(parseVersion(`${line}.0-dev.local+abc1234`)).not.toBeNull();
  });
});
