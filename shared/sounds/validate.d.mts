// Types for validate.mjs (the client's Vitest suites import it).
import type { SoundData } from '../../client/src/audio/types';

export const SOURCES: string[];
export const NOISE_COLORS: string[];
export const WAVES: string[];
export const FILTER_TYPES: string[];
export const SET_EVENTS: string[];
export const REQUIRED_SET_EVENTS: string[];
export const GAITS: string[];
export const UI_KINDS: string[];
export function validateSound(where: string, sound: unknown): void;
/** The data the client plays (without `gaitOrder`, which the generator adds). */
export function resolveSounds(files: unknown[]): Omit<SoundData, 'gaitOrder'>;
export function checkBlockSounds(
  blocks: { id: string; sound?: unknown }[],
  resolved: Omit<SoundData, 'gaitOrder'>,
): void;
