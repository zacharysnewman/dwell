// Types for validate.mjs (the client's Vitest suites import it).
import type { MusicData } from '../../client/src/audio/musicTypes';

export const WHEN: string[];
export const MAX_BYTES: number;
export function contentHash(bytes: Uint8Array): string;
export function resolveMusic(files: unknown[], dir: string): MusicData;
