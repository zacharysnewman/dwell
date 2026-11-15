import { describe, expect, it } from 'vitest';
import data from '../../../shared/theater/cinematic.json';
import {
  CINEMATIC,
  SettleGate,
  eventsDue,
  frameName,
  frameTimeS,
  parseCinematic,
  parseEvents,
  percentile,
  runCinematic,
  ticksDue,
  viewLoaded,
  withOverrides,
  type CinematicHost,
  type ViewState,
} from './cinematic';
import {
  chunkSamples,
  findMarks,
  firstAbove,
  impulseErrors,
  joinChunks,
  realign,
  wavBytes,
} from './cinematicAudio';
import type { ClearanceWorld } from './clearance';

const loaded: ViewState = { lod: { waitingInView: 0 }, terrain: { dirty: 0, meshing: 0 } };
const loading: ViewState = { lod: { waitingInView: 300 }, terrain: { dirty: 0, meshing: 0 } };

describe('cinematic settings (shared/theater/cinematic.json)', () => {
  it('the data file validates', () => {
    expect(CINEMATIC.fps).toBe(30);
    expect(CINEMATIC.supersample).toBeGreaterThanOrEqual(1);
  });
  it('a bad entry fails with a message', () => {
    expect(() => parseCinematic({ ...data, fps: 0 })).toThrow(
      /fps must be an integer in \[1, 240\]/,
    );
    expect(() => parseCinematic({ ...data, subframes: 1.5 })).toThrow(
      /subframes must be an integer/,
    );
    expect(() => parseCinematic({ ...data, extra: 1 })).toThrow(/unknown key extra/);
    expect(() => parseCinematic({ ...data, image: 'image/jpeg' })).toThrow(/image must be/);
    expect(() =>
      parseCinematic(
        withOverrides(data as Record<string, unknown>, { quality: { shadows: 'ultra' } }),
      ),
    ).toThrow(/quality.shadows/);
  });
  it("a take's overrides merge into the data (objects merge, values replace)", () => {
    const c = parseCinematic(
      withOverrides(data, {
        fps: 60,
        subframes: 4,
        settle: { capMs: 100 },
        audio: { on: false },
      }),
    );
    expect(c.fps).toBe(60);
    expect(c.subframes).toBe(4);
    expect(c.settle.capMs).toBe(100);
    expect(c.settle.minFrames).toBe(CINEMATIC.settle.minFrames);
    expect(c.audio).toEqual({ on: false, leadS: CINEMATIC.audio.leadS });
  });
});

describe('the frame clock', () => {
  it('game time per frame is exact over a long take (no drift)', () => {
    // An hour at 30 fps: frame 108000 is exactly 3600 s, and every frame is 1/30 s after the last.
    expect(frameTimeS(108_000, 0, 30, 1)).toBe(3600);
    for (let i = 1; i <= 108_000; i += 997)
      expect(Math.abs(frameTimeS(i, 0, 30, 1) - frameTimeS(i - 1, 0, 30, 1) - 1 / 30)).toBeLessThan(
        1e-9,
      );
    // Sub-frames split a frame evenly: 4 of them at 30 fps are 1/120 s apart.
    expect(frameTimeS(10, 2, 30, 4)).toBeCloseTo(10 / 30 + 2 / 120, 12);
  });
  it('the world steps exactly the ticks of its frames: 2 a frame at 30 fps, 1800 over 30 s', () => {
    let ticks = 0;
    const steps: number[] = [];
    for (let i = 1; i <= 900; i++) {
      const due = ticksDue(frameTimeS(i, 0, 30, 1), 60) - ticks;
      steps.push(due);
      ticks += due;
    }
    expect(ticks).toBe(1800);
    expect(new Set(steps)).toEqual(new Set([2]));
    // 24 fps: 2.5 ticks a frame, alternating 2 and 3, 60 a second exactly.
    let t24 = 0;
    for (let i = 1; i <= 24 * 60; i++) t24 += ticksDue(frameTimeS(i, 0, 24, 1), 60) - t24;
    expect(t24).toBe(3600);
  });
});

