import { describe, expect, it } from 'vitest';
import {
  aim,
  FAST_LEVEL,
  flyHome,
  flyToMoon,
  SLOW_LEVEL,
  teleportToMoon,
  tripStartOf,
  type MoonReport,
  type TripHost,
  type Vec3,
} from './moonTrip';

const CAPTURE = 40_000;
const MOON_G = 9.81 / 6;

interface Sim {
  grid: 'home' | 'moon';
  /** Feet to the moon's centre (m). */
  d: number;
  /** On the moon: over its ground (m). */
  ground: number;
  up: number;
  flying: boolean;
  level: number;
  held: Set<string>;
  t: number;
  groundHeld: boolean;
  /** When the home chunk under the feet comes back after `placeHome` (ms), or never. */
  heldAt: number | null;
  /** When the server released the player (ms), or null while it hasn't. */
  serverHomeAt: number | null;
}

interface Fake {
  host: TripHost;
  sim: Sim;
  calls: {
    approach: [number, number][];
    land: (number | number[])[];
    placeHome: Vec3[];
    looks: [number, number][];
  };
}

/**
 * A fake page: a moon `d` m off, flown at by the held keys (1 km/s at home, then 2 km/s down at
 * the fast level and 50 m/s at the slow one), falling at `g` with flight off on the moon.
 */
function fake(
  o: {
    g?: number;
    speed?: number;
    approach?: string | null;
    land?: string | null;
    /** When the moon's chunk under the feet arrives after `land` (ms), or never. */
    landHeldMs?: number | null;
    returnMs?: number | null;
    /** The server releases the player this long after the client shows it home (ms). */
    serverLagMs?: number;
  } = {},
): Fake {
  const g = o.g ?? MOON_G;
  const sim: Sim = {
    grid: 'home',
    d: 600_000,
    ground: 0,
    up: 0,
    flying: false,
    level: 0,
    held: new Set(),
    t: 0,
    groundHeld: true,
    heldAt: null,
    serverHomeAt: 0,
  };
  const calls: Fake['calls'] = { approach: [], land: [], placeHome: [], looks: [] };
  const step = (dt: number): void => {
    if (sim.heldAt !== null && sim.t >= sim.heldAt) sim.groundHeld = true;
    if (sim.grid === 'home') {
      if (sim.flying && sim.held.size > 0) sim.d -= (o.speed ?? 1000) * dt;
      if (sim.d < CAPTURE) {
        sim.grid = 'moon';
        sim.serverHomeAt = null;
        sim.ground = sim.d - 100;
      }
      return;
    }
    if (sim.flying) {
      const v = sim.level === FAST_LEVEL ? 2000 : sim.level === SLOW_LEVEL ? 50 : 8;
      const dy = (sim.held.has('Space') ? v : 0) - (sim.held.has('KeyC') ? v : 0);
      sim.up = 0;
      sim.ground = Math.max(0, sim.ground + dy * dt);
      sim.d = sim.ground + 100;
      if (sim.d > CAPTURE * 1.1) {
        sim.grid = 'home';
        sim.groundHeld = false;
        sim.serverHomeAt = sim.t + (o.serverLagMs ?? 0);
      }
      return;
    }
    sim.up -= g * dt;
    sim.ground += sim.up * dt;
    if (sim.ground <= 0) [sim.ground, sim.up] = [0, 0];
  };
  const report = (): MoonReport => ({
    index: 1,
    position: [0, 0, 0],
    feet: [sim.d, 0, 0],
    direction: [-1, 0, 0],
    distance: sim.d,
    captureM: CAPTURE,
    releaseM: CAPTURE * 1.1,
    grid: sim.grid,
    gravity: MOON_G,
    gravityNow: sim.grid === 'moon' ? MOON_G : 9.81,
    look: { yaw: 270, pitch: sim.grid === 'home' ? -30 : -90 },
    upMps: sim.up,
    groundM: sim.grid === 'moon' ? sim.ground : null,
    groundHeld: sim.groundHeld,
  });
  const host: TripHost = {
    moon: report,
    approach: (margin, face) => {
      calls.approach.push([margin, face]);
      if (o.approach !== undefined) return Promise.resolve(o.approach);
      sim.d = CAPTURE + margin;
      return Promise.resolve('{"distance":42000}');
    },
    land: (face, river) => {
      calls.land.push(river ? [face, 1] : face);
      if (o.land !== undefined) return Promise.resolve(o.land);
      // Captured on the next tick, 1.5 m over the ground; its chunk arrives `landHeldMs` later.
      sim.grid = 'moon';
      sim.ground = 1.5;
      sim.d = sim.ground + 100;
      sim.groundHeld = false;
      const r = o.landHeldMs === undefined ? 500 : o.landHeldMs;
      sim.heldAt = r === null ? null : sim.t + r;
      return Promise.resolve('{"grid":[0,90,0]}');
    },
    placeHome: (feet) => {
      calls.placeHome.push(feet);
      if (sim.serverHomeAt === null || sim.t < sim.serverHomeAt)
        return Promise.resolve('{"error":"not at home"}');
      sim.d = 600_000;
      const r = o.returnMs === undefined ? 2000 : o.returnMs;
      sim.heldAt = r === null ? null : sim.t + r;
      return Promise.resolve('{}');
    },
    press: (code, down) => {
      if (down) sim.held.add(code);
      else sim.held.delete(code);
    },
    fly: (on) => {
      sim.flying = on;
    },
    flySpeed: (level) => {
      sim.level = level;
    },
    look: (yaw, pitch) => {
      calls.looks.push([yaw, pitch]);
    },
    wait: (ms) => {
      for (let i = 0; i < ms; i += 10) {
        sim.t += 10;
        step(0.01);
      }
      return Promise.resolve();
    },
    now: () => sim.t,
  };
  return { host, sim, calls };
}

