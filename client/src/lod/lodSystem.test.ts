import { describe, expect, it } from 'vitest';
import type { MeshSectionOptions, SectionMeshes } from '../mesh/lodMesher';
import type { SectionMesher } from '../mesh/pool';
import { LodForm, MessageType, World } from '../protocol/constants.gen';
import type { ChunkCoord, LodSectionRequest, Vec3 } from '../protocol/messages';
import type { GeneratedSection } from '../worldgen/generator';
import type { SectionSource } from '../worldgen/pool';
import type { LodCamera } from './frustum';
import {
  cellSize,
  kindFromBounds,
  LOD_VOLUME,
  lodAncestor,
  lodCell,
  lodInWorld,
  lodId,
  LodKind,
  sectionAt,
  sectionOrigin,
  sectionSize,
  type LodBounds,
  type LodCoord,
} from './grid';
import { LodSystem, type LodView } from './lodSystem';

/** A deterministic PRNG (mulberry32). */
function rng(seed: number): () => number {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const empty = () => ({
  positions: new Float32Array(0),
  normals: new Float32Array(0),
  colors: new Float32Array(0),
  indices: new Uint32Array(0),
});
const EMPTY_MESHES: SectionMeshes = {
  opaque: empty(),
  water: empty(),
  skirts: [0, 1, 2, 3, 4, 5].map(empty),
};

const flatBounds = (): LodBounds => ({ lo: 0, hi: -1, anyInside: true });

/** The flat world at LOD: stone where a cell's bottom is below 0. */
function flatSection(c: LodCoord): GeneratedSection {
  const cells = new Uint16Array(LOD_VOLUME);
  const kind = kindFromBounds(c, flatBounds());
  const [, y0] = sectionOrigin(c);
  const size = cellSize(c[0]);
  if (kind !== LodKind.Empty) {
    for (let y = -1; y <= 32; y++) {
      if (y0 + y * size >= 0) continue;
      for (let z = -1; z <= 32; z++) for (let x = -1; x <= 32; x++) cells[lodCell(x, y, z)] = 2;
    }
  }
  return { kind, cells };
}

/** Worker pools whose jobs finish when the test says, in any order. */
class Jobs implements SectionSource, SectionMesher {
  pending: (() => void)[] = [];
  /** Generation (and bounds) below this level never finishes: a stalled or very slow device. */
  stuckBelow = 0;
  lod(c: LodCoord): Promise<GeneratedSection> {
    return new Promise((resolve) => {
      if (c[0] < this.stuckBelow) return;
      this.pending.push(() => {
        resolve(flatSection(c));
      });
    });
  }
  lodBounds(level: number): Promise<LodBounds> {
    return new Promise((resolve) => {
      if (level < this.stuckBelow) return;
      this.pending.push(() => {
        resolve(flatBounds());
      });
    });
  }
  /** Each meshing job's options. */
  options: MeshSectionOptions[] = [];
  meshSection(_cells: Uint16Array, options: MeshSectionOptions = {}): Promise<SectionMeshes> {
    // Selection only needs to know a mesh exists (lodMesher.test.ts tests meshing itself).
    this.options.push(options);
    return new Promise((resolve) =>
      this.pending.push(() => {
        resolve(EMPTY_MESHES);
      }),
    );
  }
  /** Finishes a random share of the pending jobs. */
  async finish(random: () => number, share: number): Promise<void> {
    const now = this.pending;
    this.pending = [];
    for (const job of now) {
      if (random() < share) job();
      else this.pending.push(job);
    }
    await new Promise((r) => setTimeout(r, 0));
  }
}

class View implements LodView {
  meshed = new Set<number>();
  shown = new Map<number, number>();
  setLodSection(key: number, _o: Vec3, _c: number, meshes: SectionMeshes | null): void {
    if (meshes) this.meshed.add(key);
    else this.meshed.delete(key);
  }
  showLodSections(visible: ReadonlyMap<number, number>): void {
    this.shown = new Map(visible);
  }
}

function camera(position: Vec3, yawDeg: number, pitchDeg: number): LodCamera {
  return { position, yawDeg, pitchDeg, fovYDeg: 75, aspect: 16 / 9, heightPx: 1080 };
}

function contains(c: LodCoord, p: Vec3): boolean {
  const o = sectionOrigin(c);
  const s = sectionSize(c[0]);
  return p.every((v, a) => v >= (o[a] ?? 0) && v < (o[a] ?? 0) + s);
}

/** A random point in the camera's view, up to `range` away, inside the world. */
function pointInView(cam: LodCamera, random: () => number, range: number): Vec3 | null {
  const yaw = (cam.yawDeg * Math.PI) / 180;
  const pitch = (cam.pitchDeg * Math.PI) / 180;
  const f: Vec3 = [
    Math.sin(yaw) * Math.cos(pitch),
    Math.sin(pitch),
    Math.cos(yaw) * Math.cos(pitch),
  ];
  const r: Vec3 = [-Math.cos(yaw), 0, Math.sin(yaw)];
  const u: Vec3 = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]];
  const ty = Math.tan((cam.fovYDeg * Math.PI) / 360);
  const sx = (random() * 2 - 1) * ty * cam.aspect;
  const sy = (random() * 2 - 1) * ty;
  const d = random() ** 2 * range;
  const p: Vec3 = [0, 0, 0];
  for (let a = 0; a < 3; a++) {
    p[a] = (cam.position[a] ?? 0) + d * ((f[a] ?? 0) + sx * (r[a] ?? 0) + sy * (u[a] ?? 0));
  }
  const inside =
    p[1] >= World.worldMinY &&
    p[1] < World.worldMaxY &&
    p[0] ** 2 + p[2] ** 2 < World.worldRadius ** 2;
  return inside ? p : null;
}

