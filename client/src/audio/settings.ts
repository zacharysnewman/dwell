// The volumes (docs/AUDIO.md §6): master, effects and interface, kept per browser in
// `dwell.audio`. The key is shared by every app version on the origin (RELEASES.md §6), so the
// store is append-only: new fields optional, and a rewrite keeps fields it does not know.
import { DEFAULT_VOLUMES, type Volumes } from './mixer';

export const AUDIO_KEY = 'dwell.audio';

const clamp01 = (v: unknown, d: number): number =>
  typeof v === 'number' && Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : d;

/** Volumes from a stored value: each 0–1, anything missing or invalid the default. */
export function sanitizeVolumes(value: unknown): Volumes {
  const v = (typeof value === 'object' && value !== null ? value : {}) as Record<string, unknown>;
  return {
    master: clamp01(v.master, DEFAULT_VOLUMES.master),
    effects: clamp01(v.effects, DEFAULT_VOLUMES.effects),
    ui: clamp01(v.ui, DEFAULT_VOLUMES.ui),
  };
}

type Store = Pick<Storage, 'getItem' | 'setItem'>;

function read(store: Store | null): Record<string, unknown> {
  try {
    const raw: unknown = JSON.parse(store?.getItem(AUDIO_KEY) ?? 'null');
    return typeof raw === 'object' && raw !== null && !Array.isArray(raw)
      ? (raw as Record<string, unknown>)
      : {};
  } catch {
    return {}; // unreadable: defaults (and rewritten on the next save)
  }
}

export function loadVolumes(store: Store | null = localStore()): Volumes {
  return sanitizeVolumes(read(store));
}

/** Saves the volumes, keeping any other fields already stored. */
export function saveVolumes(v: Volumes, store: Store | null = localStore()): void {
  try {
    store?.setItem(AUDIO_KEY, JSON.stringify({ ...read(store), ...v }));
  } catch {
    // Not kept: the volumes still apply for this visit.
  }
}

function localStore(): Store | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}
