import { describe, expect, it } from 'vitest';
import { MAP } from './map.gen';
import { World } from '../protocol/constants.gen';
import {
  addExplored,
  minimapSize,
  sameDots,
  cellKey,
  exploredRecord,
  headingOnScreen,
  pan,
  parseExplored,
  toScreen,
  toWorld,
  zoomAt,
  parseRevealTile,
  RevealCache,
  isLive,
  ExploredLevels,
  LEVELS,
  setMapWorldSize,
  worldRadiusM,
  islandRadiusM,
  Steps,
  levelPxPerM,
  levelCellM,
  tilePrefix,
  type TileStore,
  type ExploredHeights,
  tileOnScreen,
  type BigView,
  type ExploredCells,
  BigMapState,
  NO_HEIGHT,
  PadMap,
  bigMapPanel,
  parseHeights,
  scaleBar,
  tileCellAt,
  tileGrid,
  tilesShowing,
} from './minimap';
import { GAMEPAD } from '../input/gamepad.gen';
import { defaultHud, hudRecord, sanitizeHud } from './hudSettings';

const view = (face: number): BigView => ({ face, cx: 100, cz: -40, level: 1, pxPerM: 0.75 });

describe('the explored map view', () => {
  for (const face of [0, 1]) {
    it(`face ${String(face)}: screen and world points round-trip`, () => {
      const v = view(face);
      for (const [x, z] of [
        [100, -40],
        [130, -10],
        [-5, 77],
      ] as const) {
        const [sx, sy] = toScreen(v, x, z);
        const [bx, bz] = toWorld(v, sx, sy);
        expect(bx).toBeCloseTo(x, 9);
        expect(bz).toBeCloseTo(z, 9);
      }
    });

    it(`face ${String(face)}: +z (a yaw-0 heading) is up, and the player's right is right`, () => {
      const v = view(face);
      const [, up] = toScreen(v, v.cx, v.cz + 10);
      expect(up).toBeLessThan(0);
      // A yaw-0 player's right is face-local −x on face A and +x on face B (the mirror), as the
      // minimap's (dwell-game minimap.rs, tested on both faces there).
      const right = face === 1 ? 10 : -10;
      const [sx] = toScreen(v, v.cx + right, v.cz);
      expect(sx).toBeGreaterThan(0);
      // The arrow at yaw 0 points up; at the player's right-turn (yaw such that the heading is
      // their right) it points right.
      expect(headingOnScreen(face, 0)).toBeCloseTo(0, 9);
      const turned = face === 1 ? 90 : -90;
      expect(headingOnScreen(face, turned)).toBeCloseTo(Math.PI / 2, 9);
    });

    it(`face ${String(face)}: zooming keeps the point under the pointer`, () => {
      const v = view(face);
      const at: [number, number] = [120, -35];
      const before = toWorld(v, ...at);
      const z = zoomAt(v, 2.5, ...at);
      expect(z.pxPerM).toBe(2.5);
      const after = toWorld(z, ...at);
      expect(after[0]).toBeCloseTo(before[0], 9);
      expect(after[1]).toBeCloseTo(before[1], 9);
    });

    it(`face ${String(face)}: a drag moves the ground with the pointer`, () => {
      const v = view(face);
      const grabbed = toWorld(v, 10, 20);
      const p = pan(v, 30, -12);
      const now = toWorld(p, 40, 8);
      expect(now[0]).toBeCloseTo(grabbed[0], 9);
      expect(now[1]).toBeCloseTo(grabbed[1], 9);
    });
  }
});

describe('the explored cells', () => {
  it('are added from the client, and only changes count', () => {
    const cells = new Map<string, number>();
    const heights = new Map<string, number>();
    expect(
      addExplored(cells, [0, 1, -2, 0x336699, 84, 1, 1, -2, 0x112233, -2147483648], heights),
    ).toBe(true);
    expect(cells.get(cellKey(0, 1, -2))).toBe(0x336699);
    expect(cells.get(cellKey(1, 1, -2))).toBe(0x112233);
    expect(heights.get(cellKey(0, 1, -2))).toBe(84);
    expect(heights.has(cellKey(1, 1, -2))).toBe(false);
    expect(addExplored(cells, [0, 1, -2, 0x336699, 84], heights)).toBe(false);
    // A new height alone is a change (the shading follows it).
    const changed: string[] = [];
    expect(addExplored(cells, [0, 1, -2, 0x336699, 90], heights, changed)).toBe(true);
    expect(changed).toEqual([cellKey(0, 1, -2)]);
    expect(addExplored(cells, [0, 1, -2, 0x336600, 90], heights)).toBe(true);
  });

  it('are saved and loaded', () => {
    const cells = new Map([
      [cellKey(0, 3, 4), 0x445566],
      [cellKey(1, -7, 2), 0x0a0b0c],
    ]);
    const record = exploredRecord(undefined, cells);
    const back = parseExplored(JSON.parse(JSON.stringify(record)));
    expect([...back]).toEqual([...cells]);
  });

  it('keep what they do not know on rewrite (the store is append-only)', () => {
    const stored = {
      format: 1,
      cellM: MAP.explored.cellM,
      cells: { [cellKey(0, 9, 9)]: 0x010203 },
      newerField: { kept: true },
    };
    const record = exploredRecord(stored, new Map([[cellKey(0, 1, 1), 0xffffff]]));
    expect(record.newerField).toEqual({ kept: true });
    expect(record.cells).toEqual({ [cellKey(0, 9, 9)]: 0x010203, [cellKey(0, 1, 1)]: 0xffffff });
    // A record of another cell size is left as it was, ours kept beside it.
    const other = { format: 1, cellM: 16, cells: { '0,1,1': 5 } };
    const kept = exploredRecord(other, new Map([[cellKey(0, 2, 2), 7]]));
    expect(kept.cells).toEqual({ '0,1,1': 5 });
    expect(kept.cellM).toBe(16);
    expect(kept.cellsByM).toEqual({ [String(MAP.explored.cellM)]: { [cellKey(0, 2, 2)]: 7 } });
    expect(parseExplored(other).size).toBe(0);
    // Unreadable entries are skipped, not fatal.
    expect(
      parseExplored({ cellM: MAP.explored.cellM, cells: { junk: 1, '0,1,2': 'x' } }).size,
    ).toBe(0);
    expect(parseExplored(null).size).toBe(0);
  });
});

describe('the height shading toggle', () => {
  it("defaults to the data's, and is kept with the other on-screen settings", () => {
    expect(defaultHud().mapShading).toBe(MAP.heightShading.default);
    expect(sanitizeHud({ shown: {}, mapShading: false }).mapShading).toBe(false);
    expect(sanitizeHud({ shown: {}, mapShading: 'no' }).mapShading).toBe(MAP.heightShading.default);
    const rec = hudRecord(
      { shown: { future: true }, other: 1 },
      { ...defaultHud(), mapShading: false },
    );
    expect(rec.mapShading).toBe(false);
    expect(rec.other).toBe(1);
    expect((rec.shown as Record<string, boolean>).future).toBe(true);
  });

  it('the minimap is a part Settings, On screen, can hide', () => {
    expect(defaultHud().shown.minimap).toBe(true);
  });
});

