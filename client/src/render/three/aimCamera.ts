import type { PerspectiveCamera } from 'three';
import { Vector3 } from 'three';
import type { Vec3 } from '../../protocol/messages';
import { viewForward, type FaceSign } from '../../world/face';

/**
 * Aims the camera: at `eye`, along yaw/pitch for the player's `face` (up is ±y). `flip` (radians, 0
 * at rest) then pitches the view over about its own right axis, nose first: the camera turning over
 * across the midplane (game/faceFlip.ts). The heading has already turned by 180°, so a flip of π
 * is exactly the view before the crossing.
 */
export function aimCamera(
  camera: PerspectiveCamera,
  eye: Vec3,
  yawDeg: number,
  pitchDeg: number,
  face: FaceSign,
  flip: number,
): void {
  camera.position.set(...eye);
  camera.up.set(0, face, 0); // a face-B player's head points toward −y
  camera.updateMatrixWorld();
  camera.lookAt(camera.position.clone().add(new Vector3(...viewForward(yawDeg, pitchDeg, face))));
  if (flip !== 0) camera.rotateX(flip); // the camera's local X is its right axis
  camera.updateMatrixWorld();
}