describe('LOD selection (§6.6)', { timeout: 120_000 }, () => {
  it('covers the view with no holes or overlaps every frame, while moving and swapping', async () => {
    const random = rng(7);
    const jobs = new Jobs();
    const view = new View();
    const body: Vec3 = [0.5, 1.6, 0.5];
    // Chunks within 3 of the body are streamed (and drawable after a while).
    const chunks = {
      drawable: (c: ChunkCoord) =>
        Math.max(
          Math.abs(c[0] - Math.floor(body[0] / 32)),
          Math.abs(c[1] - Math.floor(body[1] / 32)),
          Math.abs(c[2] - Math.floor(body[2] / 32)),
        ) <= 3,
    };
    const lod = new LodSystem(jobs, jobs, view, chunks, () => undefined, {
      pixelError: 4,
      cacheBytes: 64 * 1048576,
      maxGenerationJobs: 16,
      maxMeshJobs: 8,
    });
    // Walk, look around, then rise with the dev camera to 2,000 km.
    const path: LodCamera[] = [];
    for (let i = 0; i < 60; i++) path.push(camera([0.5 + i * 3, 1.6, 0.5], i * 6, -10));
    for (let i = 0; i < 90; i++) {
      const alt = 1.6 * Math.pow(2e6 / 1.6, i / 89);
      path.push(camera([180, alt, 0.5 + i * alt * 0.01], 30, -20 - i * 0.7));
    }
    let checked = 0;
    let frames = 0;
    for (const cam of path) {
      for (let repeat = 0; repeat < 3; repeat++) {
        frames++;
        // A device that keeps up: well within FORCE_CHUNKS_AFTER_MS, so never a forced path.
        lod.update(cam, frames);
        await jobs.finish(random, 0.5);
        if (!lod.active) continue;
        const s = lod.lastSelection();
        // Every drawn section has a mesh and is shown; nothing else is shown.
        for (const c of s.drawn) expect(view.meshed.has(lodId(...c))).toBe(true);
        expect(view.shown.size).toBe(s.drawn.length);
        const leaves = [...s.drawn, ...s.empty, ...s.chunks];
        for (let n = 0; n < 100; n++) {
          const p = pointInView(cam, random, 3e6);
          if (!p) continue;
          const covering = leaves.filter((c) => contains(c, p)).length;
          if (covering !== 1) {
            throw new Error(
              `frame ${String(frames)}: ${String(covering)} leaves hold (${p.join(', ')})`,
            );
          }
          checked++;
        }
      }
    }
    expect(checked).toBeGreaterThan(2000);
    // Near the body the streamed chunks stand in for level 0; high above, coarse levels only.
    const start = camera([0.5, 1.6, 0.5], 0, -10);
    for (let i = 0; i < 2000 && lod.lastSelection().chunks.length === 0; i++) {
      lod.update(start, ++frames * 16);
      await jobs.finish(random, 1);
    }
    expect(lod.lastSelection().chunks.length).toBeGreaterThan(0);
    expect(lod.chunkVisible([0, 0, 0])).toBe(true);
    expect(lod.chunkVisible([40, 0, 0])).toBe(false); // not streamed: LOD draws there
  });

  it('shows the streamed chunks around the player while the levels above them are not ready', async () => {
    // Regression (phone playtest): chunks were drawn only once every LOD level down to them was
    // generated, so a device whose LOD generation stalled drew nothing at all.
    const jobs = new Jobs();
    jobs.stuckBelow = 12;
    const view = new View();
    const chunks = {
      drawable: (c: ChunkCoord) => Math.max(Math.abs(c[0]), Math.abs(c[1]), Math.abs(c[2])) <= 3,
    };
    const lod = new LodSystem(jobs, jobs, view, chunks, () => undefined, {
      pixelError: 4,
      cacheBytes: 64 * 1048576,
      maxGenerationJobs: 16,
      maxMeshJobs: 8,
    });
    const cam = camera([0.5, 1.6, 0.5], 0, -10);
    const random = rng(3);
    const noOverlaps = (at: LodCamera): void => {
      const s = lod.lastSelection();
      const leaves = [...s.drawn, ...s.empty, ...s.chunks];
      for (let n = 0; n < 100; n++) {
        const p = pointInView(at, random, 3e6);
        if (p) expect(leaves.filter((c) => contains(c, p)).length).toBeLessThanOrEqual(1);
      }
    };
    // Looking around, then standing still: 3 s, past FORCE_CHUNKS_AFTER_MS.
    for (let frame = 1; frame <= 60; frame++) {
      const at = frame < 40 ? camera([0.5, 1.6, 0.5], frame * 9, -10) : cam;
      lod.update(at, frame * 50);
      await jobs.finish(() => 0, 1);
      noOverlaps(at);
    }
    expect(lod.active).toBe(true);
    for (const c of [
      [0, 0, 0],
      [1, 0, 2],
      [-2, -1, 1],
    ] as ChunkCoord[]) {
      expect(lod.chunkVisible(c)).toBe(true);
    }
    // Coarse sections are not drawn over the chunks (unready ones are holes); drawn ones have meshes.
    for (const c of lod.lastSelection().drawn) expect(view.meshed.has(lodId(...c))).toBe(true);
    expect(lod.chunkVisible([40, 0, 0])).toBe(false); // not streamed
  });

  it('turning around shows the detail already loaded: it does not depend on the view', async () => {
    // Regression (phone playtest): sections out of view were never refined, so turning showed
    // coarse sections popping to fine ones everywhere the view swept.
    const jobs = new Jobs();
    const lod = new LodSystem(jobs, jobs, new View(), { drawable: () => false }, () => undefined, {
      pixelError: 8,
      cacheBytes: 256 * 1048576,
      maxGenerationJobs: 64,
      maxMeshJobs: 64,
    });
    let frame = 0;
    const settle = async (cam: LodCamera): Promise<void> => {
      for (let i = 0; i < 600; i++) {
        lod.update(cam, ++frame * 16);
        if (jobs.pending.length === 0 && lod.active) return;
        await jobs.finish(() => 0, 1);
      }
    };
    const levelAt = (p: Vec3): number => {
      const s = lod.lastSelection();
      const leaf = [...s.drawn, ...s.empty, ...s.chunks].find((c) => contains(c, p));
      return leaf ? leaf[0] : Infinity;
    };
    const random = rng(5);
    await settle(camera([0, 40, 0], 0, -10));
    for (const yaw of [90, 180, 270]) {
      const cam = camera([0, 40, 0], yaw, -10);
      lod.update(cam, ++frame * 16); // the first frame after turning: nothing new has loaded
      const points: Vec3[] = [];
      for (let n = 0; n < 300; n++) {
        const p = pointInView(cam, random, 50_000);
        if (p) points.push(p);
      }
      const before = points.map(levelAt);
      await settle(cam);
      points.forEach((p, n) => {
        expect(before[n] ?? Infinity).toBeLessThanOrEqual(levelAt(p));
      });
    }
  });

  it("draws water at every level as see-through, at the chunks' water height", async () => {
    // Regression (playtest: near and coarse water did not join): every level draws see-through
    // water (the mesher's only way) with its surface 1/8 m lower in world units, as the chunks do.
    const jobs = new Jobs();
    const lod = new LodSystem(jobs, jobs, new View(), { drawable: () => false }, () => undefined, {
      pixelError: 4,
      cacheBytes: 64 * 1048576,
      maxGenerationJobs: 64,
      maxMeshJobs: 64,
    });
    for (const altitude of [100, 1e6]) {
      for (let i = 0; i < 200; i++) {
        lod.update(camera([0, altitude, 0], 0, -90), i * 16);
        if (jobs.pending.length === 0 && lod.active) break;
        await jobs.finish(() => 0, 1);
      }
    }
    expect(jobs.options.length).toBeGreaterThan(0);
    // waterDrop is in cells: 1/8 m over the section's cell size, a power of two.
    const cellSizes = new Set<number>();
    for (const o of jobs.options) {
      const size = 0.125 / (o.waterDrop ?? 0);
      expect(Number.isInteger(Math.log2(size))).toBe(true);
      cellSizes.add(size);
    }
    expect(Math.min(...cellSizes)).toBeLessThanOrEqual(2); // near levels...
    expect(Math.max(...cellSizes)).toBeGreaterThanOrEqual(256); // ...and coarse ones
  });

  it('refines by screen-space error: coarser with distance and altitude', async () => {
    const jobs = new Jobs();
    const lod = new LodSystem(jobs, jobs, new View(), { drawable: () => false }, () => undefined, {
      pixelError: 2,
      cacheBytes: 256 * 1048576,
      maxGenerationJobs: 64,
      maxMeshJobs: 64,
    });
    const finest = async (cam: LodCamera): Promise<number> => {
      for (let i = 0; i < 400; i++) {
        lod.update(cam, i * 16);
        if (jobs.pending.length === 0 && lod.active) break;
        await jobs.finish(() => 0, 1);
      }
      return Math.min(...lod.lastSelection().drawn.map((c) => c[0]));
    };
    const low = await finest(camera([0, 10, 0], 0, -30));
    const high = await finest(camera([0, 1e6, 0], 0, -90));
    expect(low).toBe(1);
    expect(high).toBeGreaterThanOrEqual(10);
  });
});