describe('the dots', () => {
  it('redraw only when one moved', () => {
    expect(sameDots([1, 2, 3, 4], [1.05, 2, 3, 4])).toBe(true);
    expect(sameDots([1, 2, 3, 4], [1.5, 2, 3, 4])).toBe(false);
    expect(sameDots([1, 2], [1, 2, 3, 4])).toBe(false);
  });
});

describe('the minimap size', () => {
  it('is compact on a phone either way up, full on a tablet or a desktop', () => {
    expect(minimapSize(750, 342)).toBe(MAP.minimap.compactSizePx);
    // Upright (owner's screenshot, 0.50.39: the full size covered the hotbar and the lines).
    expect(minimapSize(390, 844)).toBe(MAP.minimap.compactSizePx);
    expect(minimapSize(360, 780)).toBe(MAP.minimap.compactSizePx);
    expect(minimapSize(820, 1180)).toBe(MAP.minimap.sizePx);
    expect(minimapSize(1440, 900)).toBe(MAP.minimap.sizePx);
    // On a phone sideways (750 × 342) it stays clear of the view at (600, 150), where the touch
    // test taps to place: right edge 12 px in, top 12 px.
    const r = MAP.minimap.compactSizePx / 2;
    const [cx, cy] = [750 - 12 - r, 12 + r];
    expect(Math.hypot(600 - cx, 150 - cy)).toBeGreaterThan(r);
  });
});

/** A tile's `takeReveal` bytes: each cell's colour from its indices; alpha 0 past `edgeAt` m. */
const tileBytes = (
  n: number,
  face: number,
  ti: number,
  tk: number,
  cellM: number,
  edgeAt = Infinity,
) => {
  const b = new Uint8Array(13 + n * n * 6);
  const d = new DataView(b.buffer);
  b[0] = face;
  d.setInt32(1, ti, true);
  d.setInt32(5, tk, true);
  d.setInt32(9, cellM, true);
  for (let k = 0; k < n; k++) {
    for (let i = 0; i < n; i++) {
      const o = 13 + (k * n + i) * 4;
      b[o] = (ti * n + i) & 0xff;
      b[o + 1] = (tk * n + k) & 0xff;
      b[o + 2] = face;
      b[o + 3] = Math.abs(ti * n + i) * cellM > edgeAt ? 0 : 255;
      d.setInt16(13 + n * n * 4 + (k * n + i) * 2, 2 * (ti * n + i), true);
    }
  }
  return b;
};

/** A client whose workers finish every tile asked for (`tileBytes`). */
const fakeClient = (n: number, edgeAt = Infinity) => {
  const done: Uint8Array[] = [];
  const asked: string[] = [];
  return {
    asked,
    requestReveal(face: number, ti: number, tk: number, cellM: number) {
      asked.push(RevealCache.key(cellM, face, ti, tk));
      done.push(tileBytes(n, face, ti, tk, cellM, edgeAt));
    },
    takeReveal: () => done.shift(),
  };
};

/** A memory tile store. */
const memoryTiles = () => {
  const kept = new Map<string, Uint8Array>();
  const store: TileStore = {
    list: (prefix) =>
      Promise.resolve(
        [...kept]
          .filter(([k]) => k.startsWith(prefix))
          .map(([k, v]) => [k.slice(prefix.length), v]),
      ),
    put: (key, bytes) => {
      kept.set(key, bytes);
      return Promise.resolve();
    },
  };
  return { kept, store };
};

describe('the zoom levels (owner, 2026-11-09)', () => {
  it('are at most five, the closest the minimap area at 1 m a cell, the widest the world disc, geometric between', () => {
    expect(LEVELS.length).toBeGreaterThanOrEqual(2);
    expect(LEVELS.length).toBeLessThanOrEqual(5);
    const first = LEVELS[0];
    const last = LEVELS[LEVELS.length - 1];
    expect(first).toEqual({ cellM: 1, spanM: 2 * MAP.minimap.radiusM, reach: 'player' });
    // shared/protocol/constants.json's worldRadius: the whole world, its sea too.
    expect(last?.spanM).toBe(2 * World.worldRadius);
    const ratios = LEVELS.slice(1).map((l, i) => l.spanM / (LEVELS[i]?.spanM ?? 1));
    for (const r of ratios) expect(r).toBeCloseTo(ratios[0] ?? 0, 2);
    // Each level its own, coarser cell.
    for (let i = 1; i < LEVELS.length; i++) {
      expect(LEVELS[i]?.cellM).toBeGreaterThan(LEVELS[i - 1]?.cellM ?? 0);
    }
  });

  it("follow a bigger world's size (BIG_MAP.md M2b): its disc, island and coarser levels", () => {
    const data = MAP.bigMap.levels;
    const before = data.map((_, l) => new RevealCache(32).levelTiles(l, 0, 0, 0).length);
    // The client's `bigMapLevels` at 2× (`MapTunables::big_map_levels_sized`): the closest level
    // as the data's, every coarser cell × 2, the spans geometric up to the 2× disc.
    const n = data.length;
    const near = 2 * MAP.minimap.radiusM;
    const far = 4 * World.worldRadius;
    const two = data.flatMap((l, i) => [
      i === 0 ? l.cellM : l.cellM * 2,
      i === 0 ? l.spanM : Math.round(near * (far / near) ** (i / (n - 1))),
    ]);
    try {
      setMapWorldSize(2, two);
      expect(worldRadiusM()).toBe(2 * World.worldRadius);
      expect(islandRadiusM()).toBe(2 * World.islandRadius);
      expect(LEVELS[0]).toEqual(data[0]);
      expect(LEVELS[n - 1]?.spanM).toBe(4 * World.worldRadius);
      expect(LEVELS.map((l) => l.reach)).toEqual(data.map((l) => l.reach));
      const cache = new RevealCache(32);
      // The 2× disc is on the map past today's rim; no coarse level builds more tiles than today.
      const widest = LEVELS[n - 1]?.cellM ?? 1;
      expect(cache.onWorld(widest, Math.floor(75_000 / (32 * widest)), 0)).toBe(true);
      for (let l = 1; l < n; l++) {
        expect(cache.levelTiles(l, 0, 0, 0).length).toBeLessThanOrEqual(before[l] ?? 0);
      }
    } finally {
      // Today's size restores the data's levels and disc.
      setMapWorldSize(1, []);
    }
    expect(LEVELS).toBe(data);
    expect(worldRadiusM()).toBe(World.worldRadius);
    // Levels that don't match the data's are refused: today's.
    setMapWorldSize(2, [1, 2]);
    expect(LEVELS).toBe(data);
    expect(worldRadiusM()).toBe(World.worldRadius);
  });

  it("a level's span fills the map's short side", () => {
    for (let l = 0; l < LEVELS.length; l++) {
      expect(levelPxPerM(l, 1000, 700) * (LEVELS[l]?.spanM ?? 0)).toBeCloseTo(700, 9);
    }
  });

  it('a wheel or a pinch steps whole levels, the rest kept', () => {
    const w = new Steps(100);
    expect(w.add(40)).toBe(0);
    expect(w.add(70)).toBe(1);
    expect(w.add(-30)).toBe(0);
    expect(w.add(-250)).toBe(-2);
    w.reset();
    expect(w.add(99)).toBe(0);
  });
});

