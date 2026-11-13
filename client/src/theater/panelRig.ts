// A test rig for theater mode's panel (panel.test.ts, panel360.test.ts): a fake WASM client and
// the mode with the panels' owner over a canvas whose pointer lock is recorded. Not part of the game.

import presets from '../../../shared/theater/presets.json';
import { Panels } from '../ui/panels';
import { type Subject, TheaterMode, type TheaterModeClient } from './mode';
import { PathStore } from './store';

const PRESETS = JSON.stringify(presets);

/** A fake WASM client: a camera at the origin, a path that plays until `finish`. */
export function fakeClient(subjects: Subject[] = []) {
  const calls: string[] = [];
  const state = { playing: false, finished: false };
  const client: TheaterModeClient = {
    theaterFly: () => [0, 50, 0, 0, 0],
    theaterPlay: () => {
      calls.push('play');
      state.playing = true;
      state.finished = false;
    },
    theaterStop: () => {
      calls.push('stop');
      state.playing = false;
    },
    theaterPause: (p) => calls.push(p ? 'pause' : 'resume'),
    theaterExact: (on) => calls.push(`exact ${String(on)}`),
    theaterAdvance: (dt) => calls.push(`advance ${dt.toFixed(4)}`),
    theaterState: () => (state.playing ? { time: 1, finished: state.finished } : null),
    theaterSubjects: () => JSON.stringify(subjects),
    theaterPresets: () => PRESETS,
    theaterPreset: (name) =>
      JSON.stringify({
        formatVersion: 1,
        name,
        keyframes: [
          { t: 0, position: [0, 50, 0], look: [0, 0] },
          { t: 4, position: [4, 50, 0], look: [0, 0] },
        ],
        segments: [{ move: 'ease' }],
      }),
    theaterUp: () => undefined,
    releaseCamera: () => calls.push('release'),
    setTimeOfDay: () => undefined,
    creatures: () => [],
    placement: () => null,
  };
  return { client, calls, state };
}

/** Theater mode with the panels' owner and a canvas whose pointer lock is recorded. */
export function rig(subjects: Subject[] = [], document: Document = globalThis.document) {
  document.body.replaceChildren();
  const canvas = document.createElement('canvas');
  canvas.id = 'view';
  document.body.appendChild(canvas);
  const locks: number[] = [];
  const lockDoc = { pointerLockElement: null as Element | null, exitPointerLock: () => undefined };
  const target = {
    requestPointerLock: () => {
      locks.push(1);
      return Promise.resolve();
    },
  };
  const { client, calls, state } = fakeClient(subjects);
  let theater: TheaterMode | null = null;
  const panels = new Panels(lockDoc, target, {
    show: (p, open) => (p === 'theater' && theater ? theater.setShown(open) : false),
    escape: (p) => p === 'theater' && (theater?.escape() ?? false),
  });
  const memory = new Map<string, string>();
  theater = new TheaterMode({
    doc: document,
    canvas,
    client,
    store: new PathStore({
      getItem: (k) => memory.get(k) ?? null,
      setItem: (k, v) => memory.set(k, v),
    }),
    arms: () => undefined,
    touch: () => undefined,
    canvasSize: () => undefined,
    where: () => ({}),
    audioTap: () => null,
    panels: {
      open: () => {
        panels.open('theater');
      },
      close: () => {
        panels.close('theater');
      },
    },
  });
  return { theater, panels, locks, calls, state, bar: theater.bar };
}