describe('LOD requests (§6.6)', { timeout: 60_000 }, () => {
  function setup() {
    const jobs = new Jobs();
    const requests: LodSectionRequest[][] = [];
    const lod = new LodSystem(
      jobs,
      jobs,
      new View(),
      { drawable: () => false },
      (r) => {
        requests.push(r);
      },
      { pixelError: 4, cacheBytes: 256 * 1048576, maxGenerationJobs: 64, maxMeshJobs: 64 },
    );
    const cam = camera([100, 30, 100], 0, -20);
    let now = 0;
    const run = async (frames: number) => {
      for (let i = 0; i < frames; i++) {
        now += 16;
        lod.update(cam, now);
        await jobs.finish(() => 0, 1);
      }
    };
    return { lod, requests, run, now: () => now };
  }

  it('asks nothing while the index has no modification in view', async () => {
    const { lod, requests, run } = setup();
    lod.onMessage({ type: MessageType.LodIndex, last: true, entries: [] }, 10, 0);
    await run(200);
    expect(lod.active).toBe(true);
    expect(requests).toEqual([]);
  });

  /**
   * A server with one modified path (the sections above a build at `site`, levels 1–19, all at
   * `revision`): Explicit on it (Unchanged for the held revision), Generated elsewhere.
   */
  function server(lod: LodSystem, site: Vec3) {
    const path = new Map<number, number>();
    const state = { revision: 5, asked: [] as LodSectionRequest[] };
    for (let level = 1; level <= 19; level++) path.set(lodId(...sectionAt(level, site)), 0);
    const answer = (requests: LodSectionRequest[][], now: number) => {
      for (const r of requests.splice(0).flat()) {
        state.asked.push(r);
        const onPath = path.has(lodId(r.level, ...r.section));
        const form = !onPath
          ? LodForm.Generated
          : r.knownRevision === state.revision
            ? LodForm.Unchanged
            : LodForm.Explicit;
        lod.onMessage(
          {
            type: MessageType.LodData,
            form,
            level: r.level,
            section: r.section,
            revision: onPath ? state.revision : 0,
            cells: form === LodForm.Explicit ? flatSection([r.level, ...r.section]).cells : null,
          },
          100,
          now,
        );
      }
    };
    const index = (revision: number) => {
      state.revision = revision;
      const entry = sectionAt(8, site);
      lod.onMessage(
        { type: MessageType.LodIndexUpdate, entries: [{ i: entry[1], k: entry[3], revision }] },
        15,
        0,
      );
    };
    return { path, state, answer, index };
  }

  it('asks for sections under an index entry, top down, and trusts Generated for subtrees', async () => {
    const { lod, requests, run, now } = setup();
    const site: Vec3 = [100, 0, 100];
    const s = server(lod, site);
    s.index(5);
    for (let i = 0; i < 60; i++) {
      await run(1);
      s.answer(requests, now());
    }
    expect(lod.active).toBe(true);
    // Every section on the modified path was asked about (down to the finest level in view)...
    const askedIds = new Set(s.state.asked.map((r) => lodId(r.level, ...r.section)));
    for (let level = 8; level <= 19; level++) {
      expect(askedIds.has(lodId(...sectionAt(level, site)))).toBe(true);
    }
    // ...and nothing else but their children: a Generated answer covered the rest of a subtree.
    for (const r of s.state.asked) {
      const onPath = s.path.has(lodId(r.level, ...r.section));
      const parent = lodAncestor([r.level, ...r.section], r.level + 1);
      expect(onPath || r.level === 19 || s.path.has(lodId(...parent))).toBe(true);
      if (r.level > 8) expect(onPath).toBe(true); // above the index level: from the index alone
    }
    // Held sections are asked once each.
    expect(askedIds.size).toBe(s.state.asked.length);
  });

  it('asks again with the revision it holds when an index entry changes; unchanged stays', async () => {
    const { lod, requests, run, now } = setup();
    const site: Vec3 = [100, 0, 100];
    const s = server(lod, site);
    s.index(5);
    for (let i = 0; i < 60; i++) {
      await run(1);
      s.answer(requests, now());
    }
    s.state.asked.length = 0;
    s.index(9);
    for (let i = 0; i < 60; i++) {
      await run(1);
      s.answer(requests, now());
    }
    const again = s.state.asked;
    // The entry and its ancestors, with the revision held; then the path below, top down.
    const above = again.filter((r) => r.level >= 8);
    expect(above.length).toBe(20 - 8);
    for (const r of above) expect(r.knownRevision).toBe(5);
    for (const r of again) {
      const onPath = s.path.has(lodId(r.level, ...r.section));
      const parent = lodAncestor([r.level, ...r.section], r.level + 1);
      expect(onPath || s.path.has(lodId(...parent))).toBe(true);
    }
    // A third index update at the same revision changes nothing.
    s.state.asked.length = 0;
    s.index(9);
    await run(10);
    s.answer(requests, now());
    expect(s.state.asked).toEqual([]);
    expect(lodInWorld(sectionAt(8, site))).toBe(true);
  });
});