/** The tiles of side `span` (m) that intersect the world disc and the square [x0, x1] × [z0, z1]. */
const discTiles = (
  span: number,
  x0: number,
  x1: number,
  z0: number,
  z1: number,
  r: number = World.worldRadius,
): string[] => {
  const out: string[] = [];
  for (let k = Math.floor(z0 / span); k <= Math.floor(z1 / span); k++) {
    for (let i = Math.floor(x0 / span); i <= Math.floor(x1 / span); i++) {
      const nx = Math.max(i * span, Math.min(0, (i + 1) * span));
      const nz = Math.max(k * span, Math.min(0, (k + 1) * span));
      if (nx * nx + nz * nz <= r * r) out.push(`${String(i)},${String(k)}`);
    }
  }
  return out.sort();
};

describe('the widest level shows the whole world (owner, 2026-11-09: "Including all the water")', () => {
  // A desktop's wide panel, a phone upright and sideways, a square.
  for (const [w, h] of [
    [1536, 691],
    [374, 707],
    [828, 358],
    [600, 600],
  ] as const) {
    it(`a ${String(w)} × ${String(h)} panel holds the whole disc, centred`, () => {
      const state = new BigMapState(
        () => [0, 31_000, -12_000],
        () => [w, h],
      );
      state.openAt();
      while (state.canZoom(-1)) state.zoomStep(-1);
      const v = state.view;
      expect(v?.level).toBe(LEVELS.length - 1);
      if (!v) return;
      const r = World.worldRadius;
      for (const [x, z] of [
        [r, 0],
        [-r, 0],
        [0, r],
        [0, -r],
      ] as const) {
        const [sx, sy] = toScreen(v, x, z);
        expect(Math.abs(sx)).toBeLessThanOrEqual(w / 2 + 1e-6);
        expect(Math.abs(sy)).toBeLessThanOrEqual(h / 2 + 1e-6);
      }
      // And fills the short side: the disc's diameter is the short side.
      expect(2 * r * v.pxPerM).toBeCloseTo(Math.min(w, h), 6);
      // Its view asks for every tile on the disc it shows, the sea's too.
      const span = 32 * (LEVELS[v.level]?.cellM ?? 1);
      const asked = new RevealCache(32)
        .tilesFor(v, w, h)
        .map(([, , i, k]) => `${String(i)},${String(k)}`)
        .sort();
      expect(asked).toEqual(discTiles(span, -r, r, -r, r));
    });
  }

  it('three levels (owner, 2026-11-10): 1 m round the player, the coarse ones the whole island and the whole disc', () => {
    expect(LEVELS.map((l) => l.reach)).toEqual(['player', 'island', 'disc']);
    const cache = new RevealCache(32, 2, 1000, 1);
    // Out at sea, beyond the island's coast but well inside the world.
    const [x, z] = [33_000, -9_000];
    const list = cache.backgroundFor(1, x, z);
    for (let l = 0; l < LEVELS.length; l++) {
      const { cellM, spanM, reach } = LEVELS[l] ?? { cellM: 1, spanM: 1, reach: 'player' };
      const span = 32 * cellM;
      const got = list
        .filter(([c]) => c === cellM)
        .map(([, , i, k]) => `${String(i)},${String(k)}`)
        .sort();
      const half = (spanM * MAP.revealed.prebuildSpans) / 2;
      const r = reach === 'island' ? World.islandRadius : World.worldRadius;
      const want =
        reach === 'player'
          ? discTiles(span, x - half, x + half, z - half, z + half)
          : discTiles(span, -r, r, -r, r, r);
      expect(got, `level ${String(l)}`).toEqual(want);
    }
    // The 1 m level first (always streamed round the player), then the coarsest, then the island's.
    const order = [...new Set(list.map(([c]) => c))];
    expect(order).toEqual([LEVELS[0]?.cellM, LEVELS[2]?.cellM, LEVELS[1]?.cellM]);
    // The island's 16 m tiles: a few thousand, each built once.
    expect(list.filter(([c]) => c === LEVELS[1]?.cellM).length).toBeLessThan(10_000);
  });
});

