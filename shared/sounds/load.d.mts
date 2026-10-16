// Types for load.mjs (the client's Vitest suites import it).
import type { SoundData } from '../../client/src/audio/types';

export const SOUNDS_DIR: string;
export function readSoundFiles(dir?: string): unknown[];
export function loadSounds(dir?: string): Omit<SoundData, 'gaitOrder'>;