describe('aim', () => {
  it('flies level by the yaw and up or down for a steep pitch', () => {
    expect(aim({ yaw: 30, pitch: 0 })).toEqual({ yaw: 30, pitch: 0, keys: ['KeyW'] });
    expect(aim({ yaw: 30, pitch: 45 }).keys).toEqual(['KeyW', 'Space']);
    expect(aim({ yaw: 30, pitch: -45 }).keys).toEqual(['KeyW', 'KeyC']);
    expect(aim({ yaw: 30, pitch: -89 }).keys).toEqual(['KeyC']);
    expect(aim({ yaw: 30, pitch: 80 }).keys).toEqual(['Space']);
  });
});

describe('flyToMoon', () => {
  it('moves outside the capture, flies in, down, measures the fall and lands', async () => {
    const { host, sim, calls } = fake();
    const trip = await flyToMoon(host);
    expect(calls.approach).toEqual([[2000, 1]]);
    expect(sim.grid).toBe('moon');
    expect(trip.home).toEqual([600_000, 0, 0]);
    expect(trip.captureS).toBeGreaterThan(1);
    expect(trip.captureS).toBeLessThan(3);
    expect(Math.abs(trip.fallMps2 / MOON_G - 1)).toBeLessThan(0.05);
    expect(trip.gravity).toBe(MOON_G);
    expect(sim.ground).toBe(0);
    expect(sim.flying).toBe(false);
    expect(sim.held.size).toBe(0);
    // It slowed for the last of the descent.
    expect(sim.level).toBe(SLOW_LEVEL);
  });

  it('lands on face B when asked', async () => {
    const { host, calls } = fake();
    await flyToMoon(host, { face: 'b', marginM: 500 });
    expect(calls.approach).toEqual([[500, -1]]);
  });

  it('fails when the fall is not the moon gravity', async () => {
    const { host, sim } = fake({ g: 9.81 });
    await expect(flyToMoon(host)).rejects.toThrow(/fell at 9\.\d+ m\/s².*1\.6/);
    expect(sim.held.size).toBe(0);
  });

  it('fails, keys let go, when it is never captured', async () => {
    const { host, sim } = fake({ speed: 0 });
    await expect(flyToMoon(host, { maxS: 5 })).rejects.toThrow(/not captured in 5 s/);
    expect(sim.held.size).toBe(0);
  });

  it('fails outside a local world and on the dev move error', async () => {
    await expect(flyToMoon(fake({ approach: null }).host)).rejects.toThrow(/not a local world/);
    await expect(flyToMoon(fake({ approach: '{"error":"no moon"}' }).host)).rejects.toThrow(
      /no moon/,
    );
  });

  it('fails without a moon report', async () => {
    const { host } = fake();
    host.moon = () => null;
    await expect(flyToMoon(host)).rejects.toThrow(/grid\.frames/);
  });
});

