# Fix spec: LOD holes, the other face's LOD, the sky sphere, the forward flip

> Status: **specified, not implemented** (written 2026-10-07 from a playtest on a phone, build
> `0.8.0 · 3361024`). This document tells an implementer exactly what to change. The diagnosis
> behind each fix is summarised in its section; do not re-litigate it, but do run every "red first"
> test against the unfixed code before fixing, as CLAUDE.md requires.

There are four independent work items. Do them as **four separate commits (or PRs), in this order**:

1. [LOD holes: chopped mountains](#1-lod-holes-chopped-mountains) (a bug: test red → green).
2. [The other face's LOD](#2-the-other-faces-lod) (a bug and wasted work).
3. [The forward flip](#3-the-forward-flip-when-crossing-the-midplane) (a behaviour change: test pins it).
4. [The sky sphere](#4-the-sky-sphere-day-to-night-rotating-as-a-unit) (a visual feature, phase 1 of 2).

Rules that apply to all four (from CLAUDE.md, repeated so you don't miss them):

- Every bug fix and every behaviour change has an automated test that **fails on the old code**.
  Run it before the fix and paste the failing line into the commit message ("red → green").
- Update `docs/ARCHITECTURE.md` (and `docs/BIFACIAL_WORLD.md` / `docs/PLAYER_CONTROLLER.md` where
  named below) **in the same commit**. Remove text that describes the old behaviour.
- Note the follow-up in the **Progress** table of `docs/IMPLEMENTATION_PLAN.md` and in the affected
  phase's `**Status:**` line (Phase 4 for item 1, Phase 13 for items 2 and 3, Phase 7 for item 4).
  No PR numbers.
- Do **not** raise `client/package.json`'s version. None of these changes touches the network
  protocol, the world format or generated terrain, so none needs a new compatibility line.
- Before pushing, from `client/`: `npm run format`, `npm run lint`, `npm run typecheck`,
  `npx vitest run`. All must pass.
- Glossary used below: *face A* is the day side (above the midplane, y ≥ −2,048), *face B* the
  underworld (below it). `FaceSign` is +1 for A, −1 for B. A *section* is an LOD octree node
  (`client/src/lod/grid.ts`); *level 1* sections are 64 m and are drawn either as LOD or as their
  8 streamed chunks.

---

## 1. LOD holes: chopped mountains

### What is wrong

Near the player, mountain tops vanish and look cut flat at a section boundary, while their
collision is still there. Cause, in `client/src/lod/lodSystem.ts`:

- About a second after the chunks around the camera are drawable, `findCovered` forces the
  traversal down to them (`coveredAncestors`). On that forced path a ready ancestor **stops drawing
  itself**, and every child that is not ready yet is drawn as **nothing** (see the comments
  "everything else here waits (a hole)" in `visit`, and the level-1 branch that draws only when
  `ready`).
- A loaded chunk is only shown when its whole level-1 section is shown as chunks
  (`chunkVisible`), so the loaded peak inside such a hole is invisible. The section below it is
  drawn, and its top (its +Y skirt, or a chunk's border face) is the flat "cut".
- Measured with the test harness: once the LOD has caught up, 0 of 20,402 ground points within
  200 m are undrawn; with a device whose level-0/1 jobs lag and a spherical chunk stream,
  **17,842** are undrawn (2,176 of them over loaded chunks); with the same slow device and no
  streamed chunks, 0. Phones flying fast are that slow device.
- Ruled out: section bounds marking real terrain as empty sky (the native core was sampled; every
  peak sits 10–36 m under its section's `hi` at every level). Do not change the bounds.

### The fix: clipped stand-ins

Where the traversal would leave a hole, draw the **nearest ready ancestor's mesh, clipped to the
hole's box**, instead of nothing. Chunks and drawn sections are unchanged; stand-ins only fill
what used to be empty.

#### 1a. `client/src/lod/lodSystem.ts`

1. Extend `Selection`:

   ```ts
   export interface StandIn {
     /** The region with nothing ready to draw (its box is what gets drawn). */
     hole: LodCoord;
     /** The nearest ready, meshed, non-buried ancestor whose mesh is drawn clipped to `hole`. */
     from: LodCoord;
   }
   export interface Selection {
     drawn: LodCoord[];
     empty: LodCoord[];
     chunks: LodCoord[];
     /** Holes filled by an ancestor's mesh clipped to them (see visit). */
     standIns: StandIn[];
   }
   ```

   Initialise `standIns: []` everywhere a `Selection` is built.

2. Give `visit`, `visitBuried` and `visitKids` one more parameter, `fallback: Node | null`: the
   nearest ancestor that is ready, has `kind === LodKind.Content` and `meshed === true`. The root
   call is `this.visit(root, frustum, selection, null)`.

3. In `visit(node, frustum, selection, fallback)`:
   - Compute `const next = ready && node.kind === LodKind.Content && node.meshed ? node : fallback;`
     and pass `next` to every recursive call made from this node (all of `visit`'s calls on kids,
     and `visitKids`).
   - Level-1 branch: keep the two existing cases (chunks; `else if (ready) draw`). Add a third:
     `else if (fallback) this.standIn(node, fallback, selection);`.
   - The `!ready` branch for levels ≥ 2: keep `if (this.coveredAncestors.has(node.id))
     this.visitKids(node, frustum, selection, fallback)`; otherwise, **instead of returning with
     nothing**, call `if (fallback) this.standIn(node, fallback, selection);` and return.
4. In `visitBuried`, pass `null` as the fallback to every recursive call. (A buried region drew
   nothing before; a coarse ancestor's surface must not be drawn into solid rock.)
5. In `visitKids`, pass the `fallback` it was given through to `visit`.
6. Add:

   ```ts
   private standIn(hole: Node, from: Node, selection: Selection): void {
     this.touch(from); // keeps it in the cache while it stands in
     selection.standIns.push({ hole: hole.coord, from: from.coord });
   }
   ```

   Do **not** set `from.drawnFrame` (skirts between drawn sections must not count stand-ins).
7. Extend `LodView` with

   ```ts
   /** Ancestor meshes drawn clipped to boxes (world metres) this frame; [] for none. */
   showLodStandIns(standIns: readonly { id: number; lo: Vec3; hi: Vec3 }[]): void;
   ```

   and in `update`, right after `this.view.showLodSections(visible)`, call it with
   `selection.standIns.map((s) => ({ id: lodId(...s.from), ...this.box(s.hole) }))`.
8. Update the `View` test double in `lodSystem.test.ts` and any other `LodView` implementer
   (search for `showLodSections(`) to implement `showLodStandIns` (the test double just stores the
   list).

#### 1b. The renderer: `client/src/render/three/ThreeRenderer.ts` and `Renderer.ts`

1. Add `showLodStandIns` to the `Renderer` interface (`render/Renderer.ts`) with the same signature,
   and wire `LodSystem`'s view to it wherever `showLodSections` is wired (search `showLodSections`).
2. Add a pure helper in a new file `client/src/render/three/clipBox.ts`:

   ```ts
   import { Plane, Vector3 } from 'three';
   /** Six planes keeping only what lies inside [lo, hi] (three.js clips the negative side). */
   export function boxClipPlanes(lo: Vec3, hi: Vec3): Plane[] {
     return [
       new Plane(new Vector3(1, 0, 0), -lo[0]),
       new Plane(new Vector3(-1, 0, 0), hi[0]),
       new Plane(new Vector3(0, 1, 0), -lo[1]),
       new Plane(new Vector3(0, -1, 0), hi[1]),
       new Plane(new Vector3(0, 0, 1), -lo[2]),
       new Plane(new Vector3(0, 0, -1), hi[2]),
     ];
   }
   ```

   Test it (`clipBox.test.ts`): a point inside has `distanceToPoint ≥ 0` for all six planes; a point
   just outside any face is negative for exactly that face's plane.
3. In `ThreeRenderer`:
   - In the constructor set `this.renderer.localClippingEnabled = true;`.
   - Keep a pool of stand-in meshes: `private readonly standIns: Mesh[] = [];` with
     `MAX_STAND_INS = 64`. Each pool entry is created once, lazily, as
     `new Mesh(new BufferGeometry(), withHeightFog(new MeshLambertMaterial({ vertexColors: true })))`
     (its own material, because clipping planes are per material), added to `this.scene`,
     `visible = false`.
   - `showLodStandIns(list)`: store the list. In `renderFrame`, after the LOD loop: for each entry `i`
     (up to `MAX_STAND_INS`; beyond that, skip the rest and `console.warn` once per session), look up
     `const l = this.lod.get(entry.id)`; if `!l?.section || !l.mesh` skip it. Otherwise set
     `slot.geometry = l.section.geometry`, copy `l.mesh.position` and `l.mesh.scale` into the slot,
     call `ThreeRenderer.placed(slot)`, set `slot.material.clippingPlanes = boxClipPlanes(lo, hi)`,
     `slot.visible = true`. Hide the unused slots. Call `l.section.setSkirts(0)` on the source (its
     own mesh is not shown this frame, because it was refined).
   - Never dispose a stand-in slot's geometry (it belongs to the section). When `setLodSection`
     replaces or removes a section, its next frame's stand-ins simply look it up again.
   - The `?batch=1` path (`BatchedTerrain`) does **not** draw stand-ins. It is experimental and
     opt-in; note this in ARCHITECTURE §6.6 rather than implementing it.
   - Water is not drawn in stand-ins (lakes and sea can be missing there for the moment it takes
     the real section to load). Note this in §6.6.

#### 1c. Tests

Add to `client/src/lod/lodSystem.test.ts`. **Run it on the unfixed code first: it must fail** (the
unfixed code leaves thousands of points uncovered and has no `standIns` field; to see the real red,
first add only the `standIns: []` field and the type, then run).

```ts
it('fills the holes of the forced path with clipped ancestors: no mountain is cut off', async () => {
  // Regression (phone playtest, 2026-10-07): with level-0/1 jobs lagging and the chunks streamed
  // as a sphere, the forced path to the chunks left the unready sections around them undrawn:
  // peaks vanished (loaded, collidable, hidden) and the section below showed a flat cut.
  const jobs = new Jobs();
  jobs.stuckBelow = 2;
  const view = new View();
  const loaded = (c: ChunkCoord) => c[0] * c[0] + c[1] * c[1] + c[2] * c[2] <= 9 + 3;
  const lod = new LodSystem(jobs, jobs, view, { drawable: loaded }, () => undefined, {
    pixelError: 4, cacheBytes: 64 * 1048576, maxGenerationJobs: 16, maxMeshJobs: 8,
  });
  lod.setDetailDistance(128);
  const cam = camera([0.5, 1.6, 0.5], 0, -10);
  for (let frame = 1; frame <= 1500; frame++) {
    lod.update(cam, frame * 50);
    await jobs.finish(() => 0, 1);
  }
  const s = lod.lastSelection();
  expect(s.standIns.length).toBeGreaterThan(0);
  const leaves = [...s.drawn, ...s.empty, ...s.chunks, ...s.standIns.map((x) => x.hole)];
  for (let x = -200; x <= 200; x += 4)
    for (let z = -200; z <= 200; z += 4)
      for (const y of [-1, 1]) {
        const p: Vec3 = [x + 0.5, y, z + 0.5];
        expect(leaves.filter((c) => contains(c, p)).length).toBe(1);
      }
  // Each stand-in's source is a meshed ancestor of its hole.
  for (const { hole, from } of s.standIns) {
    expect(view.meshed.has(lodId(...from))).toBe(true);
    expect(from[0]).toBeGreaterThan(hole[0]);
    expect(contains(from, sectionOrigin(hole))).toBe(true);
  }
}, 120_000);
```

(`camera`, `contains`, `Jobs`, `View` already exist in that file; import `sectionOrigin` from
`./grid` if it is not imported yet.) Also update the existing test "shows the streamed chunks
around the player while the levels above them are not ready": its comment "(unready ones are
holes)" becomes "(unready ones are stand-ins)", and its no-overlap check must include
`s.standIns.map((x) => x.hole)` in `leaves`.

Manual check (describe it in the PR): on a phone, fly at full speed along a mountain range: no
flat-topped peaks; with `?lodcolors=1` the stand-in regions show the coarser level's colour.

#### 1d. Docs

`docs/ARCHITECTURE.md` §6.6, the paragraph starting "**The player's surroundings never wait for
coarse levels:**": replace "leaving the unready ones empty (sky) until they are" with a description
of stand-ins (the nearest ready ancestor's mesh drawn clipped to the unready section's box; not in
`?batch=1`; no water in stand-ins).

---

## 2. The other face's LOD

### What is wrong

The LOD traversal ignores which face the camera is on: `refine()` decides by distance alone, so
from the underworld the day side's sections (≈ 4 km away through the core) are generated, meshed,
cached and drawn like the underworld's own. That wastes generation, memory and draw calls, and the
cache pressure raises `errorScale`, making the underworld coarser. The playtest also saw the day
side's continents through the underworld's sea from high up. That exact path was not reproduced, but
there is one confirmed cross-face leak: sections of **level ≥ 7** (which may hold rows of both faces)
are meshed with **face A's** surface data and biome tints only (`generate()` sets
`n.surface = isFaceBSection(...) ? s.surfaceB : s.surface`, and `isFaceBSection` is false from
level 7 up). Since `LodTint` tints every grass/leaves face by x/z, face A's biome map is printed onto
face B's vegetation in those sections. The fix below removes the other face from what is drawn, so
whatever the exact leak was, it cannot recur.

### The fix

#### 2a. Which face the camera is on, and the exceptions

In `lodSystem.ts`:

```ts
/** Other-face sections closer than this to the camera are kept (a shaft through the core). */
export const OTHER_FACE_NEAR_M = 512;
/** Within this of the rim the other face can be seen past the edge: nothing is culled. */
export const RIM_VIEW_M = 16_384;
```

At the top of `update()`:

```ts
const p = camera.position;
this.viewFace = p[1] >= World.midplaneY ? 1 : -1;
this.nearRim = Math.hypot(p[0], p[2]) > World.worldRadius - RIM_VIEW_M;
```

(`viewFace: FaceSign`, `nearRim: boolean` are new private fields; import `FaceSign` from
`../world/face`.)

#### 2b. Cull other-face sections up to level 6

```ts
/** A section wholly on the face the camera is not on, far from it: not loaded or drawn. */
private culled(n: Node): boolean {
  if (this.nearRim || n.coord[0] > MAX_ALIGNED_LEVEL) return false;
  const onB = isFaceBSection(n.coord);
  if (onB === (this.viewFace === -1)) return false;
  return boxDistance(this.position, n.lo, n.hi) > OTHER_FACE_NEAR_M;
}
```

(Store `this.position = camera.position` in `update`. Import `MAX_ALIGNED_LEVEL` from `./grid`.)

Apply it in **every** place children are handled, before anything else is done with the child:

- `visit` and `visitBuried`: first line, `if (this.culled(node)) { selection.empty.push(node.coord);
  return; }`.
- In the "are all kids ready" loops of `visit` and `visitBuried`, and in `visitKids`: skip culled
  kids: `if (this.culled(kid)) continue;` before `this.touch(kid)`. A culled kid therefore never
  makes its parent wait, is never scheduled, and is evicted like any unused node.

#### 2c. Level ≥ 7: mesh only the viewer's face

1. Add `surfaceB: Float32Array<ArrayBuffer> | null` to `Node` (initialised `null`, cleared with
   `surface` in `setCells`). In `generate()`, for `n.coord[0] > MAX_ALIGNED_LEVEL` store both
   `n.surface = s.surface ?? null` and `n.surfaceB = s.surfaceB ?? null`; for lower levels keep the
   current code.
2. In `mesh(n)`, when `n.coord[0] > MAX_ALIGNED_LEVEL && !this.nearRim`:
   - After `patchApron`, fill every row `r` from −1 to 32 with `lodRowFace(n.coord, r) !== viewer
     face` with stone (material id 2, `dwell:stone`; take it from the block registry, e.g.
     `stateId('dwell:stone')` or whatever `world/blocks.ts` exposes, not a literal). Rows are
     `lodCell(x, r, z)` for x, z from −1 to 32. Stone against stone draws no faces, so the other
     face disappears and the viewer's side is unchanged.
   - Viewer on A: `surface: this.surfaceInCells(n)` as now, and `tint: n.surface`.
   - Viewer on B: `surface: null` (B's surfaces are face-local and this section is not mirrored)
     and `tint: n.surfaceB`.
3. In `client/src/mesh/lodMesher.ts`, add to `MeshSectionOptions`:

   ```ts
   /** Column tints (the surface layout; only its tint values are read); default: `surface`. */
   tint?: Float32Array | null;
   ```

   and in `meshSectionAbove` build the tint from it: `new LodTint(options.tint !== undefined ?
   options.tint : surface)`. Test in `lodMesher.test.ts`: the same cells meshed with `surface: null`
   and a `tint` whose grass tint is red give red-tinted grass vertices; with neither, untinted.
4. **When the viewer's face changes** (compare `viewFace` with last frame's) mark every node with
   level > 6 that is `meshed` as `remesh = true`. (They have no cells once meshed unless modified;
   `schedule` then regenerates and re-meshes them, which is the intended path.)

#### 2d. Tests (red first)

In `lodSystem.test.ts` the flat test world is not bifacial, so build bifacial bounds for this test:
`{ lo: -Infinity, hi: 0, loB: -Infinity, hiB: 0, anyInside: true, bifacial: true }` and a
`flatSection` that fills every row whose face (`lodRowFace`) is A and whose bottom is below 0, and
every face-B row whose mirrored bottom (`mirrorRowBottom`) is below 0, with stone. Then:

- *Camera on face A* at `[0.5, 100, 0.5]` and *on face B* at `[0.5, 2 * World.midplaneY - 100, 0.5]`,
  after the view settles (≈ 1,500 frames): record every coord passed to `Jobs.lod`. **Assert no
  section of level ≤ 6 wholly on the other face, farther than `OTHER_FACE_NEAR_M`, was generated, and
  none is in `selection.drawn`.** This fails on the old code.
- *Near the rim*: the same camera moved to `x = World.worldRadius - 1000` still generates
  other-face sections (nothing culled).
- *Level ≥ 7, viewer on B*: capture the cells and options passed to `Jobs.meshSection` for a level-7
  section: every face-A row is stone, `options.surface` is `null` and `options.tint` is the
  section's `surfaceB`. (Extend `Jobs.meshSection` to record the cells as well as the options.)

Manual check: in the underworld, fly to the flight ceiling over the sea with `?lodcolors=1`; no
day-side continent shapes; F3's LOD cache and node counts are lower than before at the same spot.

#### 2e. Docs

`docs/ARCHITECTURE.md` §6.6: the traversal culls sections wholly on the other face beyond
`OTHER_FACE_NEAR_M` (except within `RIM_VIEW_M` of the rim); level ≥ 7 sections are meshed with the
other face's rows as stone and the viewer's face's tints, and re-meshed when the camera's face
changes. List both constants with the other LOD tunables. `docs/BIFACIAL_WORLD.md` §6
"Rendering" bullet: same, one sentence.

---

## 3. The forward flip when crossing the midplane

### What is wrong

On crossing, the client keeps the world heading (`input.yaw`) and the face-local pitch, flips the
camera's up, and hides the jump with a 180° roll **about the view direction**
(`client/src/game/flipRoll.ts`, `camera.rotateZ(roll)` in `ThreeRenderer.setCamera`). That is a
sideways barrel roll by construction, and because the pitch is face-local the view direction also
**snaps** by twice the pitch on the crossing frame (looking 60° down a shaft, it jumps 120°), which
a roll about the view axis cannot hide.

### The fix: a half forward somersault about the camera's right axis

On the crossing, turn the heading by 180° and keep the pitch; animate the camera from the old view to
the new one by a rotation about its **right** axis, nose first (head toward the midplane). Rotating
the new view by π about its right axis gives exactly the old view, so the first frame matches the
last one at any pitch, and left/right stay the same for the player. It is also how going over the rim
reads: walking outward on face A, around the edge, then walking inward on face B with W still held.

#### 3a. Replace `client/src/game/flipRoll.ts` with `client/src/game/faceFlip.ts`

```ts
import type { FaceSign } from '../world/face';

/** How long the camera takes to turn over. */
export const FLIP_SECONDS = 0.5;

const smoothstep = (t: number): number => {
  const x = Math.min(1, Math.max(0, t));
  return x * x * (3 - 2 * x);
};

/**
 * The camera turning over when the player's face changes (BIFACIAL_WORLD.md §6): the heading turns
 * by 180° at once and the view is drawn rotated about its right axis by π, easing to 0, so it
 * starts exactly at the old view and turns over nose first.
 */
export class FaceFlip {
  private face: FaceSign | null = null;
  private drawFace: FaceSign | null = null;
  private elapsed = FLIP_SECONDS;

  /** Per simulation tick with the predicted face: degrees to add to the heading (0 or 180). */
  tick(face: FaceSign): number {
    const turn = this.face !== null && face !== this.face ? 180 : 0;
    this.face = face;
    return turn;
  }

  /** Per drawn frame: the camera's pitch-over angle (radians, π … 0) about its right axis. */
  draw(face: FaceSign, dtSeconds: number): number {
    if (this.drawFace !== null && face !== this.drawFace) this.elapsed = 0;
    this.drawFace = face;
    this.elapsed = Math.min(FLIP_SECONDS, this.elapsed + dtSeconds);
    return Math.PI * (1 - smoothstep(this.elapsed / FLIP_SECONDS));
  }
}
```

Delete `flipRoll.ts` and `flipRoll.test.ts`; move their tests into `faceFlip.test.ts` (the easing
tests carry over to `draw`).

#### 3b. `client/src/game/game.ts`

- Replace the `FlipRoll` field with `private readonly flip = new FaceFlip();`.
- In `tick()`, right after `this.current = this.core.state();`, before `this.eye.tick(...)`:

  ```ts
  // Crossing the midplane turns the heading round (a half somersault, faceFlip.ts).
  const turn = this.flip.tick(this.current.face);
  if (turn !== 0) this.input.yaw = (this.input.yaw + turn) % 360;
  ```

  This is the only place `this.current` changes during play, so a face change from a server
  correction (replayed in `core`) is caught too. In the constructor (where `this.current` is first
  set, ≈ line 125) and on activation (≈ line 179) call `this.flip.tick(this.current.face)` once and
  ignore the result, so joining on face B does not turn the player.
- Check that touch look (`predict/touch.ts`, `this.look.yaw`) writes the same object as
  `this.input`; if it keeps its own yaw, apply the same `+ turn` to it.
- In `draw()`, replace `this.flip.update(face, dt)` with `this.flip.draw(face, dt)` and pass the
  result as the last argument of `setCamera` (rename the parameter, below).
- `turnSign`, `move_x` negation and the face-local pitch stay as they are.

#### 3c. The renderer

`Renderer.setCamera(eye, yawDeg, pitchDeg, face?, flip?)` (rename `roll` to `flip` in
`Renderer.ts` and `ThreeRenderer.ts`, and its doc comment). In `ThreeRenderer.setCamera` replace
`if (roll !== 0) this.camera.rotateZ(roll);` with `if (flip !== 0) this.camera.rotateX(flip);`
(the camera's local X is its right axis).

#### 3d. Tests (red first)

New `client/src/render/three/flipCamera.test.ts` (or in an existing renderer test; it needs only
three's `PerspectiveCamera`, `Vector3`, and `viewForward` from `world/face`). Write a helper that
builds the camera exactly as `ThreeRenderer.setCamera` does (extract that body into an exported
pure function `aimCamera(camera, eye, yawDeg, pitchDeg, face, flip)` in `ThreeRenderer.ts` or a new
`render/three/aimCamera.ts`, and make `setCamera` call it; that is the testable unit). Then, for
pitches −80°, −30°, 0°, 45° and yaw 37°:

- **No snap:** the camera aimed with `(yaw, pitch, face +1, flip 0)` and the camera aimed with
  `(yaw + 180, pitch, face −1, flip π)` have the same world forward and up vectors (within 1e-6).
  On the old code (keeping the yaw, rolling about Z) this fails for every pitch other than 0.
- **Right axis fixed:** for flip π, π/2 and 0 (face −1, yaw + 180) the camera's world right vector
  is the same, and equals the face-A camera's right vector.
- **Nose first:** at flip π/2 the forward vector is the face-A camera's **down** (−up) vector.

And `faceFlip.test.ts`: `tick` returns 0 while the face is unchanged, 180 on each change either
way, 0 on the first call.

Manual check: dig a shaft to the midplane while looking down it and fall through; the view pitches
over forwards with no jump, and W, A, S, D still go forward, left, back and right.

#### 3e. Docs

- `docs/BIFACIAL_WORLD.md` §6 "Crossing the band": replace "a roll through 180° over ~0.5 s" with
  the half forward somersault (heading turned 180°, rotation about the camera's right axis, 0.5 s,
  starting at the old view).
- `docs/PLAYER_CONTROLLER.md` §9 "Camera and look": same. In the controller bullet that says the
  heading `yaw` "is a world azimuth and unchanged", add that the client turns its heading by 180°
  when its face changes (the server only sees the inputs).
- Known side effect to note there: in the flip band a body can swing back and forth across the
  midplane, and each crossing turns the heading round again. Do not add hysteresis in this change.

---

## 4. The sky sphere: day to night, rotating as a unit

### What is wrong

The sky is two separate skies switched by the viewer's face (`dwellSky` in
`client/src/render/look.ts` branches on `dwellFace`): a day gradient by elevation above face A and a
night one below face B, with the sun and the moon at fixed directions (`SUN_DIRECTION`,
`MOON_DIRECTION_WORLD`) and lights switched per fragment by its side of the midplane
(`dwellOwn` in `render/three/heightFog.ts`). The owner wants one sky sphere graded from day to
night, that first rotates as a whole (sun, moon and gradient together) and later lets the sun and
moon move separately with the gradient following them. **This change is phase 1: the sphere and
rotation as a unit, visual only** (client-side, not synchronised between players).

### The model

A *sky frame* is three world unit vectors: `dayPole` (the day sky's zenith), `sun` and `moon`.
At rest `dayPole = [0, 1, 0]`, `sun = SUN_DIRECTION`, `moon = MOON_DIRECTION_WORLD`, which is
exactly today's sky. The frame rotates by an angle about the horizontal axis perpendicular to the
sun's azimuth, so the sun's elevation changes by that angle and π turns day and night over.

The colour toward a world direction `dir` for a viewer on face `face` (viewer up `up = [0, face, 0]`):

```
e    = clamp(dot(dir, up), 0, 1)                       // elevation above the viewer's horizon
d    = e > 0 ? dir : normalize(dir - dot(dir, up) * up) // below the horizon: the horizon there
                                                        // (if that length is < 1e-6, d = dir)
u    = dot(d, frame.dayPole)                            // −1 night zenith … +1 day zenith
base = gradient(SKY_SPHERE_STOPS, u)
dayness = smoothstep(-TWILIGHT, TWILIGHT, dot(up, frame.dayPole))
haze = mix(NIGHT_HORIZON_COLOR, HORIZON_COLOR, dayness)
c    = mix(base, haze, 1 - smoothstep(0, HAZE_BAND, e))
c    = mix(c, SUN_GLOW.color,  min(1, glow(SUN_GLOW,  max(0, dot(d, frame.sun)))))
c    = mix(c, MOON_GLOW.color, min(1, glow(MOON_GLOW, max(0, dot(d, frame.moon)))))
```

with `glow(g, x) = g.tight * x^g.tightPower + g.broad * x^g.broadPower` (today's formula),
`TWILIGHT = 0.1`, `HAZE_BAND = 0.12`, and

```ts
export const SKY_SPHERE_STOPS: readonly { at: number; color: Rgb }[] = [
  { at: -1, color: hex(0x0a1026) },
  { at: -0.4, color: hex(0x18213f) },
  { at: -0.12, color: hex(0x2b3856) },
  { at: 0, color: mix(hex(0x2b3856), hex(0xb3cce6), 0.5) }, // twilight
  { at: 0.12, color: hex(0xb3cce6) },
  { at: 0.4, color: hex(0x91b7e6) },
  { at: 1, color: hex(0x649ada) },
];
```

`gradient` must accept stops starting at −1 (today's starts at 0; generalise it: clamp `e` to the
first and last stop). The day stops above 0.12 and the night stops below −0.12 are today's, so at
rest the sky is **identical to today's for elevations ≥ 0.12** on both faces and within 0.09 per
channel below that (checked numerically while writing this spec: worst 0.080 on face A and 0.073 on
face B, at the haze band).

### Steps

#### 4a. `client/src/render/skyFrame.ts` (new, pure)

```ts
export interface SkyFrame { dayPole: Rgb; sun: Rgb; moon: Rgb; }
/** Horizontal, perpendicular to the sun's azimuth. */
export const SKY_AXIS: Rgb; // normalize([SUN_DIRECTION[2], 0, -SUN_DIRECTION[0]])
/** The frame turned by `angle` radians about SKY_AXIS (Rodrigues' formula, right-handed). */
export function skyFrame(angle: number): SkyFrame;
export const SKY_REST: SkyFrame; // skyFrame(0)
```

Tests: `skyFrame(0)` equals the rest vectors; `skyFrame(Math.PI).dayPole ≈ [0, -1, 0]` and its
`sun ≈ MOON_DIRECTION_WORLD`; all three stay unit length and keep their mutual dot products for
several angles.

#### 4b. `client/src/render/look.ts`

- Add `SKY_SPHERE_STOPS`, `TWILIGHT`, `HAZE_BAND` as above.
- Change `skyColor(dir, face = 1, frame: SkyFrame = SKY_REST)` to the model above. Keep
  `SKY_STOPS`, `NIGHT_SKY_STOPS`, `HORIZON_COLOR`, `NIGHT_HORIZON_COLOR`, the glow constants (they
  are still used or pinned).
- Rewrite `SKY_GLSL` as the same function reading uniforms instead of baked directions:

  ```glsl
  uniform float dwellFace;
  uniform vec3 dwellDayPole;
  uniform vec3 dwellSun;
  uniform vec3 dwellMoon;
  vec3 dwellSky(vec3 dir) { ... }  // the model above, line for line
  ```

  Generate the stop chain from `SKY_SPHERE_STOPS` the way `skyBranch` does today; remove the
  `dwellFace` branch on gradients (the face is now used only for `up`).

#### 4c. Uniforms and lights: `client/src/render/three/heightFog.ts`, `sky.ts`, `ThreeRenderer.ts`

- Add `dwellDayPole`, `dwellSun`, `dwellMoon` (`Vector3`) to `fogUniforms`, and to the sky
  material's uniforms in `sky.ts` (share the same objects, as `dwellFace` is shared today).
- `export function setSkyFrame(frame: SkyFrame)` in `heightFog.ts` sets them.
- Lights: replace `dwellOwn(lightUp, fragOnA)` (a 0/1 switch) with a smooth horizon weight:

  ```glsl
  float dwellOwn(float lightUp, float fragOnA) {
    float faceUp = fragOnA > 0.5 ? 1.0 : -1.0;          // the fragment's face's up is ±y
    return smoothstep(-0.1, 0.1, lightUp * faceUp);
  }
  ```

  Keep the two existing call sites unchanged: they already pass `lightUp` as the light's world-y
  component (`dot(direction, viewMatrix[1].xyz)`; three's light directions are in view space and
  `viewMatrix[1].xyz` is world up in view space). At rest this gives the same lights per face as
  today (the sun's y is +0.7, the moon's −0.7). Mirror the formula in a TS twin for the test (4e).
- `ThreeRenderer` gains `setSkyFrame(frame: SkyFrame)` (add it to `Renderer`): calls
  `setSkyFrame` for the uniforms, sets `sun.position` to `frame.sun`, `moon.position` to
  `frame.moon`, the day hemisphere light's `position` to `frame.dayPole` and the night one's to its
  negation. Keep references to the four lights as fields.
- The baked face shading in the meshers (`faceTint`: warm top, cool sides, blue underneath) stays as
  it is in phase 1. Write that in the docs as the main thing phase 2 must change (the shading would
  otherwise not follow a moving sun).

#### 4d. Driving the rotation (client, phase 1)

In `client/src/main.ts`, where other `params.get(...)` debug switches are read: `?skyrot=<degrees>`
sets a fixed angle and `?skyspin=<degrees per second>` turns it continuously (default 0, 0). Each
frame call `renderer.setSkyFrame(skyFrame(angleRad))`. Add a settings-menu slider "Sky rotation"
(0–360°, kept in local storage with the other settings, using the existing settings pattern in
`ui/settingsMenu.ts`; new field optional, so the settings store stays append-only per CLAUDE.md).

#### 4e. Tests

- `look.test.ts`: the existing tests that pin "the horizon colour at and below the horizon" and
  "grades to the zenith" keep passing at rest (rewrite them against `skyColor(dir, 1, SKY_REST)`
  where needed). Add: at rest, for elevations 0.12…1 in steps of 0.01, `skyColor` on face A equals
  `gradient(SKY_STOPS, e)` plus the sun glow as before (≤ 1e-6 off), and on face B the night
  equivalent; below 0.12, within 0.09. Add: continuity in the angle (for 100 random directions,
  `skyColor` at angles a and a + 0.001 rad differ by < 0.01); at angle π a face-A viewer's zenith is
  the night zenith colour (within 0.03); at π/2 a face-A viewer's zenith is the twilight stop
  (within 0.05).
- The test "is one function for the sky drawn and the haze: the fog shader carries the same GLSL"
  must still pass.
- Lights: unit-test the weight in TypeScript (export a TS twin `lightWeight(lightUp, faceUp)`
  next to the GLSL and test: 1 for a light high over its face, 0 for one under it, 0.5 at the
  horizon, monotone).

---

## Verification done while writing this spec

- Item 1: the hole counts above come from a temporary test with the existing harness (`Jobs`,
  `View`), run with `stuckBelow` 0 and 2 and with the chunk stream on and off. The bounds check
  sampled 3,000 columns ≥ 500 m high per seed (seeds 0, 1, 7) against `LodBoundsAt` at levels 1–9.
- Item 3: the camera claims were checked numerically with three.js at pitches −80°, −30°, 0°, 45°:
  the new scheme matches forward and up exactly and keeps the right vector fixed, and the frame
  halfway through looks along the old down vector; the old scheme's forward vector jumps by up to
  1.97 (unit vectors) at −80°.
- Item 4: the "identical above 0.12, within 0.09 below" figures were computed from the formula and
  stops above.

#### 4f. Docs

- `docs/ARCHITECTURE.md`: the `render/` row's "a sky gradient" text and §6.3's **Light** bullet:
  describe the sky sphere (one function of the world direction and a sky frame, with a viewer-side
  horizon haze), the frame turning as a unit about `SKY_AXIS` (client-only, `?skyrot`, `?skyspin`,
  the settings slider), and lights weighted smoothly by their height over each face. Keep the
  "static sun and moon" text only as what happens when the angle is 0.
- `docs/BIFACIAL_WORLD.md` §6 "Light" bullet: the same, briefly.
- `docs/FUTURE.md` (or the relevant phase in `docs/IMPLEMENTATION_PLAN.md`): phase 2: the sun
  and moon move separately with the gradient weighted toward each; the meshers' baked face shading
  moves into the shader so it follows the light; the angle comes from the server's tick so all
  players share it.
