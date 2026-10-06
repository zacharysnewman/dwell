// The block dump (debug tooling, F6 or the debug overlay's button): the targeted block and the
// cells around it as JSON, with the world and build they came from, so a report of how the
// terrain looks somewhere can be regenerated and examined exactly (e.g. which slope piece the
// generator chose, from the seed and the coordinates) instead of read off a screenshot.
import type { BlockTarget } from '../sim/clientCore';
import { stateString } from '../world/blocks';

type Vec3 = [number, number, number];

export interface BlockDumpInput {
  /** The state id at a cell of the client's world (0 for air or a cell it does not hold). */
  voxel: (x: number, y: number, z: number) => number;
  /** The targeted block, or null (the dump is then centred on the cell under the player). */
  target: BlockTarget | null;
  feet: readonly number[];
  view: { yaw: number; pitch: number };
  world: { seed: bigint; generatorVersion: number } | null;
  build: { version: string; sha: string };
  /** Cells on each side of the centre: the dump covers a cube of 2 × radius + 1. */
  radius?: number;
}

export interface BlockDump {
  /** What this is, for whoever reads a pasted dump. */
  kind: 'dwell-block-dump';
  build: { version: string; sha: string };
  /** The world: its seed (a decimal string: it can exceed 2^53) and generator version. */
  world: { seed: string; generatorVersion: number } | null;
  player: { feet: number[]; yaw: number; pitch: number };
  /** The targeted block and the face the view entered (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z). */
  target: BlockTarget | null;
  /** The dumped cube's lowest corner (world cell), and its size along x, y and z. */
  origin: Vec3;
  size: number;
  /** The states in the cube, each named once; `layers` refers to them by index. */
  palette: string[];
  /**
   * The cube bottom to top: one entry per y (origin y first), each a list of rows from north to
   * south (z ascending), each row the palette indices west to east (x ascending).
   */
  layers: number[][][];
}

const round = (v: number) => Math.round(v * 1000) / 1000;

/** The dump of the cube around the target (or the player's feet). */
export function blockDump(input: BlockDumpInput): BlockDump {
  const r = input.radius ?? 2;
  const [fx = 0, fy = 0, fz = 0] = input.feet;
  const centre: Vec3 = input.target
    ? [...input.target.cell]
    : [Math.floor(fx), Math.floor(fy) - 1, Math.floor(fz)];
  const origin: Vec3 = [centre[0] - r, centre[1] - r, centre[2] - r];
  const size = 2 * r + 1;
  const palette: string[] = [];
  const index = new Map<number, number>();
  const layers: number[][][] = [];
  for (let dy = 0; dy < size; dy++) {
    const layer: number[][] = [];
    for (let dz = 0; dz < size; dz++) {
      const row: number[] = [];
      for (let dx = 0; dx < size; dx++) {
        const id = input.voxel(origin[0] + dx, origin[1] + dy, origin[2] + dz);
        let i = index.get(id);
        if (i === undefined) {
          i = palette.length;
          index.set(id, i);
          palette.push(stateString(id) ?? `unknown:${String(id)}`);
        }
        row.push(i);
      }
      layer.push(row);
    }
    layers.push(layer);
  }
  return {
    kind: 'dwell-block-dump',
    build: input.build,
    world: input.world
      ? { seed: input.world.seed.toString(), generatorVersion: input.world.generatorVersion }
      : null,
    player: {
      feet: input.feet.map(round),
      yaw: round(input.view.yaw),
      pitch: round(input.view.pitch),
    },
    target: input.target ? { cell: [...input.target.cell], face: input.target.face } : null,
    origin,
    size,
    palette,
    layers,
  };
}

/** The dump as text to paste: JSON, one row of a layer per line so the grid stays readable. */
export function formatBlockDump(dump: BlockDump): string {
  const { layers, ...rest } = dump;
  const head = JSON.stringify(rest, null, 2).replace(/\n}$/, '');
  const body = layers
    .map(
      (layer) => `    [\n${layer.map((row) => `      ${JSON.stringify(row)}`).join(',\n')}\n    ]`,
    )
    .join(',\n');
  return `${head},\n  "layers": [\n${body}\n  ]\n}`;
}