describe('the settle gate', () => {
  it('waits for the view to be loaded on minFrames drawn frames in a row', () => {
    const g = new SettleGate(2);
    g.start(0, 1000);
    expect(g.update(loading, true, 10)).toBe('wait');
    expect(g.update(loaded, true, 20)).toBe('wait');
    // a skipped (undrawn) frame neither counts nor breaks the run
    expect(g.update(loaded, false, 25)).toBe('wait');
    expect(g.update(loaded, true, 30)).toBe('ready');
    g.start(100, 1000);
    expect(g.update(loaded, true, 110)).toBe('wait');
    expect(g.update(loading, true, 120)).toBe('wait');
    expect(g.update(loaded, true, 130)).toBe('wait');
    expect(g.update(loaded, true, 140)).toBe('ready');
  });
  it('captures anyway at the cap, and says so', () => {
    const g = new SettleGate(1);
    g.start(0, 500);
    expect(g.update(loading, true, 499)).toBe('wait');
    expect(g.update(loading, false, 600)).toBe('wait');
    expect(g.update(loading, true, 600)).toBe('capped');
    expect(g.waitedMs(600)).toBe(600);
  });
  it('a view with no LOD report is not loaded', () => {
    expect(viewLoaded(null)).toBe(false);
    expect(viewLoaded({ lod: { waitingInView: 0 }, terrain: { dirty: 1, meshing: 0 } })).toBe(
      false,
    );
    expect(viewLoaded(loaded)).toBe(true);
    // a small residue allowed by the data
    expect(viewLoaded(loading, 299)).toBe(false);
    expect(viewLoaded(loading, 300)).toBe(true);
  });
});

it('frame names and percentiles', () => {
  expect(frameName(7, 'image/png')).toBe('f00007.png');
  expect(percentile([5, 1, 3, 2, 4], 0.95)).toBe(5);
  expect(percentile([], 0.5)).toBe(0);
});

