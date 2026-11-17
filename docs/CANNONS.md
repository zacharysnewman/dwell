# Cannons, cannonballs and steering ships

**Status:** designed (2026-11-17), not built. The goal (owner, 2026-11-17): "ship battles where cannons can blow off
pieces of ships." Sinking by holes below the waterline, and water filling a hull instead of standing in it, come after
this design (WORK_QUEUE; questions first).

## 1. The owner's answers (2026-11-17)

1. **Firing:** "using the same input as doors, you can fire the cannon, clicking on it. It faces the direction of
   placement (still correctly rotates with dynamic objects), no re-aiming for now. We can add aiming later."
2. **Balls:** "2 cannonball variants one explosive, one simple impact knocking loose cells it strikes (and maybe knocks
   loose in an area around where it collides). Should have high mass as it's a heavy steel ball."
3. **Settling:** anything that comes to rest on a moving ship (a ball, a piece knocked loose) rides along with it, not
   falling through or sliding off: it joins that body rather than settling into the world's grid.
4. **Data:** cannon and ball types are data, so more are added without code.
5. **Ships:** "ship should be moveable with player controls, force should be exerted like a sail would, from the
   center of mass I think. Turning like you have a rudder." The wheel steers; no test push.

The first cannon is a land cannon: "cannon takes up 4 blocks 2m long by 1m tall, shoots physics cannon ball in aimed
direction." No ammunition or reloading for now.

## 2. Data (`shared/cannons/cannons.json`, generator `shared/cannons/gen.mjs`)

- **cannons**: each a block (by name) with its barrel along the block's facing: the muzzle's offset from the block's
  anchor cell (face-local, turned with the facing), the ball it fires, the muzzle speed (m/s), the time between shots
  (s), the recoil impulse on a body it stands on, the firing sound and smoke.
- **balls**: each a name, radius (m), density (kg/m³; steel 7,850: a 0.12 m ball weighs ~57 kg), its look, and its
  impact: `impact` (`knockRadiusM`: cells within it of the contact become loose pieces, `maxCells`, `minSpeedMs` below
  which it knocks nothing, the speed it keeps per cell knocked) or `explosive` (an entry of
  `shared/explosives/explosives.json`, blown at the contact, as dynamite's blast).
- Validation by name; a fixture cannon and ball added by data alone in the generator's tests. The engine's vocabulary
  is fixed: the two impact kinds, the muzzle and the facing.

## 3. Phases (each its own release)

- **C1 — the land cannon block.** A four-cell block (2 m long × 1 m tall, 1 m wide; `part` as the door's), placed
  facing away from the player, a prototype look (textured cells). Using it (the door's input) asks the server to fire;
  the server checks the time between shots and answers with a shot. Tests: placing makes its four cells, using it
  fires once per interval, on both faces.
- **C2 — the cannonball.** A server physics body: a sphere collider of the ball's radius and mass, spawned at the
  muzzle with the muzzle speed along the barrel (the cannon's facing turned by the body it stands on, if any), its
  recoil pushing that body; sent to clients as an entity and drawn as its look. Gravity and water drag as other
  bodies. At rest it settles as a loose item (a ball to pick up later) or, on a body, joins it (C4).
- **C3 — impacts.** On the first contact above `minSpeedMs`: the impact ball turns the cells within `knockRadiusM` of
  the contact (terrain or a body's) into loose pieces (felling's one-cell bodies, flung along the ball's path), up to
  `maxCells`, and keeps going slowed; the explosive ball blows its explosive's blast there and is gone. Both faces.
- **C4 — riding along.** A body or piece that comes to rest on a built body (a ship) joins it instead of settling into
  the grid; a ball resting on a deck rides with it.
- **C5 — steering a ship.** A wheel block: using it puts the player at the helm (their movement keys steer). Forward
  is a push at the body's centre of mass along its heading, as a sail would give (strength by data); turning is a
  rudder's: a turning force that grows with the ship's speed through the water (none when still). A rudder block may
  be required for turning (owner: "while it'll be required, it'll be mostly for show"); sails later.

## 4. Open (later)

- Aiming (turning and raising the barrel), ammunition and reloading by hand.
- Sinking by holes below the waterline; water filling a hull rather than standing inside it (owner: "We'll clarify
  some questions first").
- Damage to players from a ball's hit.