describe("the big map's tiles", () => {
  for (const face of [0, 1]) {
    it(`face ${String(face)}: each level's view resolves at its own cell size; adventure keeps the fog at every level`, () => {
      const [w, h] = [800, 600];
      const cells: ExploredCells = new Map([[cellKey(face, 40, -110), 0xabcdef]]);
      const heights: ExploredHeights = new Map([[cellKey(face, 40, -110), 900]]);
      const levels = new ExploredLevels(cells, heights);
      levels.note(cellKey(face, 40, -110));
      // The explored cell's centre, face-local.
      const [ex, ez] = [40.5 * MAP.explored.cellM, -109.5 * MAP.explored.cellM];
      for (let l = 0; l < LEVELS.length; l++) {
        const { cellM } = LEVELS[l] ?? { cellM: 1 };
        const v: BigView = { face, cx: ex, cz: ez, level: l, pxPerM: levelPxPerM(l, w, h) };
        const cache = new RevealCache();
        const client = fakeClient(cache.tileCells);
        const all = cache.tilesFor(v, w, h);
        expect(all.every(([c]) => c === cellM)).toBe(true);
        let clock = 0;
        for (let f = 0; f < 4000 && cache.tiles.size < all.length; f++) {
          cache.pump(client, v, w, h, () => clock++);
        }
        expect(cache.tiles.size).toBe(all.length);
        // The explored cell's place at this level.
        const [i, k] = [Math.floor(ex / cellM), Math.floor(ez / cellM)];
        // Creative: every cell resolves; adventure: only the explored ones.
        const at = levels.at(cache, true, cellM, face, i, k);
        expect(at).toBeDefined();
        // A cell two explored cells away, still in view.
        const d = Math.max(2, (2 * MAP.explored.cellM) / cellM);
        expect(levels.at(cache, true, cellM, face, i + d, k)).toBeDefined();
        expect(levels.at(cache, false, cellM, face, i + d, k)).toBeUndefined();
        expect(levels.at(cache, false, cellM, face, i, k)).toBeDefined();
        // Finer than the explored cells: the generator's detail; as coarse or coarser: the
        // explored (loaded, edited) terrain's colour.
        if (cellM < MAP.explored.cellM) {
          expect(at?.[0]).toBe(((i & 0xff) << 16) | ((k & 0xff) << 8) | face);
          expect(at?.[1]).toBe(2 * i);
        } else expect(at).toEqual([0xabcdef, 900]);
        // The other face's is another tile, not yet built.
        expect(levels.at(cache, true, cellM, 1 - face, i, k)).toBeUndefined();
      }
    });
  }

  it('beyond the world edge a cell resolves to nothing there (null), not fog', () => {
    const cache = new RevealCache();
    const client = fakeClient(cache.tileCells, 100);
    const v: BigView = { face: 0, cx: 0, cz: 0, level: 0, pxPerM: 1 };
    for (let f = 0; f < 50; f++) cache.pump(client, v, 400, 400, () => 0);
    expect(cache.colour(1, 0, 0, 0)).toBeTypeOf('number');
    expect(cache.colour(1, 0, 150, 0)).toBeNull();
  });

  it('the fill stays under its per-frame budget and its jobs in flight, nearest tiles first', () => {
    const cache = new RevealCache(32, 2, 2, 1);
    const client = fakeClient(32);
    const v: BigView = { face: 0, cx: 1000, cz: 1000, level: 2, pxPerM: levelPxPerM(2, 800, 600) };
    // Many finished tiles waiting (another view's): each costs 1 ms on this clock.
    for (let t = 0; t < 40; t++) client.requestReveal(0, 100 + t, 0, 8);
    let clock = 0;
    const taken = cache.pump(client, v, 800, 600, () => clock++);
    expect(taken).toBeGreaterThan(0);
    expect(taken).toBeLessThanOrEqual(cache.budgetMs);
    expect(cache.inFlight).toBeLessThanOrEqual(2);
    // The first tiles asked for are the view centre's.
    const c2 = LEVELS[2]?.cellM ?? 1;
    const centre = Math.floor(1000 / (32 * c2));
    expect(client.asked.slice(40, 42)).toContain(RevealCache.key(c2, 0, centre, centre));
    for (let f = 0; f < 30; f++) {
      const t0 = clock;
      const n = cache.pump(client, v, 800, 600, () => clock++);
      expect(n).toBeLessThanOrEqual(cache.budgetMs);
      expect(clock - t0).toBeLessThanOrEqual(cache.budgetMs + 2);
      expect(cache.inFlight).toBeLessThanOrEqual(2);
    }
  });

  it('the background builds each level round the player, the widest the whole world disc, a few at a time', () => {
    const cache = new RevealCache(32, 2, 1000, 1);
    const list = cache.backgroundFor(0, 300, -200);
    const by = new Map<number, number>();
    for (const [c] of list) by.set(c, (by.get(c) ?? 0) + 1);
    expect([...by.keys()].sort((a, b) => a - b)).toEqual(LEVELS.map((l) => l.cellM));
    // The widest level: every tile touching the world disc, wherever the player stands.
    const widest = LEVELS[LEVELS.length - 1]?.cellM ?? 1;
    const span = 32 * widest;
    const r = World.worldRadius;
    let island = 0;
    for (let k = -Math.ceil(r / span) - 1; k <= Math.ceil(r / span); k++) {
      for (let i = -Math.ceil(r / span) - 1; i <= Math.ceil(r / span); i++) {
        const nx = Math.max(i * span, Math.min(0, (i + 1) * span));
        const nz = Math.max(k * span, Math.min(0, (k + 1) * span));
        if (nx * nx + nz * nz <= r * r) island++;
      }
    }
    expect(by.get(widest)).toBe(island);
    // The 1 m level's first, nearest the player first.
    const start = LEVELS[0]?.cellM ?? 1;
    expect(list[0]?.[0]).toBe(start);
    const s = 32 * start;
    expect(list[0]?.slice(2)).toEqual([Math.floor(300 / s), Math.floor(-200 / s)]);
    // With the map shut, one job in flight at a time (backgroundJobs), and only once the kept
    // tiles are read.
    const client = fakeClient(32);
    const held: Uint8Array[] = [];
    const slow = {
      requestReveal: (face: number, ti: number, tk: number, cellM: number) => {
        client.requestReveal(face, ti, tk, cellM);
      },
      takeReveal: () => held.shift(),
    };
    cache.pump(slow, null, 0, 0, () => 0, undefined, [0, 300, -200]);
    expect(cache.inFlight).toBe(1);
    cache.pump(slow, null, 0, 0, () => 0, undefined, [0, 300, -200]);
    expect(cache.inFlight).toBe(1);
  });

  it('tiles are built once: a second open of the world reads them from the store and builds nothing', async () => {
    const { kept, store } = memoryTiles();
    const prefix = tilePrefix('local:seed-7', '0.50.61');
    const pose = [0, 40, 40];
    const first = new RevealCache(32, 2, 1000, 4, store, prefix);
    await first.load();
    const c1 = fakeClient(32);
    for (let f = 0; f < 2000 && (f === 0 || first.backgroundLeft > 0 || first.inFlight > 0); f++) {
      first.pump(c1, null, 0, 0, () => 0, undefined, pose);
    }
    expect(first.built).toBeGreaterThan(100);
    expect(first.backgroundLeft).toBe(0);
    expect(kept.size).toBe(first.built);
    // Each tile once.
    expect(new Set(c1.asked).size).toBe(c1.asked.length);
    // The world again (a reload): everything read, nothing asked for, opened at any level.
    const second = new RevealCache(32, 2, 1000, 4, store, prefix);
    const c2 = fakeClient(32);
    // Not yet read: no background (it would build them twice).
    second.pump(c2, null, 0, 0, () => 0, undefined, pose);
    expect(c2.asked).toEqual([]);
    expect(await second.load()).toBe(first.built);
    for (let l = 0; l < LEVELS.length; l++) {
      const v: BigView = { face: 0, cx: 40, cz: 40, level: l, pxPerM: levelPxPerM(l, 800, 600) };
      for (let f = 0; f < 10; f++) second.pump(c2, v, 800, 600, () => 0, undefined, pose);
    }
    expect(c2.asked).toEqual([]);
    expect(second.built).toBe(0);
    // A new patch on the same compatibility line reuses them (owner, 2026-11-09).
    const patch = new RevealCache(32, 2, 1000, 4, store, tilePrefix('local:seed-7', '0.50.62'));
    expect(await patch.load()).toBe(first.built);
    // A new line (or another world) builds its own.
    const line = new RevealCache(32, 2, 1000, 4, store, tilePrefix('local:seed-7', '0.51.0'));
    expect(await line.load()).toBe(0);
    const world = new RevealCache(32, 2, 1000, 4, store, tilePrefix('local:seed-8', '0.50.61'));
    expect(await world.load()).toBe(0);
  });

  it("a tile prefix is the world's and the compatibility line's, not the build's", () => {
    expect(tilePrefix('w', '0.50.74')).toBe(tilePrefix('w', '0.50.75'));
    expect(tilePrefix('w', '0.50.74')).not.toBe(tilePrefix('w', '0.51.0'));
    expect(tilePrefix('w', '1.2.0')).toBe(tilePrefix('w', '1.3.4'));
    // A pre-release promises nothing: its own tiles.
    expect(tilePrefix('w', '0.50.75-dev.3')).not.toBe(tilePrefix('w', '0.50.75-dev.4'));
  });

  it('a world without a terrain builds no background', () => {
    const cache = new RevealCache(32, 2, 1000, 1);
    const none = {
      asked: 0,
      requestReveal(face: number, ti: number, tk: number, cellM: number) {
        this.asked++;
        const b = new Uint8Array(13);
        const d = new DataView(b.buffer);
        b[0] = face;
        d.setInt32(1, ti, true);
        d.setInt32(5, tk, true);
        d.setInt32(9, cellM, true);
        this.done.push(b);
      },
      done: [] as Uint8Array[],
      takeReveal() {
        return this.done.shift();
      },
    };
    for (let f = 0; f < 20; f++) cache.pump(none, null, 0, 0, () => 0, undefined, [0, 0, 0]);
    expect(cache.noTerrain).toBe(true);
    expect(none.asked).toBe(1);
  });

  it('a tile from the client parses its face, place, level and pixels', () => {
    const b = new Uint8Array([1, 0xfd, 0xff, 0xff, 0xff, 7, 0, 0, 0, 32, 0, 0, 0, 9, 8, 7, 255]);
    const t = parseRevealTile(b);
    expect([t.face, t.ti, t.tk, t.cellM]).toEqual([1, -3, 7, 32]);
    expect(Array.from(t.rgba ?? [])).toEqual([9, 8, 7, 255]);
    expect(parseRevealTile(b.subarray(0, 13)).rgba).toBeNull();
  });

  it('an explored cell fogs and stales the right cells and tiles at every level', () => {
    const cells: ExploredCells = new Map([[cellKey(1, 5, -1), 0x123456]]);
    const levels = new ExploredLevels(cells, new Map());
    levels.note(cellKey(1, 5, -1));
    const e = MAP.explored.cellM;
    for (const { cellM } of LEVELS) {
      // A level cell inside the explored cell is explored; one just outside it is not.
      const [x, z] = [5 * e + e / 2, -e / 2];
      const [i, k] = [Math.floor(x / cellM), Math.floor(z / cellM)];
      expect(levels.keyAt(cellM, 1, i, k)).toBe(cellKey(1, 5, -1));
      expect(levels.keyAt(cellM, 0, i, k)).toBeUndefined();
      if (cellM <= e)
        expect(levels.keyAt(cellM, 1, i, Math.floor((z + e) / cellM))).toBeUndefined();
      // Its tiles at that level include the one holding it.
      const n = 32;
      expect(levels.tilesOf(cellM, cellKey(1, 5, -1), n)).toContain(
        RevealCache.key(cellM, 1, Math.floor(i / n), Math.floor(k / n)),
      );
    }
  });
});

