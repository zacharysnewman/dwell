// The client's start as loading-screen stages (ARCHITECTURE.md §6.3): the module's download and
// compile ('game'), then the graphics ('graphics'). Owner (2026-11-11): "the loading game (xMB)
// step still takes like 10+ seconds sometimes and shows as 0.0s on the loading screen". The time
// was the graphics': `Client.create` makes the renderer's pipelines in one long task on the main
// thread (seconds on a cold GPU shader cache), and it began in the same task that ended 'game',
// so the browser never painted "Starting the graphics…": the screen stood on "Loading the game…
// x MB 0.0 s" until it was done. Each stage is now painted before its work starts.
//
// Bug #71 (owner, 2026-11-11: "'Starting the graphics' now takes a long time without showing
// elapsed time"): measured, `create` holds the main thread ~40 ms; the time is the GPU's. On a cold
// shader cache the request for the device takes ~1.7 s, and then the browser's GPU process compiles
// the renderer's pipelines for ~9 s (Metal, M-series), during which the page paints no frame at
// all, though its main thread is free: the screen stood on "Starting the graphics… 1.8 s". That
// compile is its own stage now, 'shaders', from `create`'s end until the GPU has done the work
// sent so far (`settled`); it runs beside the world's stages rather than holding them back.
import type { WasmBytes } from '../engine/wasm';
import type { StageTracker } from './loadingStages';

export interface StartSteps<C> {
  /** Downloads and compiles the module, counting its bytes. */
  load(onBytes: WasmBytes): Promise<void>;
  /** Makes the client (its graphics): may block the main thread for seconds. */
  create(): Promise<C>;
  /** Draws the screen now and resolves once the browser has painted it (`paintedFrame`). */
  painted(): Promise<void>;
  /** Resolves once the GPU has done the work `create` sent (its pipelines compiled). */
  settled(client: C): Promise<void>;
  /** The page's clock (ms). */
  now(): number;
}

/** Runs the start's 'game' and 'graphics' stages, each painted on the screen before it works. */
export async function runStartStages<C>(stages: StageTracker, steps: StartSteps<C>): Promise<C> {
  stages.begin('game', steps.now());
  await steps.painted();
  await steps.load((done, total) => {
    stages.progress('game', done, total, steps.now());
  });
  stages.end('game', steps.now());
  stages.begin('graphics', steps.now());
  await steps.painted();
  const client = await steps.create();
  stages.end('graphics', steps.now());
  // Not awaited: the world's stages (its file, maps, server) run on the CPU meanwhile.
  stages.begin('shaders', steps.now());
  void steps.settled(client).then(
    () => {
      stages.end('shaders', steps.now());
    },
    () => {
      stages.end('shaders', steps.now());
    },
  );
  return client;
}

/**
 * Resolves after the browser's next paint: a frame callback, then a task after it (the paint
 * comes between), or `timeoutMs` (a background tab has no frames).
 */
export function paintedFrame(
  raf: (cb: () => void) => unknown = (cb) => requestAnimationFrame(cb),
  later: (cb: () => void, ms: number) => unknown = (cb, ms) => setTimeout(cb, ms),
  timeoutMs = 100,
): Promise<void> {
  return new Promise((resolve) => {
    raf(() => later(resolve, 0));
    later(resolve, timeoutMs);
  });
}
