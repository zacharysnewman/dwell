// Types of fluids.mjs, for the client's tests (client/src/world/fluids.test.ts).
export interface Fluid {
  name: string;
  opacity: number;
  surfaceDrop: number;
  absorption: [number, number, number];
  scatter: string;
  roughness: number;
  ior: number;
  normalScale: number;
  waveScale: number;
  flowSpeed: number;
  foam: { depth: number; color: string };
  caustics: number;
}
export function checkFluid(name: string, f: unknown, namespace?: string): Fluid;
export function readFluids(namespaces: readonly { namespace: string; fluids?: Record<string, unknown> }[]): Fluid[];
export function blockFluid(id: string, def: Record<string, unknown>, fluids: readonly Fluid[]): number | null;
export function fluidsRust(fluids: readonly Fluid[], rf: (v: number) => string): string[];