describe('a revealed tile on screen', () => {
  for (const face of [0, 1]) {
    it(`face ${String(face)}: each texel lands where its cell is on the map`, () => {
      const n = 4;
      const rgba = new Uint8Array(n * n * 4);
      for (let b = 0; b < n; b++) for (let a = 0; a < n; a++) rgba[(b * n + a) * 4] = b * n + a;
      const img = tileOnScreen(rgba, n, face);
      const v: BigView = { face, cx: 0, cz: 0, level: 2, pxPerM: 1 };
      const cell = MAP.explored.cellM;
      // The tile's screen corner: the least screen x and y of its cells' centres.
      const at = (a: number, b: number) => toScreen(v, (a + 0.5) * cell, (b + 0.5) * cell);
      const xs = [0, n - 1].map((a) => at(a, 0)[0]);
      const ys = [0, n - 1].map((b) => at(0, b)[1]);
      for (let b = 0; b < n; b++) {
        for (let a = 0; a < n; a++) {
          const [sx, sy] = at(a, b);
          const c = Math.round((sx - Math.min(...xs)) / cell);
          const r = Math.round((sy - Math.min(...ys)) / cell);
          expect(img[(r * n + c) * 4]).toBe(b * n + a);
        }
      }
    });
  }
});

describe('the big map panel (owner, 2026-11-09)', () => {
  const pose = (face: number, x: number, z: number) => () => [face, x, z, 0];

  it('opens on the player at the start level, closes, and its zoom buttons step and clamp the levels', () => {
    const s = new BigMapState(pose(0, 12, -30), () => [900, 600]);
    expect(s.open).toBe(false);
    s.openAt();
    expect(s.open).toBe(true);
    expect(s.view).toMatchObject({ face: 0, cx: 12, cz: -30, level: MAP.bigMap.startLevel });
    expect(s.view?.pxPerM).toBeCloseTo(levelPxPerM(MAP.bigMap.startLevel, 900, 600));
    // In to the closest: the minimap's area across the short side.
    for (let n = 0; n < 10; n++) s.zoomStep(1);
    expect(s.view?.level).toBe(0);
    expect((s.view?.pxPerM ?? 0) * 2 * MAP.minimap.radiusM).toBeCloseTo(600, 6);
    expect(s.canZoom(1)).toBe(false);
    expect(s.canZoom(-1)).toBe(true);
    expect(s.zoomStep(1)).toBe(false);
    // Out a level at a time, to the whole world disc.
    const seen = [0];
    while (s.zoomStep(-1)) seen.push(s.view?.level ?? -1);
    expect(seen).toEqual(LEVELS.map((_, l) => l));
    expect((s.view?.pxPerM ?? 0) * 2 * World.worldRadius).toBeCloseTo(600, 6);
    // The widest is centred on the world, the whole of it in view.
    expect(s.view).toMatchObject({ cx: 0, cz: 0 });
    expect(s.canZoom(-1)).toBe(false);
    // A big wheel spin is a few levels at most, never past the ends.
    s.zoomStep(40);
    expect(s.view?.level).toBe(0);
    // The buttons zoom about the centre: the place in the middle stays put.
    s.center();
    s.zoomStep(1);
    expect(s.view?.cx).toBeCloseTo(12);
    expect(s.view?.cz).toBeCloseTo(-30);
    s.close();
    expect(s.open).toBe(false);
    expect(s.canZoom(1)).toBe(false);
  });

  for (const face of [0, 1]) {
    it(`face ${String(face)}: "Center on me" brings back the player's place, the zoom kept`, () => {
      let at = [face, 5, 6, 0];
      const s = new BigMapState(() => at);
      s.openAt();
      s.zoomStep(1);
      const zoom = s.view?.pxPerM;
      if (s.view) s.view = pan(s.view, 300, -120);
      expect(Math.abs((s.view?.cx ?? 5) - 5)).toBeGreaterThan(1);
      at = [face, -40, 90, 0];
      s.center();
      expect(s.view).toMatchObject({ face, cx: -40, cz: 90, pxPerM: zoom });
    });
  }

  it('the scale bar is a 1, 2 or 5 × 10ⁿ metres no longer than its most, per zoom', () => {
    for (const [pxPerM, m] of [
      [5, 20],
      [1, 100],
      [0.125, 500],
      [1 / 16, 1000],
    ] as const) {
      const s = scaleBar(pxPerM, 120);
      expect(s.metres).toBe(m);
      expect(s.px).toBeLessThanOrEqual(120);
      expect(s.px).toBeGreaterThan(120 / 2.5);
    }
    expect(scaleBar(1 / 16, 120).label).toBe('1 km');
    expect(scaleBar(1, 120).label).toBe('100 m');
    expect(scaleBar(0.01, 120).label).toBe('10 km');
    // Every zoom level has a bar, on a phone and on a desktop.
    for (let l = 0; l < LEVELS.length; l++) {
      for (const [w, h] of [
        [380, 700],
        [1600, 900],
      ] as const) {
        expect(scaleBar(levelPxPerM(l, w, h)).px).toBeGreaterThan(MAP.bigMap.scaleBarMaxPx / 2.5);
      }
    }
  });

  it('is a framed panel round most of a desktop, nearly the whole of a phone with bigger buttons', () => {
    const desk = bigMapPanel(1600, 1000);
    expect(desk.phone).toBe(false);
    expect(desk.marginX).toBe(Math.round((1600 * (1 - MAP.bigMap.panelFraction)) / 2));
    expect(desk.marginY).toBe(Math.round((1000 * (1 - MAP.bigMap.panelFraction)) / 2));
    for (const [w, h] of [
      [390, 844],
      [844, 390],
    ] as const) {
      const phone = bigMapPanel(w, h);
      expect(phone.phone).toBe(true);
      expect(phone.marginX).toBe(MAP.bigMap.phoneMarginPx);
      expect(phone.buttonPx).toBe(MAP.bigMap.phoneButtonPx);
      // Big enough to tap.
      expect(phone.buttonPx).toBeGreaterThanOrEqual(44);
    }
  });

  describe('the controller', () => {
    const idx = (name: string) => GAMEPAD.buttons.find((b) => b.name === name)?.index ?? -1;
    const buttons = (...names: string[]) => {
      const v = new Array<number>(16).fill(0);
      for (const n of names) v[idx(n)] = 1;
      return v;
    };
    const still = [0, 0, 0, 0];

    it('B or View closes, but not when it was held as the map opened', () => {
      const s = new BigMapState(pose(0, 0, 0));
      s.openAt();
      // View held from the press that opened it: no close until it lets go and comes again.
      expect(s.padFrame(buttons('view'), still, 0).closed).toBe(false);
      expect(s.padFrame(buttons(), still, 16).closed).toBe(false);
      expect(s.padFrame(buttons(MAP.bigMap.padClose), still, 32).closed).toBe(true);
      expect(s.open).toBe(false);
    });

    for (const face of [0, 1]) {
      it(`face ${String(face)}: the stick and D-pad pan the way pushed, a zoom button press steps a level`, () => {
        const s = new BigMapState(pose(face, 0, 0));
        s.openAt();
        s.padFrame(buttons(), still, 0);
        const before = s.view as BigView;
        // Stick right for 0.1 s: the view moves right on screen.
        s.padFrame(buttons(), [1, 0, 0, 0], 100);
        const v = s.view as BigView;
        const [sx, sy] = toScreen(before, v.cx, v.cz);
        expect(sx).toBeGreaterThan(0);
        expect(Math.abs(sy)).toBeLessThan(1e-9);
        // D-pad up: up the screen.
        const b2 = s.view as BigView;
        s.padFrame(buttons('up'), still, 200);
        const [, uy] = toScreen(b2, (s.view as BigView).cx, (s.view as BigView).cz);
        expect(uy).toBeLessThan(0);
        // A press of zoom-in steps one level in (held, no more); zoom-out one out.
        const l0 = (s.view as BigView).level;
        s.padFrame(buttons(MAP.bigMap.padZoomIn), still, 300);
        expect((s.view as BigView).level).toBe(l0 - 1);
        s.padFrame(buttons(MAP.bigMap.padZoomIn), still, 350);
        expect((s.view as BigView).level).toBe(l0 - 1);
        s.padFrame(buttons(), still, 360);
        s.padFrame(buttons(MAP.bigMap.padZoomOutAlt), still, 400);
        expect((s.view as BigView).level).toBe(l0);
        // Y: back on the player.
        s.padFrame(buttons(MAP.bigMap.padCenter), still, 410);
        expect(s.view).toMatchObject({ cx: 0, cz: 0 });
      });
    }

    it('a stick inside the dead zone does not pan', () => {
      const p = new PadMap();
      p.step(buttons(), still, 0);
      const s = p.step(buttons(), [GAMEPAD.tunables.deadzone * 0.9, 0, 0, 0], 100);
      expect(s.panX).toBe(0);
    });
  });
});

