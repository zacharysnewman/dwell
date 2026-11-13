// Map markers (docs/MAP_MARKERS.md): the record against the protocol's golden vector, the list's
// rules (place, edit, move, delete, the limits, the auto kinds), the picker's draft, the minimap's
// geometry on both faces, and the waypoint's place on the screen.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  type MapMarker,
  MarkerList,
  clampLabel,
  decodeMarkers,
  encodeMarkers,
  iconOf,
  markersInRange,
  minimapOffset,
  paletteRgb,
  pickMarker,
  waypointAlpha,
  waypointOnScreen,
  waypointShown,
  bigMapPins,
  directionWord,
  heightOfCode,
  markerGroundY,
  markerRows,
  seenPortalSpots,
} from './markers';
import { toScreen } from './minimap';
import { AUTO_MARKERS, MARKER_ICONS, MARKER_LIMITS, MARKER_WORLD } from './markers.gen';
import { PickerDraft } from './markerPicker';
import { MessageType } from '../protocol/constants.gen';

const vectors = new Map(
  readFileSync(new URL('../../../shared/protocol/vectors.txt', import.meta.url), 'utf8')
    .split('\n')
    .filter((l) => l && !l.startsWith('#'))
    .map((l) => {
      const [name = '', hex = ''] = l.split(' ');
      return [name, Uint8Array.from(hex.match(/../g) ?? [], (h) => parseInt(h, 16))];
    }),
);

const golden: MapMarker[] = [
  { id: 1, icon: 'home', rgb: '#e8322a', label: 'Base', face: 0, x: 12.5, y: 70, z: -3, auto: '' },
  {
    id: 7,
    icon: 'skull',
    rgb: '#f4f1ea',
    label: 'Last death',
    face: 1,
    x: -40,
    y: 66.5,
    z: 900.25,
    auto: 'death',
  },
];

describe('the markers record', () => {
  it('is the protocol golden vector, byte for byte (the Rust codec reads the same)', () => {
    const v = vectors.get('markers');
    expect(v?.[0]).toBe(MessageType.Markers);
    expect(encodeMarkers(golden)).toEqual(v?.subarray(1));
    expect(decodeMarkers(v?.subarray(1) ?? new Uint8Array())).toEqual(golden);
    expect(decodeMarkers(vectors.get('markers_none')?.subarray(1) ?? new Uint8Array())).toEqual([]);
  });

  it('refuses the malformed vectors', () => {
    for (const name of [
      '!markers_bad_version',
      '!markers_truncated',
      '!markers_id_twice',
      '!markers_bad_face',
    ]) {
      expect(decodeMarkers(vectors.get(name)?.subarray(1) ?? new Uint8Array([9])), name).toBeNull();
    }
  });

  it('keeps an icon name this build does not know, and draws it as the fallback', () => {
    const list = [{ ...golden[0], icon: 'mymod:anchor' } as MapMarker];
    expect(decodeMarkers(encodeMarkers(list))?.[0]?.icon).toBe('mymod:anchor');
    expect(iconOf('mymod:anchor').id).toBe('question');
  });
});

const spot = (face: number, x: number, z: number) => ({ face, x, y: 64, z });

