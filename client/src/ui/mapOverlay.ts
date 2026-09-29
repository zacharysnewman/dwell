// Biome/height map overlay (F4; Phase 3e debug tooling, ARCHITECTURE.md §6.3): the terrain
// generator's columns around the player, sampled in a worldgen worker (worldgen/generator.ts
// `map`), coloured by biome and hill-shaded by height. Biomes and colours are prototype (§6.1).

/** Biome colours, indexed by worldgen::Biome (ocean, beach, plains, forest, desert, snowy, mountains). */
export const BIOME_COLORS = [0x2f5fa8, 0xd8cc8f, 0x7fb04a, 0x3e7a2e, 0xd9c17a, 0xeef2f5, 0x8a8580];
export const BIOME_NAMES = ['ocean', 'beach', 'plains', 'forest', 'desert', 'snowy', 'mountains'];

/** Columns per map edge and metres per column. */
export const MAP_SIZE = 128;
export const MAP_STEP = 8;

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
      const slope = (east - c.height) / step;
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

  toggle(): void {
    this.root.hidden = !this.root.hidden;
  }

  /** Draws a map centred on the player, with its heading (degrees; 0 = +z). */
  draw(bytes: Uint8Array | null, yawDeg: number): void {
    const ctx = this.canvas.getContext('2d');
    if (!ctx) return;
    if (!bytes) {
      ctx.clearRect(0, 0, MAP_SIZE, MAP_SIZE);
      this.caption.textContent = 'no terrain map for this generator';
      return;
    }
    ctx.putImageData(new ImageData(mapPixels(bytes, MAP_SIZE, MAP_STEP), MAP_SIZE, MAP_SIZE), 0, 0);
    // The player: an arrow at the centre along its heading (+z is down the map).
    const c = MAP_SIZE / 2;
    const yaw = (yawDeg * Math.PI) / 180;
    const [dx, dy] = [Math.sin(yaw), Math.cos(yaw)];
    ctx.strokeStyle = '#e0283c';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(c - dx * 3, c - dy * 3);
    ctx.lineTo(c + dx * 7, c + dy * 7);
    ctx.stroke();
    const here = mapColumn(bytes, MAP_SIZE, c, c);
    this.caption.textContent = `${BIOME_NAMES[here.biome] ?? '?'} · ground ${String(here.height)} m · ${String(MAP_STEP)} m/px · ${String((MAP_SIZE * MAP_STEP) / 1000)} km`;
  }
}
