// Map markers (WORK_QUEUE item 53, docs/MAP_MARKERS.md; owner 2026-11-12: "Let's use non-Unicode
// vector style symbols to let you annotate your own map so you can find your way around."): a
// player's own markers in a world, each an icon from the set (vector paths, shared/ui/markers.json →
// markers.gen.ts), a colour, a short label and a face-local spot, plus the game's auto markers
// (spawn, last death, portals once seen). This module is the list and its rules (place, edit, move,
// delete, the auto kinds, the limits), its record (the same bytes as `dwell_protocol::markers`,
// pinned by the golden vector), and the geometry the minimap and the world waypoint use. The server
// keeps the record for the player alone (`markers.private`); the map draws it (minimap.ts).

import {
  AUTO_MARKERS,
  type AutoMarkerKind,
  MARKER_ICONS,
  MARKER_LIMITS,
  MARKER_PALETTE,
  MARKER_WORLD,
  type MarkerIcon,
} from './markers.gen';

/** One marker. Its spot is face-local (x, y, z: the face's up is +y, as `FaceFrame::point`). */
export interface MapMarker {
  id: number;
  /** Its icon's id (`MARKER_ICONS`); one this build doesn't know draws as the fallback. */
  icon: string;
  /** Its colour, #rrggbb. */
  rgb: string;
  label: string;
  /** 0 face A, 1 face B. */
  face: number;
  x: number;
  y: number;
  z: number;
  /** The auto kind that placed it; '' the player's own. */
  auto: string;
}

/** The record's format version (`dwell_protocol::markers::VERSION`). */
export const MARKERS_VERSION = 1;

/** An icon by id, else the fallback (the "question" icon, else the first). */
export function iconOf(id: string): MarkerIcon {
  const icons: readonly MarkerIcon[] = MARKER_ICONS;
  const found = icons.find((i) => i.id === id);
  if (found) return found;
  const fallback = icons.find((i) => i.id === 'question') ?? icons[0];
  if (!fallback) throw new Error('markers.json has no icons');
  return fallback;
}

/** A palette colour by name (#rrggbb), else the first's. */
export function paletteRgb(name: string): string {
  return (MARKER_PALETTE.find((c) => c.name === name) ?? MARKER_PALETTE[0])?.rgb ?? '#ffffff';
}

/** A label cut to the most characters (whole characters, not UTF-16 units), trimmed. */
export function clampLabel(label: string): string {
  // Code points, as the server counts them (Rust's `chars`), control characters dropped.
  const printable = Array.from(label.trim()).filter((c) => {
    const n = c.codePointAt(0) ?? 0;
    return n >= 0x20 && n !== 0x7f;
  });
  return printable.slice(0, MARKER_LIMITS.labelChars).join('').trim();
}

const utf8 = new TextEncoder();
const fromUtf8 = new TextDecoder('utf-8', { fatal: true });

/** The record's bytes (`dwell_protocol::markers::Markers::to_bytes`). */
export function encodeMarkers(list: readonly MapMarker[]): Uint8Array {
  const parts: number[] = [];
  const u8 = (v: number) => parts.push(v & 0xff);
  const u16 = (v: number) => {
    u8(v);
    u8(v >> 8);
  };
  const u32 = (v: number) => {
    u16(v & 0xffff);
    u16(v >>> 16);
  };
  const f32 = (v: number) => {
    const b = new Uint8Array(new Float32Array([v]).buffer);
    for (const x of b) u8(x);
  };
  const str = (s: string) => {
    const b = utf8.encode(s);
    u16(b.length);
    for (const x of b) u8(x);
  };
  u8(MARKERS_VERSION);
  u16(list.length);
  for (const m of list) {
    u32(m.id);
    str(m.icon);
    const rgb = parseInt(m.rgb.slice(1), 16) || 0;
    u8(rgb >> 16);
    u8(rgb >> 8);
    u8(rgb);
    str(m.label);
    u8(m.face);
    f32(m.x);
    f32(m.y);
    f32(m.z);
    str(m.auto);
  }
  return new Uint8Array(parts);
}

