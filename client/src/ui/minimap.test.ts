import { describe, expect, it } from 'vitest';
import { MAP } from './map.gen';
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
  zoomRange,
  cellColour,
  parseRevealTile,
  RevealCache,
  tileOnScreen,
  type BigView,
  type ExploredCells,
} from './minimap';
import { defaultHud, hudRecord, sanitizeHud } from './hudSettings';

const view = (face: number): BigView => ({ face, cx: 100, cz: -40, pxPerM: 0.75 });

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

    it(`face ${String(face)}: zooming keeps the point under the pointer and stays in range`, () => {
      const v = view(face);
      const at: [number, number] = [120, -35];
      const before = toWorld(v, ...at);
      const z = zoomAt(v, MAP.bigMap.zoomStep, ...at);
      expect(z.pxPerM).toBeCloseTo(v.pxPerM * MAP.bigMap.zoomStep, 9);
      const after = toWorld(z, ...at);
      expect(after[0]).toBeCloseTo(before[0], 9);
      expect(after[1]).toBeCloseTo(before[1], 9);
      const [lo, hi] = zoomRange();
      let w = v;
      for (let n = 0; n < 100; n++) w = zoomAt(w, MAP.bigMap.zoomStep, 0, 0);
      expect(w.pxPerM).toBeCloseTo(hi, 9);
      for (let n = 0; n < 200; n++) w = zoomAt(w, 1 / MAP.bigMap.zoomStep, 0, 0);
      expect(w.pxPerM).toBeCloseTo(lo, 9);
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
    expect(addExplored(cells, [0, 1, -2, 0x336699, 1, 1, -2, 0x112233])).toBe(true);
    expect(cells.get(cellKey(0, 1, -2))).toBe(0x336699);
    expect(cells.get(cellKey(1, 1, -2))).toBe(0x112233);
    expect(addExplored(cells, [0, 1, -2, 0x336699])).toBe(false);
    expect(addExplored(cells, [0, 1, -2, 0x336600])).toBe(true);
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

describe('the revealed big map (mapRevealed, creative)', () => {
  /** A client whose workers finish every tile asked for: each cell coloured from its indices. */
  const fakeClient = (n: number, edgeAt = Infinity) => {
    const done: Uint8Array[] = [];
    const asked: string[] = [];
    return {
      asked,
      requestReveal(face: number, ti: number, tk: number) {
        asked.push(`${String(face)},${String(ti)},${String(tk)}`);
        const b = new Uint8Array(9 + n * n * 4);
        const d = new DataView(b.buffer);
        b[0] = face;
        d.setInt32(1, ti, true);
        d.setInt32(5, tk, true);
        for (let k = 0; k < n; k++) {
          for (let i = 0; i < n; i++) {
            const o = 9 + (k * n + i) * 4;
            b[o] = (ti * n + i) & 0xff;
            b[o + 1] = (tk * n + k) & 0xff;
            b[o + 2] = face;
            b[o + 3] = Math.abs(ti * n + i) * MAP.explored.cellM > edgeAt ? 0 : 255;
          }
        }
        done.push(b);
      },
      takeReveal: () => done.shift(),
    };
  };
  const cellsIn = (v: BigView, w: number, h: number) => {
    const c = MAP.explored.cellM;
    const [ax, az] = toWorld(v, -w / 2, -h / 2);
    const [bx, bz] = toWorld(v, w / 2, h / 2);
    const out: [number, number][] = [];
    for (let k = Math.floor(Math.min(az, bz) / c); k <= Math.floor(Math.max(az, bz) / c); k++) {
      for (let i = Math.floor(Math.min(ax, bx) / c); i <= Math.floor(Math.max(ax, bx) / c); i++) {
        out.push([i, k]);
      }
    }
    return out;
  };

  for (const face of [0, 1]) {
    it(`face ${String(face)}: zoomed out over km, every cell in view resolves to a colour; adventure keeps the fog`, () => {
      // The widest zoom: a cell a pixel, 800 × 600 px = 6.4 × 4.8 km.
      const v: BigView = { face, cx: 300, cz: -900, pxPerM: zoomRange()[0] };
      const [w, h] = [800, 600];
      const cache = new RevealCache();
      const client = fakeClient(cache.tileCells);
      const explored: ExploredCells = new Map([[cellKey(face, 40, -110), 0xabcdef]]);
      let clock = 0;
      const all = cache.tilesFor(v, w, h).length;
      for (let frame = 0; frame < 2000 && cache.tiles.size < all; frame++) {
        cache.pump(client, v, w, h, () => clock++);
      }
      const cells = cellsIn(v, w, h);
      expect(cells.length).toBeGreaterThan(400_000);
      for (const [i, k] of cells) {
        const c = cellColour(explored, cache, face, i, k);
        if (typeof c !== 'number') throw new Error(`cell (${String(i)}, ${String(k)}) is fogged`);
      }
      // An explored cell keeps the minimap's colour; another the generator's, on its own face.
      expect(cellColour(explored, cache, face, 40, -110)).toBe(0xabcdef);
      expect(cellColour(explored, cache, face, 41, -110)).toBe(
        (41 << 16) | ((-110 & 0xff) << 8) | face,
      );
      expect(cellColour(explored, cache, 1 - face, 41, -110)).toBeUndefined();
      // Adventure (no reveal): the unexplored cells stay fogged.
      for (const [i, k] of cells.slice(0, 5000)) {
        if (i === 40 && k === -110) continue;
        expect(cellColour(explored, null, face, i, k)).toBeUndefined();
      }
      expect(cellColour(explored, null, face, 40, -110)).toBe(0xabcdef);
    });
  }

  it('beyond the world edge a cell resolves to nothing there (null), not fog', () => {
    const cache = new RevealCache();
    const client = fakeClient(cache.tileCells, 100);
    const v: BigView = { face: 0, cx: 0, cz: 0, pxPerM: 1 };
    for (let f = 0; f < 50; f++) cache.pump(client, v, 400, 400, () => 0);
    expect(cache.colour(0, 0, 0)).toBeTypeOf('number');
    expect(cache.colour(0, 20, 0)).toBeNull();
  });

  it('the fill stays under its per-frame budget and its jobs in flight, nearest tiles first', () => {
    const cache = new RevealCache(32, 2, 2);
    const client = fakeClient(32);
    const v: BigView = { face: 0, cx: 1000, cz: 1000, pxPerM: zoomRange()[0] };
    // Many finished tiles waiting (another view's): each costs 1 ms on this clock.
    for (let t = 0; t < 40; t++) client.requestReveal(0, 100 + t, 0);
    let clock = 0;
    const taken = cache.pump(client, v, 800, 600, () => clock++);
    expect(taken).toBeGreaterThan(0);
    expect(taken).toBeLessThanOrEqual(cache.budgetMs);
    expect(cache.inFlight).toBeLessThanOrEqual(2);
    // The first tiles asked for are the view centre's.
    const centre = Math.floor(1000 / (32 * MAP.explored.cellM));
    expect(client.asked.slice(40, 42)).toContain(`0,${String(centre)},${String(centre)}`);
    for (let f = 0; f < 30; f++) {
      const t0 = clock;
      const n = cache.pump(client, v, 800, 600, () => clock++);
      expect(n).toBeLessThanOrEqual(cache.budgetMs);
      expect(clock - t0).toBeLessThanOrEqual(cache.budgetMs + 2);
      expect(cache.inFlight).toBeLessThanOrEqual(2);
    }
  });

  it('a tile from the client parses its face, place and pixels', () => {
    const b = new Uint8Array([1, 0xfd, 0xff, 0xff, 0xff, 7, 0, 0, 0, 9, 8, 7, 255]);
    const t = parseRevealTile(b);
    expect([t.face, t.ti, t.tk]).toEqual([1, -3, 7]);
    expect(Array.from(t.rgba ?? [])).toEqual([9, 8, 7, 255]);
    expect(parseRevealTile(b.subarray(0, 9)).rgba).toBeNull();
  });
});

describe('a revealed tile on screen', () => {
  for (const face of [0, 1]) {
    it(`face ${String(face)}: each texel lands where its cell is on the map`, () => {
      const n = 4;
      const rgba = new Uint8Array(n * n * 4);
      for (let b = 0; b < n; b++) for (let a = 0; a < n; a++) rgba[(b * n + a) * 4] = b * n + a;
      const img = tileOnScreen(rgba, n, face);
      const v: BigView = { face, cx: 0, cz: 0, pxPerM: 1 };
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
