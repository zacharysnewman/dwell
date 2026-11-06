// The status line's performance figures (owner, 2026-11-06: "so we can tell what's chugging instead of
// just one all powerful number"): frames drawn a second, the CPU's and the GPU's time per frame, and
// the server's ticks a second and (for a local world, whose server runs in this page's worker) the
// time a tick takes. At 60 a second each has 16.7 ms; the one over it is what holds the frame back.

export interface PerfFigures {
  /** Frames drawn a second (ui/fps.ts). */
  fps: number | null;
  /** The game's time on the main thread per frame (ms). */
  cpuMs: number | null;
  /** The GPU's time per frame (ms); null where the browser can't time it. */
  gpuMs: number | null;
  /** The server's ticks a second (60 when it keeps up). */
  serverTicks: number | null;
  /** The server's time per tick (ms): a local world's only. */
  serverTickMs: number | null;
}

const ms = (v: number | null): string =>
  v === null || !Number.isFinite(v) ? '–' : `${v.toFixed(1)} ms`;

/** One line, e.g. "30 fps · CPU 4.1 ms · GPU 31.2 ms · server 60/s, 2.8 ms". */
export function formatPerf(p: PerfFigures): string {
  const fps = p.fps === null ? '– fps' : `${String(Math.round(p.fps))} fps`;
  let server =
    p.serverTicks === null ? 'server –' : `server ${String(Math.round(p.serverTicks))}/s`;
  if (p.serverTickMs !== null && Number.isFinite(p.serverTickMs))
    server += `, ${ms(p.serverTickMs)}`;
  return `${fps} · CPU ${ms(p.cpuMs)} · GPU ${ms(p.gpuMs)} · ${server}`;
}

/**
 * The server's ticks a second, from the tick number it sends (any server: no message needed),
 * over each full second, as the frame rate is.
 */
export class TickRate {
  private start: { at: number; tick: number } | null = null;
  private value: number | null = null;

  sample(nowMs: number, serverTick: number): void {
    if (this.start === null || serverTick < this.start.tick) {
      this.start = { at: nowMs, tick: serverTick };
      return;
    }
    const elapsed = nowMs - this.start.at;
    if (elapsed >= 1000) {
      this.value = ((serverTick - this.start.tick) * 1000) / elapsed;
      this.start = { at: nowMs, tick: serverTick };
    }
  }

  get rate(): number | null {
    return this.value;
  }
}
