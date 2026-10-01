// Rendering switches from the address, for comparing performance on a device (ARCHITECTURE.md
// §6.6): ?batch=1 draws the chunks and LOD sections in a few batches instead of a mesh each, and
// ?scale=0.5 renders at half the resolution (a frame rate that rises with it is bound by pixels,
// not by draw calls). The debug overlay (F3) shows the frame's draw calls and triangles.
import type { RenderStats } from './Renderer';

export interface DisplayOptions {
  batched: boolean;
  /** Multiplies the drawing buffer's pixel ratio. */
  scale: number;
}

export const SCALE_LIMITS = { min: 0.25, max: 2 } as const;

export function displayOptions(search: string): DisplayOptions {
  const params = new URLSearchParams(search);
  const scale = Number(params.get('scale') ?? 1);
  return {
    batched: params.get('batch') === '1',
    scale: Number.isFinite(scale)
      ? Math.min(SCALE_LIMITS.max, Math.max(SCALE_LIMITS.min, scale))
      : 1,
  };
}

/** The F3 line: "render 712 draws · 1.23 M tris · meshes · 2.00× pixels". */
export function formatRenderStats(s: RenderStats): string {
  const tris =
    s.triangles >= 1e6
      ? `${(s.triangles / 1e6).toFixed(2)} M`
      : `${(s.triangles / 1e3).toFixed(0)} k`;
  return (
    `render ${String(s.calls)} draws · ${tris} tris · ${s.batched ? 'batched' : 'meshes'} · ` +
    `${s.pixelRatio.toFixed(2)}× pixels`
  );
}