describe('the big map tinted by height', () => {
  for (const face of [0, 1]) {
    it(`face ${String(face)}: a tile's grid is in screen order, its border its neighbours' cells`, () => {
      const n = 4;
      const [ti, tk] = [-1, 2];
      // Each cell's colour names it, its height code its i.
      const g = tileGrid(
        face,
        ti,
        tk,
        n,
        (i, k) => ((i & 0xff) << 8) | (k & 0xff),
        (i) => i * 4,
      );
      const s = n + 2;
      expect(g.colours.length).toBe(s * s);
      expect(g.any).toBe(true);
      const v: BigView = { face, cx: 0, cz: 0, level: 2, pxPerM: 1 };
      const cell = MAP.explored.cellM;
      const [i0, k0] = tileCellAt(face, ti, tk, 0, 0, n);
      const [x0, y0] = toScreen(v, (i0 + 0.5) * cell, (k0 + 0.5) * cell);
      for (const [c, r] of [
        [0, 0],
        [n - 1, 0],
        [-1, 2],
        [n, n],
      ] as const) {
        const [i, k] = tileCellAt(face, ti, tk, c, r, n);
        expect(g.colours[(r + 1) * s + (c + 1)]).toBe(((i & 0xff) << 8) | (k & 0xff));
        expect(g.codes[(r + 1) * s + (c + 1)]).toBe(i * 4);
        // Where toScreen puts that cell: further right for a larger c, lower for a larger r.
        const [x1, y1] = toScreen(v, (i + 0.5) * cell, (k + 0.5) * cell);
        expect(Math.round(x1 - x0)).toBe(c * cell);
        expect(Math.round(y1 - y0)).toBe(r * cell);
      }
    });
  }

  it('fog and the world edge are nothing there; a tile with none of its own cells is empty', () => {
    const g = tileGrid(
      0,
      0,
      0,
      2,
      () => undefined,
      () => 0,
    );
    expect(g.any).toBe(false);
    expect(Array.from(g.colours).every((c) => c === 0xff000000)).toBe(true);
    expect(Array.from(g.codes).every((c) => c === NO_HEIGHT)).toBe(true);
  });

  it("a cell's change makes its own tile and the tiles it borders stale", () => {
    const key = (face: number, ti: number, tk: number) => RevealCache.key(8, face, ti, tk);
    expect(tilesShowing(8, 0, 5, 5, 32)).toEqual([key(0, 0, 0)]);
    expect(new Set(tilesShowing(8, 1, 0, 31, 32))).toEqual(
      new Set([key(1, -1, 0), key(1, 0, 0), key(1, -1, 1), key(1, 0, 1)]),
    );
  });

  it('heights are saved and loaded with the cells, unknown ones kept', () => {
    const cells = new Map([[cellKey(0, 3, 4), 0x445566]]);
    const heights = new Map([[cellKey(0, 3, 4), 168]]);
    const stored = { cellM: MAP.explored.cellM, cells: {}, heights: { '1,9,9': 4 } };
    const record = exploredRecord(stored, cells, heights);
    const back = parseHeights(JSON.parse(JSON.stringify(record)));
    expect(back.get(cellKey(0, 3, 4))).toBe(168);
    expect(back.get('1,9,9')).toBe(4);
    expect(parseHeights({ cellM: MAP.explored.cellM + 1, heights: { '0,1,1': 2 } }).size).toBe(0);
  });

  it("a revealed tile's heights follow its colours", () => {
    const n = 2;
    const b = new Uint8Array(13 + n * n * 6);
    const d = new DataView(b.buffer);
    d.setInt32(9, 8, true);
    for (let j = 0; j < n * n; j++) {
      b[13 + j * 4 + 3] = 255;
      d.setInt16(13 + n * n * 4 + j * 2, j === 3 ? -32768 : 10 * j + 1, true);
    }
    const t = parseRevealTile(b, n);
    expect(Array.from(t.heights ?? [])).toEqual([1, 11, 21, -32768]);
    expect(t.rgba?.length).toBe(n * n * 4);
    const cache = new RevealCache(n, 1, 1);
    cache.tiles.set(RevealCache.key(8, 0, 0, 0), t);
    expect(cache.height(8, 0, 1, 0)).toBe(11);
    expect(cache.height(8, 0, 1, 1)).toBe(NO_HEIGHT);
    expect(cache.height(8, 0, 5, 5)).toBe(NO_HEIGHT);
    // Another level's cell is another tile.
    expect(cache.height(2, 0, 1, 0)).toBe(NO_HEIGHT);
  });
});

