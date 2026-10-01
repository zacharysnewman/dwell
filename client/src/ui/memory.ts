// The debug overlay's memory line (F3): what the game itself knows it holds, as a phone's browser
// may close the tab, without warning, when the page uses too much (mobile Safari reloads it). The
// WebAssembly memories (the client's sim core, a local world's server core, the terrain-generation
// workers) and the GPU's terrain geometry and drawing buffers are counted; the JavaScript heap is
// shown where the browser tells it (Chrome), and not added in: it may include the main thread's
// WebAssembly memory. The browser's own overhead is not known.

export interface MemoryReport {
  /** The client's sim core (main thread). */
  coreBytes: number;
  /** A local world's server core (its worker); null when playing on a server. */
  serverBytes: number | null;
  /** Each terrain-generation worker's generator. */
  worldgenBytes: number[];
  /** GPU: terrain and LOD geometry, and the drawing buffers (an estimate). */
  meshBytes: number;
  screenBytes: number;
  /** The main thread's JavaScript heap, where the browser tells it (Chrome); else null. */
  jsHeapBytes: number | null;
}

/** The bytes the report counts (WebAssembly and GPU). */
export function memoryTotal(r: MemoryReport): number {
  return (
    r.coreBytes +
    (r.serverBytes ?? 0) +
    r.worldgenBytes.reduce((a, b) => a + b, 0) +
    r.meshBytes +
    r.screenBytes
  );
}

const mb = (bytes: number): string => String(Math.round(bytes / 1048576));

/** "memory 612 MB · core 64 · world 180 · worldgen 3×16 · GPU 120 + screen 30 · JS 210". */
export function formatMemory(r: MemoryReport): string {
  const parts = [`memory ${mb(memoryTotal(r))} MB`, `core ${mb(r.coreBytes)}`];
  if (r.serverBytes !== null) parts.push(`world ${mb(r.serverBytes)}`);
  const gen = r.worldgenBytes;
  if (gen.length > 0) {
    const same = gen.every((b) => b === gen[0]);
    parts.push(
      same
        ? `worldgen ${String(gen.length)}×${mb(gen[0] ?? 0)}`
        : `worldgen ${gen.map(mb).join('+')}`,
    );
  }
  parts.push(`GPU ${mb(r.meshBytes)} + screen ${mb(r.screenBytes)}`);
  if (r.jsHeapBytes !== null) parts.push(`JS ${mb(r.jsHeapBytes)}`);
  return parts.join(' · ');
}

/** The main thread's JavaScript heap where the browser reports it (Chrome's performance.memory). */
export function jsHeapBytes(): number | null {
  const memory = (performance as Performance & { memory?: { usedJSHeapSize: number } }).memory;
  return memory ? memory.usedJSHeapSize : null;
}
