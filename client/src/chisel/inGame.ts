// Chisel in the game (Phase 56 B7, WORK_QUEUE item 34): the biome sliders of a running local world,
// the logic behind ui/chiselPanel.ts. The world's worldgen config opened as an edit, a slider's
// change to it (with undo and reset to the edit the panel opened on), the sliders made from the
// validation's own ranges (shared/worldgen/biomes.mjs `TERRAIN`), and the debounce that sends one
// retune when the player stops dragging. A change goes to the local server as the world's new config
// (`retuneWorldgen`): saved with the world, the terrain near the player generated again.
import { TERRAIN, type Range } from '../../../shared/worldgen/biomes.mjs';
import { buildEdit, fromJson, starterEdit, toJson, validateEdit, type Edit } from './state';

/** The biome groups offered in the game: the biome's own shaping (the ground itself stays). */
export const IN_GAME_GROUPS: readonly string[] = ['detail', 'shaping', 'banks'];

/** The terrain chunks around the player generated again by a retune (horizontally). */
export const RETUNE_RADIUS_CHUNKS = 3;

/** The quiet time after the last slider change before the retune is sent. */
export const RETUNE_DEBOUNCE_MS = 400;

/** One slider of a group: its key, range and step. */
export interface SliderSpec {
  key: string;
  min: number;
  max: number;
  step: number;
  whole: boolean;
}

/**
 * A group's sliders from its `TERRAIN` ranges, around `obj`'s values (as Chisel's page makes them:
 * a range up to four times the value, or 1, the data's bound if smaller).
 */
export function sliderSpecs(
  group: string,
  obj: Record<string, unknown>,
  table: Record<string, Record<string, Range>> = TERRAIN,
): SliderSpec[] {
  return Object.entries(table[group] ?? {}).map(([key, [lo, hi, whole = false]]) => {
    const min = typeof lo === 'object' ? lo.above : lo;
    const v = Number(obj[key]);
    const max = Math.min(hi, Math.max(whole ? 8 : 1, Math.abs(v) * 4));
    const step = whole ? 1 : max <= 2 ? 0.01 : max <= 50 ? 0.1 : 1;
    return { key, min, max, step, whole };
  });
}

/**
 * The edit a world's worldgen config stands for: the build's for `""`, the starter world's for
 * `test:starter`, a Chisel edit's JSON as it is; null for a config the game can't tune (another
 * fixture's name, or JSON that does not validate).
 */
export function editOfWorld(worldgen: string): Edit | null {
  const w = worldgen.trim();
  if (w === '') return buildEdit();
  if (w === 'test:starter') return starterEdit();
  if (!w.startsWith('{')) return null;
  const r = fromJson(w);
  return 'edit' in r ? r.edit : null;
}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

/** The panel's edit: a slider's change, undo, reset, and the config text sent to the server. */
export class InGameEdit {
  private edit: Edit;
  private readonly opened: Edit;
  private readonly undos: Edit[] = [];

  constructor(edit: Edit) {
    this.opened = clone(edit);
    this.edit = clone(edit);
  }

  /** The biomes' names, in the edit's order. */
  biomes(): string[] {
    return this.edit.biomes.map((b) => b.name);
  }

  /** A biome's group as it is now (`{}` for none). */
  group(biome: number, group: string): Record<string, unknown> {
    const b = this.edit.biomes[biome] as Record<string, unknown> | undefined;
    return (b?.[group] as Record<string, unknown> | undefined) ?? {};
  }

  /**
   * Sets a biome's group value; the change is undoable. Returns the edit's problems (the change is
   * taken back when there are any).
   */
  set(biome: number, group: string, key: string, value: number): string[] {
    const before = clone(this.edit);
    const b = this.edit.biomes[biome] as Record<string, unknown> | undefined;
    if (!b) return [`no biome ${String(biome)}`];
    const g = (b[group] ??= {}) as Record<string, unknown>;
    if (g[key] === value) return [];
    g[key] = value;
    const errors = validateEdit(this.edit);
    if (errors.length) {
      this.edit = before;
      return errors;
    }
    this.undos.push(before);
    return [];
  }

  /** Takes the last change back; false with nothing to undo. */
  undo(): boolean {
    const last = this.undos.pop();
    if (!last) return false;
    this.edit = last;
    return true;
  }

  canUndo(): boolean {
    return this.undos.length > 0;
  }

  /** Back to the edit the panel opened on (undoable). */
  reset(): void {
    if (JSON.stringify(this.edit) === JSON.stringify(this.opened)) return;
    this.undos.push(this.edit);
    this.edit = clone(this.opened);
  }

  /** The config text for the server (`toJson`, compact). */
  text(): string {
    return JSON.stringify(JSON.parse(toJson(this.edit)));
  }
}

/**
 * Calls `send` once `quietMs` after the last `poke` (a slider dragged sends one retune when it
 * rests), with the value of the last poke.
 */
export function debounce<T>(
  send: (v: T) => void,
  quietMs: number,
  timers: {
    set: (f: () => void, ms: number) => unknown;
    clear: (h: unknown) => void;
  } = {
    set: (f, ms) => setTimeout(f, ms),
    clear: (h) => {
      clearTimeout(h as ReturnType<typeof setTimeout>);
    },
  },
): (v: T) => void {
  let pending: unknown = null;
  return (v: T) => {
    if (pending !== null) timers.clear(pending);
    pending = timers.set(() => {
      pending = null;
      send(v);
    }, quietMs);
  };
}