describe('the marker list', () => {
  it('places, edits, moves and deletes, telling each change', () => {
    const l = new MarkerList();
    let heard = 0;
    l.onChange = () => heard++;
    const id = l.place({ icon: 'flag', rgb: '#2f7df6', label: 'Camp one' }, spot(0, 10, 20));
    expect(id).not.toBeNull();
    const n = id ?? -1;
    expect(l.get(n)).toMatchObject({ icon: 'flag', label: 'Camp one', face: 0, x: 10, z: 20 });
    l.edit(n, { label: 'Camp two', icon: 'camp' });
    expect(l.get(n)).toMatchObject({ icon: 'camp', label: 'Camp two', rgb: '#2f7df6' });
    l.move(n, spot(1, -5, 7));
    expect(l.get(n)).toMatchObject({ face: 1, x: -5, z: 7 });
    expect(l.remove(n)).toBe(true);
    expect(l.all).toHaveLength(0);
    expect(heard).toBe(4);
  });

  it('cuts a label to the limit in whole characters and refuses past the most markers', () => {
    expect(Array.from(clampLabel('é'.repeat(40)))).toHaveLength(MARKER_LIMITS.labelChars);
    expect(clampLabel('  a\u0007b  ')).toBe('ab');
    const l = new MarkerList();
    for (let i = 0; i < MARKER_LIMITS.maxMarkers; i++)
      l.place({ icon: 'star', rgb: '#ffffff', label: '' }, spot(0, i, 0));
    expect(l.full).toBe(true);
    expect(l.place({ icon: 'star', rgb: '#ffffff', label: '' }, spot(0, 0, 0))).toBeNull();
    expect(decodeMarkers(encodeMarkers(l.all))).toHaveLength(MARKER_LIMITS.maxMarkers);
  });

  it('keeps one spawn and one last death, moved to the newest; one portal each', () => {
    const l = new MarkerList();
    const death = AUTO_MARKERS.find((a) => a.kind === 'death');
    const a = l.noteAuto('death', spot(0, 1, 1));
    const b = l.noteAuto('death', spot(1, 5, 5));
    expect(a).toBe(b);
    expect(l.all.filter((m) => m.auto === 'death')).toHaveLength(1);
    expect(l.get(a ?? -1)).toMatchObject({ face: 1, x: 5, label: death?.label, icon: death?.icon });
    expect(l.get(a ?? -1)?.rgb).toBe(paletteRgb(death?.colour ?? ''));
    l.noteAuto('portal', spot(0, 0, 0));
    l.noteAuto('portal', spot(0, 0.5, 0)); // the same portal seen again
    l.noteAuto('portal', spot(0, 100, 0));
    // The same portal at another height (the map's, then the loaded rim's): still the one.
    l.noteAuto('portal', { face: 0, x: 100.5, y: 71, z: 0 });
    // The other face's opening of a portal is its own.
    l.noteAuto('portal', spot(1, 100, 0));
    expect(l.all.filter((m) => m.auto === 'portal')).toHaveLength(3);
    expect(l.noteAuto('nonsense', spot(0, 0, 0))).toBeNull();
  });

  it('places no auto marker of a kind the data turns off (a kind by data alone)', () => {
    const l = new MarkerList([
      { kind: 'campfire', on: true, keep: 'each', icon: 'camp', colour: 'orange', label: 'Fire' },
      { kind: 'spawn', on: false, keep: 'one', icon: 'home', colour: 'yellow', label: 'Spawn' },
    ]);
    expect(l.noteAuto('spawn', spot(0, 0, 0))).toBeNull();
    expect(l.get(l.noteAuto('campfire', spot(0, 3, 3)) ?? -1)).toMatchObject({
      label: 'Fire',
      icon: 'camp',
    });
  });

  it('makes an edited auto marker the player own', () => {
    const l = new MarkerList();
    const id = l.noteAuto('spawn', spot(0, 0, 0)) ?? -1;
    l.edit(id, { label: 'Home' });
    expect(l.get(id)?.auto).toBe('');
    // A later spawn note places a new spawn marker, the edited one kept.
    l.noteAuto('spawn', spot(0, 9, 9));
    expect(l.all).toHaveLength(2);
  });

  it('loads the server record and continues its ids', () => {
    const l = new MarkerList();
    l.load(golden);
    expect(l.all).toEqual(golden);
    expect(l.place({ icon: 'eye', rgb: '#ffffff', label: '' }, spot(0, 0, 0))).toBe(8);
  });
});

describe('the picker', () => {
  it('takes an icon colour until a colour is picked by hand, and names an unlabelled marker', () => {
    const d = new PickerDraft();
    d.pickIcon('water');
    expect(d.rgb).toBe(paletteRgb('blue'));
    d.pickColour('#e8322a');
    d.pickIcon('tree');
    expect(d.rgb).toBe('#e8322a');
    expect(d.look()).toEqual({ icon: 'tree', rgb: '#e8322a', label: 'Tree' });
    d.setLabel('x'.repeat(99));
    expect(d.look().label).toHaveLength(MARKER_LIMITS.labelChars);
  });

  it('editing keeps the marker colour when its icon changes', () => {
    const d = new PickerDraft({ icon: 'flag', rgb: '#4caf50', label: 'Here' });
    d.pickIcon('skull');
    expect(d.look()).toEqual({ icon: 'skull', rgb: '#4caf50', label: 'Here' });
  });

  it('every icon is vector path data, never a glyph', () => {
    expect(MARKER_ICONS.length).toBeGreaterThanOrEqual(16);
    for (const i of MARKER_ICONS) expect(i.path).toMatch(/^M[\d\s.MLHVCQAZ-]+$/);
  });
});

