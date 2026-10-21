// Types for load.mjs (the client's Vitest suites import it).
import type { MusicData } from '../../client/src/audio/musicTypes';

export const MUSIC_DIR: string;
export function readMusicFiles(dir?: string): unknown[];
export function loadMusic(dir?: string): MusicData;
