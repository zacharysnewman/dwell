// Vertex arrays for debug line segments (F3 overlay). Vertex buffers are 32-bit floats, which step
// by 0.5 m ~8,000 km from the origin (ADR 0011), so vertices are stored relative to `origin` (the
// first segment's start) and the renderer places the lines there; the translation is combined with
// the camera's in double precision, like every other object.
import type { Vec3 } from '../protocol/messages';

export interface DebugSegment {
  from: Vec3;
  to: Vec3;
  color: number;
}

export interface DebugLineArrays {
  origin: Vec3;
  positions: Float32Array;
  /** Colour per vertex as 0xRRGGBB. */
  colors: number[];
}

export function debugLineArrays(segments: readonly DebugSegment[]): DebugLineArrays {
  const origin: Vec3 = segments[0] ? [...segments[0].from] : [0, 0, 0];
  const positions = new Float32Array(segments.length * 6);
  const colors: number[] = [];
  const [ox, oy, oz] = origin;
  segments.forEach((s, i) => {
    positions.set(
      [s.from[0] - ox, s.from[1] - oy, s.from[2] - oz, s.to[0] - ox, s.to[1] - oy, s.to[2] - oz],
      i * 6,
    );
    colors.push(s.color, s.color);
  });
  return { origin, positions, colors };
}