/** A fake page: the view loads `loadFrames` frames after each step; the canvas is a stand-in. */
function fakeHost(loadFrames: number, capAt: number[] = []) {
  let wall = 0;
  let clock = 1000;
  let sinceStep = 0;
  let frame = 0;
  const log = {
    steps: [] as number[],
    clocks: [] as number[],
    advances: [] as number[],
    files: [] as string[],
    captures: 0,
  };
  const host: CinematicHost = {
    holdClock: () => clock,
    stepClockTo: (ms) => {
      clock = ms;
      log.clocks.push(ms);
    },
    world: {
      hold: () => undefined,
      step: (n) => {
        log.steps.push(n);
        sinceStep = 0;
        frame++;
        return Promise.resolve(0);
      },
    },
    theaterExact: () => undefined,
    theaterAdvance: (dt) => {
      log.advances.push(dt);
    },
    theaterPlay: () => undefined,
    theaterStop: () => undefined,
    theaterState: () => ({ time: 0, duration: 1, finished: false }),
    state: () => (sinceStep >= loadFrames && !capAt.includes(frame) ? loaded : loading),
    setView: () => undefined,
    setShadows: () => undefined,
    canvas: {} as HTMLCanvasElement,
    canvasSize: () => undefined,
    nextFrame: (capture) => {
      wall += 50;
      sinceStep++;
      if (capture) log.captures++;
      return Promise.resolve(true);
    },
    sink: (file) => {
      log.files.push(file);
      return Promise.resolve();
    },
    audio: () => Promise.resolve(null),
    wallMs: () => wall,
    clearance: null,
  };
  return { host, log };
}
describe('the take', () => {
  const offscreen = globalThis.OffscreenCanvas;
  it('steps the world and the clock exactly per frame, settles each, captures each once', async () => {
    // a stand-in OffscreenCanvas for node
    (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas = class {
      getContext() {
        return {
          drawImage: () => undefined,
          globalAlpha: 1,
          transferFromImageBitmap: () => undefined,
        };
      }
      transferToImageBitmap() {
        return {};
      }
      convertToBlob() {
        return Promise.resolve(new Blob([new Uint8Array([1])]));
      }
    };
    try {
      const { host, log } = fakeHost(3, [4]);
      const r = await runCinematic(host, {
        path: {},
        size: [16, 9],
        settings: { settle: { capMs: 400 }, preroll: { stepS: 0 }, audio: { on: false } },
      });
      // 1 s at 30 fps: 31 frames (both ends), 30 steps of 2 ticks
      expect(r.frames).toBe(31);
      expect(log.steps).toEqual(Array(30).fill(2));
      expect(r.ticks).toBe(60);
      expect(log.clocks.at(-1)).toBe(1000 + 1000);
      expect(log.advances.reduce((a, b) => a + b, 0)).toBeCloseTo(1, 12);
      expect(log.files).toEqual(Array.from({ length: 31 }, (_, i) => frameName(i, 'image/png')));
      expect(log.captures).toBe(31);
      // frame 4 never loaded: captured at the cap, and reported
      expect(r.settle.capped).toEqual([4]);
    } finally {
      (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas = offscreen;
    }
  });
});

describe("a take's timed events (§6.3)", () => {
  it('fire before the world steps past them, once each, in time order', () => {
    const ev = [
      { atS: 3, op: 'scene', arg: '0' },
      { atS: 0, op: 'scene', arg: '1' },
    ];
    // at 30 fps frame 90 is exactly 3 s: the step from 89 to 90 doesn't run it, 90 to 91 does
    expect(eventsDue(ev, frameTimeS(89, 0, 30, 1), frameTimeS(90, 0, 30, 1))).toEqual([]);
    expect(eventsDue(ev, frameTimeS(90, 0, 30, 1), frameTimeS(91, 0, 30, 1))).toEqual([ev[0]]);
    expect(eventsDue(ev, 0, 1 / 30)).toEqual([ev[1]]);
    expect(eventsDue(ev, 0, 10).map((e) => e.arg)).toEqual(['1', '0']);
  });
  it('are checked', () => {
    expect(parseEvents(undefined)).toEqual([]);
    expect(() => parseEvents({})).toThrow('expected a list');
    expect(() => parseEvents([{ atS: -1, op: 'scene' }])).toThrow('events[0]');
    expect(parseEvents([{ atS: 1, op: 'scene' }])).toEqual([{ atS: 1, op: 'scene', arg: '' }]);
  });
  it('run on their frame of a take, between its world steps', async () => {
    const offscreen = globalThis.OffscreenCanvas;
    (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas = class {
      getContext() {
        return {
          drawImage: () => undefined,
          globalAlpha: 1,
          transferFromImageBitmap: () => undefined,
        };
      }
      transferToImageBitmap() {
        return {};
      }
      convertToBlob() {
        return Promise.resolve(new Blob([new Uint8Array([1])]));
      }
    };
    try {
      const { host, log } = fakeHost(1);
      const ran: [string, string, number][] = [];
      host.event = (op, arg) => {
        ran.push([op, arg, log.steps.length]);
        return Promise.resolve('{"removed":3}');
      };
      const r = await runCinematic(host, {
        path: {},
        size: [16, 9],
        settings: { preroll: { stepS: 0 }, audio: { on: false } },
        events: [{ atS: 0.5, op: 'scene', arg: '0' }],
      });
      // frame 15 is 0.5 s: 15 steps made before it runs, and frame 16 is the first to show it
      expect(ran).toEqual([['scene', '0', 15]]);
      expect(r.events).toEqual([{ atS: 0.5, frame: 16, op: 'scene', result: '{"removed":3}' }]);
      delete host.event;
      await expect(
        runCinematic(host, {
          path: {},
          size: [16, 9],
          events: [{ atS: 0, op: 'scene', arg: '0' }],
        }),
      ).rejects.toThrow('runs no events');
    } finally {
      (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas = offscreen;
    }
  });
});

describe("the take's camera clearance (§6.2)", () => {
  const offscreen = globalThis.OffscreenCanvas;
  /** A world where every eye is in solid ground (the lift that clears it 2 m). */
  const walled: ClearanceWorld = {
    samples: (_json, stepS) => {
      const out: number[] = [];
      for (let t = 0; t <= 1 + 1e-9; t += stepS) out.push(t, t, t, 5, 0);
      return out;
    },
    probe: (eyes) => {
      const n = eyes.length / 3;
      return Promise.resolve({ codes: Array(n).fill(1), lifts: Array(n).fill(2) });
    },
  };
  it('a path through solid ground fails before the pre-roll; allowClipping takes it and reports the clip', async () => {
    (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas = class {
      getContext() {
        return {
          drawImage: () => undefined,
          globalAlpha: 1,
          transferFromImageBitmap: () => undefined,
        };
      }
      transferToImageBitmap() {
        return {};
      }
      convertToBlob() {
        return Promise.resolve(new Blob([new Uint8Array([1])]));
      }
    };
    try {
      const settings = { audio: { on: false }, preroll: { stepS: 0.5 } };
      const a = fakeHost(0);
      a.host.clearance = walled;
      await expect(runCinematic(a.host, { path: {}, size: [16, 9], settings })).rejects.toThrow(
        /cinematic: the camera path passes through solid ground .*\n.*lift the camera ≥ 2\.0 m/,
      );
      expect(a.log.advances).toEqual([]); // nothing flown
      expect(a.log.files).toEqual([]);
      const b = fakeHost(0);
      b.host.clearance = walled;
      const r = await runCinematic(b.host, {
        path: {},
        size: [16, 9],
        settings,
        allowClipping: true,
      });
      expect(r.clearance?.clipping).toHaveLength(1);
      expect(r.clearance?.unchecked).toBe(0);
      expect(b.log.files).toHaveLength(31);
    } finally {
      (globalThis as { OffscreenCanvas?: unknown }).OffscreenCanvas = offscreen;
    }
  });
});

describe("the take's sound", () => {
  it('joins the recorded blocks in order, from a start sample, padded with silence', () => {
    const block = (v: number): { channels: Float32Array[] } => ({
      channels: [new Float32Array(4).fill(v)],
    });
    const chunks = [block(1), block(2), block(3)];
    expect(chunkSamples(chunks)).toBe(12);
    const [ch] = joinChunks(chunks, 1, 3, 12);
    expect(Array.from(ch ?? [])).toEqual([1, 2, 2, 2, 2, 3, 3, 3, 3, 0, 0, 0]);
  });
  it('an event at game time t is found at t (±1 ms) over a long take, and a late one is measured', () => {
    const sr = 48_000;
    const fps = 30;
    const frames = 9000; // 300 s
    const n = Math.round((frames / fps) * sr);
    const ch = new Float32Array(n);
    const expected: number[] = [];
    for (let i = 0; i < frames; i++) {
      const at = Math.round((i / fps) * sr);
      expected.push(at);
      ch[at + (i === 5000 ? 96 : 0)] = 1; // frame 5000's mark 2 ms late
    }
    const errs = impulseErrors(ch, expected, sr);
    expect(errs.length).toBe(frames);
    const others = errs.filter((_, i) => i !== 5000);
    expect(Math.max(...others.map(Math.abs))).toBeLessThanOrEqual(1);
    expect(errs[5000]).toBeCloseTo(2, 6);
    expect(firstAbove(ch, 1)).toBe(expected[1]);
  });
  it('a stream the recorder slipped is re-timed on its frame marks: every event back on its frame', () => {
    const sr = 48_000;
    const fps = 30;
    const frames = 300;
    const expected = Array.from({ length: frames }, (_, i) => Math.round((i / fps) * sr));
    const n = Math.round((frames / fps) * sr);
    // a recording that lost 768 samples at frame 100 and gained 256 at frame 200 (a block across a suspend)
    const slip = (i: number): number => (i >= 200 ? -512 : i >= 100 ? -768 : 0);
    const marksCh = new Float32Array(n + sr);
    const sound = new Float32Array(n + sr);
    for (let i = 0; i < frames; i++) {
      const at = (expected[i] ?? 0) + slip(i);
      marksCh[at] = 1;
      if (i % 30 === 7) sound[at] = 0.9; // a sound triggered on frame i
    }
    expect(Math.max(...impulseErrors(marksCh, expected, sr).map(Math.abs))).toBeGreaterThan(10);
    const marks = findMarks(marksCh, expected);
    const [snd, m] = realign([sound, marksCh], marks, expected, n);
    expect(
      Math.max(...impulseErrors(m ?? new Float32Array(0), expected, sr).map(Math.abs)),
    ).toBeLessThanOrEqual(1);
    for (let i = 7; i < frames; i += 30) expect(snd?.[expected[i] ?? 0]).toBeCloseTo(0.9, 6);
    expect(snd?.length).toBe(n);
  });
  it("the track's length is the frames' length: a 16-bit stereo WAV of exactly frames / fps", () => {
    const sr = 48_000;
    const n = Math.round((301 / 30) * sr);
    const wav = wavBytes([new Float32Array(n), new Float32Array(n)], sr);
    const v = new DataView(wav.buffer);
    expect(String.fromCharCode(...wav.subarray(0, 4))).toBe('RIFF');
    expect(v.getUint32(24, true)).toBe(sr);
    expect(v.getUint32(40, true) / 4 / sr).toBeCloseTo(301 / 30, 6);
  });
});
