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

describe('the big map panel (owner, 2026-11-09)', () => {
  const pose = (face: number, x: number, z: number) => () => [face, x, z, 0];

  it('opens on the player, closes, and its zoom buttons stop at the zoom range', () => {
    const s = new BigMapState(pose(0, 12, -30));
    expect(s.open).toBe(false);
    s.openAt();
    expect(s.open).toBe(true);
    expect(s.view).toMatchObject({ face: 0, cx: 12, cz: -30 });
    expect(s.view?.pxPerM).toBeCloseTo(MAP.bigMap.startPxPerCell / MAP.explored.cellM);
    const [lo, hi] = zoomRange();
    for (let n = 0; n < 200; n++) s.zoomStep(1);
    expect(s.view?.pxPerM).toBeCloseTo(hi);
    expect(s.canZoom(1)).toBe(false);
    expect(s.canZoom(-1)).toBe(true);
    for (let n = 0; n < 200; n++) s.zoomStep(-1);
    expect(s.view?.pxPerM).toBeCloseTo(lo);
    expect(s.canZoom(-1)).toBe(false);
    // The buttons zoom about the centre: the place in the middle stays put.
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
    // Every zoom the data allows has a bar.
    const [lo, hi] = zoomRange();
    for (const z of [lo, hi])
      expect(scaleBar(z).px).toBeGreaterThan(MAP.bigMap.scaleBarMaxPx / 2.5);
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
      it(`face ${String(face)}: the stick and D-pad pan the way pushed, the zoom buttons zoom`, () => {
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
        // A held zoom-in zooms in; zoom-out out.
        const z0 = (s.view as BigView).pxPerM;
        s.padFrame(buttons(MAP.bigMap.padZoomIn), still, 300);
        expect((s.view as BigView).pxPerM).toBeGreaterThan(z0);
        const z1 = (s.view as BigView).pxPerM;
        s.padFrame(buttons(MAP.bigMap.padZoomOutAlt), still, 400);
        expect((s.view as BigView).pxPerM).toBeLessThan(z1);
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

describe('the big map elevation shading', () => {
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
      const v: BigView = { face, cx: 0, cz: 0, pxPerM: 1 };
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
    expect(tilesShowing(0, 5, 5, 32)).toEqual([cellKey(0, 0, 0)]);
    expect(new Set(tilesShowing(1, 0, 31, 32))).toEqual(
      new Set([cellKey(1, -1, 0), cellKey(1, 0, 0), cellKey(1, -1, 1), cellKey(1, 0, 1)]),
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
    const b = new Uint8Array(9 + n * n * 6);
    const d = new DataView(b.buffer);
    for (let j = 0; j < n * n; j++) {
      b[9 + j * 4 + 3] = 255;
      d.setInt16(9 + n * n * 4 + j * 2, j === 3 ? -32768 : 10 * j + 1, true);
    }
    const t = parseRevealTile(b, n);
    expect(Array.from(t.heights ?? [])).toEqual([1, 11, 21, -32768]);
    expect(t.rgba?.length).toBe(n * n * 4);
    const cache = new RevealCache(n, 1, 1);
    cache.tiles.set(RevealCache.key(0, 0, 0), t);
    expect(cache.height(0, 1, 0)).toBe(11);
    expect(cache.height(0, 1, 1)).toBe(NO_HEIGHT);
    expect(cache.height(0, 5, 5)).toBe(NO_HEIGHT);
  });
});