describe('the minimap and the big map', () => {
  // both-faces: compares the faces' mirrored frames.
  it('puts a marker ahead up, and to the player right on each face (face B mirrored)', () => {
    for (const face of [0, 1]) {
      const [r0, u0] = minimapOffset(face, 0, 0, 0, 0, 10);
      expect([Math.abs(r0) < 1e-9, u0]).toEqual([true, 10]);
      // A yaw-0 player's right: face-local −x on face A, +x on face B (`MapFrame::new`'s handed).
      const [r] = minimapOffset(face, 0, 0, 0, face === 1 ? 5 : -5, 0);
      expect(r).toBeCloseTo(5);
      // Turned to face +x (yaw 90): +x is ahead on both faces.
      const [rr, uu] = minimapOffset(face, 0, 0, 90, 7, 0);
      expect([rr, uu].map((v) => Math.round(v) + 0)).toEqual([0, 7]);
    }
  });

  it('shows only the markers of the player face within the disc', () => {
    const list: MapMarker[] = [
      { ...spot(0, 10, 0), id: 1, icon: 'star', rgb: '#ffffff', label: '', auto: '' },
      { ...spot(1, 10, 0), id: 2, icon: 'star', rgb: '#ffffff', label: '', auto: '' },
      { ...spot(0, 500, 0), id: 3, icon: 'star', rgb: '#ffffff', label: '', auto: '' },
    ];
    const near = markersInRange(list, { face: 0, x: 0, z: 0, yaw: 0 }, 64);
    expect(near.map((n) => n.marker.id)).toEqual([1]);
  });

  it('picks the nearest marker under a click', () => {
    const list = [1, 2].map((id) => ({ ...golden[0], id }) as MapMarker);
    const at = (m: MapMarker): [number, number] => (m.id === 1 ? [100, 100] : [108, 100]);
    expect(pickMarker(list, at, 107, 101)?.id).toBe(2);
    expect(pickMarker(list, at, 300, 300)).toBeNull();
  });
});

describe('the world waypoint', () => {
  it('fades with distance', () => {
    expect(waypointAlpha(0)).toBe(1);
    expect(waypointAlpha(MARKER_WORLD.farM * 2)).toBe(MARKER_WORLD.fadedAlpha);
    const mid = waypointAlpha((MARKER_WORLD.nearM + MARKER_WORLD.farM) / 2);
    expect(mid).toBeLessThan(1);
    expect(mid).toBeGreaterThan(MARKER_WORLD.fadedAlpha);
  });

  it('sits on its spot on screen, and clamps to the edge off screen or behind', () => {
    expect(waypointOnScreen(0.5, 0.5, 10, 800, 600)).toEqual({ x: 600, y: 150, onScreen: true });
    const right = waypointOnScreen(3, 0, 10, 800, 600, 20);
    expect(right).toEqual({ x: 780, y: 300, onScreen: false });
    const behind = waypointOnScreen(0, 0, -5, 800, 600, 20);
    expect(behind.onScreen).toBe(false);
    expect(behind.y).toBe(580);
    const behindLeft = waypointOnScreen(-0.2, 0.1, -5, 800, 600, 20);
    expect(behindLeft.x).toBeLessThan(400);
    expect(behindLeft.y).toBeGreaterThanOrEqual(300);
  });
});

// BUGS.md #116: standing on the spawn, its waypoint read "1 m" pinned to the top of the screen.
describe('a waypoint at or under the camera', () => {
  it('is hidden within hideWithinM, from the eye or across the ground', () => {
    const near = MARKER_WORLD.hideWithinM;
    expect(near).toBeGreaterThan(0);
    // On the spawn: the spot 1.5 m over the feet is about the eye.
    expect(waypointShown(0.9, 0.2)).toBe(false);
    // Straight below a cliff top: far by the eye, but under the camera.
    expect(waypointShown(40, near * 0.5)).toBe(false);
    expect(waypointShown(near + 10, near + 5)).toBe(true);
  });
});

// BUGS.md #117: the picker open for a new marker drew nothing at the clicked spot.
describe('the big map pins', () => {
  const placed = {
    id: 1,
    icon: 'star',
    rgb: '#ff0000',
    label: 'a',
    face: 0,
    x: 1,
    y: 0,
    z: 2,
    auto: '',
  };
  it('include a pending pin at the clicked spot while the picker is open for it', () => {
    const pending = { face: 0, x: 50, z: -20, icon: 'flag', rgb: '#00ff00' };
    const pins = bigMapPins([placed], 0, pending);
    expect(pins).toContainEqual({ icon: 'flag', rgb: '#00ff00', x: 50, z: -20, pending: true });
    expect(pins).toContainEqual({ icon: 'star', rgb: '#ff0000', x: 1, z: 2, pending: false });
    // Saved or cancelled: gone; another face's: not drawn on this one.
    expect(bigMapPins([placed], 0, null).some((p) => p.pending)).toBe(false);
    expect(bigMapPins([placed], 1, { ...pending, face: 0 })).toEqual([]);
  });
});

