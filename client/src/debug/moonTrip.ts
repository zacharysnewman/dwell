// The bench's moon trip (docs/DEV_TOOLS.md "The moon", MOON_WORLD.md §7.3): `__dwell.flyTo('moon')`
// puts the local player just outside the moon's capture sphere with the dev move (local.rs
// `moon_approach`), flies it in for real until it is captured, down to the moon's ground, measures
// the fall with flight off and lands; `__dwell.flyTo('home')` flies up and out past the release
// sphere, moves the player back to where the trip started and waits for home's chunks. The page's
// side is a host (main.ts `tripHost`), so the steering is tested here without a browser.

export type Vec3 = [number, number, number];

/** `__dwell.moon()` (moon_probe.rs `MoonReport::json`). */
export interface MoonReport {
  /** The moon's grid index (grids.json). */
  index: number;
  /** Its centre (world m) at the clock's time. */
  position: Vec3;
  /** The player's feet (world m). */
  feet: Vec3;
  /** Unit world direction from the feet to the moon's centre. */
  direction: Vec3;
  /** Feet to the moon's centre (m). */
  distance: number;
  captureM: number;
  releaseM: number;
  /** The player's grid now: `home` or `moon`. */
  grid: string;
  /** The moon's gravity (m/s²). */
  gravity: number;
  /** The gravity where the player is (m/s²). */
  gravityNow: number;
  /** Toward the moon's centre, face-local degrees (yaw 0 = +Z, 90 = +X). */
  look: { yaw: number; pitch: number };
  /** Velocity along the face's up (m/s). */
  upMps: number;
  /** On the moon: the feet's height over its ground (m); null at home. */
  groundM: number | null;
  /** The client holds the chunk under the feet. */
  groundHeld: boolean;
}

export interface TripHost {
  moon(): MoonReport | null;
  /** local.rs `moon_approach` JSON; null outside a local world. */
  approach(marginM: number, face: number): Promise<string | null>;
  /** local.rs `place_home` JSON; null outside a local world. */
  placeHome(feet: Vec3): Promise<string | null>;
  press(code: string, down: boolean): void;
  fly(on: boolean): void;
  flySpeed(level: number): void;
  look(yaw: number, pitch: number): void;
  wait(ms: number): Promise<void>;
  /** Milliseconds, monotonic. */
  now(): number;
}

export interface TripOptions {
  /** The moon's face to land on: `a` (default) or `b`. */
  face?: 'a' | 'b';
  /** How far outside the capture sphere the dev move puts the player (m; default 2000). */
  marginM?: number;
  /** The height over the moon's ground where flight stops and the fall is measured (m; 40). */
  landM?: number;
  /** The fall's acceleration may differ from the moon's gravity by this share (0.25). */
  tolerance?: number;
  /** Each leg's time limit (s; 120). */
  maxS?: number;
}

export interface MoonTrip {
  to: 'moon';
  /** Where the trip started (world feet), for `flyTo('home')`. */
  home: Vec3 | null;
  /** The dev move's answer (local.rs `moon_approach`). */
  approach: unknown;
  /** Seconds of real flight from the dev move to the capture, from there down to `landM`, and
   * from flight off to standing on the ground. */
  captureS: number;
  descentS: number;
  landS: number;
  /** The measured fall (m/s²) and the moon's gravity it is checked against. */
  fallMps2: number;
  gravity: number;
  /** World feet on the moon's ground. */
  at: Vec3;
}

export interface HomeTrip {
  to: 'home';
  /** Seconds of flight up to the release (0 when already home), then from the move back to the
   * trip's start until the client holds the chunk under the feet again. */
  releaseS: number;
  returnS: number;
  at: Vec3;
}

/** Faster than every other fly speed: the trip's (the slider's top, flySpeedMaxLevel). */
export const FAST_LEVEL = 11;
/** Near the ground, slow enough to stop by `landM`. */
export const SLOW_LEVEL = 6;
/** Under this height over the moon's ground the descent slows to `SLOW_LEVEL`. */
export const SLOW_BELOW_M = 600;
const POLL_MS = 100;
const FORWARD = 'KeyW';
const UP = 'Space';
const DOWN = 'KeyC';

/**
 * The look and the held keys that fly toward a face-local direction (yaw, pitch in degrees):
 * flight moves level by the yaw and up or down by Space or crouch (controller.rs `step_fly`), so
 * a steep pitch flies straight up or down, a shallow one level, and between them both.
 */
export function aim(look: { yaw: number; pitch: number }): {
  yaw: number;
  pitch: number;
  keys: string[];
} {
  const keys: string[] = [];
  if (Math.abs(look.pitch) < 70) keys.push(FORWARD);
  if (look.pitch > 20) keys.push(UP);
  if (look.pitch < -20) keys.push(DOWN);
  return { yaw: look.yaw, pitch: look.pitch, keys };
}

/** Holds exactly `want` of the keys this trip uses (pressing and letting go of the difference). */
function hold(host: TripHost, held: Set<string>, want: string[]): void {
  for (const k of [...held]) {
    if (!want.includes(k)) {
      host.press(k, false);
      held.delete(k);
    }
  }
  for (const k of want) {
    if (!held.has(k)) {
      host.press(k, true);
      held.add(k);
    }
  }
}

function report(host: TripHost): MoonReport {
  const m = host.moon();
  if (!m) throw new Error('moon: no moon (not playing, or a server without grid.frames)');
  return m;
}

