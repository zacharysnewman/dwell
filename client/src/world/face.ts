// The disc's two faces (docs/BIFACIAL_WORLD.md §2): face A is the world at or above the midplane,
// face B its mirror image below it, with down always toward the midplane. Mirrors core/voxel.h.
import { CHUNK_SIZE, World } from '../protocol/constants.gen';
import { STATE_COUNT, stateString, withProperty } from './blocks';

export type Face = 0 | 1;
/** The face as a sign: +1 face A (head up), −1 face B (head down, toward the midplane's other side). */
export type FaceSign = 1 | -1;
export const faceSign = (face: Face): FaceSign => (face === 1 ? -1 : 1);
export const FACE_A: Face = 0;
export const FACE_B: Face = 1;

/** A voxel at height y on face B is the face-local voxel MIRROR_SUM − y (−4,097). */
export const MIRROR_SUM = 2 * World.midplaneY - 1;
/** The first chunk row of face A (−64). */
export const MIDPLANE_CHUNK_Y = World.midplaneY / CHUNK_SIZE;
/** A chunk row cy of face B is the face-local row MIRROR_CHUNK_SUM − cy (−129). */
export const MIRROR_CHUNK_SUM = 2 * MIDPLANE_CHUNK_Y - 1;

export const faceOfY = (y: number): Face => (y >= World.midplaneY ? FACE_A : FACE_B);
export const faceOfChunkY = (cy: number): Face => (cy >= MIDPLANE_CHUNK_Y ? FACE_A : FACE_B);
export const mirrorY = (y: number): number => MIRROR_SUM - y;
export const mirrorChunkY = (cy: number): number => MIRROR_CHUNK_SUM - cy;

let mirrorTable: Uint16Array | null = null;

/**
 * The state a block takes turned upside down (the vertical mirror): slabs and slopes swap `half`;
 * every other state is its own mirror (C++ core::MirrorMaterial).
 */
export function mirrorMaterial(id: number): number {
  if (!mirrorTable) {
    const t = new Uint16Array(STATE_COUNT);
    for (let i = 0; i < t.length; i++) {
      t[i] = i;
      const state = stateString(i);
      if (state?.includes('half=bottom')) t[i] = withProperty(i, 'half', 'top') ?? i;
      else if (state?.includes('half=top')) t[i] = withProperty(i, 'half', 'bottom') ?? i;
    }
    mirrorTable = t;
  }
  return mirrorTable[id] ?? id;
}

/**
 * The unit view direction (world) for yaw and pitch in degrees (yaw 0 = +Z, 90 = +X; pitch positive
 * toward the player's up). A face-B player's up is −y, so its pitch tilts the view the other way.
 */
export function viewForward(
  yawDeg: number,
  pitchDeg: number,
  face: FaceSign = 1,
): [number, number, number] {
  const yaw = (yawDeg * Math.PI) / 180;
  const pitch = (pitchDeg * Math.PI) / 180;
  return [Math.sin(yaw) * Math.cos(pitch), face * Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch)];
}