// BUGS.md #122: the picker for a marker not yet placed showed Delete (its `hidden` lost to the map
// buttons' `display: inline-flex`): a hidden map button is not drawn.
describe("the picker's hidden buttons", () => {
  it('a hidden map button is display: none (the new marker shows Save and Cancel only)', () => {
    const css = readFileSync(new URL('../style.css', import.meta.url), 'utf8').replace(/\s+/g, ' ');
    expect(css).toMatch(/#world-map \.map-button\[hidden\] \{ display: none;? \}/);
  });
});

describe('the markers list, portals and the ground under a placed marker', () => {
  const mk = (id: number, face: number, x: number, z: number, label = `m${String(id)}`) => ({
    id,
    icon: 'flag',
    rgb: '#ffffff',
    label,
    face,
    x,
    y: 64,
    z,
    auto: '',
  });

  // both-faces: each face's rows against its own mirrored map (`toScreen`).
  it('lists the markers nearest first with the way to each as the map shows it, on both faces', () => {
    for (const face of [0, 1]) {
      const other = 1 - face;
      const list = [
        mk(1, face, 0, 300),
        mk(2, other, 0, 1, 'Zed'),
        mk(3, face, 40, 0),
        mk(4, other, 0, 2, 'Ann'),
        mk(5, face, 0, -10),
      ];
      const me = { face, x: 0, y: 64, z: 0 };
      const rows = markerRows(list, me);
      expect(rows.map((r) => r.marker.id)).toEqual([5, 3, 1, 4, 2]);
      expect(rows[0]?.distanceM).toBeCloseTo(10);
      expect(rows[3]?.distanceM).toBeNull();
      expect(rows[3]?.angle).toBeNull();
      // Each row's way is the marker's direction on the big map from the player.
      const v = { face, cx: 0, cz: 0, pxPerM: 1, level: 0 };
      for (const r of rows.filter((r) => r.angle !== null)) {
        const [sx, sy] = toScreen(v, r.marker.x, r.marker.z);
        const a = r.angle ?? 0;
        expect(Math.sin(a)).toBeCloseTo(sx / Math.hypot(sx, sy));
        expect(-Math.cos(a)).toBeCloseTo(sy / Math.hypot(sx, sy));
      }
      // +z is up on both faces; +x is left on face A, right on face B (the mirror).
      expect(directionWord(markerRows([mk(9, face, 0, 50)], me)[0]?.angle ?? -1)).toBe('up');
      expect(directionWord(markerRows([mk(9, face, 50, 0)], me)[0]?.angle ?? -1)).toBe(
        face === 1 ? 'right' : 'left',
      );
    }
    expect(
      markerRows([mk(1, 0, 5, 5, 'b'), mk(2, 0, 1, 1, 'a')], null).map((r) => r.marker.id),
    ).toEqual([2, 1]);
  });

  it('places a marker on the loaded ground, else the map height, else the fallback', () => {
    // A height code: half-metres × 2, + 1 for water (`dwell_game::minimap::height_code`).
    const code = (h: number, water = false) => Math.round(h * 2) * 2 + (water ? 1 : 0);
    expect(heightOfCode(code(71.5))).toBe(71.5);
    expect(heightOfCode(code(40, true))).toBe(40);
    expect(heightOfCode(-2147483648)).toBeNull();
    expect(heightOfCode(-32768)).toBeNull();
    expect(markerGroundY(80.5, code(71.5), 64)).toBe(80.5);
    expect(markerGroundY(undefined, code(71.5), 64)).toBe(71.5);
    expect(markerGroundY(Number.NaN, -2147483648, 64)).toBe(64);
  });

  // both-faces: a seen portal keeps its face.
  it('notes each seen portal on its face, the map height where its rim is not loaded', () => {
    for (const face of [0, 1]) {
      const spots = seenPortalSpots(
        [face, 10.5, 72, 0.5, face, -200.5, Number.NaN, 4.5],
        (f, x, z) => (f === face && x === -200.5 && z === 4.5 ? 66 : -1),
      );
      expect(spots).toEqual([
        { face, x: 10.5, y: 72, z: 0.5 },
        { face, x: -200.5, y: 66, z: 4.5 },
      ]);
      const l = new MarkerList();
      for (const at of spots) l.noteAuto('portal', at);
      for (const at of spots) l.noteAuto('portal', { ...at, y: at.y + 3 }); // seen again
      const portal = AUTO_MARKERS.find((a) => a.kind === 'portal');
      expect(l.all).toHaveLength(2);
      expect(l.all[0]).toMatchObject({ auto: 'portal', face, icon: portal?.icon, y: 72 });
    }
  });
});