function answer(what: string, r: string | null): unknown {
  if (r === null) throw new Error(`${what}: not a local world`);
  const j = JSON.parse(r) as { error?: string };
  if (j.error) throw new Error(`${what}: ${j.error}`);
  return j;
}

/** Polls until `done` (true) or `maxS` passed (false), calling `each` on every report first. */
async function until(
  host: TripHost,
  maxS: number,
  done: (m: MoonReport) => boolean,
  each: (m: MoonReport) => void = () => undefined,
): Promise<boolean> {
  const end = host.now() + maxS * 1000;
  for (;;) {
    const m = report(host);
    if (done(m)) return true;
    if (host.now() >= end) return false;
    each(m);
    await host.wait(POLL_MS);
  }
}

const secondsSince = (host: TripHost, t: number): number => (host.now() - t) / 1000;

/** Flies to the moon and lands (see the file's head). Throws with what failed. */
export async function flyToMoon(host: TripHost, o: TripOptions = {}): Promise<MoonTrip> {
  const maxS = o.maxS ?? 120;
  const landM = o.landM ?? 40;
  const held = new Set<string>();
  const start = report(host);
  const home = start.grid === 'home' ? start.feet : null;
  let approach: unknown = null;
  let t = host.now();
  try {
    if (start.grid !== 'moon') {
      host.fly(true);
      host.flySpeed(FAST_LEVEL);
      approach = answer(
        'flyTo moon',
        await host.approach(o.marginM ?? 2000, o.face === 'b' ? -1 : 1),
      );
      t = host.now();
      const captured = await until(
        host,
        maxS,
        (m) => m.grid === 'moon',
        (m) => {
          const a = aim(m.look);
          host.look(a.yaw, a.pitch);
          hold(host, held, a.keys);
        },
      );
      if (!captured) {
        const m = report(host);
        throw new Error(
          `flyTo moon: not captured in ${String(maxS)} s (${m.distance.toFixed(0)} m from its centre, capture ${m.captureM.toFixed(0)} m)`,
        );
      }
    }
    const captureS = secondsSince(host, t);
    // Down the face: level look, crouch held; slower near the ground.
    host.fly(true);
    host.look(report(host).look.yaw, 0);
    t = host.now();
    const down = await until(
      host,
      maxS,
      (m) => m.groundM !== null && m.groundM < landM,
      (m) => {
        host.flySpeed(m.groundM !== null && m.groundM < SLOW_BELOW_M ? SLOW_LEVEL : FAST_LEVEL);
        hold(host, held, [DOWN]);
      },
    );
    hold(host, held, []);
    if (!down) {
      throw new Error(
        `flyTo moon: not down to ${String(landM)} m over the ground in ${String(maxS)} s (${String(report(host).groundM)} m)`,
      );
    }
    const descentS = secondsSince(host, t);
    // Hover until the flight's drag has stopped it, then fall.
    await host.wait(1000);
    host.fly(false);
    await host.wait(250);
    const [v0, t0] = [report(host).upMps, host.now()];
    await host.wait(1000);
    const [v1, t1] = [report(host).upMps, host.now()];
    const fallMps2 = (v0 - v1) / ((t1 - t0) / 1000);
    const gravity = report(host).gravity;
    t = host.now();
    const landed = await until(
      host,
      maxS,
      (m) => (m.groundM ?? 1) < 0.3 && Math.abs(m.upMps) < 0.1,
    );
    if (!landed) {
      throw new Error(
        `flyTo moon: not on the ground ${String(maxS)} s after flight off (${String(report(host).groundM)} m over it)`,
      );
    }
    const tolerance = o.tolerance ?? 0.25;
    if (Math.abs(fallMps2 / gravity - 1) > tolerance) {
      throw new Error(
        `flyTo moon: fell at ${fallMps2.toFixed(2)} m/s², not the moon's ${gravity.toFixed(2)} (±${String(tolerance * 100)} %)`,
      );
    }
    return {
      to: 'moon',
      home,
      approach,
      captureS,
      descentS,
      landS: secondsSince(host, t),
      fallMps2,
      gravity,
      at: report(host).feet,
    };
  } finally {
    hold(host, held, []);
  }
}

/**
 * Flies up and out past the moon's release sphere, then moves the player back to `home` (world
 * feet: where `flyToMoon` started) and waits for the client to hold home's chunk under the feet.
 */
export async function flyHome(host: TripHost, home: Vec3, o: TripOptions = {}): Promise<HomeTrip> {
  const maxS = o.maxS ?? 120;
  const held = new Set<string>();
  let t = host.now();
  try {
    if (report(host).grid !== 'home') {
      host.fly(true);
      host.flySpeed(FAST_LEVEL);
      host.look(report(host).look.yaw, 0);
      const released = await until(
        host,
        maxS,
        (m) => m.grid === 'home',
        () => {
          hold(host, held, [UP]);
        },
      );
      if (!released) {
        const m = report(host);
        throw new Error(
          `flyTo home: not released in ${String(maxS)} s (${m.distance.toFixed(0)} m from the moon's centre, release ${m.releaseM.toFixed(0)} m)`,
        );
      }
    }
    hold(host, held, []);
    const releaseS = secondsSince(host, t);
    answer('flyTo home', await host.placeHome(home));
    t = host.now();
    const back = await until(host, maxS, (m) => m.grid === 'home' && m.groundHeld);
    if (!back)
      throw new Error(`flyTo home: home's chunk under the feet not back in ${String(maxS)} s`);
    host.fly(false);
    return { to: 'home', releaseS, returnS: secondsSince(host, t), at: report(host).feet };
  } finally {
    hold(host, held, []);
  }
}
