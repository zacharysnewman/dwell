import { PerspectiveCamera, Vector3 } from 'three';
import { describe, expect, it } from 'vitest';
import { aimCamera } from './aimCamera';

const dirs = (cam: PerspectiveCamera) => ({
  forward: cam.getWorldDirection(new Vector3()),
  up: new Vector3(0, 1, 0).applyQuaternion(cam.quaternion),
  right: new Vector3(1, 0, 0).applyQuaternion(cam.quaternion),
});
const close = (a: Vector3, b: Vector3): void => {
  expect(a.distanceTo(b)).toBeLessThan(1e-6);
};

describe('aimCamera: the half forward somersault across the midplane', () => {
  const yaw = 37;
  for (const pitch of [-80, -30, 0, 45]) {
    it(`pitch ${String(pitch)}°: no snap, the right axis stays, nose first`, () => {
      const eye: [number, number, number] = [1, 2, 3];
      const before = new PerspectiveCamera();
      aimCamera(before, eye, yaw, pitch, 1, 0);
      const b = dirs(before);

      // The crossing frame: heading turned by 180°, face −1, a flip of π: the same view.
      const after = new PerspectiveCamera();
      aimCamera(after, eye, yaw + 180, pitch, -1, Math.PI);
      const a = dirs(after);
      close(a.forward, b.forward);
      close(a.up, b.up);

      // The right vector is the same all the way, and the player's own left/right.
      for (const flip of [Math.PI, Math.PI / 2, 0]) {
        const cam = new PerspectiveCamera();
        aimCamera(cam, eye, yaw + 180, pitch, -1, flip);
        close(dirs(cam).right, b.right);
      }

      // Halfway the view looks along the old camera's down.
      const mid = new PerspectiveCamera();
      aimCamera(mid, eye, yaw + 180, pitch, -1, Math.PI / 2);
      close(dirs(mid).forward, b.up.clone().negate());
    });
  }
});