/** A record's markers, or null if it is malformed (the Rust decoder's rules). */
export function decodeMarkers(b: Uint8Array): MapMarker[] | null {
  const view = new DataView(b.buffer, b.byteOffset, b.byteLength);
  let at = 0;
  const need = (n: number) => {
    if (at + n > b.length) throw new Error('truncated');
  };
  const u8 = () => {
    need(1);
    return b[at++] ?? 0;
  };
  const u16 = () => {
    need(2);
    const v = view.getUint16(at, true);
    at += 2;
    return v;
  };
  const u32 = () => {
    need(4);
    const v = view.getUint32(at, true);
    at += 4;
    return v;
  };
  const f32 = () => {
    need(4);
    const v = view.getFloat32(at, true);
    at += 4;
    if (!Number.isFinite(v)) throw new Error('not finite');
    return v;
  };
  const str = (max: number) => {
    const n = u16();
    if (n > max) throw new Error('long');
    need(n);
    const s = fromUtf8.decode(b.subarray(at, at + n));
    at += n;
    return s;
  };
  try {
    if (u8() !== MARKERS_VERSION) return null;
    const n = u16();
    if (n > MARKER_LIMITS.maxMarkers) return null;
    const out: MapMarker[] = [];
    const ids = new Set<number>();
    for (let k = 0; k < n; k++) {
      const id = u32();
      if (ids.has(id)) return null;
      ids.add(id);
      const icon = str(MARKER_LIMITS.nameBytes);
      if (icon === '') return null;
      const rgb = (u8() << 16) | (u8() << 8) | u8();
      const label = str(MARKER_LIMITS.labelChars * 4);
      if (Array.from(label).length > MARKER_LIMITS.labelChars) return null;
      const face = u8();
      if (face > 1) return null;
      const [x, y, z] = [f32(), f32(), f32()];
      const auto = str(MARKER_LIMITS.nameBytes);
      out.push({
        id,
        icon,
        rgb: `#${rgb.toString(16).padStart(6, '0')}`,
        label,
        face,
        x,
        y,
        z,
        auto,
      });
    }
    return at === b.length ? out : null;
  } catch {
    return null;
  }
}

/** What a place or an edit sets: an icon, a colour and a label. */
export interface MarkerLook {
  icon: string;
  rgb: string;
  label: string;
}

/** A face-local spot. */
export interface MarkerSpot {
  face: number;
  x: number;
  y: number;
  z: number;
}

/**
 * A player's markers in a world and the rules for changing them: placing (up to the limit), editing
 * the look, moving, deleting, and the auto kinds (one moved to the newest place, or one each).
 * `onChange` hears every change (the shell sends the record and draws the maps again).
 */
export class MarkerList {
  private items: MapMarker[] = [];
  private nextId = 1;
  onChange: ((list: readonly MapMarker[]) => void) | null = null;

  constructor(private readonly kinds: readonly AutoMarkerKind[] = AUTO_MARKERS) {}

  get all(): readonly MapMarker[] {
    return this.items;
  }

  get(id: number): MapMarker | undefined {
    return this.items.find((m) => m.id === id);
  }

  /** Every marker replaced by a record's (the server's, at join); no change heard. */
  load(list: readonly MapMarker[]): void {
    this.items = list.slice(0, MARKER_LIMITS.maxMarkers).map((m) => ({ ...m }));
    this.nextId = this.items.reduce((n, m) => Math.max(n, m.id + 1), 1);
  }

  /** Whether another marker fits. */
  get full(): boolean {
    return this.items.length >= MARKER_LIMITS.maxMarkers;
  }

  /** Places the player's own marker; its id, or null when the list is full. */
  place(look: MarkerLook, at: MarkerSpot): number | null {
    if (this.full) return null;
    const id = this.nextId++;
    this.items.push({ id, ...this.clean(look), ...spot(at), auto: '' });
    this.changed();
    return id;
  }

  /** A marker's look changed (an auto marker becomes the player's own once edited). */
  edit(id: number, look: Partial<MarkerLook>): boolean {
    const m = this.get(id);
    if (!m) return false;
    Object.assign(m, this.clean({ icon: m.icon, rgb: m.rgb, label: m.label, ...look }), {
      auto: '',
    });
    this.changed();
    return true;
  }

  /** A marker moved to another spot. */
  move(id: number, at: MarkerSpot): boolean {
    const m = this.get(id);
    if (!m) return false;
    Object.assign(m, spot(at));
    this.changed();
    return true;
  }

  remove(id: number): boolean {
    const n = this.items.length;
    this.items = this.items.filter((m) => m.id !== id);
    if (this.items.length === n) return false;
    this.changed();
    return true;
  }