describe('flyHome', () => {
  it('flies out past the release, goes back to the start and waits for its chunk', async () => {
    const { host, sim, calls } = fake();
    const out = await flyToMoon(host);
    const trip = await flyHome(host, out.home ?? [0, 0, 0]);
    expect(sim.grid).toBe('home');
    expect(calls.placeHome).toEqual([[600_000, 0, 0]]);
    expect(trip.releaseS).toBeGreaterThan(10);
    expect(trip.returnS).toBeCloseTo(2, 0);
    expect(sim.held.size).toBe(0);
  });

  it('waits for the server to release the player before moving it home', async () => {
    // The browser flight (0.58.8, 2 of 4): the client showed the release, `placeHome` asked the
    // server at once, was told "not at home", and left the player in space.
    const { host, sim, calls } = fake({ serverLagMs: 250 });
    const out = await flyToMoon(host);
    await flyHome(host, out.home ?? [0, 0, 0]);
    expect(calls.placeHome.length).toBeGreaterThan(1);
    expect(sim.d).toBe(600_000);
    expect(sim.groundHeld).toBe(true);
  });

  it('gives up when the server never releases the player', async () => {
    const { host } = fake({ serverLagMs: 1e9 });
    const out = await flyToMoon(host);
    await expect(flyHome(host, out.home ?? [0, 0, 0])).rejects.toThrow(/not at home/);
  });

  it('never saves a spot in space as the next trip home', async () => {
    // Left in space by a failed flight home (on the home grid, nothing under the feet), the next
    // trip to the moon had saved that spot as "home".
    const { host, sim } = fake();
    sim.d = CAPTURE * 1.2;
    sim.groundHeld = false;
    expect((await flyToMoon(host)).home).toBeNull();
  });

  it('fails when home chunk under the feet never comes back', async () => {
    const { host } = fake({ returnMs: null });
    await flyToMoon(host);
    await expect(flyHome(host, [0, 40, 0], { maxS: 30 })).rejects.toThrow(/not back in 30 s/);
  });
});

describe('tripStartOf', () => {
  it('is the feet on home ground, else nothing (BUGS.md #147)', () => {
    const { host, sim } = fake();
    expect(tripStartOf(host.moon())).toEqual([600_000, 0, 0]);
    // Left out in space on the home grid by a teleport that was never captured.
    sim.d = CAPTURE * 0.5;
    sim.groundHeld = false;
    expect(tripStartOf(host.moon())).toBeNull();
    sim.grid = 'moon';
    expect(tripStartOf(host.moon())).toBeNull();
    expect(tripStartOf(null)).toBeNull();
  });
});

describe('teleportToMoon', () => {
  it('teleports onto the ground, flight off, and waits for its chunk', async () => {
    const { host, sim, calls } = fake();
    sim.flying = true;
    const trip = await teleportToMoon(host);
    expect(calls.land).toEqual([1]);
    expect(calls.approach).toEqual([]);
    expect(sim.grid).toBe('moon');
    expect(sim.flying).toBe(false);
    expect(sim.ground).toBe(0);
    expect(sim.groundHeld).toBe(true);
    expect(trip.home).toEqual([600_000, 0, 0]);
    expect(trip.landS).toBeGreaterThanOrEqual(0.5);
    expect(trip.landS).toBeLessThan(3);
  });

  it('lands on face B when asked', async () => {
    const { host, calls } = fake();
    await teleportToMoon(host, { face: 'b' });
    expect(calls.land).toEqual([-1]);
  });

  it('lands on the sun beside a river head when asked (BUGS.md #146)', async () => {
    const { host, calls } = fake();
    await teleportToMoon(host, { river: true }, 'sun').catch(() => undefined);
    expect(calls.land).toEqual([[1, 1]]);
    await expect(teleportToMoon(fake().host, { river: true })).rejects.toThrow(/sun's/);
  });

  it("fails when the moon's chunk under the feet never arrives", async () => {
    const { host } = fake({ landHeldMs: null });
    await expect(teleportToMoon(host, { maxS: 5 })).rejects.toThrow(/chunk held false/);
  });

  it('fails outside a local world, on the dev move error and away from home', async () => {
    await expect(teleportToMoon(fake({ land: null }).host)).rejects.toThrow(/not a local world/);
    await expect(teleportToMoon(fake({ land: '{"error":"no moon"}' }).host)).rejects.toThrow(
      /no moon/,
    );
    const { host } = fake();
    await teleportToMoon(host);
    await expect(teleportToMoon(host)).rejects.toThrow(/not at home/);
  });
});
