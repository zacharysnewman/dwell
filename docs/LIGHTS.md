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
  namespaced name → `{ color, strength, radiusM, flicker: { amount, speed }, shadows }`. `strength`
  is the irradiance at the flame (× the linear colour); the light ends smoothly at `radiusM`; `flicker`
  takes up to `amount` of the strength, wavering at `speed` cycles a second; `shadows` says whether
  it may cast shadows (T2). The build has `dwell:torch` (#ff8a3d, warmed from #ffa45c by the owner 2026-10-21, 3.8, 14 m, 16 % at 6 Hz, shadows: the flood light's block light at level 14, 1.2 × π, reaching 14 m as a placed torch's does);
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
  (`Material::emissive`). The flame's tile (`torch_top`) and the held flame (`flame`, `flame_core`)
  are 1.0. Owner (T1b): the held flame read as "a cream colored blob", and "the flame should stay
  illuminated regardless of brightness since it's the source of the light". So a self-lit surface
  is unaffected by the sun, the ambient, the flood and sky light, corner occlusion and shadows, and
  the canvas shows it as the tone curve shows its colour at the reference exposure, whatever the
  frame's exposure (`glow` in `render/lights.rs`; the brightest channel `glowPeak`, 0.7, at most, so
  its hue holds), and its pixels are flagged in the view depth (`self_lit_depth`, the depth's second
  bit) so the composite leaves them out of the time of day's desaturation (moonlight's 0.5, which
  had washed it to cream). The held flame shows sRGB (0.74, 0.41, 0.10) by day, at night and in a
  cave (a test runs the CPU mirrors, `emitted`, `displayed`, through the tone curve). Its shape is a
  yellow `flame_core` bulb under an orange cone; the placed flame is painted as upward tongues
  (blades) of the `flame` ramp round a yellow core. The torch's upper tile is self-lit whole: its
  short stick top shows its own colour too.
- **The held torch item** (`items.torch` in `shared/characters/characters.json`): a stick, a wrap and a
  flame (a bulb and a cone) on the right hand's grip, and **`light`**, the point on the item's socket where its light
  sits (the flame), resolved by the generator to a bone and an offset (`Item::light`).
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

**[planned, Phase 47 T2]** With Torch shadows on, a placed torch's `light` (`shadows: true`) is also
a dynamic, shadowed point light layered on its flood light: point-light shadow maps (cube or
dual-paraboloid) for the nearest lights per the setting's tier, cached for still placed torches and
redrawn only when the blocks near them change, a held torch's every frame within a budget; the
CPU adapter Off; the setting stored append-only.

## 5. Both faces

Point lights have no up: a light is placed by its holder's bones, which follow the holder's
`FaceFrame` (`body_frame`), and shades every surface the same way on either face. Tests: the torch
lighting the ground under it alike on both faces (`dwell-look` lights), the held light's height over
its holder's feet on both faces (`render/characters.rs`), the placed torch's flood light on both
faces (`dwell-world`).

## 6. Compared with Minecraft

Vanilla Minecraft lights a held torch not at all; placed torches are block light in the lightmap.
The "dynamic lights" mods (LambDynamicLights, OptiFine's Dynamic Lights) light a held torch by
writing a fake block light into the lightmap around the player each tick, which is cheap but steps
by whole blocks and lags; shader packs on Iris/OptiFine (Complementary, BSL) instead add the held
light per fragment from the player's position, a smooth falloff by distance (Iris exposes
`heldBlockLightValue`), unshadowed. Dwell takes the shader packs' way for held lights (smooth,
per fragment, never in the flood light) and Minecraft's lightmap way for placed ones (the flood
light), as the owner asked; and it lights other players' torches too, which the packs don't (they
know only the local player's hand).