  /**
   * The game noted an auto kind's place (spawn, a death, a portal seen): a kind kept `one` has its
   * marker moved there (placed the first time); one kept `each` gets a new one unless one of its
   * markers is already within 2 m. Nothing for a kind that is off or unknown. The marker's id.
   */
  noteAuto(kind: string, at: MarkerSpot): number | null {
    const k = this.kinds.find((a) => a.kind === kind);
    if (!k?.on) return null;
    const mine = this.items.filter((m) => m.auto === kind);
    const look = { icon: k.icon, rgb: paletteRgb(k.colour), label: k.label };
    if (k.keep === 'one' && mine[0]) {
      const m = mine[0];
      if (m.face === at.face && Math.hypot(m.x - at.x, m.y - at.y, m.z - at.z) < 0.01) return m.id;
      Object.assign(m, spot(at));
      this.changed();
      return m.id;
    }
    // A place within 2 m across the ground is one already seen (its height may be read again
    // finer: a portal's rim from the map's height, then from the loaded ground).
    const near = mine.find((m) => m.face === at.face && Math.hypot(m.x - at.x, m.z - at.z) < 2);
    if (near) return near.id;
    if (this.full) return null;
    const id = this.nextId++;
    this.items.push({ id, ...this.clean(look), ...spot(at), auto: kind });
    this.changed();
    return id;
  }

  /** Whether an auto kind has its marker yet. */
  hasAuto(kind: string): boolean {
    return this.items.some((m) => m.auto === kind);
  }

  private clean(look: MarkerLook): MarkerLook {
    const rgb = /^#[0-9a-f]{6}$/i.test(look.rgb) ? look.rgb.toLowerCase() : paletteRgb('white');
    const icon = look.icon !== '' ? look.icon.slice(0, MARKER_LIMITS.nameBytes) : 'question';
    return { icon, rgb, label: clampLabel(look.label) };
  }

  private changed(): void {
    this.onChange?.(this.items);
  }
}

function spot(at: MarkerSpot): MarkerSpot {
  return { face: at.face === 1 ? 1 : 0, x: at.x, y: at.y, z: at.z };
}

/**
 * A face-local point (x, z) on the minimap of a player on `face` at (px, pz) facing `yawDeg`:
 * metres right of and up from them (`dwell_game::minimap::MapFrame::to_screen`; the heading up,
 * face B's right the other way along face-local x).
 */
export function minimapOffset(
  face: number,
  px: number,
  pz: number,
  yawDeg: number,
  x: number,
  z: number,
): [number, number] {
  const r = (yawDeg * Math.PI) / 180;
  const [s, c] = [Math.sin(r), Math.cos(r)];
  const hand = face === 1 ? -1 : 1;
  const right = [hand * -c, hand * s] as const;
  const [dx, dz] = [x - px, z - pz];
  return [dx * right[0] + dz * right[1], dx * s + dz * c];
}

/** The markers on the minimap's disc: each with its offset (right, up; m) within `radiusM`. */
export function markersInRange(
  list: readonly MapMarker[],
  pose: { face: number; x: number; z: number; yaw: number },
  radiusM: number,
): { marker: MapMarker; right: number; up: number }[] {
  const out: { marker: MapMarker; right: number; up: number }[] = [];
  for (const m of list) {
    if (m.face !== pose.face) continue;
    const [right, up] = minimapOffset(pose.face, pose.x, pose.z, pose.yaw, m.x, m.z);
    if (right * right + up * up <= radiusM * radiusM) out.push({ marker: m, right, up });
  }
  return out;
}

/** The marker nearest a screen point within `radiusPx` (big map picking), or null. */
export function pickMarker(
  list: readonly MapMarker[],
  toPx: (m: MapMarker) => [number, number] | null,
  sx: number,
  sy: number,
  radiusPx: number = MARKER_LIMITS.pickRadiusPx,
): MapMarker | null {
  let best: MapMarker | null = null;
  let bestD = radiusPx;
  for (const m of list) {
    const p = toPx(m);
    if (!p) continue;
    const d = Math.hypot(p[0] - sx, p[1] - sy);
    if (d <= bestD) {
      best = m;
      bestD = d;
    }
  }
  return best;
}

/** "12 m" or "1.2 km" for a waypoint's distance. */
export function formatMarkerDistance(m: number): string {
  return m < 1000 ? `${String(Math.round(m))} m` : `${(m / 1000).toFixed(1)} km`;
}