describe("the closest levels' real blocks (owner, 2026-11-09: closest zooms use 1 m)", () => {
  /** A client whose loaded terrain covers tiles with ti ≥ 0 only, the left half of each. */
  const liveClient = (n: number) => {
    const reads: string[] = [];
    let mark = 0;
    return {
      reads,
      place() {
        mark++;
      },
      liveTile(face: number, ti: number, tk: number, cellM: number) {
        reads.push(RevealCache.key(cellM, face, ti, tk));
        if (ti < 0) return undefined;
        const b = tileBytes(n, face, ti, tk, cellM);
        for (let k = 0; k < n; k++) {
          for (let i = 0; i < n; i++) {
            const o = 13 + (k * n + i) * 4;
            if (i >= n / 2) b[o + 3] = 0;
            else b[o + 2] = 200 + mark;
          }
        }
        return b;
      },
    };
  };

  it('the 1 m level is read from the terrain, the coarse ones never', () => {
    expect(isLive(LEVELS[0]?.cellM ?? 0)).toBe(true);
    for (const { cellM } of LEVELS.slice(1)) expect(isLive(cellM)).toBe(false);
  });

  it("a loaded cell shows the terrain's block, an unloaded one the generator's, within the budget", () => {
    const n = 8;
    const cache = new RevealCache(n, 2, 1000, 0);
    const gen = fakeClient(n);
    const live = liveClient(n);
    for (const face of [0, 1]) {
      const v: BigView = { face, cx: 0, cz: 0, level: 0, pxPerM: 8 };
      cache.pump(gen, v, 800, 600, () => 0);
      for (let f = 0; f < 10; f++) cache.pump(gen, v, 800, 600, () => 0);
      live.reads.length = 0;
      const changed = cache.pumpLive(live, v, 800, 600, 0, 2, 1000);
      // At most the budget's tiles a frame.
      expect(live.reads.length).toBe(2);
      expect(changed.length).toBeLessThanOrEqual(2);
      // Every view tile is read within a few frames (nearest first), not again until refreshMs.
      for (let f = 1; f < 100; f++) cache.pumpLive(live, v, 800, 600, f, 2, 1000);
      const all = cache.tilesFor(v, 800, 600).length;
      expect(new Set(live.reads).size).toBe(all);
      expect(live.reads.length).toBe(all);
      // Tile (0, 0)'s left half is the terrain's, its right half the generator's.
      expect((cache.colour(1, face, 1, 1) ?? 0) & 0xff).toBe(200);
      expect((cache.colour(1, face, n - 1, 1) ?? 0) & 0xff).toBe(face);
      // A tile with nothing loaded keeps the generator's.
      expect((cache.colour(1, face, -1, 1) ?? 0) & 0xff).toBe(face);
    }
  });

  it('an edit shows once the tile is read again, and a coarse level is never read', () => {
    const n = 8;
    const cache = new RevealCache(n, 2, 1000, 0);
    const live = liveClient(n);
    const v: BigView = { face: 0, cx: n / 2, cz: n / 2, level: 0, pxPerM: 200 };
    expect(cache.pumpLive(live, v, 10, 10, 0, 1, 1000).length).toBe(1);
    expect((cache.colour(1, 0, 1, 1) ?? 0) & 0xff).toBe(200);
    live.place();
    // Not yet: the refresh is a second.
    expect(cache.pumpLive(live, v, 10, 10, 500, 1, 1000)).toEqual([]);
    expect(cache.pumpLive(live, v, 10, 10, 1000, 1, 1000)).toEqual([[1, 0, 0, 0]]);
    expect((cache.colour(1, 0, 1, 1) ?? 0) & 0xff).toBe(201);
    // Read again unchanged: nothing to redraw.
    expect(cache.pumpLive(live, v, 10, 10, 2000, 1, 1000)).toEqual([]);
    live.reads.length = 0;
    const far: BigView = { ...v, level: 2 };
    expect(cache.pumpLive(live, far, 800, 600, 5000, 2, 1000)).toEqual([]);
    expect(live.reads).toEqual([]);
  });
});

