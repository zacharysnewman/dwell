// Types of gen.mjs's bake, for Chisel's Textures tab tests (client/src/chisel/textures.test.ts).
import type { Materials, Palette } from './paint.mjs';
export function bake(
  materials: Materials,
  palette: Palette,
  tiles: string[],
): { names: string[]; texels: Uint8Array };