/** A waypoint's opacity by its distance: full to `nearM`, fading to `fadedAlpha` at `farM`. */
export function waypointAlpha(distanceM: number): number {
  const { nearM, farM, fadedAlpha } = MARKER_WORLD;
  if (distanceM <= nearM) return 1;
  if (distanceM >= farM) return fadedAlpha;
  return 1 - ((distanceM - nearM) / (farM - nearM)) * (1 - fadedAlpha);
}

/**
 * Whether a marker shows a waypoint: not within `hideWithinM` of the eye (`distanceM`) or across the
 * ground (`flatM`, the face-local x–z distance from the player). A marker under or at the camera has
 * no way to point to, so it is not pinned to the screen's edge (BUGS.md: standing on the spawn read
 * "1 m" stuck to the top edge).
 */
export function waypointShown(
  distanceM: number,
  flatM: number,
  hideWithinM: number = MARKER_WORLD.hideWithinM,
): boolean {
  return distanceM > hideWithinM && flatM > hideWithinM;
}

/** A pin the big map draws: a placed marker, or the pending one the picker is open to place. */
export interface MapPin {
  icon: string;
  rgb: string;
  x: number;
  z: number;
  pending: boolean;
}

/** The spot a new marker goes and the picker's look for it, while the picker is open to place it. */
export interface PendingPin {
  face: number;
  x: number;
  z: number;
  icon: string;
  rgb: string;
}

/** The big map's pins on `face`: the markers there, then the pending one (drawn over them). */
export function bigMapPins(
  markers: readonly MapMarker[],
  face: number,
  pending: PendingPin | null,
): MapPin[] {
  const out: MapPin[] = markers
    .filter((m) => m.face === face)
    .map((m) => ({ icon: m.icon, rgb: m.rgb, x: m.x, z: m.z, pending: false }));
  if (pending?.face === face) {
    out.push({ icon: pending.icon, rgb: pending.rgb, x: pending.x, z: pending.z, pending: true });
  }
  return out;
}

/**
 * Where a waypoint goes on a `w` × `h` screen from its projection ([x, y] −1..1 across the view, +y
 * up, and its depth; `Client.markerScreen`): its pixel, and whether it is on screen. Off screen
 * (or behind) it is pulled along the line from the centre to the screen's edge, `edgePx` inside.
 */
export function waypointOnScreen(
  ndcX: number,
  ndcY: number,
  depth: number,
  w: number,
  h: number,
  edgePx: number = MARKER_WORLD.edgePx,
): { x: number; y: number; onScreen: boolean } {
  let [dx, dy] = [ndcX * (w / 2), -ndcY * (h / 2)];
  if (depth <= 0) {
    // Behind: the way to turn, pushed past the edge (a point straight behind goes to the bottom).
    [dx, dy] = dx === 0 && dy === 0 ? [0, h] : [dx * 1e6, dy * 1e6];
    if (dy < 0) dy = Math.abs(dy);
  }
  const [hx, hy] = [Math.max(1, w / 2 - edgePx), Math.max(1, h / 2 - edgePx)];
  const onScreen = depth > 0 && Math.abs(dx) <= w / 2 && Math.abs(dy) <= h / 2;
  if (!onScreen) {
    const k = Math.min(hx / Math.max(Math.abs(dx), 1e-9), hy / Math.max(Math.abs(dy), 1e-9), 1);
    [dx, dy] = [dx * k, dy * k];
  }
  return { x: w / 2 + dx, y: h / 2 + dy, onScreen };
}

/** An SVG element showing an icon in a colour (the picker's buttons, the world waypoints). */
export function iconSvg(doc: Document, icon: string, rgb: string, px: number): SVGSVGElement {
  const ns = 'http://www.w3.org/2000/svg';
  const svg = doc.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', String(px));
  svg.setAttribute('height', String(px));
  svg.setAttribute('aria-hidden', 'true');
  const path = doc.createElementNS(ns, 'path');
  path.setAttribute('d', iconOf(icon).path);
  path.setAttribute('fill', rgb);
  path.setAttribute('fill-rule', 'evenodd');
  path.setAttribute('stroke', 'rgb(20 16 12 / 85%)');
  path.setAttribute('stroke-width', '1.25');
  path.setAttribute('stroke-linejoin', 'round');
  svg.append(path);
  return svg;
}

