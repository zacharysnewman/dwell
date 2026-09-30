// The view the LOD octree is walked for (§6.6): a camera anywhere in the world (the player's eye
// or the dev camera), its view frustum as planes, and the screen-space size of a cell.
import type { Vec3 } from '../protocol/messages';

export interface LodCamera {
  position: Vec3;
  /** Degrees; yaw 0 = +Z, pitch up > 0 (as Renderer.setCamera). */
  yawDeg: number;
  pitchDeg: number;
  /** Vertical field of view (degrees), aspect (width / height), and drawing height in pixels. */
  fovYDeg: number;
  aspect: number;
  heightPx: number;
  /** Camera velocity (m/s): detail loads ahead along it (see LodSystem). */
  velocity?: Vec3;
}

/** Inward plane normals through the camera (sides) and the near plane, for AABB tests. */
export class Frustum {
  private readonly planes: Vec3[];
  /** Pixels per metre at 1 m distance: heightPx / (2 tan(fovY / 2)). */
  readonly pixelsPerRadian: number;

  constructor(readonly camera: LodCamera) {
    const yaw = (camera.yawDeg * Math.PI) / 180;
    const pitch = (camera.pitchDeg * Math.PI) / 180;
    const f: Vec3 = [
      Math.sin(yaw) * Math.cos(pitch),
      Math.sin(pitch),
      Math.cos(yaw) * Math.cos(pitch),
    ];
    // Horizontal right vector from the yaw alone (f × up, but defined looking straight down too).
    const r: Vec3 = [-Math.cos(yaw), 0, Math.sin(yaw)];
    const u = cross(r, f);
    const ty = Math.tan((camera.fovYDeg * Math.PI) / 360) * 1.05;
    const tx = ty * camera.aspect;
    const combine = (a: Vec3, sa: number, b: Vec3, sb: number): Vec3 => [
      a[0] * sa + b[0] * sb,
      a[1] * sa + b[1] * sb,
      a[2] * sa + b[2] * sb,
    ];
    this.planes = [
      combine(f, tx, r, 1),
      combine(f, tx, r, -1),
      combine(f, ty, u, 1),
      combine(f, ty, u, -1),
      f,
    ];
    this.pixelsPerRadian = camera.heightPx / (2 * Math.tan((camera.fovYDeg * Math.PI) / 360));
  }

  /** True when the box [lo, hi] (world metres) may be in view. */
  intersects(lo: Vec3, hi: Vec3): boolean {
    const p = this.camera.position;
    for (const n of this.planes) {
      // The box corner furthest along the normal must be on the inner side.
      const x = (n[0] >= 0 ? hi[0] : lo[0]) - p[0];
      const y = (n[1] >= 0 ? hi[1] : lo[1]) - p[1];
      const z = (n[2] >= 0 ? hi[2] : lo[2]) - p[2];
      if (n[0] * x + n[1] * y + n[2] * z < 0) return false;
    }
    return true;
  }

  /** Distance from the camera to the box (0 inside it). */
  distance(lo: Vec3, hi: Vec3): number {
    const p = this.camera.position;
    let d = 0;
    for (let a = 0; a < 3; a++) {
      const v = p[a] ?? 0;
      const e = v < (lo[a] ?? 0) ? (lo[a] ?? 0) - v : v > (hi[a] ?? 0) ? v - (hi[a] ?? 0) : 0;
      d += e * e;
    }
    return Math.sqrt(d);
  }
}

function cross(a: Vec3, b: Vec3): Vec3 {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}
