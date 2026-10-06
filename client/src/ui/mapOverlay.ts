// Biome/height map overlay (F4; Phase 3e debug tooling, ARCHITECTURE.md §6.3): the terrain
// generator's columns around the player, sampled in a worldgen worker (worldgen/generator.ts
// `map`), coloured by biome and hill-shaded by height. `-` and `=` zoom out and in, from the
// player's surroundings (1 km across) to the whole disc (Phase 10: the plate layout's continents).
// Biomes and colours are prototype (§6.1).

/** Biome colours, indexed by worldgen::Biome (ocean, beach, plains, forest, desert, snowy, mountains). */
export const BIOME_COLORS = [0x2f5fa8, 0xd8cc8f, 0x7fb04a, 0x3e7a2e, 0xd9c17a, 0xeef2f5, 0x8a8580];
export const BIOME_NAMES = ['ocean', 'beach', 'plains', 'forest', 'desert', 'snowy', 'mountains'];

/** Columns per map edge. */
export const MAP_SIZE = 128;
/**
 * Metres per column at each zoom. The last is the whole disc (128 columns × 128 km = its 16,384 km
 * diameter), centred on the origin; the others are centred on the player.
 */
export const MAP_ZOOMS = [8, 64, 512, 4096, 32768, 128000] as const;
export const DISC_ZOOM = MAP_ZOOMS.length - 1;
/** Metres per column of the map around the player at the first zoom (the old fixed map). */
export const MAP_STEP = MAP_ZOOMS[0];

/** The part of the world a map shows: its first column (m) and the metres per column. */
export interface MapView {
  x0: number;
  z0: number;
  step: number;
}

/** The view at a zoom for a player at (x, z): around them, or the whole disc about the origin. */
export function mapView(zoom: number, x: number, z: number): MapView {
  const level = Math.max(0, Math.min(DISC_ZOOM, zoom));
  const step = MAP_ZOOMS[level] ?? MAP_STEP;
  const half = (MAP_SIZE / 2) * step;
  return level === DISC_ZOOM
    ? { x0: -half, z0: -half, step }
    : { x0: Math.floor(x) - half, z0: Math.floor(z) - half, step };
}

/** "512 m" or "65.5 km" for a distance. */
export function formatMapDistance(m: number): string {
  return m < 1000 ? `${String(m)} m` : `${String(Math.round(m / 100) / 10)} km`;
}

export interface MapColumn {
  height: number;
  biome: number;
  outside: boolean;
}

/** Column (i, j) of a map (i along +x, j along +z). */
export function mapColumn(bytes: Uint8Array, n: number, i: number, j: number): MapColumn {
  const o = (j * n + i) * 4;
  const raw = (bytes[o] ?? 0) | ((bytes[o + 1] ?? 0) << 8);
  return {
    height: raw >= 0x8000 ? raw - 0x10000 : raw,
    biome: bytes[o + 2] ?? 0,
    outside: ((bytes[o + 3] ?? 0) & 1) !== 0,
  };
}

/**
 * RGBA pixels of a map, north (−z) up: biome colour, hill-shaded by the height difference to the
 * next column in +x; oceans darken with depth; beyond the rim, black.
 */
export function mapPixels(
  bytes: Uint8Array,
  n: number,
  step: number,
): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(n * n * 4);
  for (let j = 0; j < n; j++) {
    for (let i = 0; i < n; i++) {
      const c = mapColumn(bytes, n, i, j);
      const o = (j * n + i) * 4;
      out[o + 3] = 255;
      if (c.outside) continue;
      const east = mapColumn(bytes, n, Math.min(i + 1, n - 1), j).height;
      const slope = (east - c.height) / Math.min(step, 256);
      let shade = 1 - Math.max(-0.35, Math.min(0.35, slope * 0.8));
      if (c.height < 0) shade *= Math.max(0.45, 1 + c.height / 400);
      const color = BIOME_COLORS[c.biome] ?? 0xff00ff;
      out[o] = ((color >> 16) & 0xff) * shade;
      out[o + 1] = ((color >> 8) & 0xff) * shade;
      out[o + 2] = (color & 0xff) * shade;
    }
  }
  return out;
}

export class MapOverlay {
  private readonly root: HTMLDivElement;
  private readonly canvas: HTMLCanvasElement;
  private readonly caption: HTMLDivElement;
  private level = 0;

  constructor(parent: HTMLElement) {
    this.root = document.createElement('div');
    this.root.id = 'map-overlay';
    this.root.hidden = true;
    this.canvas = document.createElement('canvas');
    this.canvas.width = MAP_SIZE;
    this.canvas.height = MAP_SIZE;
    this.caption = document.createElement('div');
    this.root.append(this.canvas, this.caption);
    parent.append(this.root);
  }

  get visible(): boolean {
    return !this.root.hidden;
  }

  /** The zoom: an index into MAP_ZOOMS. */
  get zoom(): number {
    return this.level;
  }

  toggle(): void {
    this.root.hidden = !this.root.hidden;
  }

  /** One zoom level wider (+1) or closer (−1); returns whether it changed. */
  zoomBy(delta: number): boolean {
    const next = Math.max(0, Math.min(DISC_ZOOM, this.level + delta));
    const changed = next !== this.level;
    this.level = next;
    return changed;
  }

  /** The view for a player at (x, z) at the current zoom. */
  view(x: number, z: number): MapView {
    return mapView(this.level, x, z);
  }

  /**
   * Draws a map of `view`, with the player's position (x, z) and heading (degrees; 0 = +z) marked
   * where they are on it, if they are on it.
   */
  draw(bytes: Uint8Array | null, yawDeg: number, view: MapView, x: number, z: number): void {
    const ctx = this.canvas.getContext('2d');
    if (!ctx) return;
    if (!bytes) {
      ctx.clearRect(0, 0, MAP_SIZE, MAP_SIZE);
      this.caption.textContent = 'no terrain map for this generator';
      return;
    }
    ctx.putImageData(
      new ImageData(mapPixels(bytes, MAP_SIZE, view.step), MAP_SIZE, MAP_SIZE),
      0,
      0,
    );
    // The player: an arrow along their heading (+z is down the map), or a dot at wide zoom.
    const px = (x - view.x0) / view.step;
    const pz = (z - view.z0) / view.step;
    if (px >= 0 && px < MAP_SIZE && pz >= 0 && pz < MAP_SIZE) {
      ctx.strokeStyle = '#e0283c';
      ctx.fillStyle = '#e0283c';
      ctx.lineWidth = 2;
      if (view.step <= 64) {
        const yaw = (yawDeg * Math.PI) / 180;
        const [dx, dy] = [Math.sin(yaw), Math.cos(yaw)];
        ctx.beginPath();
        ctx.moveTo(px - dx * 3, pz - dy * 3);
        ctx.lineTo(px + dx * 7, pz + dy * 7);
        ctx.stroke();
      } else {
        ctx.fillRect(px - 1.5, pz - 1.5, 3, 3);
      }
    }
    const c = MAP_SIZE / 2;
    const here = mapColumn(bytes, MAP_SIZE, c, c);
    const place =
      this.level === DISC_ZOOM
        ? 'the whole disc'
        : `${BIOME_NAMES[here.biome] ?? '?'} · ground ${String(here.height)} m`;
    this.caption.textContent = `${place} · ${formatMapDistance(view.step)}/px · ${formatMapDistance(MAP_SIZE * view.step)} across · − = zoom`;
  }
}