/** Draws an icon centred at (x, y), `px` across, on a 2D canvas (the maps). */
export function drawIcon(
  ctx: CanvasRenderingContext2D,
  icon: string,
  rgb: string,
  x: number,
  y: number,
  px: number,
): void {
  if (typeof Path2D === 'undefined') return;
  const k = px / 24;
  ctx.save();
  ctx.translate(x - px / 2, y - px / 2);
  ctx.scale(k, k);
  const p = new Path2D(iconOf(icon).path);
  ctx.lineJoin = 'round';
  ctx.lineWidth = 3 / Math.max(k, 0.5);
  ctx.strokeStyle = 'rgb(20 16 12 / 85%)';
  ctx.stroke(p);
  ctx.fillStyle = rgb;
  ctx.fill(p, 'evenodd');
  ctx.restore();
}

/** A height code's metres (`dwell_game::minimap::decode_height`; water's its bed's), null for none
 * (`NO_HEIGHT`, or a tile's i16 none, −32768). */
export function heightOfCode(code: number): number | null {
  if (!Number.isFinite(code) || code === -2147483648 || code === -32768) return null;
  return (code >> 1) / 2;
}

/**
 * The face-local height a marker placed on the big map stands at (docs/MAP_MARKERS.md): the loaded
 * ground's top there (`Client.markerGround`), else the map's height for its cell (a height code),
 * else `fallback` (the player's height when on that face).
 */
export function markerGroundY(
  loaded: number | null | undefined,
  mapCode: number,
  fallback: number,
): number {
  if (typeof loaded === 'number' && Number.isFinite(loaded)) return loaded;
  return heightOfCode(mapCode) ?? fallback;
}

/** A row of the big map's markers list. */
export interface MarkerRow {
  marker: MapMarker;
  /** The distance from the player (m, face-local); null on the other face. */
  distanceM: number | null;
  /** The way to it on the big map, radians clockwise from the map's up; null on the other face
   * or at the player's spot. */
  angle: number | null;
}

/**
 * The big map's markers list (docs/MAP_MARKERS.md): each marker with its distance and the way to
 * it from the player at `me` (as the big map shows it: +z up, face A's +x left, face B's right,
 * the mirror), nearest first; the other face's after, by label. No player: all by label.
 */
export function markerRows(markers: readonly MapMarker[], me: MarkerSpot | null): MarkerRow[] {
  const rows = markers.map((m): MarkerRow => {
    if (!me || m.face !== me.face) return { marker: m, distanceM: null, angle: null };
    const [dx, dy, dz] = [m.x - me.x, m.y - me.y, m.z - me.z];
    const right = (me.face === 1 ? 1 : -1) * dx;
    const flat = Math.hypot(dx, dz);
    return {
      marker: m,
      distanceM: Math.hypot(dx, dy, dz),
      angle: flat < 1e-6 ? null : Math.atan2(right, dz),
    };
  });
  return rows.sort((a, b) => {
    if (a.distanceM !== null && b.distanceM !== null) return a.distanceM - b.distanceM;
    if (a.distanceM !== null) return -1;
    if (b.distanceM !== null) return 1;
    return a.marker.label.localeCompare(b.marker.label) || a.marker.id - b.marker.id;
  });
}

const DIRECTION_WORDS = [
  'up',
  'up right',
  'right',
  'down right',
  'down',
  'down left',
  'left',
  'up left',
] as const;

/** A row's way in words, as the map shows it ("up right"), for its arrow's label. */
export function directionWord(angle: number): string {
  const n = Math.round(angle / (Math.PI / 4));
  return DIRECTION_WORDS[((n % 8) + 8) % 8] ?? 'up';
}

/**
 * The portals to note as seen (`Client.seenPortals`' flat [face, x, y, z]*, y NaN where the rim
 * isn't loaded), each standing at `groundY(face, x, z)` when its rim's height is unknown.
 */
export function seenPortalSpots(
  flat: ArrayLike<number>,
  groundY: (face: number, x: number, z: number) => number,
): MarkerSpot[] {
  const out: MarkerSpot[] = [];
  for (let n = 0; n + 3 < flat.length; n += 4) {
    const face = flat[n] === 1 ? 1 : 0;
    const [x = 0, y = Number.NaN, z = 0] = [flat[n + 1], flat[n + 2], flat[n + 3]];
    out.push({ face, x, y: Number.isFinite(y) ? y : groundY(face, x, z), z });
  }
  return out;
}
