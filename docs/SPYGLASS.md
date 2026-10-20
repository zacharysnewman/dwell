# The Spyglass

Status: **built** (Phase 38, [ADR 0039](./adr/0039-spyglass.md)); the owner's review in a browser
outstanding.

Owner (2026-10-19): *"a spyglass zoom like feature to see the clarity difference when using
raymarching vs LODs; this would actually show higher res farther away but in a smaller cone
essentially."*

A key zooms the view in. The view narrows, and the distant terrain inside it is drawn at the detail
its pixels now need: the LOD refines far sections inside the narrow cone. Outside the cone, nothing loads that the unzoomed view would not.

## 1. The owner's answers

| Question | Answer (2026-10-19) |
| --- | --- |
| Who is it for? | **Both.** Players get it in releases, with the raster renderer and the LOD. In local builds it also serves the renderer comparison: it works with `?renderer=march` and `?renderer=compare`, so the marcher and the LOD can be compared at distance while zoomed. (The marcher was removed at Phase 25's A7, ADR 0044.) |
| How is it turned on? | **A toggle key**: press to zoom in, press again to zoom out. Cycling steps while zoomed is the implementer's call (below: the wheel). Touch can come later; the game targets desktop for now. |
| What does it do? | **A narrow view with more detail**: a narrower field of view, and distant terrain at higher detail inside it. LOD selection follows the true pixel size (the projected pixel error at the narrow FOV), the marcher's cone and LOD switch follow it too, memory and streaming stay bounded, a subtle vignette, and a smooth transition that respects reduced motion. |
| What is extendable, and how? | **A data file**: zoom steps, FOV per step, the vignette, the detail boost and the transition time in a file under `shared/`, with a generator and validation like the others; a test adds a step by data alone. |

## 2. The data: `shared/look/spyglass.json`

| Field | Value | Meaning |
| --- | --- | --- |
| `key` | `Backquote` (`` ` ``) | The toggle, a `KeyboardEvent.code`. Every letter and digit is already a control (movement, the region tools, the brush tool and its preset keys, the hotbar), so the key is the first free key that is easy to reach without a modifier. The generator refuses letters, digits, arrows and every key another control uses. |
| `steps` | `[{ magnification: 4 }, { magnification: 8 }]` | The zoom steps, least first, each over 1 and larger than the last (at most 8, up to 64×). |
| `detailBoost` | 4 | Inside the zoomed view the LOD refines as if the screen were magnified by up to this much (§4). |
| `transitionS` | 0.25 | Seconds a zoom change eases over; 0 under reduced motion. |
| `vignette` | inner 0.95, outer 1.6, opacity 0.6, `#05070a` | A dark ring: clear out to `innerRadius`, `opacity` of `color` from `outerRadius` on, in units of half the screen height, faded in with the zoom. |

**FOV per step.** A step names a magnification, not a field of view in degrees. Its FOV is the
frame's narrowed by it, `2·atan(tan(fov/2) / magnification)`, where the frame's FOV is the player's
Field of view setting (narrowed on wide screens) plus the speed boost. A magnification gives the same
zoom whatever the setting and the screen's shape; a fixed angle would zoom 60° players less than
100° ones. Each pixel covers 1/magnification of the angle it did, which is also what the LOD needs.

`shared/look/spyglass.mjs` validates the file (an unknown, missing or out-of-range entry fails naming
its path) and generates `crates/dwell-look/src/spyglass.gen.rs` (the tunables) and
`client/src/ui/spyglass.gen.ts` (the key and the magnifications, for the controls table). CI
regenerates both and fails on a diff; `shared/look/spyglass.test.mjs` adds a step by data alone.

## 3. Controls

- The key toggles: zoom in to the step last used (the first, the first time), or out. It works while
  playing (the pointer captured), never while typing in the menu, and not with Ctrl or ⌘.
- **The wheel steps while zoomed** (up zooms in, down back; it stops at the ends). The wheel picks the
  hotbar slot otherwise; while zoomed, the number keys still do. Alt + wheel stays the brush size.
  Chosen because the wheel is the usual zoom control and nothing else needs it while zoomed.
- **Mouse look slows by the magnification** while zoomed, so the view turns as many pixels per
  movement as unzoomed (a fixed rule in code: aiming at 8× would otherwise be unusable).
- The speed boost (motion effects) still applies under the zoom: the zoom narrows the boosted FOV.
- A placed camera (the bench, e2e) zooms too.
- Touch: later.

The controls table (`client/src/ui/controls.ts`, a *View* group) names the key and the steps from
the generated file, so the controls page, its tests and the data agree.

## 4. Detail inside the cone, and its budget

The LOD (ARCHITECTURE.md §6.6) refines a section while its cells project larger than the pixel error,
in every direction, so turning shows what is already loaded. Under the spyglass, measuring every
direction at the zoomed pixel size would refine the whole world 4–8× (16–64× the sections). So
`dwell-lod` measures **two** pixel sizes:

- inside the drawn (narrow) view: the unzoomed pixels per radian × the detail magnification,
  `min(magnification, detailBoost)`;
- outside it: the unzoomed FOV's, exactly as without the spyglass.

`LodCamera` carries `magnification` and `detail_magnification`; `Frustum::pixels_per_radian_for(box)`
picks one by whether the box meets the view's frustum. The view frustum is the narrow one, so it also
puts the cone's sections first in the load order.

**The budget.** On flat ground the sections a view holds per distance band grow with the angle it
covers and the square of the detail, so the cone adds about `(horizontal FOV / magnification) / 360°
× detail²` of the unzoomed view's sections: at 8× with the boost at 4, about half as many again; at
4× (detail 4), about as many again; an unbounded 8× would add ~2–3×. Measured with the LOD test's flat world
(pixel error 4, 40 m up, looking 1° down): 2,892 sections unzoomed, 4,446 at 8× with detail 4, 7,946 with detail
8. `detailBoost` is that bound, as data. The cache budget (*Distant memory*) stays the hard limit: a
cone that overflows it coarsens the view as any view does (§6.6), and finer again after unzooming.

Full detail stays as it is: the chunks the client asks for are within the request radius, so beyond
the full-detail distance the finest level the cone reaches is level 1 (2 m cells).

## 5. Renderers

- **Raster**: the projection, the sky's rays and each pass's culling already used the frame's FOV;
  the zoomed FOV is the frame's. (The ray marcher, while it existed, was fixed to use it too; it was
  removed at Phase 25's A7, ADR 0044.)

## 6. The vignette and the transition

The zoom eases in log-magnification space (each doubling takes the same share) with a smoothstep over
`transitionS`, from wherever it is (zooming out mid-way doesn't jump). Under reduced motion (the
browser's `prefers-reduced-motion`, `Client.setReducedMotion`) it changes at once. The vignette is a
DOM layer in the HUD (`ui/spyglass.ts`: a radial gradient sized to the screen height), its opacity
`opacity × progress` where progress is the zoom's share of the first step (log scale). It is over the
whole view.

## 7. Tests and tools

- `shared/look/spyglass.test.mjs`: the checked-in output, a step added by data alone, bad entries.
- `dwell-look` `spyglass.rs`: the FOV rule, the toggle and wheel steps, a step added by data, the
  easing and reduced motion.
- `dwell-lod`: `frustum.rs` (pixels at the magnification inside the view only, the boost cap) and
  `system/tests.rs` `the_spyglass_refines_far_terrain_inside_its_narrow_view_only` (three levels finer
  4 km ahead at 8×, unchanged behind, two levels at detail 4, and unchanged if the narrow FOV is not
  passed; it failed with a direction-independent pixel size, red → green).
- `dwell-client`: `game_local.rs` (the zoomed FOV reaches the renderer and the LOD camera with its
  magnification, eases in, and is instant under reduced motion) and the renderer's FOV test
  (`the_frame_uses_the_fov_the_game_chose`).
- Vitest: the controls entry and the vignette's CSS; the bench's `spyglass` step.
- The bench: the `spyglass` step and the `spyglass-far` scenario (unzoomed, both steps, unzoomed
  again); `npm run bench -- spyglass-far --compare --configs march` shoots raster and march side by
  side. `__dwell.spyglass(on, step)` drives it, and `__dwell.state()` reports `fov` and `spyglass`.

## 8. Later

- Touch (a button or a pinch).
- A zoom sound of its own (the key and the wheel's steps play the tools' `tool` sound for now).
- Full detail (chunks) in the cone beyond the request radius would need the server to stream farther;
  not planned.
