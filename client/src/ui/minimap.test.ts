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
  type BigView,
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
  it('is compact on a short screen (a phone sideways), full otherwise', () => {
    expect(minimapSize(342)).toBe(MAP.minimap.compactSizePx);
    expect(minimapSize(900)).toBe(MAP.minimap.sizePx);
    // On a phone sideways (750 × 342) it stays clear of the view at (600, 150), where the touch
    // test taps to place: right edge 12 px in, top 12 px.
    const r = MAP.minimap.compactSizePx / 2;
    const [cx, cy] = [750 - 12 - r, 12 + r];
    expect(Math.hypot(600 - cx, 150 - cy)).toBeGreaterThan(r);
  });
});
