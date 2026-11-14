// The world map stage's progress inside each face's build (BUGS.md #91, owner 2026-11-12: "up to 3
// minutes so far on 'Shaping the world map...' 0/2. Doesn't indicate well what it's doing."). Each
// face's builder reports its step (an index of `MAP_STEPS`, shared/look/loading.json `mapSteps`)
// and the fraction of it done; the stage's count then moves in fractions of a face, and its line
// says which step: "Shaping the world map — rivers (5/6), face 1 of 2".
import type { FaceBuild } from '../world/worldMaps';
import { MAP_STEPS } from './loading.gen';

/** The stage's progress: faces done in fractions (of `needed`), and the line's detail. */
export interface MapProgress {
  done: number;
  detail: string | null;
}

/** How far one face's build is, 0..1: its steps done and the running one's fraction. */
export function faceFraction(f: FaceBuild, steps = MAP_STEPS.length): number {
  if (f.done) return 1;
  if (f.step === null || steps <= 0) return 0;
  const step = Math.min(Math.max(0, f.step), steps - 1);
  return Math.min(1, (step + Math.min(1, Math.max(0, f.fraction))) / steps);
}

/**
 * The stage's count and detail from each face's build: the detail names the step of the face
 * furthest behind among those building (the one the wait is for), with its place in the steps and
 * the face's among the faces; none once every face is done or before any reports.
 */
export function mapProgress(
  faces: readonly FaceBuild[],
  steps: readonly { label: string }[] = MAP_STEPS,
): MapProgress {
  const done = faces.reduce((sum, f) => sum + faceFraction(f, steps.length), 0);
  let behind: FaceBuild | null = null;
  for (const f of faces) {
    if (f.done || f.step === null) continue;
    if (!behind || faceFraction(f, steps.length) < faceFraction(behind, steps.length)) behind = f;
  }
  if (!behind || behind.step === null) return { done, detail: null };
  const k = Math.min(Math.max(0, behind.step), steps.length - 1);
  const label = steps[k]?.label ?? '';
  const face =
    faces.length > 1 ? `, face ${String(behind.face + 1)} of ${String(faces.length)}` : '';
  return { done, detail: `${label} (${String(k + 1)}/${String(steps.length)})${face}` };
}
