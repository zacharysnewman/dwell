# Owner's ecosystem mockup (2026-11-11) — the rules, transcribed

The owner pasted a self-contained HTML mockup ("Creature Ecosystem Simulation"). Its behaviour:

- **World:** a 32×32 grid; cells EMPTY, GRASS or ROCK (rock blocks movement). One agent per cell.
  Tick every 300 ms (speed 1×/2×/5×, pause, step). Day/night: `tick % 120 > 60` is night.
- **Grass:** starts with 120 patches; regrows about 1.5 per tick on a random empty cell.
- **Agents** (deer, wolf): `hunger` 0 (full)–100 (starving, dies at 100), `energy` 100–0, `state`,
  `wants`, `destination`, `sleepTicks`. Each tick hunger +1; energy −0.6 (deer) / −0.8 (wolf).
- **Deer, in priority order:**
  1. A wolf within 6 → Fleeing (wants Safety): step away from it, steering around rock and other
     agents (try diagonal, then one axis, then a 90° turn, else reverse); destination 3 cells away.
  2. Sleeping → micro-naps: +35 energy a tick, wake after 4 ticks, at energy ≥ 85, or hunger > 70.
  3. Sleep when energy < 30, or at night with 25 % chance a tick if hunger < 75.
  4. Hunger > 30 → on grass: eat (grass → empty, hunger −55, energy +20); else go to the nearest grass
     within 12, else wander.
  5. Hunger ≤ 25 and energy ≥ 50 → find a mate within 8 (mate hunger ≤ 35, not sleeping or fleeing);
     adjacent: 50 % chance a fawn in a free adjacent cell (both hunger +35), else energy −10; not
     adjacent: walk to it.
  6. Otherwise seek company: the nearest deer within 8, else wander.
- **Wolves, in priority order:**
  1. Sleeping: +12 energy a tick, wake after ≥ 10 ticks and energy ≥ 95.
  2. Sleep when energy < 20, or at night with 40 % chance a tick if hunger < 50.
  3. Hunger > 35 → hunt the nearest deer within 14: adjacent (≤ 1.5) eats it (deer removed, hunger 0,
     energy 100, then sleeps); else Chasing with up to 2 steps a tick (a sprint), eating if it closes;
     no deer: wander.
  4. Hunger ≤ 25 and energy ≥ 50 → mate within 8 (mate hunger ≤ 35, awake); adjacent: 2 % chance a pup
     (both hunger +35), else energy −10.
  5. Otherwise pack company: the nearest wolf within 10, else wander.
- **Movement:** step toward the destination by sign(dx), sign(dy); if blocked try one axis, then the
  other. Wander: a random free neighbour (8 directions).
- **Start:** 30 rocks, 12 deer, 4 wolves.
- **UI:** paint deer/wolf/grass/rock/erase, population counts, day/night, tick counter, an inspector
  (state, wants, destination, hunger and energy bars).

Owner: "We'd likely want to simulate at a data level only when you're not nearby, render simply from
afar, and exactly when close by."
