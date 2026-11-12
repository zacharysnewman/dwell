# Far trees as multi-angle impostors

**Status:** proposal (2026-11-12), WORK_QUEUE item 31 (and item 30's step 3). Phase **I-1 (the bake)
is built** (`dwell_worldgen::impostor`, data `shared/trees/impostors/impostors.json`). **I-3's CPU half is
built** (2026-11-12): the hand-over per Far trees tier as data (`far.tiers`, §5: `on` an impostor from 96 m
over a 24 m fade, `off` none), `Far::share`/`near_share` (the crossfade, shares summing to 1),
`Config::dither` (the one view a pixel draws, each view on its weight's share of the pixels),
`Config::baked_for` (nearest baked stage, variant by seed) and `view_dir` (through `FaceFrame`, tested on
both faces). **I-2's tree lists and I-3's GPU draw are built** (2026-11-12, client `render/impostors.rs`,
ARCHITECTURE.md §6.6 "Far trees as impostors"): the atlas baked by the job workers and uploaded per layer, the
instances by 64 m tile from `far_trees`, `imp_vs`/`imp_fs` (the dithered view, baked normals, sun, ambient, fog
and received shadows at the texel's baked position), the near leaves' complementary dither and the chunks' cutout
cut past the hand-over; the Far trees setting picks the tier, `?impostors=0` the old `lod_trees.rs` path. Changed
from this proposal: the hand-over is **48 m over 16 m** on High (measured, §5); the depth offset (`frag_depth`) is
not drawn (it disabled early depth: the quad writes its own depth, the texel's baked depth only places its light
and fog); no mip-maps yet (texels loaded exactly). Not yet: felled trees left out, shadows cast (I-5), the
LOD ground under far trees (BUGS.md #95), retiring `lod_trees.rs` (I-4), the phone run. Open questions for the
owner at the end.

The owner (2026-11-12): "billboards (maybe from above billboards too, possibly a few different angles so we
can switch between them like old Mario kart did with carts, and some trees too)… We're going to want to have
dense forests and so we really need to optimize." And: "our low LOD trees look pretty bad and not very much
like our closer trees, so it's a rough transition." The budget: **High at 60 fps on an M1 MacBook Air**
(item 30); extra fidelity goes in Ultra.

![The bake's preview: oak, palm, pine, mature, seed 3; per species the albedo lit by a sun from the baked
normals, the normals, the depth; 17 views each (8 around at 0°, 8 at 40°, 1 from above)](tree-impostors-bake.png)

*(`cargo run --release -p dwell-worldgen --example impostor_bake -- docs/proposals/tree-impostors-bake.png`)*

## 1. What an impostor is here

A far tree is one camera-facing quad. Its texels come from an atlas baked from **the real tree**: the
cells `species::grow(species, seed, stage)` builds, exactly the voxels the near tree is meshed from (logs
as the regular octagon of their width, leaf cells as 1 m × ½ m × 1 m boxes). The atlas holds the tree seen
from a fixed set of directions; the drawer picks (and blends) the views nearest the camera's direction, so
walking around a far tree turns it, and looking down from a hill or the sky shows its crown from above —
the "Mario Kart" switch, with a blend instead of a pop.

Each texel stores what the near tree's shading needs:

| Texture | Channels | Use |
|---|---|---|
| albedo | RGB colour, A coverage (0/255) | the palette colour of a leaf or log (the per-tree leaf tint is applied at draw time, as the near leaves' is); alpha-tested, no blending |
| normal + depth | RGB face-local normal (−1..1 as 0..255), A depth | the sun's N·L, the ambient's sky/ground split and the fog by the texel's real position; leaves' normals lean out of the crown by `leaves.json` `canopyBlend` (the near trees' crown lighting) |

Depth (0 nearest, 255 farthest across the bounding sphere's diameter) lets the fragment shader move the
texel to its real depth (`frag_depth`), so an impostor meets the ground, a hill and its neighbours where the
real tree would, and receives the cascaded shadow map at its real position (shadows-ish: it can also be
drawn into the shadow map as an alpha-tested quad facing the sun, picking the sun's view).

## 2. The view layout

**Chosen for I-1: a ring + top** (data: `views`): 8 azimuths × 2 elevations (0°, 40°) + 1 straight down =
**17 views**. The atlas packs them row-major in a ⌈√n⌉-wide grid (5 × 4, 3 slots spare).

| Layout | Views | For | Against |
|---|---|---|---|
| **Ring + top** (built) | 8 × 2 + 1 = 17 | easy to reason about, the views the game needs (players stand on the ground: most views near 0–20°; hills and flight at 30–60°; the map/sky straight down); picking is a bilinear blend of ≤ 4 views | uneven coverage between 40° and 90° |
| Hemi-octahedral grid | e.g. 6 × 6 = 36 | even coverage of the hemisphere, the standard (Fortnite's, Unity's) | twice the views for the same detail; trees' near-horizontal views get no more than steep ones |
| Ring only | 8 | cheapest | no view from above (the owner asked for one) |

The layout is data (`kind: "ring"`, `azimuths`, `elevationsDeg`, `top`), so a hemi-octahedral kind can be
added to the engine later and chosen by data. Trees are close to symmetric about their trunk, so 8 azimuths
read well; a palm's lean is what most needs them.

**Picking and blending** (`Config::pick(dir)`, built and tested): the camera's direction from the tree
gives an azimuth and elevation; the two azimuths either side in the two elevation bands either side (the top
standing for every azimuth above 40°) weigh in bilinearly — up to 4 views, weights summing to 1. The drawer
either dithers between them (cheapest: one texture fetch per texel, the `lodblend` bayer pattern choosing
the view, stable on screen) or blends the 2–4 fetches (smoother, 2–4× the fetches). Recommendation: dither on
High and phone, blend on Ultra. With depth, a parallax-corrected fetch (sample each view at the texel its
own depth puts the point at) removes most of the ghosting between views; that is Ultra too.

## 3. The variant budget

A species tree varies by stage (8 stages) and seed (height, crown scale, branch angles, the palm's lean).
Baking every seed is impossible; the impostor is chosen per tree:

- **Stages:** `stages: [2, 4, 7]` (data). A tree takes the nearest baked stage, its quad **scaled** to its
  own height and crown radius (the record has both: the tree's seed draws them), so a stage-5 tree is a
  stage-4 bake slightly larger.
- **Seeds:** `variants: 4` (data) seeds baked per species and stage; a tree takes variant
  `hash(seed) mod variants`. Its leaf tint (`species::tint(species, seed)`) multiplies the albedo at draw,
  the same tint its near leaves get, so a far forest shows the same light/dark variety as near.
- **Yaw:** each instance turns by a per-tree yaw (from its seed, in 90° steps so the grid-aligned crown
  still matches the near tree's orientation when it hands over), which turns which view faces the camera:
  4 variants × 4 yaws read as 16 different trees at a distance.

Today's three species × 3 stages × 4 variants = **36 baked trees**.

## 4. Memory

Per baked tree: 17 views at `viewPx`² texels, 8 bytes a texel (two RGBA8), in a 5 × 4 grid:

| Tier (`viewPx`) | Atlas per tree | 36 trees | Notes |
|---|---|---|---|
| desktop (64) | 320 × 256, **640 KB** | **23 MB** | M1 Air: fine (unified memory, the budget is bandwidth, and impostors sample little of it per frame) |
| phone (32) | 160 × 128, 160 KB | 5.8 MB | iPhone: fine; the starter world's crash (item 20) is about the heap, so the atlas is uploaded and then dropped from the WASM heap |
| cpu (16) | 80 × 64, 40 KB | 1.4 MB | the CPU adapter's cheapest tier |

All 36 in one `texture_2d_array` (one layer a baked tree), so a whole forest is one draw call. A texel at
64 px across a ~15 m bounding sphere is ~0.23 m: a screen pixel covers that much at ~180 m on a 1080-line
window at 70° FOV, so 64 px views are right beyond ~120 m and magnified nearer. Mip-maps per layer
(alpha coverage preserved by a coverage-scaled mip, as for the leaf cards). The bounding sphere is centred
on the trunk's axis and the tree's mid-height, so a tall pine wastes some width in its side views and a
broad oak some height (the preview shows it); a per-view tight frame is a later saving (≈ 30 %).

Baking cost: 17 views × 64² rays, each a DDA through the tree's cells. Natively the preview's three
mature trees bake (grown, baked and written as a PNG) in 0.3 s, so tens of milliseconds a tree; the 36
trees take ~1–2 s, to be done in a worker at load (I-3 measures it in WASM on a phone; if slow, bake the
phone's 32 px views, a quarter of the rays). No baked files are shipped and nothing goes on the wire: the
bake is a pure function of the species data.

## 5. Where impostors take over

The hand-over is **data per tier** (the `far` block in the impostors file, **built**: `far.tiers` by the
Far trees setting's values, each `draw` `impostor` or `none`): `fromM` (where an
impostor starts), `fadeM` (the crossfade's width) and `toM` (where trees stop, 0 for never). Proposed:
High (M1) from 96 m, Ultra 160 m, phone 48 m — chosen by measurement in I-3 against the leaf cards'
fade (48–64 m) and the chunk meshes' reach.

**No pop:** across `fadeM` both the near tree and its impostor draw, each pixel showing exactly one of
them by the screen-space dither the LOD swaps already use (`lodblend.json` `pattern`), the impostor's
share rising with distance. Its colours are the near tree's (the same palette entries, tint, canopy-leaning
normals and sun), its silhouette the same voxels, so the crossfade changes texel density, not shape.

## 6. Instancing for dense forests

One instance per tree: position (face-local, through `FaceFrame` as every tree), the variant layer, the
scale, the yaw, the tint — 16–20 bytes. A forest of 10 000 trees is 200 KB of instances and one draw of
10 000 quads (40 000 vertices), alpha-tested. Positions come from **the tree records, not meshes**: the
generator already knows every tree's anchor, species, seed and stage (placements and the growth table),
so each chunk/LOD section lists its trees (a few bytes each) and the client builds the instance buffer from
the lists of the sections in view. Edited or felled trees leave the list (the growth record knows a felled
tree, ADR 0079). The vertex shader expands the quad to face the camera (cylindrical about the face's up
for low views, spherical above 40° so the top view lies flat).

Overdraw is the cost to watch on the M1 and phones: alpha-tested quads with early depth (`frag_depth`
disables early-z; on phone/High write depth from the quad and keep the depth offset to Ultra, I-3
measures both).

## 7. What replaces `lod_trees.rs`

`dwell-mesh` `lod_trees.rs` draws a section's leaf and log cells at the LOD's 1 m and 2 m levels as the
chunks do (chamfered canopy cells, octagon trunks); coarser levels show only canopy-tinted column tops.
With impostors: the LOD sections stop meshing tree cells at every level (the ground under a tree is meshed
as if the tree were not there — the generator can answer "ground without trees" as it answers "has
leaves"), and each section emits its trees' instances instead. `lod_trees.rs` is deleted in I-4 once the
impostors are on in every tier (the CPU adapter's included); until then a `?impostors=0|1` switch picks.

## 8. Phases (each ≤ 45 minutes of an agent's work, with exit criteria)

- **I-1 — the bake** (**built**, cloud): `dwell_worldgen::impostor` (`Config` from
  `shared/trees/impostors/impostors.json`, validated; `bake(species, seed, stage, config, px)`; `pick`), the
  preview example and sheet. Exit: deterministic per seed; a pine's top view round and its side views taller
  than wide; leaf normals outward on the crown's silhouette; an empty tree bakes an empty atlas; the fixture
  shrub and a layout added by data alone bake; a standing log's width constant down its length (a red →
  green check on the axis mapping) — all tests in `impostor.rs`.
- **I-2 — tree lists** (**built**, 2026-11-12, but for felled trees): each chunk / LOD section lists its trees
  (anchor, species index, seed, stage) from the placements and growth records, the same tree from either side
  of a border; felled trees gone. Built: `TerrainGenerator::far_trees(lo, hi)` (the species trees anchored in
  the rectangle, sorted, each in one list) and `impostor::Config::instances` (foot, layer, quarter turn); tested
  on the forest world (the stand's count, four quarters' lists equal the square's, both faces). Not yet:
  felled trees left out (the growth records, server side). Exit: tests on the starter world (counts match
  the placed trees, both faces through `FaceFrame`).
- **I-3 — drawing** (local, GPU): the atlas array uploaded at load per tier, the instance buffer, the
  billboard shader (pick/dither, tint, N·L, ambient, fog, depth offset, shadow receive), the far hand-over
  data and crossfade, the CPU tier. Exit: `npm run bench` before/after on the M1 at High (frame time with the
  starter world's forests), screenshots at the hand-over with no visible pop, a phone run.
- **I-4 — retire `lod_trees.rs`:** LOD sections stop meshing tree cells; impostors at every LOD level (dense
  forests visible to the horizon). Exit: LOD triangle counts down by the trees' share; the old path deleted.
- **I-5 — shadows and Ultra:** impostors in the shadow map (sun-view pick), blended views with parallax
  correction on Ultra. Exit: measured on the M1 at Ultra, screenshots.

## 9. Open questions for the owner

1. **Layout:** ring + top (17 views, built) or the hemi-octahedral grid (36 views, more even, twice the
   memory)? Is 8 around enough for the palms' lean?
2. **Variants:** 3 stages × 4 seeds per species, scaled to each tree's height — enough variety at a distance,
   or more seeds (each is 640 KB at desktop)?
3. **Hand-over distances:** start near (~64–96 m on High, cheapest, needs the impostor to hold up close)
   or far (~160 m, prettier, more real trees drawn)?
4. **The CPU adapter and phones:** impostors there too (they are cheaper than `lod_trees.rs`), or keep the
   phone's far trees off?
5. **Snowy leaves and seasons:** bake a snowy variant (another layer per tree) or tint at draw time?
6. **Shadows from far trees** (I-5): wanted on High, or Ultra only?