describe('the minimap and the open map keep building (owner, 2026-11-10)', () => {
  for (const face of [0, 1]) {
    it(`face ${String(face)}: the minimap's 1 m tiles are built first and handed over, kept ones at once`, () => {
      const cache = new RevealCache(32, 2, 1000, 1);
      const client = fakeClient(32);
      // The client wants four 1 m tiles until it is given them.
      const wanted = new Set(['0,0', '-1,0', '0,-1', '-1,-1']);
      const given: string[] = [];
      const feed = {
        ...client,
        minimapWants: () =>
          Int32Array.from(
            [...wanted].flatMap((s) => [face, ...s.split(',').map(Number)] as number[]),
          ),
        minimapTile(f: number, ti: number, tk: number, rgba: Uint8Array, heights: Int16Array) {
          expect(f).toBe(face);
          expect(rgba.length).toBe(32 * 32 * 4);
          expect(heights.length).toBe(32 * 32);
          wanted.delete(`${String(ti)},${String(tk)}`);
          given.push(`${String(ti)},${String(tk)}`);
        },
      };
      // Asked before the background (which waits on the pending ones), at the closest level.
      cache.feedMinimap(feed);
      expect(client.asked.length).toBe(2);
      expect(client.asked.every((k) => k.startsWith(`${String(levelCellM(0))}|`))).toBe(true);
      for (let f = 0; f < 6 && wanted.size > 0; f++) {
        cache.pump(feed, null, 0, 0, () => 0, undefined, [face, 0, 0]);
        cache.feedMinimap(feed);
      }
      expect(wanted.size).toBe(0);
      expect(given.length).toBe(4);
      // A tile already kept (from the store or the big map) is handed over without a build.
      const before = client.asked.length;
      wanted.add('0,0');
      cache.feedMinimap(feed);
      expect(wanted.size).toBe(0);
      expect(client.asked.length).toBe(before);
    });
  }

  it("with the map open every level's tiles round the view and the player get built until none remain", () => {
    const cache = new RevealCache(32, 2, 1000, 1);
    const client = fakeClient(32);
    // Panned away from the player, at the open level 1.
    const [w, h] = [800, 600];
    const v: BigView = { face: 0, cx: 900, cz: -700, level: 1, pxPerM: levelPxPerM(1, w, h) };
    const pose = [0, 0, 0];
    const visible = cache.tilesFor(v, w, h).map(([c, f, i, k]) => RevealCache.key(c, f, i, k));
    const inRange = cache.backgroundFor(0, 0, 0, [0, v.cx, v.cz]);
    let frames = 0;
    for (; frames < 40000; frames++) {
      cache.pump(client, v, w, h, () => 0, undefined, pose);
      if (frames > 0 && cache.backgroundLeft === 0 && cache.inFlight === 0) break;
    }
    // Over many frames, a few jobs at a time.
    expect(frames).toBeGreaterThan(50);
    for (const [c, f, i, k] of inRange) {
      expect(
        cache.tiles.has(RevealCache.key(c, f, i, k)),
        `${String(c)} m tile ${String(i)},${String(k)}`,
      ).toBe(true);
    }
    // Every level.
    const levels = new Set([...cache.tiles.values()].map((t) => t.cellM));
    expect([...levels].sort((a, b) => a - b)).toEqual(LEVELS.map((l) => l.cellM));
    // The view's tiles were all asked for before anything else.
    expect(client.asked.slice(0, visible.length).sort()).toEqual([...visible].sort());
    // Then the 1 m area round the player, then the whole disc, then the island, nearest the view.
    const after = client.asked.slice(visible.length);
    const firstOf = (cellM: number) => after.findIndex((k) => k.startsWith(`${String(cellM)}|`));
    const [near, mid, wide] = LEVELS.map((l) => l.cellM);
    expect(
      after[0]?.startsWith(`${String(near)}|0,0,`) || after[0] === `${String(near)}|0,-1,-1`,
    ).toBe(true);
    expect(firstOf(near ?? 0)).toBeLessThan(firstOf(wide ?? 0));
    expect(firstOf(wide ?? 0)).toBeLessThan(firstOf(mid ?? 0));
    const midAsked = after.filter((k) => k.startsWith(`${String(mid)}|`));
    const span = 32 * (mid ?? 1);
    const dist = (k: string) => {
      const [, , i = 0, kk = 0] = k.split(/[|,]/).map(Number);
      return Math.hypot((i + 0.5) * span - v.cx, (kk + 0.5) * span - v.cz);
    };
    // The island's are nearest the view's centre first (the visible ones were built already).
    for (let n = 1; n < midAsked.length; n++) {
      expect(dist(midAsked[n] ?? '')).toBeGreaterThanOrEqual(
        dist(midAsked[n - 1] ?? '') - span * 6,
      );
    }
    expect(dist(midAsked[0] ?? '')).toBeLessThan(dist(midAsked[midAsked.length - 1] ?? ''));
  });

  it('panning re-centres the island build on the view; shut, it carries on round the player a job at a time', () => {
    const cache = new RevealCache(32, 2, 1000, 1);
    const client = fakeClient(32);
    const [w, h] = [800, 600];
    const mid = LEVELS[1]?.cellM ?? 16;
    const span = 32 * mid;
    const pose = [0, 0, 0];
    const run = (v: BigView | null, frames: number) => {
      const from = client.asked.length;
      for (let f = 0; f < frames; f++) cache.pump(client, v, w, h, () => 0, undefined, pose);
      return client.asked.slice(from).filter((k) => k.startsWith(`${String(mid)}|`));
    };
    const at = (k: string) => k.split(/[|,]/).map(Number).slice(2) as [number, number];
    const view = (cx: number, cz: number): BigView => ({
      face: 0,
      cx,
      cz,
      level: 0,
      pxPerM: levelPxPerM(0, w, h),
    });
    // Open at the 1 m level well away: once the near tiles are done, the island's go nearest it.
    run(view(10_000, 0), 200);
    const east = run(view(10_000, 0), 50);
    expect(east.length).toBeGreaterThan(0);
    for (const k of east) expect(Math.abs(at(k)[0] * span - 10_000)).toBeLessThan(15 * span);
    // Panned to the far side: the next ones are there.
    run(view(-10_000, 0), 20);
    const west = run(view(-10_000, 0), 50);
    expect(west.length).toBeGreaterThan(0);
    for (const k of west) expect(Math.abs(at(k)[0] * span + 10_000)).toBeLessThan(15 * span);
    // Shut: one job at a time (backgroundJobs), round the player.
    const held: Uint8Array[] = [];
    const slow = {
      requestReveal: (face: number, ti: number, tk: number, cellM: number) => {
        client.requestReveal(face, ti, tk, cellM);
        const done = client.takeReveal();
        if (done) held.push(done);
      },
      takeReveal: () => held.shift() ?? client.takeReveal(),
    };
    const from = client.asked.length;
    for (let f = 0; f < 30; f++) {
      cache.pump(slow, null, 0, 0, () => 0, undefined, pose);
      expect(cache.inFlight).toBeLessThanOrEqual(1);
    }
    const shut = client.asked.slice(from).filter((k) => k.startsWith(`${String(mid)}|`));
    expect(shut.length).toBeGreaterThan(0);
    for (const k of shut) expect(Math.hypot(...at(k)) * span).toBeLessThan(15 * span);
  });
});
