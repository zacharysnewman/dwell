# Dwell — Torches and Dynamic Lights

Phase 47 ([`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md)), [ADR 0047](./adr/0047-torches-and-dynamic-lights.md).
The lighting it builds on: the flood light ([`DAY_NIGHT.md`](./DAY_NIGHT.md) §11, ADR 0045), the
sun and moon shadows (§10) and the ambient light (§9). Content is prototype (ARCHITECTURE.md §6.1):
the torch exercises the system; the real light sources come later, as data.

## 1. Brief and answers

Owner (2026-10-21): "I want to implement a torch, it's 3 fold, it should cast shadows / illuminate
nearby when held (a new dynamic shadow caster setting) which likely doesn't need to / shouldn't
update the block light level (I would imagine it's not instant and would look off), when placed as a
block on the ground it should cast shadows when "shadows setting" are on (possibly a separate setting
entirely from sun/moon shadows, or a sub setting enum like just directional, just sun/moon, just
block lights, both), and it should also update surrounding block light levels when placed on the
ground."

The owner's answers to the design questions (2026-10-21):

1. **Two separate settings.** "Sun & moon shadows" (the Shadows setting, Off / Low / High) stays as
   it is. A new **Torch shadows** setting (Off / Low / High) sets how many nearby lights cast shadows
   and at what resolution (T2).
2. **With Torch shadows Off** a held or placed torch still lights its surroundings smoothly, without
   shadows:
   - a **held** torch is a smooth dynamic light that falls off with distance, unshadowed, and
     **never** updates the block-light flood fill;
   - a **placed** torch lights through the flood fill (`emits`, `dwell_world::light`, ADR 0045)
     either way; the dynamic shadowed light layers on top when the setting is on (T2).
3. **Other players' held torches light the world for you.** Held items are already in snapshots
   (protocol v17, CHARACTERS.md); remote torches light, and cast shadows within the budget, nearest
   first. No protocol change.
4. **Data-driven.** The torch is a block (`emits` for its block light) plus a **`light` entry**
   (colour, radius, strength, flicker, casts shadows), and both the placed block and the held item
   use it. More lights (lanterns, …) are added by data alone. Floor placement first; wall torches
   can come later as a state.

What is added later, and how: more light sources and blocks that use them (data in
`shared/blocks/*.json`, by hand or a tool; per build, synced to clients by name), more held items
(`shared/characters/characters.json`), more glowing textures (`emissive` in
`shared/textures/materials.json`). Fixed in code: the falloff, the flicker's noise, the choice of the
nearest lights and (T2) the shadow maps' kinds.

## 2. Data

- **Light sources** (`lights` in a namespace of `shared/blocks/*.json`, validated by
  `shared/blocks/lights.mjs`, generated into `dwell-blocks` `LIGHTS`; `lights.test.mjs`): a
  namespaced name → `{ color, strength, radiusM, flicker: { amount, speed }, shadows, flame? }`. `strength`
  is the irradiance at the flame (× the linear colour); the light ends smoothly at `radiusM`; `flicker`
  takes up to `amount` of the strength, wavering at `speed` cycles a second; `shadows` says whether
  it may cast shadows (T2); `flame` (optional) names the flame look drawn where it burns (§6, a
  name in `shared/look/flames.json`; `LightInfo::flame`). The build has `dwell:torch` (#ff8a3d, warmed from #ffa45c by the owner 2026-10-21, 3.8, 14 m, 16 % at 6 Hz, shadows: the flood light's block light at level 14, 1.2 × π, reaching 14 m as a placed torch's does; flame `dwell:torch`);
  the test fixture adds `test:lantern` by data alone, and its `test:lamp` uses it.
- **Blocks** reference one by name: `"light": "dwell:torch"` (`StateInfo::light`, a reference into
  `LIGHTS`); `emits` (0–15) is the flood light a placed block gives off; `"held": "torch"` names the
  item a hand holding it shows (`StateInfo::held`, an item of the characters' data; else the block
  itself). Unknown names fail with a message. Registry sync (ADR 0038) carries both as look tags
  (`0x8e` the light's name, `0x8f` the held item), read by name, so a client without a light source
  draws none and a server without these tags is still read.
- **The torch** (prototype; T1b, owner 2026-10-21: "Make torches a full 1m in height, so two voxels,
  breaking one destroys the other"): one block over two ½ m cells, a **multi-cell block**
  (BLOCK_REGISTRY.md, Multi-cell blocks). `dwell:torch` is its lower cell, the stick (the palette's
  slot 9, `needsSupport: below`, `held: torch`, the held light `light: dwell:torch`, tile `torch`),
  with `extends: { up: "dwell:torch_upper" }`; `dwell:torch_upper` is the upper cell, the stick's top
  and the flame (`partOf: dwell:torch`, not placeable on its own, `emits: 14` — the flood light comes
  from the flame's cell — and `light: dwell:torch` for T2's placed light, tile `torchTop`). Both are
  crosses of two quads (cutout, no wind, fade or sinking into a ramp, so the halves line up), `shape:
  empty` (no collision), `medium: air`; their selection boxes join into one 1 m outline
  (`dwell_world::edit::outline_box`). Placing puts both cells (both must be free, with support
  below); breaking either cell, or the block it stands on, clears both, in one edit's writes
  (`edit_writes`), so they replicate together and the client predicts the same. A new block,
  `torch_upper`, was added rather than a `half` property: `dwell:torch` shipped in 0.22.26.
- **Glow** (`emissive`, 0–4, on a material in `shared/textures/materials.json`): how much of a
  surface's lit colour its self-lit colour replaces (1 or more: fully **self-lit**). For blocks it is
  per atlas tile (`PAINTERLY_EMISSIVE`, generated with the tiles; the frame's `tile_grids` table
  carries it in `z`, so the vertex carries nothing); for characters per material
  (`Material::emissive`). No tile or material glows in the build now: the torch's flame is the
  flame effect (§6), which gives off its colour the same way. Owner (T1b): the held flame read as "a cream colored blob", and "the flame should stay
  illuminated regardless of brightness since it's the source of the light". So a self-lit surface
  is unaffected by the sun, the ambient, the flood and sky light, corner occlusion and shadows, and
  the canvas shows it as the tone curve shows its colour at the reference exposure, whatever the
  frame's exposure (`glow` in `render/lights.rs`; the brightest channel `glowPeak`, 0.7, at most, so
  its hue holds), and its pixels are flagged in the view depth (`self_lit_depth`, the depth's second
  bit) so the composite leaves them out of the time of day's desaturation (moonlight's 0.5, which
  had washed it to cream). T1b's flames (a yellow bulb under an orange cone on the held item, upward
  tongues painted on the torch's upper tile) were replaced by the flame effect (§6; owner: "That
  flame looks really bad, please make it look pretty like fire"); the torch's tiles are a plain
  stick now.
- **The held torch item** (`items.torch` in `shared/characters/characters.json`): a stick and a wrap
  on the right hand's grip, and **`light`**, the point on the item's socket where its light sits and
  its flame rises from (the wrap's top), resolved by the generator to a bone and an offset
  (`Item::light`).
- **Dynamic-light tunables** (`shared/look/lights.json` → `dwell-look` `lights.gen.rs`;
  `lights.test.mjs`): `maxLights` (8, the most a frame; the shader holds 16), `cullM` (64 m),
  `curve` (3, the flood light's block-light curve, so a held torch falls off as a placed one),
  `wrap` (0.5: a surface edge-on to the flame still takes some) and `glowPeak` (0.7: a self-lit
  surface's brightest channel as the tone curve sees it, at most).

## 3. Held lights (T1)

**[built, Phase 47 T1]** A hand holding a block with a `light` gives off that light at its item's
light point (`characters::held_light`): your arms' hand (or, with the arms hidden, your body's), and
every other player's body (their held item comes in the snapshots; stand-in puppets too). Each
frame the renderer gathers them (`CharacterRenderer::held_lights`), keeps the nearest `maxLights`
within `cullM` of the eye (`dwell_look::lights::pick`; ties by holder, so the choice is stable) and
packs them into one uniform block (`pack`: per light its place relative to the eye, radius, colour ×
strength × flicker; frame binding 14, `render/lights.rs`). The flicker is two octaves of
smooth value noise per holder (`flicker`, deterministic in time and holder), so a torch wavers.

Every lit surface adds the lights' irradiance to its Lambert sum (`TerrainLight::points`,
`point_lights(rel, n)`): `strength × colour × wrapped cos × x / (curve − (curve − 1)·x)` with `x = 1 − d /
radius` and the cosine wrapped, `(n·l + wrap) / (1 + wrap)`. So the light is the source's strength at the
flame and 0 at its radius, and falls as the flood light's block light falls over its levels (a level a
metre): a held torch lights a tunnel as a placed one does, only smoothly and moving with its holder.
Terrain, plants, LOD sections, water (body and foam), glass, other players' bodies and the items they
hold, your arms and your held block all take it (characters by the fragment's own normal). The CPU mirror is `dwell_look::lights::irradiance`.

A first build used an inverse-square falloff (strength at 1 m, softened near the flame): beside a
placed torch's flood light it lit only a small pool at the holder's feet (the first shots), so the
falloff follows the flood light's curve instead.

The held light **never enters the flood light** (owner): the flood light changes cells, re-meshes
chunks and spreads over frames, which would lag and step behind a moving torch.

**Limits (T1).** The light is unshadowed: it reaches through a thin wall to what is within its
radius on the other side and facing it. Torch shadows (T2) remove that. A light past `maxLights`
is dropped, nearest first kept; at 8 that is a crowd of torch bearers.

**Cost** (Apple M1, Chrome, 1280×720, `npm run bench -- torch-night-puppets --unthrottled`, Shadows
High, one run, load average 3–5): midnight in the open with 8 torch-bearing stand-ins and your own
torch (8 lights drawn, the budget) against the same stand-ins holding stone (none). `gpu.frame` p50
8.75 ms against 8.09 (+0.66 ms; the near pass +0.6, the far +0.5, the arms' +0.7, overlapping on the
M1's tiled GPU), with every pixel of the view inside all 8 lights' 14 m; one torch in a tunnel is a
small share of that. The main thread is unchanged (`frame.cpu` p50 1.8 ms both, no long tasks): the
block is 528 bytes written once a frame, the choice a sort of a few lights. With nobody holding a
light, a fragment's loop runs no iteration.

## 4. Placed lights

**[built, Phase 47 T1, T1b]** A placed torch lights through the flood light: its flame cell's
`emits` 14 spreads by flood fill on every edit, predicted edits included (`dwell_world::light`, ADR
0045; the stick's cell under it 13), smoothly averaged into each vertex, lit by the block light's
warm curve (`light.json` `block`), and breaking it (either cell) takes its light with it (tests:
`a_placed_torch_lights_the_tunnel_and_takes_its_light_when_broken`, both faces). Its flame is
self-lit (§2).

**[built, Phase 47 T2]** **Torch shadows.** With the setting on, the nearest lights whose source may
cast shadows (`shadows: true`) get shadow maps, and a placed torch's `light` is also a dynamic,
shadowed point light layered on its flood light:

- **The setting** (Settings → Look → *Torch shadows*: Off / Low / High; owner: a setting of its own
  beside *Sun & moon shadows*): Off on phones and Low on desktops by default, kept in the browser's
  append-only `dwell.shadows` record as its `torch` field (`ui/shadowSettings.ts`; builds before it
  keep the field on rewrite), `?torchshadows=` overriding it (the bench's `torch-shadows-off` and
  `torch-shadows-high` configurations). None on a CPU adapter (`torch_shadows_for`). With Off, a held
  torch still lights smoothly, unshadowed, and a placed one by its flood light alone.
- **The tiers** (`shared/look/lights.json` `shadows.tiers`): Low 2 lights at 256², High 4 at 384².
- **The maps** (`render/point_shadows.rs`): per light a cube of six perspective depth maps
  (`Depth32Float`, one array of six layers a slot; frame bindings 15–16, the cascades' compare
  sampler), drawn with the cascades' depth-only pipelines from the light, each face 2·atan(1.08)
  wide so the filter stays on its face, near 0.05 m, far the light's radius. The same eye-relative
  matrices draw and read them (rebuilt each frame from where each map was drawn), so no cube
  orientation has to agree. A fragment picks its face by its direction's largest axis, moves off its
  surface by `normalOffsetM`, compares its depth along that axis less `biasM` (`map_depth`), four
  hardware 2 × 2 compares, and the light's contribution in `point_lights` is multiplied by it.
- **Casters**: the held chunks' opaque terrain within the light's radius. Plants, ladders and the
  torch's own quads (cutouts) and characters cast none (a torch's own sprite would shadow everything
  round it; bodies are left for later).
- **Kept maps** (`dwell_look::lights::TorchShadowCache`): each light keeps its slot frame to frame;
  its maps are drawn again only when it is new, moved (1 cm) or the terrain within its reach changed
  (an order-free signature of those chunks' meshes), new ones first, at most `redrawsPerFrame` (2)
  lights a frame. A still placed torch costs no draws; a walking holder's torch is drawn every frame.
  A light whose maps aren't drawn yet has none: a held one lights unshadowed that frame, a placed one
  is left to its flood light.
- **Placed lights** (`VoxelWorld::light_sources_near`): the client indexes the cells of blocks that
  give off a light and block light (a torch's flame) as chunks load and edits land; each frame the
  game hands the renderer those within `placedM` (24 m) of the eye (`placed_lights`), which become
  dynamic lights at `placedStrength` (0.6) of their strength, drawn only with a map (the nearest
  shadowed lights, held and placed alike, take the slots).
- **Both faces**: a point light has no up; the cube's faces are the world's axes on either face, and
  the placed light and its index are tested on both (`placed_light_sources_are_found_near_the_eye`,
  `a_placed_torch_is_a_placed_light_at_its_flame`).

**Cost** (Apple M1, Chrome, 1280×720, `npm run bench -- torch-shadows-a --compare --configs
torch-shadows-off,torch-shadows-high --unthrottled`, one run each, load average 3–5; the baseline
is Low): a torch placed before a stone pillar in a dark room at midnight, the view filled by its
light, then the torch held too. `gpu.frame` p50 (ms):

| Record | Off | Low | High |
| --- | --- | --- | --- |
| placed | 7.32 | 7.87 (+0.55) | 7.86 (+0.54) |
| held and placed | 7.42 | 7.92 (+0.50) | 8.16 (+0.74) |

The maps' own passes (`gpu.torchshadows`) take 0.3–0.4 ms on the frames that draw them (3 in 4 s of
a held torch standing still; none for the placed one once drawn); the rest is the near pass's
lookups and the placed light itself (Off draws no placed light). The main thread is unchanged
(`frame.cpu` 2.1–2.5 ms, no long tasks). Face B's run (`torch-shadows-b`) was taken while the
machine got busier (load 9) and reads +0.1–0.9 ms. A first build copied each slot's six matrices
per fragment in the shader (`let s = slots[slot]`): +5 ms; it reads the fields it needs now.

## 5. Both faces

Flames have an up: their billboards rise along the face's up (§6), written face-local and crossed
into the world through the `FaceFrame` (`dwell_look::flames::billboard`; tests on both faces: the
billboard's axes, a flame's quads, a placed torch's flame on its stick).

Point lights have no up: a light is placed by its holder's bones, which follow the holder's
`FaceFrame` (`body_frame`), and shades every surface the same way on either face. Tests: the torch
lighting the ground under it alike on both faces (`dwell-look` lights), the held light's height over
its holder's feet on both faces (`render/characters.rs`), the placed torch's flood light on both
faces (`dwell-world`).

## 6. Flames

**[built, Phase 47 T1c]** Owner (2026-10-21): "That flame looks really bad, please make it look
pretty like fire"; "It can be a 2D billboard or 2 billboards"; on the placed torch, "flame
originated from the base not around it". Where a light source with a `flame` burns — a placed
torch's upper cell, a torch in a hand (yours in the arms view, other players', stand-ins') — the
renderer draws its flame look (`render/flames.rs`; the platform-free part in `dwell-look`
`flames`):

- **Data** (`shared/look/flames.json`, validated and generated by `shared/look/flames.mjs` into
  `dwell-look` `flames.gen.rs`, CI fails on a stale file; `flames.test.mjs` adds a fixture candle
  by data alone): `drawM` (48 m), `maxFlames` (64, nearest first), `peak` (1.6: a flame's brightest
  channel as the tone curve sees it, higher than `glowPeak` so the core shines) and per look a
  `ramp` (2–6 sRGB colours, the tips' deep red-orange to the core's yellow; the torch's warmed with
  its light, #ff8a3d), `bands` (painted steps of the ramp, 4), `heightM`/`widthM` (0.42 × 0.2 m),
  `baseWidthM` (0.12 m: its width where it rises from the stick's tip, no wider than the tip),
  `baseM` (a placed flame's base over its cell's floor: the stick's tip, 0.095 m), `heldScale`
  (0.42: a hand's flame, close to the eye), `speed`, `noiseScale`, `distort`, `tongues`, `sway`,
  `sizeFlicker`, a `glow` (colour, radius, strength) and `embers` (count, colour, size, rise,
  speed). A light source names its look (`"flame": "dwell:torch"`); a lantern or a campfire is a new
  look and a light that names it. Light sources are matched by name, so this is no registry or
  block-state change.
- **Shape**: two camera-facing billboards per flame. The flame's quad: a teardrop cut from rising
  value noise (`flame_shade`): narrow at its base (`baseWidthM`), widest a third of the way up, then
  tapering; the noise bends it sideways more toward the top, a slow noise sways its tip, and a
  second noise, thresholded, splits its top into separate tongues; coloured along the ramp by its
  heat in soft painted bands, its edge soft. Behind it the glow's quad: a soft halo, added. A few
  embers rise from its upper part and fade (`ember`, placed on the CPU, each a small added disc).
  Per flame its seed (a placed torch's cell, a holder's id) offsets the noise, so no two burn alike.
- **Linked to the light**: its height shrinks with the light's flicker (`sizeFlicker` × the dip,
  `lights::flicker` at the same seed and clock, so a held torch's light and flame, and a placed
  torch's flame and its T2 light, pulse together) and the glow follows it.
- **Up and both faces**: the billboard's up is the face's up projected on the view plane (turning
  to the camera's up looking straight down), its right across the screen (`billboard`, written
  face-local).
- **Drawing**: in the near scene pass after its opaque draws (and the characters), depth-tested,
  writing no depth: the glows (colour only), then the embers and flames (`flame_lit_fs`), which
  write the view depth at the flame flagged self-lit and uninked (`self_lit_depth`, `frame_depth`):
  the time of day's desaturation leaves them alone and the ink draws no outline round them. With
  see-through water in the pass the flames' mask (the flagged view depth, no colour) draws before
  the water and their colour after it, in the water's half. Your own torch's flame draws in the arms
  pass the same way. A flame gives off its colour whatever the light (`flame_light`, as `glow`
  with `peak`): the same by day, at midnight and in a cave (a test runs the CPU twin through the
  tone curve and the saturation).
- **CPU adapter**: a still flame (no noise, no sway or tongues, no embers): the same teardrop
  gradient (`flames_animated`).
- **Tests**: the data's validation and a fixture by data alone (`flames.test.mjs`); the light
  source's reference (`shared/blocks/lights.test.mjs`); the shader's WGSL in both tiers; the CPU
  twin's colour (warm hues, red ≥ green ≥ blue) and its alpha falling to 0 at the quad's edges; that
  it moves and two flames differ; the glow's flicker and the embers' rise; the billboard's axes and
  a flame's quads on both faces; a placed torch's flame on its stick on both faces; the flame's base
  at the stick's tip and its bottom no wider than the tip, over the noise's frames, and no flame
  texels left on the tile (`the_placed_flame_rises_from_the_sticks_tip_no_wider_than_it`); the
  flag set by the entries and pipelines that draw the flame.

**Cost** (Apple M1, Chrome, 1280×720, `npm run bench -- torch-night-puppets --unthrottled`, Shadows
High, Torch shadows default, load average 3–4): 9 flames (8 stand-ins' torches and yours) against
main without them: `gpu.frame` p50 9.92 ms against 9.98 (mean 10.43 against 10.47), within the
run's noise (under 0.1 ms); the `no-torches` record 8.32 against 8.31. The main thread is
unchanged: a few hundred vertices written a frame. A first build drew a mask then the colour in
every pass (the noise run twice) and the embers over the whole glow quad: +0.33 ms.

## 7. Compared with Minecraft

Vanilla Minecraft lights a held torch not at all; placed torches are block light in the lightmap.
The "dynamic lights" mods (LambDynamicLights, OptiFine's Dynamic Lights) light a held torch by
writing a fake block light into the lightmap around the player each tick, which is cheap but steps
by whole blocks and lags; shader packs on Iris/OptiFine (Complementary, BSL) instead add the held
light per fragment from the player's position, a smooth falloff by distance (Iris exposes
`heldBlockLightValue`), unshadowed. Dwell takes the shader packs' way for held lights (smooth,
per fragment, never in the flood light) and Minecraft's lightmap way for placed ones (the flood
light), as the owner asked; and it lights other players' torches too, which the packs don't (they
know only the local player's hand).
