// The message shown when the game cannot start (the "unsupported" screen, ADR 0028): WebGPU missing
// altogether, or present without a usable adapter or device (e.g. a blocklisted GPU), reads as
// "needs WebGPU"; anything else names the error. A browser without WebAssembly SIMD is told it is
// too old before the module is loaded (NEEDS_SIMD).

/** A browser whose WebAssembly has no SIMD (engine/simd.ts), which the game's module needs. */
export const NEEDS_SIMD =
  'This browser is too old for Dwell, which needs WebAssembly SIMD: try Safari 16.4 or later, Chrome 91 or later, or Firefox 89 or later.';

export const NEEDS_WEBGPU =
  'Dwell needs WebGPU, which this browser does not have (or has turned off). Try a current Chrome, Edge or Safari.';

/** Whether a start-up failure means WebGPU is unavailable (dwell-client's request_device errors). */
export function isWebGpuUnavailable(err: unknown, hasGpu: boolean): boolean {
  const message = err instanceof Error ? err.message : String(err);
  return !hasGpu || /no WebGPU (adapter|device)/.test(message);
}

export function startupMessage(err: unknown, hasGpu: boolean): string {
  if (isWebGpuUnavailable(err, hasGpu)) return NEEDS_WEBGPU;
  return `Dwell failed to start: ${err instanceof Error ? err.message : String(err)}`;
}
