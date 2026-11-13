# Work queue

The owner's ordered queue of work, with the decisions behind each item, so it survives a session
ending. Update it in the same change that starts, finishes, re-orders or re-scopes an item. The phase
status lives in `docs/IMPLEMENTATION_PLAN.md`; this file is the order and the owner's words.

**Shots (owner, 2026-11-12):** bench screenshots are taken with clouds off unless the shot is about clouds ("The
clouds make those screenshots hard to read").

**Rule (owner, 2026-11-11: "This is why we need to be documenting these as we go"):** every request, answer
or decision the owner gives goes into this file **in the same turn it arrives**, before or alongside starting
the work, quoting the owner's words; never only in a session's memory or an agent's brief. An item nobody
recorded doesn't exist for the next session.

Last updated: 2026-11-11 (0.52.108).

## Queued next, in order (2026-11-12)

- **Realistic spawn without `at` (brief, 2026-11-13; BUGS.md #132):** "a fresh Realistic world never loads without a
  start point … fix (bounded search with a fallback; the whole spawn ≤ ~2 s natively); a test that the Realistic spawn
  resolves within a time/step budget on seeds 1, 7, 42 and stands at a shrine. Also check whether the client shows a
  stage for it in the loading screen (if a spawn search is slow, the loading screen should say so)." Done on
  `claude/realistic-spawn-9poyrj` as far as it reproduces: not reproduced natively or in the cloud's Chromium (the
  spawn ≤ 0.25 s a face natively; the search was already bounded, `spawn_wider`, with a fallback); the spawn's shrine's
  cache key cost 134 ms a face on Realistic and now 1 ms; the budget test is `tests/realistic_spawn.rs`. The loading
  screen has the stage: "Finding a place to stand" (the local worker's `spawn`, `shared/look/loading.json`), seen in
  the browser at 1.3–1.6 s. **Open:** the owner's repro on the Mac (which stage it sits on, the console).
- **In progress (local):** underwater look pass toward the owner's ABZÛ reference (item 40: the surface seen from
  below first — "This is more what I want from the underwater surface" — then shafts, turquoise fog, caustics);
  branch `claude/underwater-look`; the reference image at the session scratchpad `abzu-ref.jpg` (not in the repo).
  Video 2 (water/underwater, plan in item 40) waits for the owner's look approval.
- **Next, in order:** (1) PR #856 slopes show tool hits (`claude/ramp-hit-marks-eaj3ru`) — conflicts with main,
  resolve and merge; (2) big-world terrain quick fix (item 52) — built on `claude/notes-batch-2` (also carries the
  v5 approval and ABZÛ notes), its PR open, not merged: it waits for the owner to raise the 0.56 line; (3) the moon's client side M-7c (item 27) — built on `claude/moon-client-m7c-mxftge` (the browser flight a local check);
  (4) browser check of Realistic worlds (R6 merged #858: create, play, reload); (5) the forest edge (~9 ms over at
  resolve and merge; (2) big-world terrain quick fix (item 51) — WIP on `claude/notes-batch-2` (also carries the
  v5 approval and ABZÛ notes; the cloud session was stopped mid-work: read its status note on that branch); needs
  the 0.56 line raised by the owner; (3) the moon's client side M-7c (item 27) — built on `claude/moon-client-m7c-mxftge` (the browser flight a local check);
  (4) ~~browser check of Realistic worlds~~ (done 2026-11-12: R6's exit met, item 15); (5) the forest edge (~9 ms over at
  High on the M1; impostors save ~4 ms, #848); (6) the in-game cloud check (#822 shapes/colour, #790, #801, #823).
- **Creative hit fixes (brief, 2026-11-13; BUGS.md #127, #128):** (1) "Fix so Creative breaks every block, slopes
  included, in one hit": not slope-specific, a held tool keeps its hits in creative by the owner's 2026-11-11 rule
  (BLOCK_DAMAGE.md §7). Orchestrator: "The creative question goes to the owner; until answered keep the rule … and fix
  only the form text": creative's description now reads "without a tool any block breaks in one hit (held tools keep
  their hits)". **Open for the owner:** should a held tool also break in one hit in creative? (2) "The pick ray must
  hit the wedge's sloped face … targetable from every angle its face is visible": not reproduced natively (BUGS.md
  #128); a both-faces guard test added; needs the screenshot's world, position and look.
- **Starter pin (brief, 2026-11-13):** "`the_starter_worlds_terrain_is_unchanged` fails on main … find which merge
  after the 0.55 re-pin changed the starter world's terrain … If intended … re-pin with the commit cited; if it was
  NOT meant to change the starter … fix the leak." Done (BUGS.md #114): no merge changed it; the 0.55 line's
  combined pin (58f72da) was wrong from the start, re-pinned to what 0.55.0 onward generates.
- **Merged today (late):** #857 the axe like the other tools; #858 Realistic in New world; #854 hits at contact;
  #852 sound placement; #853/#855 one-command video re-records (`take.mjs`); #851 big-world placing; #848 far-tree
  impostors fixed (BUGS #106); #845 big-world LOD; #846 portal exit on face B; #847 leaves flush with logs; #841 the
  0.55 line (tree pads, tall grass stages); #842 more animals; #833 grass sounds and leg cadence; #836 underwater
  audio; #829 floating trees, leaves let water through; #832 deny sound; wind only when flying.
- **Showcase videos (item 40):** video 1 (bifacial) approved at v5; its clips cleared. Tool: `take.mjs <video>
  [--only sN]` (SKILL.md §0).

**Current order (2026-11-12, evening; owner asked "What is the current priority of upcoming items"):**
Releases were blocked ~18:45–21:40 (a test type error); unblocked, so everything merged since ships in the next
releases (incl. the first-person body #791). Local (the Mac, one at a time): (1) bifacial video v3 (portal exit
merged #824, upside-down camera #821); (2) the in-game check of the cloud rework (#822 shapes/colour, #790 sunsets,
#801 dither, #798 deck, #823 rim) + the extinction A/B; (3) forest edge to 60 fps at High (~7 ms over); (4) next
showcase videos, each with a shot plan first; (5) Realistic world R6 in New world. Cloud now: tall grass sounds
and leg speed (#103), underwater/surface swim audio (#104), floating trees + leaves (item 45), tree pads (item 44,
needs the owner's 0.55 line), tall grass height (item 14), far-sky rim/band (#823, conflicts). Then: the moon's
client side (item 27), orbits O4–O5, big worlds memory (#76/#77).

Owner, 2026-11-12 (before showcase video 2), with a screenshot from ABZÛ: **"Underwater reference photo from the
game abzu for pretty underwater with transparent surface, light rays etc."**, then **"This is more what I want from
the underwater surface."** → an underwater look pass: the surface from below first (bright and see-through, the sky
in Snell's window, the sun a blown-out disc with light radiating from it, the ripples catching the light, a soft
glow round the window, the mirror outside it turquoise rather than dark), then the shafts and a turquoise fog with
a longer clear view and a colourful floor. **Built (`claude/underwater-look`):** the look's `window.sunGlow`,
`sunPower`, `rimGlow` and a `tone` grade (fog colour, brightness, mirror, floor light) as data
(WATER_RENDERING.md §19), shafts longer. Next: the owner's look.

Owner, 2026-11-12 (BUGS.md #110): **"I think the sound 'source' direction might be the issue with some of our sound
effects. They sound like they're coming from right behind your head. Are we using 2D sound?"**, then **"Like
footsteps sound like they're in your ears."** → own footsteps grounded (below and slightly ahead, at the feet, a touch
of the room), positioned rather than non-positional, never behind the head looking up or down. **Fixed (cloud,
2026-11-12):** the listener frame checked on both faces; own sounds placed round the ear by data (`shared/sounds`
`self`: feet, body, hand; AUDIO.md §5a). Next: the owner listens on headphones.

Owner, 2026-11-12: **"How are realistic worlds going? And forests? Those are the two I'm most interested in
seeing."** → next on the Mac after the flying frame rate: a look at both (screenshots/video of a Realistic world —
plates, ranges, rain shadow, biomes by climate — and the old-growth forest), then R6 (the Realistic world in New
world) and the far-tree impostors drawn (I-2/I-3); R5 (geology) in the cloud now.



The running order across the items below; each line names its item, where the owner's words and details are.

1. **High at 60 fps on the M1 Air (item 30, P0)** — running locally: measure each feature at High, cull leaf cards
   inside crowns and on the far side, pull their range in, a Trees group and a setting for every costly feature.
2. **Distance haze tuning (item 29)** — only the height fog's three numbers, ground-level and hilltop shots, clouds off.
3. **Cloud lighting (item 13)** — shaded bases and self-shadowing, matched against the old 2D deck.
3b. **BUGS #106, trees drawn as bare sticks (owner, 2026-11-12, URGENT):** "What happened to the trees, they just
   look like sticks?" Do: (1) the leaves fade out and the LOD omits its trees only when the impostors are ready to
   draw (atlas uploaded, the area's instances in), with a no-GPU test; (2) find why the impostors draw nothing on the
   release and add `__dwell.impostors()`. Then (local GPU check): the impostors draw but small and faint: make a far
   impostor's crown match the real crown's size and density at the hand-over, with a CPU test. Gate and readout done
   (claude/tree-sticks); the CPU test shows the bake and the fragment's sampling match the real crown, so the cause
   is on the GPU path. **Fixed** (claude/impostors-gpu-fix, found on the GPU): the quad stood at its sphere's centre,
   behind the near tree's own trunk and branches (still drawn), which showed through as sticks; and the leaf texels
   were lit as open sky, lighter than the near crowns. The quad now stands at the sphere's near side (same pixels)
   and leaf texels take `impostors.json` `leafShade` (BUGS.md #106, Fixed).
4. **Far-tree impostors (item 31)** — the bake in the cloud now; drawing them locally after, replacing lod_trees.
5. **Dense forest reference (item 32)** — in the cloud now: `old_growth`, `test:forest`, the `forest-perf` bench.
6. **The moon, client side (item 27, M-7c)** — the client follows a player onto the moon (its chunks in the moon's
   frame, the view rolled to its down, `grid.frames` turned on); then M-5 (LOD per grid), M-8 (moon spawns A/B),
   the bake in a worker + cached, `MOON_PHASE` wired, the disc oriented by `moon_axes`.
7. **Orbits O1–O3 (item 15)** — in the cloud now; then O4–O5 (seasons).
8. **Chopping follow-ups (items 24, 26)** — see bites round 2 and the kept gash in game; stone chips that break the
   cell's outline (item 18 lever 3); half the hit shards on phones (the flag nothing sets yet).
9. **Big worlds (BIG_MAP.md)** — BUGS.md #76 (browser memory grows with world size) before 8×/10×; #77 (bigger
   islands lose their ranges, speckled lakes: an owner look question); region maps built ahead with no hitches.
   **10× world creation (BUGS.md #91, owner 2026-11-12):** "I tried to start a 10x world … up to 3 minutes so far
   on 'Shaping the world map...' 0/2. Doesn't indicate well what it's doing."; then "started the server 200+
   seconds in … almost 1 minute looking for a place to stand … still 0/2 280+ seconds in"; then "Loaded in 300s
   in, but I can only see the skies of both faces, no world." / "Can't move." Target: a 10× world playable in
   well under 30 s on a fast Mac, with progress inside each face's map build.
   Then (owner): the empty-sky world shows "Could not connect to 10x…": the client must wait for the local server's
   ready signal, not a fixed timeout, and a dead worker must report its error clearly.
   **Fixed natively (2026-11-12):** the uplift and the channels' index grew with the radius (a 10× face 43.7 s and a
   12.4 GB server natively); now a 10× face is 3.1 s (1×: 1.9 s) and its server 6.2 s at 484 MB; the map stage shows
   each face's steps; a dying server worker says why (BUGS.md #91, BIG_MAP.md §3.1). **Next: the owner tries 10× in
   the browser again.**
   **Terrain stops drawing past a stepped edge (BUGS.md #108, owner 2026-11-12):** "worlds bigger than 2x seem to
   give up loading chunks at some point, and just cutoff. The full minimap shows the whole world, and the terrain is
   playable. It just doesn't show up. It even has collision on the chunks that didn't load in." Then: "Sizes 8x and
   10x for the map spawn you in the void where there's no terrain only collision." **Fixed (2026-11-12):** the
   LOD classified sections by the 1× disc, so everything past 50 km read as empty space at every size; it now
   classifies in the world's own disc (`kind_from_bounds_of`), and `__dwell.debug().lod` counts what the view wants,
   has built, draws as nothing and the budget dropped. Next: the owner flies a 4× and spawns in an 8×/10× world.
   **Placing past 50 km (BUGS.md #109, found 2026-11-12 fixing #108):** in a 4×–10× world a Place past the 1×
   disc's 50 km did nothing (the client dropped it, and the server refused it as out of the world); breaking worked.
   **Fixed (2026-11-12):** every edit check (the client's pre-checks and prediction, the server's block and region
   checks) reads the world's own bounds (`VoxelWorld::bounds`).
10. **Realistic R5–R6 (item 15)** — geology by plate history with today's blocks; the World option.

### Waiting on the owner (each with what it means and the default meanwhile)

Answered 2026-11-12: (1) **"Yes add those biomes"** — cold desert and tropical rainforest (in progress, cloud);
(2) **"climates should align with the systems in place, orbits, etc."** and (3) **"Follow orbital systems"** — the
two faces' climates and the gradient across the disc come from the orbits model alone, no extra tuning; (5) a 1×
vs 2× comparison of screenshots for perspective (queued, local); (6) **"iOS runs at 60fps on Low settings"** —
item 16 done; (7) **"Metal tools break double sized bigger chunks off and reach 50% broken faster, which is when
the blocks break. Also have double durability."** (built, branch `claude/metal-tools`: a tier's
`chunk` and metal speed 2 × stone's, so the metal pickaxe and shovel break in 3 hits to 5 with marks twice
the size, the metal axe fells in 5 swings to 11, metal tools 64 hits; BLOCK_DAMAGE.md §9; outstanding:
the owner's look in game); (8) **"I'd like to see a comparison of
the two"** — palm fronds as leaf blocks vs frond cards side by side: built in the Tree Growth prototype
(`docs/prototypes/tree-growth.html`, *Palm fronds* → *Split*, or `?species=palm&fronds=split`; to be republished to its link).

Still open:
0. **Which showcase recordings first** (item 40's list).
1. **Pick a haze setting** (item 29), once the ground-level and hilltop shots (clouds off) are taken. Default:
   today's fog.
2. ~~**The in-game Chisel editor and the textures tab extras**~~ — **approved (owner, 2026-11-12: "these all seem
   solid yes"); now item 34.** (Phase 56 B7/B6's outstanding parts.) The owner
   asked what the scope is: *Chisel* is the separate worldgen page (`chisel.html`: biome sliders, a live preview,
   Export). The "in-game editor" would open those sliders inside a running world and regenerate the terrain
   around you as you tune. The "textures tab extras" would let Chisel's Textures tab add new materials and blocks
   by data, edit the water and sky colours, and paint item and model textures. Both were left for the owner to
   say whether they're wanted and what they should do. Default: not started.

## Next, in order

0. **P0: the phone's frame rate at Low** (owner, 2026-11-11: "~23 fps on mobile … used to get 60 fps"; BUGS.md #72).
   - Done (branch `claude/mobile-fps`): leaf cards off on the phone tier by data (`leaves.json` `cards.tiers`), a
     `?cards=` override, no card into a trunk (the z-fighting). On the bench's phone page in a forest −0.4 ms of 4.8 ms GPU.
   - Done (branch `claude/mobile-fps-2`): the Low preset turns the cards off (`leafCards`, a Leaf cards setting);
     the bench opens released builds (`--base …/dwell/v/<version>/`). 0.51.24, 0.52.77-dev and now measure the same
     on the desktop phone page (CPU 4×): no regression there (BUGS.md #72's table).
   - Next: the owner's iPhone on a build with the cards off. If still slow: the arms pass (its own pass: a store and
     reload on a tile GPU), the composite (~2.2 ms), the caustic tile (0.9 ms every other frame); why 0.52.101+
     releases still 404 under the bench.

5. **Leaves, option 5, with matching far trees.**
   - Leaf Lab option 5 at its defaults ("#5 looks really good with defaults"): canopy normals 75 %,
     chamfer 0.28 m, leaf cards density 55 % and size 0.95 m. Leaves stay blocks. Fix the dark gaps at
     chamfer joins. Data-driven; the CPU tier without cards.
   - Cards on near trees only ("Cards could only show on near trees"), faded out by distance (data,
     about 48–64 m to start).
   - Far trees: "We do need better low rez trees the ones directly beyond high detail look nothing like
     the main trees." The LOD's trees match the near ones (octagon trunks, the same crown shape at
     coarser detail, the same colours and canopy lighting), no pop at the hand-over.
   - **Status (0.52.56):** chamfered leaf cells (gaps fixed) and canopy normals built
     (`shared/look/leaves.json`, TREE_GROWTH.md §10; the crown is a local 3 m centroid, not a per-tree
     ellipsoid); leaf cards built (`cards`, faded 48–64 m, none on a CPU adapter).
   - **Status (0.52.57):** the LOD's 1 m and 2 m levels draw trees as the chunks do (chamfered canopy
     cells, crown normals, octagon trunks; `lod_trees.rs`); the cards' painted clump tile (`leafCard`).
     Outstanding: coarser levels still show only canopy-tinted column tops; a per-tree crown.
6. **Tree species and growth in game.** Owner: "trees start with small trunks, thin, same shape, but
   shorter, smaller leaves … it should grow over time" and "Oak trees …, Pine trees …, and Palm
   trees." Species as data (`shared/trees`), growth stages stored, per climate (vegetation.json),
   from the Tree Growth prototype once the owner has looked at it. Open: palm fronds (cards vs blocks).
   - **Status (0.52.58, ADR 0079):** species as data (`shared/trees`), the builder (a pure function of
     species, seed, stage) and placement in the starter world (oaks, pines, palms; mostly mature, some
     young; heights by seed) built; the default world unchanged. Fronds are leaf cells for now.
     Outstanding: growth over time (per-tree stage on the world clock, `growers`), thin trunks (a log
     width), per-tree leaf tints, frond blades, palm density and climate tuning.
   - **Status (0.52.59):** thin trunks built (`dwell:log[width]` in ¼ m, the narrower octagon drawn and
     collided; saplings thin, thickening by stage); tree generation made
     cheaper (lazy ground columns: the starter's tree spots faster than 0.52.57). Outstanding: growth
     over time (planned, ADR 0079), per-tree leaf tints (not cheap: the mesher doesn't know a leaf's
     tree), frond blades, narrow logs in felled pieces and the LOD.
   - **Status (0.52.60):** growth over time built (ADR 0079 S3: per-tree records saved in an optional
     `tree_growth` table, the growers step, edits win, felled trees stop; `?growth=N` time-lapse).
     Outstanding: per-tree leaf tints, a sapling item, frond blades.
   - **Status (leaf tints, ADR 0079 S4):** per-tree leaf tints built: each species tree's leaves and
     cards tinted by its seed's `variety` (lightness, hue), the far (LOD 1 m/2 m) trees alike, the same
     tree from either chunk at a border; placed leaves and the default world unchanged. Mesher logic
     and tests only: the in-game look is still to be checked on a GPU. Outstanding: a sapling item,
     frond blades.
   - **Status (saplings, ADR 0079 S4):** saplings built: `dwell:oak_sapling`, `dwell:pine_sapling`,
     `dwell:palm_sapling`, named by each species' `sapling` data (`plantOn`: grass/dirt for oak and
     pine, sand for palm; `dropChance` 5 %); placed on its ground a sapling becomes stage 0 and a
     growth record that grows by the world days; other ground is refused; leaves of species trees drop
     them in adventure; creative lists them under Plants. Outstanding: per-tree leaf tints, frond
     blades, a sapling's own model.
7. **Chopping prototype** (behind a switch, not replacing chopping until approved). Decided:
   - A tree falls once more than 50 % of its cross-section is cut, from one side or two.
   - The back cut is supported, not required: notch + back cut leaves a hinge that steers the fall
     toward the notch; one-sided chopping falls toward the cut side.
   - Stone axe about 12 good chops; each chop removes a share of the cross-section; the metal axe cuts
     more per chop; chopping along the grain is half as effective ("wrong way is chopping along the
     grain").
   - The height you chop is the stump height.
   - The split keeps the notch, not one flat slice ("it's effectively just slicing it like fruit
     ninja"): seen from the side `| / \ |`, the falling trunk keeps the notch's upper face and the
     stump its lower face; the rest breaks flat across the back cut and hinge.
   - Also: the axe looks big mid-swing (the swing clips' pose).
   - **Status (0.52.61, first slice):** the prototype is built behind a switch (`chop2` in
     `shared/tools/tools.json`, off; dev `?chop=v2` for a local world; ADR 0067's 0.52.61 amendment,
     CHOPPING_REDESIGN.md §7): chops land at the crosshair (the row hit is the stump), each removes a
     share of the section (stone 0.045: 12 chops; metal 0.075: 7; along the grain half), the first
     side cut is the notch (V) and a cut on another side a flat back cut, it falls past 50 % toward
     the notch (else the side cut), and the split keeps the notch (the stump the lower face, the
     trunk's bottom the upper face: the `open` wedges). Tests: dwell-protocol and dwell-sim (both
     faces). Outstanding: the hinge's flat break is cell-sized (the whole cut cell is the stump's
     wedge); the notch is still carved as today's V and kerf; adjacent sides don't add up (only
     opposite ones do); the swing animation is the arc's (timed to the middle of the arc), not aimed
     at the bite; the crosshair reaches as far as the swing (`reachM`, 2.2 m), less than the target
     outline; the dedicated server has no switch yet; the owner's review. In game (bench, `?chop=v2`,
     config `chop-v2`): the notch at the crosshair and its deepening shown, the fall taken; the stump's
     and trunk's wedge faces not yet shown in game (the bench's placed logs didn't stack; the server
     test checks them). The bench's `axe` scenario no longer builds its trunk
     (its `region fill` of logs leaves none; cause not looked into): needs a fix.
   - **Status (0.52.62, round 2):** cuts on adjacent sides add up (what is left is the section after
     every side's slab; `Cut::removed`); the aim reaches as far as the block-target outline
     (`REACH_DISTANCE`, 5 m); a cut octagonal log is carved inside its octagon (`carve_in`: the carve
     clipped to the prism, the chamfers as bark where wood is left); the look clears only what it
     passes through and stops at a stump's wedge (seen in game: the swings after the fall broke it);
     the bench's `axe` scenario builds its trunk again (the bench's `select` now updates the creative
     view at once: a `pick` right after it went into the old slot and the fill was the pickaxe's
     nothing). Seen in game (bench `chop2` scratch scenario, `chop-v2`, a tree built on the flat world:
     no generated tree was within 256 m of the default world's spawn, and the starter world isn't
     reachable from the page): the notch's V at the crosshair, the flat back cut from the far side
     beside it, the fall toward the notch, the stump's slanted face after. Outstanding: the trunk's
     upper face is not clearly seen in the shots, and a short octagon piece lies beside the stump
     after the fall (not looked into); the hinge's break is cell-sized; the hold-to-chop rhythm and
     IK (C2, C3) and the dedicated server's switch, for later; the owner's review.
8. **Lava (VOLCANOES.md phase V1)** — next after the terrain leftovers' current round (0.52.66), ahead of
   the remaining leftover sections (owner, 2026-11-11: "Let's implement lava", "Into our queue"): lava as
   a fluid — flows slowly, cools into rock, gives light, damages, steam where it meets water. The other
   volcano phases stay under Future. Lava's danger (owner): heavy damage while in it, a moment of burning after.
   **Core built (V1, `crates/dwell-lava`):** lava's own surface grid (viscous, cooling to basalt, obsidian
   and steam with water, lake and river vents, burning, saves, near/far), as data in
   `shared/fluids/lava.json`; VOLCANOES.md §6. **Server built:** the fields per face in `dwell-sim`
   (near/far like the water), lava, basalt and obsidian cells written into chunks through the edit sync,
   steam taking the water, players burning through the server's damage, the optional `lava` save table,
   `dwell:lava` block, `Server::place_lava_vent` (dev). **Slice 3 built (logic):** the starter world's lake
   vent on its youngest hotspot island's summit as worldgen data (`tectonics.lavaVents`, opt-in per cone,
   placed by the server when the world is created or first loaded; the default world lists none), a `Burn`
   damage cause sent only to clients that say `CausesHello` (older ones hear `Fall`), and fields re-reading
   an edited column's bed and water (a dug channel takes the lava); VOLCANOES.md §6.2. **Look, part 1
   built:** the painterly lava tile (self-lit, crust drifting and cracks breathing by `molten` material
   data, flat on the CPU tier) and basalt and obsidian tiles; VOLCANOES.md §6.3, WATER_RENDERING.md §17.
   **Part 2:** `__dwell.lava` dev tools (the summit lake was there: the first shots were misplaced),
   steam sent to clients that say `SteamHello` and drawn as puffs. **Round 3:** world-space crust plates
   with glowing seams, steam's look as data and a hiss, the burning screen edge and a sizzle
   (WATER_RENDERING.md §17.1–17.2). **V2 built (logic):** rivers to the sea: fields that grow with their
   flow (tiles added as lava reaches them, dropped when all rock, capped by `maxTiles`), cost by molten
   area, lava tubes (`tubeSpeedMs`), lava running into the sea and building land above it, save record
   version 2 (version 1 still loads), and a river vent on the starter island's flank on face B whose way
   to the sea is 1.3 km (lavaVents `faces`), and breakouts through crust dams (`breakoutDepthM`);
   VOLCANOES.md §6.4. **Outcomes, not a target (owner, 2026-11-11: "Sometimes it may, other times it may
   not" reach the sea):** breakouts melt only a dam on the pond's way down, so levees beside a channel
   hold; tested that a strong vent on a steep flank reaches the sea and builds land and a weak one on a
   shallow slope crusts and stops short; the starter river runs ~600 m of its 1.3 km and crusts (no land;
   the ignored long test reports it, `DWELL_LAVA_TUNE` for tuning). Lava stiffening as it cools was
   tried and only slowed the whole flow (§6.4). **Lava glow (owner, 2026-11-11: bloom, bright red-orange, darker as it
   solidifies):** heat stops as data (`lava.json` `heat`), a `heat` property on `dwell:lava` written by the
   server as a column cools past a stop and drawn per cell, orange-red stops that keep their hue; **left:**
   the bloom pass (VOLCANOES.md §6.5).
   **Next:** heat per cell; the water grid's columns as the fields' water; eruptions.
9. ~~**Loading-time display bug.**~~ Done (0.52.71): the 10 s was the graphics' start (one long task on the
   main thread making the renderer's pipelines) begun in the same task that ended the game's load, so the
   screen never painted "Starting the graphics…" and stood on "Loading the game… x MB 0.0 s". Each stage is
   now painted before its work (`ui/startStages.ts`). 0.52.75 measured it cold (fresh Chrome profile,
   `--disable-gpu-shader-disk-cache`, M1): the main-thread task was ~90 ms, ~40 of it the atlas mips'
   `powf` (now a table: ~50–65 ms); the pipelines compile in Chrome's GPU process, a ~270 ms frame stall
   after the stage, not in the task. Open: that GPU-process stall (seconds on a slow GPU?) needs
   `createRenderPipelineAsync`, which wgpu 30 lacks: a wgpu patch, or warming Dawn's pipeline cache
   with the same descriptors through `Device::as_webgpu`.
10. **Speed-ups 1–6** (owner: "Implement 1-6 now"): the version out of build scripts (dwell-sim's build.rs reads
   client/package.json → every release recompiles), version + release note assigned at merge, an agent
   quickstart doc, a reliable browser kit (warm prebuilt client, exact camera goto, stall retries, saved
   spots), per-area check commands, lighter paperwork for small changes. (Runs before lava.)
   Done: 1 (local builds read `shared/version/line.json`, `<line>.0-dev.local`), 2 (pending release notes,
   `shared/releases/pending/`, assigned by the Release workflow; RELEASES.md §3; its first run on `main`
   still to watch), 3 (`docs/AGENT_QUICKSTART.md`), 5 (`npm run check:<area>`) and 6 (CLAUDE.md "lighter
   paperwork"), with `docs/CLOUD_SESSIONS.md`; 4, the browser kit (`claude/browser-kit`, in review): the
   warm prebuilt client (`npm run bench:warm`, `--warm`), placed `shot`s (`at` / `above`, face-checked,
   settled), `look` turning a placed camera, stall retries, saved spots (`client/bench/spots.json`,
   `--spot`); DEV_TOOLS.md "Screenshots of a place".
11. **Deer/wolf ecology simulation, first slice** (owner 2026-11-11, with a mockup:
   docs/proposals/ECOSYSTEM_MOCKUP_NOTES.md). "Simulate at a data level only when you're not nearby, render
   simply from afar, and exactly when close by." Answers: far = herds and packs (one record each;
   individuals made when near); deer eat tall grass, but "it shouldn't affect the world until you are close
   enough to see" — a grazing value per area is the truth far away, applied to real tall grass near
   players; watch only for now (no player interaction); the whole island, coarse (fast-forwarded like water).
   Tuning (owner): resistant to complete destruction, quick to replicate up to each group's cap, slow to
   starve, and no runaway (the island's total bounded by grazing for deer, by prey for wolves).
   **Started 2026-11-11:** the data-level core is built — `crates/dwell-ecology`, `shared/ecology/ecology.json`,
   docs/ECOLOGY.md (herds and packs, forage per cell, the day step, 15 tests). **Slice 2 built:** the server
   holds one ecology per face, stepped by the world's days (bounded catch-up: 4 steps a tick), saved in an
   optional `ecology` table with a per-world switch (default on, additive) and speed; a server-side read-out
   (ECOLOGY.md §6). Owner's correction (2026-11-11): no floors — herds and packs can die, resisted by the
   system (Holling type III predation, refuges, fast low-density breeding, dispersal, slow starving).
   **Slice 3 built:** individuals near players (ECOLOGY.md §8, `dwell-sim` `herds.rs`): a record within
   64 m of a player becomes its deer or wolves (the existing creatures, driven) and folds back past 96 m
   (survivors, averaged needs, centroid); the mockup's needs-driven rules as data (flee, sleep at night,
   graze the cell and sometimes a real tall grass block, chase and kill, mate up to the cap, company);
   the creature rules for deer and wolves left out while the ecology runs. **Slice 3b built** (logic, no
   rendering): the far step sees near groups through a summary the near side keeps (hunted, fled, counted
   in the totals; a far kill becomes a death near players); a herd comes near as the creature rule's mix
   (does, sometimes a stag; fawns by the same data); individuals only on the ecology's ground (walkable
   island cell, dry, ≤ `near.maxSlopeDeg`), else the nearest within `near.searchM`; the far groups sent
   every 2 s to clients that ask (`EcologyHello`, `inventory_flags::ECOLOGY`) and `__dwell.ecology()`
   (ECOLOGY.md §9). **Far rendering built (simple):** heads in the species' `far` look, on the loaded
   ground, fading by distance, none on the CPU tier (ECOLOGY.md §11); not yet past the loaded chunks,
   screenshots and bench cost. **Local worlds built** (2026-11-11, ECOLOGY.md §6): the local server already
   had both maps installed in its instance, so its ecology runs, saves and reloads as on native; the bug
   was the client's session dropping `Ecology`, so `__dwell.ecology()` and the far herds were empty on
   every server (fixed). **Smaller island built** (2026-11-11, ECOLOGY.md §6): only land cells stored,
   forage f32, the saved forage as runs (a cell at its most not written; version 2, version 1 still
   read): 12.5 MB → 0.93 MB for both faces, the record ~3 MB → ~1 KB fresh, a few KB grazed, and it is
   still written only when it changed. Next: grazing applied to tall grass as chunks load.
12. **Players' saved structures as placement sources** (owner 2026-11-11): per player (their library, usable
   in any world they create); placed copies editable, not protected; 32³ for now (may change), or larger
   structures as a grid of 32³ tiles. Sharing between players: Future. The owner's words (2026-11-11): "Structures are
   per player, eventually shareable (add to roadmap). Yes can be edited, not protected. Yes we can keep
   32^3 for now, though we may change this later. Or save as an array of that size." **Built**
   (2026-11-12, `claude/saved-structures-library`, STRUCTURES.md §9): the structures store as the player's
   library (per device); a selection over 32 across / 64 tall saved as one tiled file of 32³ tiles, up to
   `shared/structures/library.json`'s limits (128 × 64 × 128); the creative inventory's Structures tab
   places one with the paste ghost (R turns 90°, right click places) as `Paste` region edits of at most
   32³, which the server validates as any paste; a `structures` mode ability (creative only). Sharing:
   FUTURE.md. Open then: the server couldn't refuse a structure placement as such outside creative (both
   capability flag bytes full; a paste there was paid in items; done since, below), the anchor, a ghost of the blocks.
   Owner, 2026-11-12, on the server not refusing placements outside creative because `welcome_flags` and
   `inventory_flags` are full: **"Sounds like we need a new flags field for server protocol."** **Built**
   (2026-11-12, cloud, ARCHITECTURE.md §8.3 "Capabilities"): capabilities named by string in
   `shared/protocol/capabilities.json` (generated to Rust and TS); the server lists its own in an optional
   `WorldSettings` field (older clients skip it), a client that sees it answers with a `Capabilities` message
   (never sent to an older server), a feature is on when both listed it. The rule: new optional features add a
   capability, not a flag bit; nothing existing migrated. First use: `structures.place`, a library
   structure's tiles go as `RegionEditOp::Structure`, which the server refuses where the mode lacks
   `structures` (a 0.54 server still gets plain pastes). Not breaking: every new byte is skipped by or never
   sent to an older peer. Still open: the anchor, a ghost of the blocks.
13. **Volumetric clouds** (owner 2026-11-11: "volumetric clouds in the sky. These clouds will likely be
   useful for weather sim, and volcano smoke. So they need to be performant."). **Built (branch
   `claude/volumetric-clouds`, CLOUDS.md §8):** a ray-marched layer in its own reduced-resolution pass
   with temporal accumulation and a depth-aware upsample; tiers as data (High ½ opt-in, Medium ¼ the
   desktop default, Low the 2D deck); plumes with a colour by data; the cheap deck kept for Snell's
   window and reflections. Bench from above: High +1.5 ms (budget 1.5), Medium +0.9 ms (budget 0.8).
   Next: trace the clouds pass's fixed cost; plumes as run-time uniforms for a volcano or weather
   system. **Built (2026-11-12, CLOUDS.md §8.7):** plumes as a run-time list by name
   (`dwell_look::plumes`, `plumeSlots` 8 as data) packed each frame into one uniform and marched apart
   from the layer over each crossed plume's own span (`plumeSteps` per tier: High 8, Medium 6, none on
   Low/Off/CPU), so a plume is a buffer write, not a pipeline; the first source a faint steam plume
   over each world lava vent's top molten cell (`sources.lavaVent`, both faces); `__dwell.plume(op,
   args)`. The pass's cost read from the shader: the march's arithmetic is ~0.15 G ALU a frame on
   Medium, so the measured 0.8–0.9 ms is the pass's fixed part; cut the sky per texel, the plumes in
   every layer step and the layer's samples with no coverage. Left: timing those cuts and tracing the
   pass's start and history copy on a GPU (local bench).

Cloud sessions: logic-only items can run as `claude --cloud` sessions in parallel (verified 2026-11-11).

   Owner, 2026-11-12: **"The clouds make those screenshots hard to read. Also the lighting on the new clouds just
   looks like it's all bright white so the actual shape is difficult or impossible to read. Our old 2D clouds had
   better lighting, use that as a reference."** Next: light the volume like the old deck read — shaded bases and
   self-shadowing toward the sun (a short light march), sky-coloured ambient on the shaded side, a brighter
   sun-facing rim — matched against the old 2D deck's look side by side; and bench shots taken with clouds off
   unless the shot is about clouds.
   Owner, 2026-11-12, again: **"Also the volumetric clouds still look bad no proper lighting or shading?"** In progress
   (cloud): a light march toward the sun for self-shadowing, darker bases, sky-coloured ambient, silver lining —
   matched against the old 2D deck; then the in-game look and cost (local).
   **Built (2026-11-12, branch `claude/cloud-lighting`, CLOUDS.md §8.8):** the volume lit as data: Medium takes 4
   light samples toward the sun (High 6; growing steps, stopping where the path leaves the layer; the cause was
   Medium taking none), Beer with a multiple-scattering floor and a powder term, a dual-lobe Henyey–Greenstein
   phase (the silver lining toward the sun), the sky's light from above and a darker warm ground bounce from
   below dimmed by the cloud over and under each sample (read free from its own noise): bases ~0.45, tops ~0.95
   in the deck's palette; night and sunset tints kept; the light samples reuse the sample's warp (half the old
   cost each). A CPU twin with tests (top brighter than base, deeper darker, phase toward the sun, tier caps from
   data, both faces). Read from the shader: Medium ≤ ~5.3k ALU a texel (was 2.6k), High ≤ 7.6k (was 4.8k).
   Owner, 2026-11-12: **"a bug with volumetric clouds z fighting against tall terrain like mountains."** (BUGS.md #88)
   **Fixed (2026-11-12, cloud, CLOUDS.md §8.9):** each texel marches only to the farthest scene pixel under it, keeps
   that scene span (not the jittered first cloud), and the composite and history weigh texels by it in metres;
   CPU twin tests (no cloud behind the rock, no flip over the jitter, silhouettes, both faces). Next (local): the look
   at a mountain in the layer and the pass's timings.
   Next (local): the in-game look side by side with the old deck (sun high, low sun, from above, night) and the
   bench before/after at the §8.3 views; tune by the data.
   Owner, 2026-11-12, after the lighting rework: **"The clouds have lighting but they don't follow cloud shapes at all,
   do some research into cloud simulation. They look like white blobs."** Today's density is the 2D deck's coverage
   cut by height and eroded by 3D value noise, so clouds have no real form. Next (cloud): research the
   production cloud models (Horizon Zero Dawn / Nubis, Frostbite, Red Dead Redemption 2) into docs/CLOUDS.md and
   build the shape model: tiling 3D Perlin–Worley base and Worley detail noise baked at load, a weather map
   (coverage, cloud type, precipitation), height profiles per cloud type (flat-based cumulus with billowing tops,
   stratus sheets, towering cumulonimbus with anvils), base-shape remapping and edge erosion with curl distortion,
   wind and shear; within the High-on-M1 budget.
   **Built (2026-11-12, branch `claude/cloud-shapes`, CLOUDS.md §9):** the research (Schneider 2015/2017/2022,
   Hillaire 2016, Bauer 2019, Häggström 2018) in §9.1; the shape model as data: a weather map from the deck's
   noise (cover, cloud type, precipitation; `weather`), height profiles per cloud type (`cloudTypes`: stratus,
   stratocumulus, cumulus, cumulonimbus with an anvil; a new type is a list entry), a Perlin–Worley base (64³)
   and Worley detail (32³) baked once in the job workers (`Job::CloudNoise`, the deck until whole), the base
   remapped by the cover, the edges eroded by curl-distorted detail (wispy bases, billowy tops; the core stays),
   wind shear; the light march on the base shape only. CPU twin and tests (flat base, rounded top, anvil,
   coverage 0, edge-only erosion, tiling, a fixture type by data, both faces, naga). Cost read from the shader:
   Medium ≤ 7.4k ALU a texel (was 5.3k), High ≤ 10.5k (was 7.6k).
   Owner, 2026-11-12 (BUGS.md #97): **"Volumetric lighting also has some weird dithering where the light shafts look
   pixelated. Not sure if this is a bug, a blending problem, or something else."** Then: **"Oh that dithering is
   definitely cloud related."** Built (branch `claude/shaft-dither`): the clouds' composite no longer hands a near
   pixel (leaves among sky gaps) the cloud behind it (CLOUDS.md §9.6), and the shafts' jitter is a 3 × 3 tile of
   strata averaged by the composite's 3 × 3 box (was a static interleaved gradient). Next (local): the look at a
   low sun behind clouds and a tree.
   Owner, 2026-11-12 (BUGS.md #90): **"The clouds also look really badly dithered when they overlap? I think
   mostly when you get to their level? Looks very pixelated."** Asked for in the same work: blue-noise jitter, a
   depth-aware bilateral upsample, neighbourhood clamping instead of history rejection near the layer, more
   resolution or steps near or inside the layer by data per tier, a CPU test of a sharp silhouette and stability
   under the jitter. **Built (same branch, CLOUDS.md §9.6).** Next (local): the in-game look (sun high, low sun,
   from above, night, through the layer) against the old blobs and the bench before/after; tune by the data.
   Owner, 2026-11-12: **"I wonder if the clouds would look better and be cheaper if they were opaque to each other,
   like you can't see through them to the ones behind?"** Finding: the march already stops once a ray is 98 % blocked
   (`trans < 0.02`), but `volume.extinction` is 0.006 /m, so a ray needs ~650 m of dense cloud to get there: clouds stay
   see-through, march their full length and self-shadow weakly (part of the flat white look). Real cumulus is ~0.05 /m.
   Next, in the in-game cloud check: an A/B of extinction 0.006 → ~0.03–0.05 (data), measuring the look and
   `gpu.clouds`; keep soft eroded edges so higher opacity doesn't read as solid blobs.
   Owner, clarifying: **"to be clear, light should still go through the clouds, but what is looking bad is the
   clouds behind the one in front and you usually can't see other separate clouds blocked behind ones in front of
   you?"** So: raise the view ray's opacity (`volume.extinction`, what hides a cloud behind another) and leave the
   light's path as it is (`light.absorption`, the sun through the cloud: lit edges, silver lining, glow); they are
   separate numbers already.
   Owner, 2026-11-12: **"Another thing about the clouds is that they don't look pretty and tinted during sunrise and
   sunset like our old 2D clouds."** The volume should take the deck's golden-hour tint: the sun's colour low in the
   sky, the warm underside glow and the horizon colour the deck mixed in; compare at sunrise/sunset against
   `?clouds=` the 2D deck (the reference since the lighting rework).
   Owner, 2026-11-12: **"Also, our new 2D clouds seem to have lost most of their lighting from the original too. Can
   you compare with our old non-volumetric clouds they were 3d to see what was different about their lighting? They
   almost looked 3D"** — compare the deck as it was before the volumetric work (before 93fd6c26, e.g. at
   122b0f0e) with today's deck term by term (fake relief/normals from the noise, self-shadowing toward the sun,
   rim/silver light, the dark undersides, the sunset ramp) and restore what was lost.
   Owner, 2026-11-12: **"In terms of color too, the clouds seem gloomy and dark like not-quite storm clouds."** Fair-
   weather cumulus should read bright white on top and light grey below; only cumulonimbus/precipitation darkens.
   Likely: the higher view extinction (#790) with the light's multiple scattering too weak (the papers' octave/
   multi-scatter approximation), ambient dimming with cloud above too strong, and `cloudTypes` darkening on the
   ordinary types.
   **Built (2026-11-12, cloud, branch `claude/deck-lighting-lp3c01`, CLOUDS.md §10):** the shading code was never changed;
   what changed is which deck shows. Before the volume, desktops drew the deck at `high` (5 octaves and a
   self-shadow sample toward the sun); since, the sky's deck is only Low (phones, `?clouds=low`, and while the
   volume's noise bakes), which never took the sample, so its light was a function of density alone: no sun side,
   grey-white cores. The sample is now data on every tier (`light.lowShadowOctaves` 2 on Low,
   `light.shadowOctaves` 5 for Medium/High's deck in reflections); Low 7 → 9 noise evaluations a pixel (old
   desktop 14). CPU reference renders (`examples/deck_lighting.rs`) and tests on both faces: Low's sun side vs far
   side +0.010 → +0.209 at 5°, cores 0.521 → 0.264 lit at 70° (old desktop +0.155, 0.286). Next (local): the look
   in game at `?clouds=low` and `gpu.composite` on Low before/after.
   **Built (2026-11-12, cloud, branch `claude/cloud-golden-hour-zjl391`, CLOUDS.md §9.8):** the tint was lost in
   the ambient: the deck warms its whole colour at a low sun, the volume only the sun's light, and a volume
   sample's light is mostly the cool sky's. Now the shade (sky light and ground bounce) takes the deck's warm
   tint too (`volume.sunsetShade`), the ramp and the far haze are one function on both; CPU twin tests at 3°
   (the deck's warmth within 8 %, bases warm; red on the old shading), noon unchanged, both faces. And the
   opacity split: the light march had taken `volume.extinction` too; now `volume.extinction` (0.03, was 0.006)
   is the view's, `volume.lightExtinction` (0.006) the light's, plumes keep theirs; a dev knob
   `?cloudExtinction=` / `__dwell.cloudExtinction(x)` for the A/B. Next (local): the look at sunrise/sunset
   against `?clouds=` the deck, the A/B of 0.006 / 0.03 / 0.05 and `gpu.clouds`; tune by the data.
   Owner, 2026-11-12: **"the clouds look ridiculous at least the volumetric ones. Look like crazy triangles"**,
   **"The more recent volumetric shape change for the clouds make them look like 3 sided pyramids"** (BUGS.md
   #100), and **"In terms of color too, the clouds seem gloomy and dark like not-quite storm clouds."** (asked:
   fair-weather stratocumulus/cumulus bright white on their sunlit tops and light grey underneath at noon, only
   cumulonimbus/precipitation darker, checked against the papers).
   **Fixed in code (2026-11-12, branch `claude/cloud-pyramids`, CLOUDS.md §9.9):** the papers' model
   (`docs/reference/cloud-papers.md`): the cloud type and the cover as regional fields (16 km, 8 km;
   `typeFromCoverage` 0), the single clouds cut by the 3D Perlin–Worley (Nubis's remap, base tile 2 km), the
   profile fading from just over the base, erosion 0.25, view extinction 0.015 (a cloud behind 400 m of cloud
   9 % visible); the light samples reuse the column's cover (cheaper). The gloom was the same cause: every
   core lit as a cumulonimbus. CPU-twin tests red on main → green: cloud-top median slope 4.1/2.6 → 0.7–1.0,
   the patch's median noon base 0.44 → 0.77 of the sunlit white, cumulus top ≥ 1.0 / base 0.57, cumulonimbus
   base 0.23. Not done: distance-adaptive steps with HZD's cheap/full switch. Next (local): the in-game look at
   noon, low sun, from above and through the layer, `?cloudExtinction=` 0.01–0.03, and `gpu.clouds`.
14. **Tall grass grows and spreads** (owner 2026-11-11: "I want tall grass to steadily grow and spread. It
   should probably have a max depending on climate/biome."). **Built** (ADR 0075 T3c, docs/ECOLOGY.md §10):
   `shared/worldgen/grass.json` (the rule as data: the block, the ground, the decoration whose climate
   envelope sets the cap, the max density, spread and regrowth a day, reach, open air above, the catch-up
   bound) → `grass.gen.rs`; `dwell_worldgen::grass` (the face-local step); the server's grass step every 5 s
   over loaded chunks, each growing the whole world days it missed (at most 20); a player's block wins;
   eaten grass regrows; the far forage's most is scaled by the same climate share. A per-world switch in an
   optional `grass_growth` table: on for new worlds, off for worlds saved before it (their meadows stay as
   they were). Not yet: a setting in the world menu.
   Since 2026-11-12 (`claude/ramp-tufts-regrow`, the follow-up item 21 left): grass.json's `onRamps`
   makes a ½ m ramp of its ground ground too (validated as decorations' `onRamps`: the ramp exists and
   the tuft's `placeOn` allows it), so a tuft on a ramp that is eaten or cut regrows and spreading
   reaches ramp cells, under the same climate cap; never a 1 m ramp's pieces (tests on both faces).
   Owner, 2026-11-12: **"Seems like tall grass isn't growing in height like I expected. I thought it would get
   taller over time?"** (the built growth only spreads; every tuft is 0.85 m from the start). Answers: max height
   **depends on climate**; **4+ stages, slower** (a week or more of game time to full height); **cutting resets it**
   (cut or eaten grass goes back to a sprout and regrows); **more drops when tall**; the hide mechanic: "I like the
   hide mechanic idea, save this for later" (tall grass hiding a crouched player from animals/NPCs — Future).
   Plan: a growth-stage property on the plant (a new property, default the full stage, so not breaking, ADR 0063),
   stages and days per stage as data in grass.json, each stage's height from the block's look, the climate share
   capping the top stage, drops per stage; one cell (≤ ~1 m) first, a two-cell top for the tallest climates later.
   **Built** (2026-11-12, `claude/grass-height`, docs/ECOLOGY.md §10 "Growing in height"): tall grass's `stage`
   property (0 sprout … 3 full, the default, so placed and saved grass is full as before); each stage's height
   block data (`cross.heights`, the mesher draws it, the selection box scales); grass.json `stages` (days per
   stage 3, 3, 4: about 10 days to full; the climate share each stage needs, 0, 0.2, 0.4, 0.6); the grass step
   grows standing tufts a stage at a time up to their column's top stage in the same catch-up; spread and
   regrown tufts start as a sprout; eaten tufts go back to a sprout (worlds that grow grass); **broken by hand
   still removes** the tuft (chosen: a player clears grass to build), and it regrows from the roots as a
   sprout; drops by stage through a new drop-rule `when` (more fiber and seed when tall, gems only at full;
   full drops as before); generation places tufts at their climate's top stage (the default one-biome world
   unchanged; **the starter's drier biomes generate shorter tufts**, a change to the terrain the starter seed
   generates: re-pinned, waiting on the owner's 0.55 line with item 32's). Not yet: a two-cell top for ~1 m
   grass in the lushest climates; the hide mechanic (Future).

15. **Realistic world** (owner 2026-11-11: "The realistic world should come from our systems (which may not be
   properly defined or built yet to support this) the seed should determine tectonic plates, rain shadow,
   ground/rock materials, etc."). Proposal: docs/proposals/REALISTIC_WORLD.md (phases R1–R6, open questions).
   Owner's answers: scale one region to start (a few plates meeting, a coastal range and its rain shadow,
   one or two climate zones), designed so the 10× map ("we will test out a 10x size map soon") stays
   possible; climate from a seed-chosen latitude band and altitude, rain from the prevailing wind and the
   rain shadows over the seed's mountains; geology from plate history (arcs → basalt/andesite, uplifted
   ranges → granite/metamorphic, basins → sandstone/shale, old seafloor → limestone), soils from rock +
   climate; the starter stays a test bed, Realistic is a new World choice (later perhaps the default, on a
   new compatibility line). Owner's answers to §7 (2026-11-11): plates decide land and sea (oceanic = sea,
   continental = land, a sea margin at the disc's edge); face B its own plates from its own seed; the seed
   picks the latitude band; Realistic seeds' terrain may change between releases while it is a test world;
   plate count unanswered (the proposal's 3–5, as data). **Status:** R1 built: the plate layout
   (`shared/worldgen/plates.json`, `plates.gen.rs`, `dwell-worldgen` `plates.rs`: plates by kind, motion and
   age, boundaries classified by relative motion with the subducting side, hotspots, a land/sea mask), tested
   on both faces. R2 built (2026-11-12): uplift from the boundaries (`plate_tectonics.rs`, `plates.json`
   `uplift`: ranges on the overriding side and trenches, collision ranges, rifts, low transform ridges,
   scaled by each boundary's rate; arc cones on the overriding side; hotspot chains older along the plate's
   motion) and the plates' coast, in the Realistic test world (`worlds/realistic.json`, `test:realistic`, not
   in the New world form); default and starter goldens unchanged; the map builds faster than the default's.
   The Chisel plates overlay moved again, to R3. R3 built (2026-11-12): the climate from latitude, wind and
   terrain (`shared/worldgen/climate.json`, `climate.rs`): each face's seed draws a latitude band (subtropical,
   temperate, subarctic, by weight) spanning the map north–south, temperature in °C from latitude and height
   (6.5 °C/km), the band's prevailing wind (trade winds, westerlies, polar easterlies) marched over the seed's
   mountains on a coarse grid (≥ 500 m, ≤ 400 a side, under 1 MiB), its rain feeding the rivers' discharge
   where `tectonics.climate` is 1 (only `realistic.json`); windward sides of the seeds' main ranges ≥ 3× wetter
   than their lees, both faces; default and starter goldens unchanged. The Chisel plates, latitude and wind
   overlays moved to R4. R4 built (2026-11-12): biomes chosen by climate alone (Whittaker-style): the presets'
   `where` in °C (`celsius`) and annual rain in mm (`rain`), read from the climate (the rain re-marched over
   the eroded map, `climate.json` `bands[].rainMm` and `biomes`); `realistic.json` lists the default and all
   14 presets by bare name, no per-world tuning; the starter keeps its 0..1 tuning (its edit byte-identical)
   and the default its goldens; `tests/realistic_biomes.rs` on both faces: no dry biome where the rain is a
   wet coast's or on the main range's wet windward side, no forest above the tree line (−1.5 °C) and tundra
   there, a colder band more boreal/tundra on the same ground. The Chisel overlays moved to R5. Next: R5
   (geology and materials); q2's bands (R3 offers three, as data), q6 and q8 still open. q9 (owner,
   2026-11-12: "Yes add those biomes") built: `preset:cold_desert` (cold, nearly rainless: dry dirt and sand,
   sparse dry grass, rocks, no trees, its own snow line) and `preset:tropical_rainforest` (hot, very wet: a
   new tall broadleaf species, `shared/trees/tall_broadleaf.json`, with oaks and palms close together in
   `rainforest_groves`, thick undergrowth, dark water) by their °C and rain envelopes, in the Realistic world
   only; the default's share of its land fell from 21–47 % to under 3 % (REALISTIC_WORLD.md §7 q9); the
   default and starter worlds unchanged (a decoration no biome of a world lists now stands nowhere in it:
   before, a rock province's columns tried every decoration). q5 (2026-11-11): no new rock blocks
   for now; R5 uses today's blocks (stone, basalt, sandstone, red sandstone, sand, dirt) named in data, so
   new ones can replace them later. R5 built (2026-11-12): rocks from plate history (`rocks.json`
   `plateProvinces`: a setting on the face's plate layout → a stack by name, the first rule that holds
   winning, the noise's provinces elsewhere): arc cones on andesite (shown as basalt), hotspot islands on a
   shield basalt, collision cores and the subducting ranges' crests on granite over schist (stone), the
   accreted strip by a trench on the limestone shelf (stone), rift floors and the plates' quiet interiors on
   sandstone beds (red sandstone); new rocks and stacks only, weight 0 in the noise, so the default and the
   starter are unchanged. Soils by rock × climate (`vegetation.json` `rockSoils` entries with `celsius` and
   `moisture`, first that holds): andesite weathers to rocky soil in the cold, sand in the dry, deep loam in
   warm wet air. Tests: `tests/realistic_rocks.rs` (both faces, seeds 1, 7, 42), `geology` fixture tests
   (each setting, a rule or a rock's block swapped by data alone), `vegetation` (soil by climate),
   `gen.test.mjs`. **Not done:** the plates/rock overlay in Chisel and on the in-game big map (§7 q8: a WASM
   export of the layout and a view in each); moved to R6. Next: R6 (the World option).

   Owner, 2026-11-12, on q2 (which latitude bands): **"I think latitude bands are based on orbits right? Orbits
   should be seed determined."** So the climate band is not a list to pick from: the seed sets the world's orbit
   (its distance from the sun → overall warmth, its axial tilt → seasons and how climate changes across the disc,
   its day and year length within playable bounds) and the moon's orbit, and the latitude band and seasons follow
   from them; one celestial model (MOON_WORLD.md §6) shared by the sky and the Realistic climate (R3).
   **Design written 2026-11-12** (docs only, REALISTIC_WORLD.md §8 "Orbits by seed", cross-linked from
   MOON_WORLD.md §6): a flat disc's latitude is its sky's pole height (today's 60° noon = 30° N on face A,
   30° S on B; parallel sunlight gives no gradient, so R3's 14° span across the map stays a game-scale
   stretch); the seed rolls the pole height and yaw (latitude and north), the distance (± a few °C), the
   axial tilt and year (seasons: opposite on the faces, optional/later; generation reads the annual mean),
   the clock within a range, and the moon's month, inclination, node and phase (its distance kept for its
   size); R3's bands become names and `rainMm`; data `shared/look/orbits.json` (ranges, limits, today's
   orbit), rolled once at creation and stored as world settings (`CELESTIAL`, required for a world whose
   terrain reads it); default and starter keep today's sky exactly, only Realistic rolls; phases O1–O5.
   Open for the owner (§8.10): seasons now or later; how varied orbits may be (and whether the day length or
   default-generator worlds vary); the moon's orbit by seed; how much the faces' climates differ; the
   gradient across the disc.
   Owner, 2026-11-12: q6 "dormant volcanoes only for now."; q8 "also on the big map" (overlays in Chisel and the
   in-game big map); q9 asked "how does the default biome fill them?" (answer pending). Big map q5: "No, no
   hitches" — region maps built ahead of the player.
   Orbits by seed (REALISTIC_WORLD.md §8), owner 2026-11-12: orbits first, seasons after; "a great setting to have
   default with some presets. Like proposed ranges, fixed day, day varies, more extreme with hotter/colder."; the
   moon's orbit by seed too (size kept); Realistic worlds only ("We'll likely roll orbits out to regular worlds too,
   but for now it's a good reference point to compare to."); q4/q5 open (the proposal's defaults). **O1–O2 built
   (2026-11-12):** `shared/look/orbits.json` (presets Default, Day varies, Extreme; limits) → `orbits.gen.rs` and
   `skyPresets.gen.ts`; `dwell-look` `orbits.rs` (`Orbit::roll`, `Orbit::today`, `day_night_for`); the optional
   `CELESTIAL` world setting (0x86); the server rolls it at creation for a Sky preset (`--sky`, the browser's New
   world → More options → Sky, shown for a world whose climate reads the sky, and `&sky=` on a `?worldgen=test:realistic`
   link); the sky (server and clients) draws the world's orbit; default and starter today's sky exactly (golden).
   Realistic is in the World list since R6, so the Sky field shows for it. **O3 not built** (the climate
   reading the orbit needs it carried to the map workers): next.
   Owner, 2026-11-12: **"Do we have realistic worlds yet?"** (R1–R5 and O1–O2 were built; the world was reachable
   only by link as the test world `test:realistic`.) **R6 built (2026-11-12, the browser check outstanding):**
   New world → World → Realistic (`realistic.json` `newWorld`), its Sky preset under More options, its worldgen,
   seed and Sky saved with the world; its own goldens (`shared/worldgen/golden/realistic-hashes.txt`,
   `tests/realistic_golden.rs`; the default's unchanged); the big map's debug layers as data
   (`shared/ui/mapLayers.json`: plates with their outlines and kinds, uplift, rain, rocks; `dwell-worldgen`
   `map_layers.rs`; the map's Layer button and `L`); the memory natively: Realistic's biome sets 14.1 MB both faces
   against the starter's 15.9 MB. **Browser check done (2026-11-12, the M1's GPU, R6's exit met):** created from
   New world (seeds 42 and 1234, Sky Day varies and Extreme), played, every map layer shown, reloaded on the same
   terrain and Sky; the layers' square-past-the-disc overdraw fixed (REALISTIC_WORLD.md R6). The phone page's total at
   load (bench `--touch`, `memory-phone`): Realistic 570 MiB against the starter's 655 MiB.
   **Not done:** the overlays in Chisel (next with its other overlays); O3.
   Owner, 2026-11-12, on the R6 check's screenshots: **"Is that screenshot from realistic world. Is all of our terrain
   still stepped? I like stepped terrain but it should be system driven not across the whole realistic world"**. Today
   the `cliffs` terrain volume (volume.json, the default terrain's terraces: stepped country and escarpments on steep
   hillsides by a 600 m noise field and every steep mountainside) shapes every world, Realistic included. Next (after
   video 2): in Realistic worlds steps come from the geology system instead — rocks.json's jointing (`bedded`
   sedimentary stacks step every `bed` m into walls and treads, `vertical` basalt stands as columns, `massive` granite
   rises unstepped) with the R5 rock provinces and `shaping`/`erosion` (mesas and scarps in arid sedimentary basins,
   smooth slopes on granite cores and soils), and the global cliffs volume off for Realistic (data: realistic.json's
   terrain volume and its rocks shaping strength). A Realistic-only terrain change (the default world unchanged).
   **Done (2026-11-13, 0.57.0, a new line):** `realistic.json` `"cliffs": 0` and `geology` shaping and erosion strength
   1, so Realistic ground steps by its rock's jointing (bedded walls and treads, vertical columns, massive granite
   smooth) and erodes into mesas and scarps in the arid sedimentary biomes; plate-rule province edges fade the shaping
   in over `rocks.json` `plateProvinces.edgeFade` (600 m) so no wall runs along them. `tests/realistic_steps.rs` (both
   faces): massive ground < 1 % stepped (as unshaped), bedded 3–4 %, no column reshaped by the volume. Default world
   unchanged (goldens); Realistic golden rewritten (REALISTIC_WORLD.md R5). Open for the owner: the collision cores'
   schist is `bedded` (steps every 10 m); `massive` would make the cores smooth throughout (data).
   Owner, 2026-11-13, on a screenshot of Realistic's bedded-rock slopes (a field of single-cell ½ m stair bumps, no
   ramps on them; mesa tops perfectly flat; the mesas and scarps read well): **"We need a red brown sand for the red
   desert areas I think. Not sure if I've clarified this but we should support ramps for all terrain blocks. Does that
   change layered steps?"** / **"Notched ramps seems fine."** And: **"The only thing about gravity blocks like sand
   settling is they shouldn't settle ramps away from other blocks they should transform into flat blocks."** (the
   settled state as data on the falling rule, e.g. `"settlesAs": "full"`).
   **Done (2026-11-13, 0.59.0, a new line; branch `claude/terrain-059-batch-v81i64`):** (1) `dwell:red_sand` (sand's
   physics, tools and drops, the palette's `red_sand` ramp, its ramps) floors the red rock mesa (its preset's bands) and
   is the talus of its own stack `dwell:red_desert_beds` (sandstone over a new red `dwell:siltstone`; rocks.json), data
   only. (2) Ramps for every terrain block: basalt, obsidian, moon dust, moon mare dust and red sand joined
   `rampFamilies`; `shared/worldgen/gen.mjs` `validateSurfaceRamps` fails naming any block a worldgen surface rule can
   place (biome bands, rocks, soils, grids, lava's rock) without ramps. The moon generator (`grids.rs`) doesn't shape
   ramps yet: its dust has them for when it does. (3) Layered steps as ledges (rocks.json `shaping`: `notched` 1,
   `ledge` 2 m every `ledgeEvery` 1 of a soft bed (one a bed), `treadRelief` 0.08): where the rock shapes the ground the stored
   ramps also top diagonal contours (`ramps::top_of_notched`), a soft bedded rock's slope is ramped runs broken by 2 m
   ledges, and treads keep 8 % of a bed's relief. `tests/realistic_steps.rs` (both faces): bare ½ m steps on bedded
   ground 61 % (0.58) → 0 %, ledges ≥ 2 m ~1.3×. (4) Sand's `"settlesAs": "full"` (blocks gen.mjs, `SETTLES_AS`,
   `dwell_blocks::settled_state`): a falling block's ramp family falls too and a fallen sand or red sand ramp settles as
   the full block (felling.rs, both faces). Rule chosen: always the full block on settling.
   **Follow-ups (brief 2026-11-13, browser check of 0.59 on a022b0d0; branch `claude/red-sand-fix-rwxphd`):** "Every
   red_sand cell (and likely red_sand_ramp) draws as the magenta/black missing checker … Fix, and make the guard real
   … Better still: derive TILE_ORDER from the texture data so it can't drift" and "Red sand isn't where the owner
   asked … Make the red desert floors (the arid sedimentary/mesa areas of Realistic and the default world's red mesas)
   surface as red_sand, by data". **Done:** the atlas layout is the texture set's list (BUGS.md #133); red rock
   floors are red sand where arid (#134). Orchestrator, 2026-11-13, before merge: **"Climate: the owner asked for red
   sand in the red DESERT areas, so don't put it under forest. Add a small climate rule on a rock's cap as data …
   Red-rock floors in arid places become red sand; elsewhere they stay red sandstone. Keep mesa tops red sand only
   where it's arid."** and **"Line: this changes worldgen after 0.59.0 shipped, so raise 0.60 yourself in this PR
   (the owner delegated minor bumps)."** Built: sandstone `cap` red sandstone with `capBy: [{"arid":
   "dwell:red_sand"}]`, rocks.json `arid` (rain ≤ 450 mm, wetness ≤ 0.3, ≥ 6 °C: the mesa preset's `where` plus a
   temperature floor so dry tundra and boreal ground stays rock); the 0.60 line (the Realistic and Starter terrain
   changed; the default world, `dwell:default` only, has no red rock at the surface and is unchanged). Open for the
   owner: the 6 °C floor counts the steppe's and cold desert's red rock floors as desert (the reported spot's red
   mesas are steppe at ~8 °C); the mesa's own 12 °C would keep red sand to hot deserts (face B of seeds 1, 7, 42
   then has none).
   **Realistic batch A: plates and coasts (2026-11-13, the 0.61 line).** Owner, 2026-11-13, on a Realistic world
   (seed 2741893632, 0.59.3): "What's up with realistic world having an enormous \"river\"? And a separated mountain
   range from the main continent? It's a very weird geography that doesn't look realistic at all" / "I meant
   horizontal slices of noise" (large uniform swaths of one terrain type in plan view) / "And noise also seems very
   uniform at a macro level, where the same terrain almost looks tiled." / "Also are we hardcoding that there's an
   ocean edge dropoff always around? Or is that just super common?" / after the diagnosis: "Yes make those changes
   as a realistic batch". Diagnosis (native, seed 2741893632 face A): the "river" a divergent continent–ocean
   boundary cut as a 256 m rift whose sea-side shoulder closed it into one 45 km lake; the separated range a
   continental plate centred at the disc's edge, clipped to a 5–7 km sliver of range; plan view one cold desert
   region of 565 km² and province and biome bands parallel to boundaries; the "tiling" no true periodicity but
   45°/90° streak hatching at 100–300 m; the rim a circular cap at `reach − seaMargin` for every plate. **Part A
   built** (this change; docs/proposals/REALISTIC_WORLD.md §6 "Realistic batch A"): landforms by plate pair
   (`plates.json` `uplift.pairs`), segmented rifts (chains of lakes with sills, gulfs at the sea), no sliver plates
   (`kinds.*.interior`, plate centres from a disc larger than the reach, spacing 12 km), ranges varied along their
   line (`uplift.along`) and arcs fitted to narrow plates, and an open rim (`rim`: shelves per sector, coast
   noise, only the outer 2.5 km always sea). On seed 2741893632 the longest lake 52.7 → 9.2 km, no plate under
   its interior, land out to 46.8 km of the 50 km radius. **Part B** (variety: the plan-view swaths, the climate
   on one band and wind, the uniform hatching) is its own branch (`claude/realistic-variety*`) and ships on the
   same 0.61 line.
16. **Phone frame rate (P0)** (owner 2026-11-11, iPhone 17, Safari, setting Low: "I'm only getting ~23 fps on
   mobile, seeing lots of z-fighting on tree trunks with the leaf cards. What might be dropping frames since the
   last while. Used to get 60 fps"). Suspect: leaf cards are on in every tier but CPU, Low included; cards lying
   on trunk faces z-fight. In progress (local): measure each feature's cost on the `--touch` page at Low (default
   and starter worlds) against an older release; cards off on Low/phone, no cards against log faces, a Low tier
   for every new visual; tier pins and a z-fight test.
   Owner, 2026-11-12 (after leaf cards went off on phones and Low): **"I got 60 fps when standing still on
   Low."** So standing still is fixed; the drops left are while moving: next, a moving scenario on the `--touch`
   page with the CPU throttled (chunk streaming, meshing, uploads, LOD refinement, the minimap), per-frame
   main-thread work and upload sizes.
   **Status (fps-moving):** measured (BUGS.md #72 round 3: the frames missed while moving are all
   main-thread loading work, `mesh_dirty` and `jobs.apply`); built: a per-frame mesh upload byte budget by tier
   (`shared/render/streaming.json`), one loading item past the deadline a frame, a faster `interleave`, bench record
   budgets and the `phone-moving` scenario. Left: the flood light off the main thread, the sky volume's copy
   (`padded_box`), a chunk's upload split across frames, the minimap's tiles; a real iPhone check while moving.
   **Status (fps-moving-2):** built: the sky volume's fluid continuation only where a neighbour is missing
   (`padded_box` 0.44 → 0.12 s in a 4× flight), mesh job messages in one buffer, a chunk mesh over the phone's
   budget written over several frames; at 6× the three routes drop 77 frames where they dropped 115 (BUGS.md #72
   round 4). Left: the flood light off the main thread (the largest main-thread cost; one chunk's light is the
   worst frames at 6×), then a real iPhone check while moving.
   **Status (fps-moving-3):** built: a chunk's flood light lit a slice at a time across frames within the light
   budget (the chunk unmeshed until lit; edits and nearby held chunks finish it first). The 6× flight's worst
   `mesh_dirty` 21.2 → 8.4 ms; frames over 16.7 ms of CPU at 6× 77 → 24 (BUGS.md #72 round 5). Left: the
   prediction's ticks in flight (13–20 ms frames at 6×), a real iPhone check while moving.
   **Status (fps-moving-4):** built: the prediction's tick puts at most 1024 background collider cells in on a phone
   (`collisionCellsPerTick`, the cells in reach at once; the player moves the same); the 6× ticks' worst 15.8–19.8 →
   10.9–18.8 ms (BUGS.md #72 round 6); #642's contact hook costs nothing measurable here. Left: building a newly
   reached chunk's collision ahead of the player, a real iPhone check while moving.
17. **Axe swing directions and animation tools** (owner 2026-11-11: "The axe animations are still trash. They
   don't swing in the right direction at all. Make yourself some tools to properly observe animations, movement
   directions, positions over time, etc. Should swing right to left for horizontal and up to down for vertical";
   "May need to see multiple perspectives to see animation and understand it"). In progress (cloud): a
   `clip_probe` CLI sampling a clip through the game's posing code (tool head, hand, elbow over time; first
   person, top, side, front and third-person views in one SVG), direction tests (horizontal strike right → left
   on screen, vertical top → bottom, the head at the crosshair at contact), the clips fixed, a bench
   `clip-strip` contact sheet from first and third person.
   **Status (0.52.11x):** built: the `clip_probe` CLI and SVG (five views), direction tests (red 7/8 → green),
   the clips refitted (level right → left, vertical top → bottom, the head on the crosshair at contact), a
   bench `clip-strip` step. Review of the probe's sheets: directions right now, but the hands barely travel
   (the head's arc is mostly a wrist turn), and the wind-up is short and small (0.1 s, no draw back over the
   shoulder); the SVG's side panel is cut off. Left: an arm-driven swing (hands travel the arc, a real wind-up),
   a probe test for hand travel, `clip-strip` run in the browser (local), remote players' chop direction (a
   protocol change).
   Owner, 2026-11-11: "Ensure your tooling helps you tell rotation too."
   **Status (swing-arms):** built: `clip_probe --fit` (keys fitted to grip, head and blade-facing targets,
   first person and body), the arm-driven clips (hands ≥ 40% of the head's travel; a 0.3 s wind-up,
   `windupS` 0.3, `activeS` 0.12; the level strike level through the crosshair; the edge leading), the
   probe's orientation (blade facing vs motion, handle, arm bones' axes, hand roll, angular speeds; drawn
   as glyphs on the square SVG) with edge-facing and wrist-flip tests, `clip-strip` run in Chrome for both
   chops (its vertical strip now holds the vertical grip). Left: the owner's look review (the first-person
   wind-up leaves the screen, the third-person body turns only at the chest), remote players' chop
   direction (a protocol change).
   Owner, 2026-11-12: **"The camera could have some sway with the chop."** In progress (cloud): a small camera
   sway following the swing (a lean into the wind-up, a kick at contact, settling in the recovery), as tool
   data, honouring the camera-shake/motion settings.
   **Status (chop-sway):** built (CHARACTERS.md §5.2): each tool's `swing.camera` per swing kind (keyed yaw,
   pitch, roll and a bob over the swing's phase time, a kick at contact; replaces `kickDeg` and the hardcoded
   jolt), validated in the generator (at rest at both ends, in order, ±10° and ±0.1 m); the level chop leans
   back and right into the wind-up and turns left through the strike, the vertical pitches up and dips at
   contact; a miss kicks at 0.35, a hit into wood at 1, a bounce at 1.6; the view only, never the aim (tested
   on both faces); Settings → Motion → Camera sway (the explosions' Screen shake names blasts only, so a new
   toggle; off with reduced motion); `clip_probe` prints the offset per sample. Left: the owner's feel review
   in game (no GPU in the cloud).
   Brief, 2026-11-12: **other players see the actual chop** ("Remote players still see a `break`. The wire's
   swing byte (`swing_flags`) carries only break or place, so it can't say which chop or which direction"),
   gated so it isn't breaking for 0.54 peers.
   **Status (remote-chop):** built (ARCHITECTURE.md §8.3 "Swing kinds"): a chop now steps the swinger's swing
   count (before, it didn't: others saw nothing, or an edit's swing), as a break, and the server keeps its
   kind (`SwingKind` in `shared/protocol/constants.json`: plain, chop level, chop vertical); a client asks with
   a `SwingHello` datagram (no Welcome or Inventory flag is free; an older server drops an unknown datagram),
   repeated each second until a snapshot answers, ten tries at most, and its snapshots then end with each
   remote's kind; the remote body plays `biped_chop_level` / `biped_chop_vertical` as the count steps (both
   faces). An older client never asks and plays `break`; an older server never sends kinds. Tests: the
   server's (hello → the kind, no hello → plain break, red → green), the body's clip (red → green), the
   hello's schedule, wire vectors. Left: a look in game with two clients (no GPU in the cloud).
   Owner, 2026-11-12: **"Level chop with off screen is fine. We can save loosening the stiff body for later."**
   The level wind-up stays off the right edge; the body's hip twist moves to Future. Left: the blue sleeve blob at
   the bottom of the screen mid-swing. **Fixed (`claude/chop-check`):** the blob was the swing arm's own tunic sleeve (not the off
   arm, which first person doesn't draw): the level chop's root lifts the shoulder 0.2 m in front of the eye from
   contact through the follow-through (the probe: shoulder at NDC +0.53,−0.93 at 0.45 s). The arms view now draws
   the arm from the elbow down (the upper arm's parts `"view": "body"`; the elbow stays below the screen in every
   swing); `clip_probe` test `no_sleeve_shows_at_the_bottom_of_the_screen_through_a_swing`, red → green.
   Owner, 2026-11-12: **"I wanted you to be able to hold the key down for tools like the axe."** Holding the use
   button keeps swinging a tool (axe, pick, shovel, sword) at its swing rate until released, each swing hitting
   what is targeted then; data per tool (`repeatOnHold`). Also bug #93: vertical swings play twice.
   Owner, 2026-11-12, on #93: **"The animation double swings"** (the clip itself, not two hits).
   **Status (swings-hold):** built: #93 fixed (a hit's pull-out played the clip backwards over the shoulder and
   then let it run on, a second swing; now the contact pose fades to rest and the clip stops at the swing's end;
   `clip_probe` stroke-count test red → green); hold to keep swinging (ITEMS.md "Hold to keep swinging"): per-tool
   `repeatOnHold` (both axes and the sword in `tools`, the pickaxes and shovels with `repeatS` in
   `breaking.tools`), validated in the generator with a fixture by data alone, each repeat a whole new swing sent
   as a click's (others see each by its swing count; wear on a hit only), mouse, controller and touch (a still
   hold in Break mode with such a tool). Left: the feel in game (no GPU in the cloud).
18. **Digging marks that look carved** (owner 2026-11-11: "the 'chunks' of dirt aren't at all what we
   discussed. It should be like a normal map change (do we support normal/bump maps)? It's completely flat
   should look like mesh deformation (preferably without actually doing it for performance)"; "That part was
   about digging"). No normal or bump maps today; 0.52.9x's scoop is a flat decal lit by a modelled bowl.
   Planned (local, after item 16): each mark's height computed per pixel in the shader (its gradient bends the
   normal; the texture shifted inside the dent for depth; the rim's shadow and damp floor), growing with damage,
   data-driven, a cheap Low tier; collision unchanged. Stone shards on the same mechanism after.
   **Status (carved-digging):** built (BLOCK_DAMAGE.md §6.1): one relief quad per struck face, the hollow per
   pixel (merged bowls, rim with crumbs, bent normal, parallax march on Medium, rim shadow, damp floor), data
   `rim`/`parallax`/`darken`, tests on the CPU copy. Left: the owner's look review (bench `dirt-relief`),
   stone shards on the same function.
   **Owner, later 2026-11-11: "Remember with the digging, mining, chopping it should be a real 3D chunk, not a 2D
   decal"**; asked what should be real 3D (the block's mesh carved, or a real piece knocked off), the owner
   chose **both**. So: (a) each hit cuts real geometry out of the block's face (a bowl in soil, angular facets
   in stone, the notch in wood), seen from every angle and at the edges, collision still the full block;
   (b) each hit also knocks off a real 3D piece (clod, shard, wood chip) that falls and is picked up as
   today's per-hit drops are. 0.52.11x's per-pixel relief (built) is the stopgap until (a) lands.
   **Status (carved-mesh):** built (BLOCK_DAMAGE.md §6.2): (a) the struck faces meshed as a grid pushed in by
   the height field (soils' bowls, stone's angular facets), watertight (the outline never moves), the face's
   uvs and light, a per-cell triangle budget in data (`damage.look` `profile`/`facets`/`grid`/`triangles`),
   on every tier (the decals and relief quads no longer emitted; cracks not drawn on carved faces); the
   axe's notch was already real geometry. (b) the per-hit drops were already 3D loose items; the rock shard
   now has its own angular `shard` look. Measured: 50 damaged cells +213 triangles and ~70 µs a cell.
   Left: the owner's look review in game (no GPU in the cloud), then removing the shader's unused relief.
   **Status (carved-check):** checked in the browser on a GPU, by day: the soil bowl reads as a real hollow
   from above, at a grazing angle (its far wall lit) and from standing height; no seams or cracks seen.
   Stone's facets were too small and pale to see from standing height after three hits (0.34 m, 0.10 m deep,
   floor 0.12 darker), now 0.44 m, 0.16 m, 0.4 (data). The terrain shader's unreached relief (`dent_*`, the
   flat `code` varying) is removed; dwell-look's tier bit 11 (`relief_parallax`) and dwell-mesh's
   `relief::pack`/`unpack` are now unread, left for a cleanup. By design the carve fades to 0 at the cell's
   edge (watertight), so it never shows in a block's silhouette. Left: the owner's review.
   Owner, 2026-11-12: **"Perhaps we also show a 'cracking' texture around the block too as you do more
   damage."** **Status (cracks):** built (BLOCK_DAMAGE.md §6.3): painterly crack tile sets as data
   (`shared/textures/cracks.json`: stone, soil's drier fissures, wood's splits along the grain; four stages
   each, hairline to branching, painted by the stroke generator into the atlas); per block
   `damage.look.cracks` (`set`, `stages` by damage fraction, `strength`); every exposed face of a damaged cell
   past its first stage carries the stage's tile centred on the hit nearest the face (running over the edge
   from the struck face), multiplied over the block texture by one atlas lookup in `terrain_fs`, on carved faces
   following the carve; every tier. Measured: 50 damaged cells +212 triangles a cell (as before), 6.0 ms
   against 2.6 ms undamaged; the terrain vertex 72 → 76 bytes. Left: the owner's look in game (no GPU in the
   cloud); the wood set has no block yet (logs have no `damage.look`).
   Owner, 2026-11-12: **"The stone hits should be elongated low poly shards stone doesn't dent, it shatters."**
   So stone gets no dent: each pickaxe hit spalls the face (flat, sharp-edged chip-outs, shallow and elongated,
   lit as broken rock, not a dark pit) and knocks off elongated low-poly shards (a few per hit, tumbling) as the
   per-hit pieces; soil keeps its bowl. **Status (stone-shatter):** built (BLOCK_DAMAGE.md §6.4): a `spall`
   profile (flat, elongated, stepped chip-outs lit lighter as fresh rock, 2 a hit, a few cm deep) on every
   stone look, and the rock shard an elongated 6-triangle shard (~3:1); one shard a hit is still the drop,
   the hit's chips the visual-only fragments. Browser check (BLOCK_DAMAGE.md §6.4): not yet read as
   shattered (the crack overlay dominates; the scar is low-contrast and never reaches the silhouette); next
   levers listed there. Round 3: faint late cracks, a fresh-rock tint and 3D flying fragments (4 a hit) built; close up
   it reads as chipped stone. Left: flakes breaking the cell's edge (the silhouette), the owner's look.
   Owner, 2026-11-12: **"One of my requirements that I think might not have been coming through for tools is that
   the chunks that fly off and show chunked off the blocks should be equivalent to 50% of the block's volume /
   number of swings required to break it."** The rule, for every tool on every block (digging, mining, chopping —
   items 18, 24–26): each hit carves away, and throws off as pieces, a volume of 0.5 × the block's volume ÷ the hits
   that block takes to break with that tool; so when it breaks, half the block has been carved and flown off (the
   block breaks at 50 % broken; metal tools take fewer, bigger bites). The carved mark and the flying pieces both
   match that volume (the pieces' total ≈ the bite).
   **Status (bite-volume):** built (BLOCK_DAMAGE.md §10): `dwell_protocol::damage::bite_m3` (0.5 × the block's
   volume, a ramp half a cell's, ÷ its hits with the tool), used by the server's marks, the carve (sized to
   k bites after k hits, half at the break, within 8 % on the mesh) and the flying pieces (`chips.shards` ×
   `chips.piece` look, together the bite; dirt clods and the axe's wood chips added); the tier `chunk` data
   removed (metal's bigger bites come from its fewer hits). Dirt one shovel hit 0.0014 → 0.05 m³. Left: the
   axe's notch carve still its own size (the chips follow the rule), the owner's look in game.
   Owner, 2026-11-12, on the axe notch left out of the rule (#794: its ¼ m band can't hold half a cell): **"Yes the
   notch should grow to follow the 50% rule."**
   **Status (notch-bite-rule):** built (BLOCK_DAMAGE.md §6.4 round 3, §10): each bitten side's notch is one V
   grown so the log cell's carve after k of its swings to fell is k × ½ × the log's volume ÷ those swings (read
   from the cut alone, `Cut::progress`: stone 11, metal 5), within 0.1 % on the mesh; its mouth starts in the
   ¼ m band and opens past it (data `cut.bite.bandM`, `depthPerMouth`, `widthPerDepth`), the bites widened into
   it; by the fell the gash's V, both faces turned out of the notch. Fell and stump unchanged. Left: in the
   octagon the fell's half is more than a V holds (0.36 of 0.41, never drawn: the cell splits); the chips are
   half the cell's, ~20 % over an octagon's carve; the owner's look in game (checked locally after).
   Owner, 2026-11-12 (BUGS.md #112): **"Slopes aren't affected visually by tools when hit, though they do break."**
   **Status (ramp-hit-marks):** built (BLOCK_DAMAGE.md §6.5): a ramp's struck planes carved by the bite rule (a
   ramp's volume ½), inside the solid and watertight, cracks over its planes once staged; the pieces already were
   the bite. Left: a 45° upper half holds ¼ cell, under the rule's ½, so its carve is capped by its solid; the
   owner's look in game.
19. **Lava glow, remaining** (owner 2026-11-11: "Lava should have bloom glow"; "Lava looks dark red instead of
   bright red/orange"; "It should darken as it solidifies though"). Built 0.52.107: colour by heat stops, heat in
   cells. **Bloom built (pending release, `claude/bloom`):** keyed on the scene's self-lit flag (lava, torches,
   flames), a halving blur chain added before the tone curve; Settings → Graphics → Bloom, High on High and
   Ultra, Low (half resolution) on Medium, off on Low and Mobile, never on a phone or CPU adapter; data in
   `shared/look/bloom.json`. Bench (M1, `bloom` scenario, the summit lake at night): Medium with bloom 5.66 ms
   GPU mean against 5.79 off (p50 5.45 against 5.44), within the noise, so under the 0.5 ms target; Low draws
   none and builds no pipeline (a test pins it). The owner's look is pending: at intensity 0.9 the glow also
   lifts the dark crust's cells toward orange (one number, `intensity`). Left: the crust covering less of hot
   lava (one number, `crust` of the hot stop, waiting on the owner's look); the cooling river not yet seen in
   game.
20. **Starter world on iPhone, again** (owner 2026-11-11: "iOS can't handle the starter world. It crashes and
   restarts quite often. What's so much heavier about it?"). 0.52.99–0.52.104: ecology 12.5 → 0.9 MB, starter
   biome grids 53.6 → 15.9 MB a process. Re-measured at 0.53.2 on the `--touch` page (BUGS.md #70): the
   starter 661 MB at load, the default 662 (the starter was 850 at 0.52.68); server 83 MB, world workers
   83 + 83, core 99, GPU meshes 258. Within a few MB of the default, so one world worker on phones is not
   needed for memory. Left: a check on the iPhone itself (the owner's crash report predates 0.52.99).
21. **Meadow tall grass** (approved 2026-11-11; built 2026-11-12, waiting on the owner's look): the meadow's grass grows in patches, never on ½ m
   grass ramps (~40 % of the meadow), drawn pale. Denser patches or tufts on ramps change the default world's
   terrain (a new compatibility line); a less pale tuft is a look change only. Asked the owner which.

   Owner, 2026-11-11, asked whether it rides along with 0.53: "What's this meadow grass change?" — explained;
   Owner, 2026-11-11: **"Yes make the meadow grass fix."** Tufts on grass ramps, denser/wider patches, less
   pale tufts. 0.53.0 has already shipped, so this worldgen change takes the next line (0.54).

   Built 2026-11-12 (`claude/meadow-tall-grass-fix-ea4p5m`, the 0.54 line; the owner raises
   `package.json` at merge): a decoration rule's `onRamps` puts tufts over ½ m grass ramps (both
   faces; the mesher already stood a plant's feet on a ramp), the meadow's patches wider and denser
   (`decorations.json`: density 0.65, wavelength 56 m, coverage 0.65), the starter steppe's
   `undergrowth` 0.75 → 0.85 so it stays sparse; the starter's meadow spot went from 11 % to 33 % of
   its grass tops with tall grass, its steppe 8 %. The tuft's tile is painted in the grass top's
   colours, darker at the foot (no light-stop tips). Goldens and the starter pin regenerated.
   Left: the owner's look in game. Grass growth regrowing and spreading tufts on ½ m ramps too: done
   2026-11-12 (item 14, grass.json `onRamps`).
22. **Settings and version buttons look like web links** (owner 2026-11-11: "I thought we had settings and
   version buttons changed queued, is that true?" — it wasn't recorded anywhere; then "The buttons should be
   aligned to match the design of the others instead of looking like web links"). Restyle them as the menu's
   own buttons, aligned with the others. **Built 2026-11-13** (branch `claude/menu-buttons-goikyd`): Settings
   and Versions are the home's paper block buttons (`mm-action mm-action-secondary`, since 0.53.x) side by
   side under Worlds and Play with others; the last link-styled controls are buttons now: Versions' All
   versions is a page-head button beside Back (`mm-head-button`), About's Third-party notices a small menu
   button; Back, All versions and the world card's ⋯ gained hover and pressed states; the unused underlined
   `.menu-link` style is gone. `mainMenuButtons.test.ts` pins that every main-menu button and link carries a
   class of `MENU_BUTTON_CLASSES` (links only through `linkButton`), that no menu control is underlined, and
   that each class has hover and pressed rules. Left: the owner's look.

23. **Expandable sections in the main menu** (owner 2026-11-12: "Our main menu has some expandable sections that
   don't have good UI best practices to indicate they can expand and collapse. Improve these."). Built
   (0.54.x): New world's More options, Versions' Earlier versions and About share one disclosure style: a
   chevron right when closed and down when open, a hover and pressed background, a focus ring; About is a
   real disclosure button (aria-expanded, aria-controls). The world card's ⋯ menu keeps its button.

24. **Felling: the gash, and trunks lost on slopes** (owner 2026-11-12: "The tree chop currently makes 2 half
   slopes both at the same angle, which doesn't line up with the chop you make in the side of the tree. It just
   slices the tree after it's broken like you cut it through with a sword in one swipe. The tree is supposed to
   have the big gash I was imagining 2 half blocks one upside down one right side up, both with the 'short
   side' facing the cut."). Restated to the owner: the stump an upright ramp low at the notch side, the trunk's
   new bottom an upside-down ramp thin at the notch side, opening a V gash toward the side chopped (the data's
   `fellWedgeTrunk: open`, today `matching`), lined up with the notch and the fall. Owner, 2026-11-12: "Yes that
   was the shape I meant too." Also BUGS.md #73: a trunk settled diagonally up a slope loses most of its cells into the
   ground. **Built 2026-11-12** (branch `claude/fell-gash`), waiting on the owner's look: the log's
   `fellWedgeTrunk` is `open` (the notch's side from the deepest cut, any of the four, both faces;
   `tests/chopping.rs` `felled_from_each_side_the_cut_opens_a_gash_toward_the_notch`, red on `matching`); a trunk
   at rest tilted settles column by column along its pose, a blocked column rising to free rows, no wood lost
   (BUGS.md #73 Fixed; docs/FELLING.md §4).
   Owner, 2026-11-12, on BUGS.md #75 (trees couldn't be felled: one aim hacked one binding kerf): **"Merge the
   fix"** — a swing at the deeper band lands in the other one, so chopping at one spot notches and fells in ~11
   swings; ADR 0067's "clumsy straight hacking 3–4× more" rule is dropped.
   Owner, 2026-11-12, on the gash wiped by a swing after the fall: "I think let's do your recommendation" —
   the stump keeps its gash (built with item 26's round 2: the axe chops the stump's ramp, never clears it).
25. **Marks appear at contact, not on click** (owner 2026-11-12: "the gashes in blocks from tools show up on
   click instead of when the animation lines up with the block. We want that timing."). Every tool's hit (axe
   chop, pickaxe, shovel; the carve, cracks, chips, drops and sound) lands when the swing's head reaches the
   block (the swing's contact time from its data), not when the button is pressed.
   Owner, 2026-11-12, added to the same change: **"Tools should only consume durability on hit."** A tool wears
   once per landed hit, at contact; never on a miss or a cancelled swing; creative unchanged (no wear).
   Built (cloud, needs the owner's look in game): tool data `contactS` (pickaxes, shovels 0.26 s; the axe its
   swing's), the server lands hits at contact and drops cancelled ones, the client's sound and chips at
   contact, a bounce wears once and leaves alone don't (docs/BLOCK_DAMAGE.md §8).
   Owner, 2026-11-12, follow-up (BUGS.md #111): "I've noticed on tool hits they don't check where they're aimed at
   hit time, if I click on a block, and move my cursor to aim at the block below it, I'll see and hear the tool
   hit, but it won't be on the block I'm aiming at, it'll be the one I clicked on first before the hit played."
   Built: the hit is aimed at its contact, the target what the crosshair holds then, a miss there landing
   nothing (capability `hits.at_contact`; docs/BLOCK_DAMAGE.md §11). The axe's committed arc is unchanged.
26. **Axe bites like the other tools' marks** (owner 2026-11-12: "The tree chopping slice doesn't match the other
   tools style. It cuts a line all the way across which is much bigger than the tool head. It should align more
   with the dirt and stone ones."). Today an axe hit cuts a kerf slit across the log's whole width. Wanted: each
   hit carves a bite the size of the axe head where it struck (a wedge into the wood, like the dirt bowl and stone
   facets), the bites growing together into the notch as the chop goes on; cracks/splits along the grain from
   the cracks work. Next after item 24 (same code).
   Owner, 2026-11-12: **"Yes axe bites proposal looks good."** (head-sized V bites where each hit lands, joining
   into the notch; grain cracks around them; felling unchanged at ~11 swings, leaving the V gash). **Built
   2026-11-12** (cloud, branch `claude/axe-bites-zzvudx`), waiting on the owner's look in game: each hit records
   where it struck (`Cut::bites`) and the mesher carves a head-sized V there (`cut.bite.widthM` 0.14), a hit on a
   bitten spot landing beside it so the bites join into the notch; the wood's `cut.cracks` (`wood` set) round them;
   felling unchanged (11 swings at one aim, both faces); bites sent after a `BitesHello` and saved, an older cut
   drawn as before (docs/BLOCK_DAMAGE.md §6.4, ADR 0067 note).
   In-game check (0.54.x): the first bite is head-sized, but more swings grow it down into a tall narrow slot
   instead of across into a notch, and no wood cracks showed. A swing after the fall cleared the stump's slanted
   top (the gash), because the axe clears `dwell:log_ramp` like leaves. Owner, 2026-11-12, on that: **"I think
   let's do your recommendation."** — the stump keeps its gash: the axe chops a fell wedge (a hit, a bite) instead
   of clearing it; the stump comes down only when chopped deliberately like a log. With the bites
   spreading across and the cracks showing, **built 2026-11-12** (cloud, branch `claude/axe-bites-2-pllttm`),
   waiting on the owner's look in game: hits at one spot alternate beside each other across the side (either
   band) and 11 span the log, a thinner trunk's too; every V within the notch's band (the cell's middle ¼ m),
   carved over the octagon's chamfers; the wood cracks painted in ink (they were the bark's own streak colour)
   at 0.85; the stump's `dwell:log_ramp` is chopped as the log (a bite, a wear), never cleared, and comes
   down only when cut through, dropping wood. Not done: the stump's bites aren't drawn (recorded only).
   Round 3 (item 18's rule, 2026-11-12): the notch grows to follow the 50% rule (a V deepening and widening from
   the ¼ m band to the gash's V by the fell, the bites joining into it); see item 18's status.
   BLOCK_DAMAGE.md §6.4 "Round 2", ADR 0067 note.
26b. **A felled trunk landing on its chopper flings them (BUGS.md #52)** (the owner's brief, 2026-11-12: fix the
   cause, "we shouldn't be capping player kick like that" (2026-11-08): no cap on the kick; a trunk hitting a
   player at speed must still knock them as physics says). **Built 2026-11-12** (branch `claude/pinch-fling`):
   a piece's contact pressing a player down takes the piece's friction (`physics::PinchGrip`, a Rapier contact
   hook; PLAYER_CONTROLLER.md §6.6), so the frictionless capsule is pinned, not squeezed out; side hits stay
   frictionless and uncapped. The repro is un-ignored (0.4–0.5 m, was 155–160 m; both faces).

27. **The moon as a real bifacial world** — **M-7c client built (2026-11-13, branch
   `claude/moon-client-m7c-mxftge`):** the game client sets the travel hook (`Game::set_grid_frames`, from the
   session's `grid.frames`), holds only its grid's chunks (dropped at a crossing; the server sends the moon's as
   voxels and streams home again from nothing after the release), draws in the moon's local frame (the sun turned
   into it, the far moon disc and home's LOD hidden, the look carried with `look_across`); `grid.frames` off
   `CLIENT_PENDING`; the `game_local` scripted flight home → moon → home on both faces (MOON_WORLD.md §7.3).
   **Bench tool (2026-11-13, branch `claude/moon-bench`):** brief: "a bench hook and scenario for flying to the moon in
   the browser. A browser pass could not fly to the moon: no `__dwell` hook gives the moon's position or the player's
   grid … and a sky scan didn't find the moon at clock 0.30." Built: `__dwell.moon()` (position, direction, distance,
   the player's grid, on-moon gravity), the `flyTo` step (`moon`: dev move near the capture, a real flight in, descent,
   the fall checked against ⅙ g, landing; `home`: out past the release, home's chunks back) and the `moon-trip`
   scenario (DEV_TOOLS.md "The moon"). **Left:** running `moon-trip` on a GPU (local).
   **First browser flight (owner's findings, 0.58 on e4220fcd, M1, seed 7, both faces; branch
   `claude/moon-chunks-culling-pfqgty`, 2026-11-13; BUGS.md #123–#125, #136):** "the moon's ground is not drawn" (232 chunks held and meshed, 16–17
   draw calls; sky above, starfield below); "(b) snaps … after the face A landing 2 snaps + 1 reset (last correction
   34 m); face B trip snaps 3 → 11 (last 18.7 m); (c) mesh memory grows across trips: 44 MB first moon visit, 164 MB
   back home, 256 MB second visit … (d) home's blue sky and clouds show on the moon … leave unless trivial: at least
   hide home's clouds on the moon." Fixed (BUGS.md #123–#125, #136, #126 partly; MOON_WORLD.md §7.3): chunks drawn off
   home, home's LOD dropped at each crossing, carries' clock-slip corrections smoothed, home's clouds hidden on the
   moon. **Left:** re-running `moon-trip` on a GPU (local); the moon's sky.
   **Second browser flight (0.58.8 on facaaaf1, seed 7, M1; brief 2026-11-13, branch `claude/moon-trip-fixes-df1uke`):**
   "1. The test tool's flight home fails 2 of 4 times: `flyHome` calls `placeHome` as soon as the client shows the
   player back on the home grid, but `place_home` checks the server, which may not have released the player yet …
   2. A grid change counts as a snap (~640 km at each capture and release), hiding real snaps around a trip …
   3. After a face B return the held stone block looked pink … 4. Also note in the PR (don't fix): page JS heap 40 →
   169 MB over three trips, not dropping at home (one page; may not be a leak)." Fixed: 1 (BUGS.md #129: `placeHome`
   asked again until the server's release, a home saved only on home ground), 2 (#130: a cross-grid correction
   measured in world coordinates). 3: not reproduced natively, open with its suspects (BUGS.md #131). 4: recorded, not chased (one page's reading).
   **Teleport for the drawing check:** owner, 2026-11-13 (via the orchestrator): **"We should just teleport to the moon to see if ground draws,
   flying there is an inefficient test of drawing. Fine if testing the flight itself."** So the drawing test
   teleports onto the moon (`LocalCore::moon_land`, a dev move onto its ground captured the next tick), which found
   #136 (the moon's chunks sent before the client crossed were dropped; now held for the crossing); the bench has a
   `moonLand` step and a `moon-draw` scenario (draw calls, triangles, mesh bytes, shots on both faces). **Left:**
   `npm run bench -- --warm moon-draw` on a GPU (local); the moon's sky.
   **Left:** the browser flight (local, GPU), the roll upright at a crossing, M-5 (the moon's LOD), M-8. Earlier
   **M-7c status (2026-11-12, branch `claude/moon-m7c-x07vsc`, local):** owner, 2026-11-12: "Do we have the real physical moon yet?" (no: the client doesn't draw the moon
   up close yet (M-5) and there are no moon spawns (M-8)). **Built:** `carry` moved into `dwell-physics`; a
   `TravelMap` in `travel.rs` (the grids' frames and rules without spaces; `Grids::travel_map`); the predictor
   in grid frames (`GridTravel` hook before each predicted tick at the server tick's clock, a fresh space per
   carry, history per grid, `regrid` and `grid_misses` on a snapshot in another grid; MOON_WORLD.md §7.3); the
   test `the_client_predicts_capture_and_release` (both faces: capture and release on the server's inputs,
   ⅙ g). **Left:** the game client: hold the grid's chunks while away (the moon's generator in the workers),
   hand the predictor that world, draw it in the moon's local frame (sun turned, LOD and far disc hidden,
   `look_across`), set the hook and take `grid.frames` off `CLIENT_PENDING`; the `game_local` flight; then
   M-5. No browser check yet (nothing to see until the client draws the moon). Original item: (owner 2026-11-12: "we're going to make the moon a real bifacial world
   like the current world, with simple gray dirt across the whole thing for now. Farthest LOD, AKA the one you
   can see now, should be a simple lighting normal map based on the actual terrain. It should glow like it is
   now and cast light like it does now, and it should be the same size in the sky as it is now."). Owner's
   answers: travel and landing later, but design for it now (the moon's terrain is the one you'd stand on);
   about ¼ of our disc (~12.5 km radius); terrain from the world's seed with moon rules (cratered plains, ridges,
   maria), gray dirt everywhere for now — every world its own moon; today's motion, size in the sky, glow and
   moonlight kept, the face shown follows its orbit. Design first (docs/proposals/MOON_WORLD.md), then build.
   Owner, 2026-11-12: **"This moon is potentially an extension of our 'other' buildable surfaces that aren't
   always oriented the same way like our physics objects."** So the moon is designed as one case of a general
   system: a voxel grid with its own transform (position, orientation, its own up/gravity), like the built
   bodies and physics pieces (docs/BUILT_BODIES.md) — the home world being the one at rest — so moons, ships
   and other buildable surfaces share one frame abstraction, one streamer, one mesher and one collider.
   Owner, 2026-11-12: **"Could we make it so you can fly to the moon, when you get close enough you begin
   following its position, locked to it, gravity shifts to align like the underside of the bifacial world.
   Gravity is lower on the moon."** So travel is by flight: past a capture distance (data) the player's frame
   switches to the moon's grid (moving with it, locked to its position and orientation), gravity turns to the
   moon's local down the way crossing to face B turns it, and the moon's gravity is weaker (data per grid).
   Leaving the capture distance hands the player back to the home frame.
   Owner, 2026-11-12: **"The earth should use the same normal map kinda technique when far enough away, when on
   the moon."** So the far view is symmetric: any grid seen from far enough (the home world from the moon, the
   moon from home) is drawn as a disc with a normal map and albedo baked from its real terrain, lit by the sun;
   one far-body renderer for every grid.
   Owner, 2026-11-12: **"Add a moon starting spawn too side A and B."** A world can start the player on the moon,
   on face A or face B, as well as at home (a spawn choice, with the moon's own spawn search per face).
   Owner, 2026-11-12: **"We may need to consider a new model for the sun, world, and moon positions in order to get
   moon phases."** (Earlier the answer was to keep today's motion; now phases are wanted, from real geometry.) So
   design a sky model as data: the sun, the home disc and the moon as bodies with positions and orbits, the moon's
   lit half facing the sun so its phase follows from where it is; today's day length, both faces' days and
   nights, the moon's size in the sky and its light kept; moonlight scaled by the phase.
   **M-1 built 2026-11-12** (branch `claude/moon-world-design-build-8k9duh`): `shared/worldgen/grids.json`
   (home and moon), the `cratered` generator on both faces (`dwell-worldgen` `grids.rs`: craters by size
   class, maria, ridges), `dwell:moon_dust`, the generic far-body bake (normal/height + albedo RGBA per
   face, 256²: ~0.1 s, 512 KiB a face), the moon spawn search; 20 tests on both faces. Not in the game yet
   (M-2 draws it in the sky). Open questions in MOON_WORLD.md §12; the owner's lines above are folded in (§2 grids, §5 far body, §6 sky model, §7 travel, §8 moon spawn).
   Design: docs/proposals/MOON_WORLD.md; M-1 built (grids.json, the moon's cratered generator on both faces, the
   generic far-body bake, `dwell:moon_dust`). Owner's answers to §12 (2026-11-12): tidally locked; physical phases
   (no dome bias); "A darker floor but a minimum still." for new-moon nights; ~1–1.5 minutes to fly there.
   Owner, 2026-11-12: **"If not already planned we want the same moon shine/glow effects we already had. So the
   moon should look bright."** The far-body moon (M-2) keeps today's halo/glow and brightness: the baked normals
   and albedo shape its surface, but its lit part is as bright as today's moon (exposure-independent emissive-like
   brightness and the same glow around it), scaled by the phase's lit share (physical phases).
   **M-2 built 2026-11-12** (branch `claude/moon-m2-sky`): the sky draws the moon from the celestial model and its baked terrain (M-2): its direction `Celestial::view`'s, its face A (tidally locked) baked once at the join (`dwell-worldgen` `bake_far`, 256², `render/moon.rs`), packed with mips into one storage buffer (frame group binding 20: the composite already binds WebGPU's 16 sampled textures), each texel lit as N·L of its baked normal (slopes × `moon.relief` 8) by the real sun direction, its albedo the bake's over its brightest (so the lit part is today's `moon.color`); halo × the lit share; moonlight `nightFloor` + the rest × lit share. A CPU adapter packs no normals (a flat disc by the albedo). Same angular size (1.2°) and glow constants. Deviations: the bake runs on the
   main thread at the join (~0.1 s native), not a worker, and is not cached; the world's `MOON_PHASE` is
   Owner, 2026-11-12, changing Q3: "You mentioned a dome something, maybe do that": `moon.domeBias` (1: a ball) shapes the disc as a sphere, its normals tilted by the baked slopes (polish round: a 0.6 mix left a 135° crescent nearly invisible), so phases read as crescents, halves and gibbous with the craters on top, and the lit share (`celestial::disc_lit`, the mean N·L of that domed disc over full's) drives the halo and the moonlight: one model for the drawn moon and its light.
   not threaded into the sky yet (the data's start phase); no browser screenshots yet (time box).
   **M-4 built 2026-11-12** (branch `claude/moon-m4-celestial-jxijig`): the celestial model (`dwell-look`
   `celestial.rs`, DAY_NIGHT.md §14): the moon's orbit as data (`daynight.json` `moonOrbit`: 5°, ~597 km, the
   node turning every 96 days; the month `moon.cycleDays` 8), tidally locked (face A toward home), phases and
   the lit share from geometry (physical: a flat disc, dark from last quarter to first), moonlight = today's ×
   (`moon.nightFloor` 0.15 + the rest × lit × over the horizon), the halo today's × lit (what M-2 needs for
   "the moon should look bright"); the month's start an optional world setting (`MOON_PHASE` 0x84, absent =
   the data's). Not drawn yet (M-2).
   Owner, 2026-11-12, after M-4 showed physical phases leave the flat disc dark from last to first quarter: **"You
   mentioned a dome something, maybe do that"** — so the drawn moon blends a dome normal (`domeBias`) for familiar
   crescents and light through more of the month (this replaces the earlier "keep it physical"). On the whole-day
   8-day month (full moon always at face A's midnight; face B sees it ~92% lit): **"Not sure"** — kept at 8 days
   for now, open; a fractional month (e.g. 8.25 days) is the recommended alternative, one number in data.
   **M-3 built 2026-11-12** (branch `claude/moon-m3-gridframe-kg7kpg`): `dwell_world::GridFrame` (world ↔ grid ↔
   face-local: points, vectors, looks, up and gravity per face, cells, capture/release; `HOME` the identity, bit for
   bit), `dwell-physics` `frame::grid_of_pose` (bodies' poses, same bits as glam); the bodies' ray, felling's hits
   and snap, home targeting (`raycast_block_in`) and the controller's spawn face go through it; the both-faces
   check's rule 3 (no hand `inverse * (p − origin)`). Renderer model matrices per grid left for M-5; grid ids for
   M-7 (MOON_WORLD.md §2.1).
   **M-7 partial 2026-11-12** (branch `claude/moon-m7-travel`; the brief: fly to the moon, ~1–1.5 min, home gravity
   until captured, the grid id behind a `grid.frames` capability so it isn't breaking; "if it doesn't fit, build the
   frame switch + gravity natively first and leave streaming for a follow-up"): built natively — a simulation space
   per grid (`dwell-sim` `travel.rs`: its chunks from its generator in its local frame, its physics and players),
   the moon's frame from M-4's orbit, capture inside `captureM` and release past × `releaseFactor` with the world
   position kept (µm) and the path smooth, gravity per player (`Players::set_gravity_scale`, the moon's ⅙: same
   take-off, 6× the jump), far flight (`grids.json` `travel.farFlight`: ≥ 0.12/s × the distance from every grid's
   slab, ≤ 20 km/s), a look continued across grids. Tested: a scripted flight home → moon → home on each of the
   moon's faces. **Left for the follow-up:** the server's session loop holding the spaces, the moon's chunks on the
   wire per grid, the grid id in snapshots and the `grid.frames` capability (not announced until a build sends it),
   the client's view roll to the new down, and the predictor's far-flight speed (MOON_WORLD.md §7.1).
   **M-7b server + wire built 2026-11-12** (branch `claude/moon-m7b`; the brief: the server's session loop holds the
   grid spaces and runs travel per player, the moon's chunks streamed per grid and stored per grid (additive), the
   grid id and frame in snapshots behind `grid.frames` (in capabilities.json now), the predictor and drawing on the
   client, tests through LocalCore; "if it doesn't fit, do the server + wire + predictor first and leave the
   client's drawing for a local follow-up"): built the server (`dwell-sim` `server/grids.rs`: the moon's space,
   capture/release per session with no jump, far flight for every player, the moon's chunks streamed to a
   `grid.frames` client, an away player's edits applied to the moon), the wire (`grid.frames`: the snapshot's
   `GridView`, the chunks' grid byte) and the per-grid save table (`grid_chunks`). Tested natively (`dwell-sim`
   `tests/grids.rs`, both faces: capture ≤ 1 cm, landing and walking at ⅙ g, chunks and grid ids to the new
   client only, release, a moon edit saved and reopened). **Left for the local follow-up:** the client's side —
   this build's client withholds `grid.frames` (`capabilities::CLIENT_PENDING`) until its predictor runs in the
   grid's space (its chunks, its gravity, the far-flight speed) and it draws the moon's chunks in its frame and
   rolls the view; the `game_local` flight through LocalCore; away players in the other systems; the player's grid
   in its saved record (MOON_WORLD.md §7.2).
   **M-6's bake built 2026-11-12** (branch `claude/moon-m6-home-bake`; the owner's "The earth should use the same
   normal map kinda technique when far enough away, when on the moon."): `dwell-worldgen` `home_far.rs`, the home
   grid's `FarSource` over its world map — heights from the map's cells (water flat at its level), the albedo from
   each column's surface block (its biome's palette, blocks.json `color`), a separate water channel
   (`FarBake::water`, the share of the texel in the sea or a lake) — at `grids.json`'s home `bake.size` 256: 576 KiB
   and ~0.17 s a face natively; 10 tests on both faces; `examples/home_bake.rs` writes preview PNGs
   (MOON_WORLD.md §5). Clouds out of scope. Not drawn yet: drawing it from the moon comes with M-5/M-7.
   Owner, 2026-11-12, after M-2: **"The moon looks smaller I think compared to the old moon?"** The disc's angular
   radius in the shader is unchanged (`MOON_RADIUS`, the same edge test), so the cause is how it's lit: the full
   ball shape darkens the limb, and the baked terrain's rim (craters cut by the disc's edge, darker rim albedo)
   eats into the bright area, so the bright part reads smaller than the old evenly lit disc. Next (local): a
   side-by-side with the old moon at the same zoom and time; keep the full moon lit to its edge (limb darkening
   only on the dark side of the terminator, rim albedo evened) so the bright disc matches the old size.
   **Fixed 2026-11-12** (branch `claude/moon-size`): the disc reflects by Lommel-Seeliger, the regolith's law
   (`moon.lunarLambert` 1, `celestial::moon_shade`, shared by sky.wgsl and the lit share), so the full moon is lit
   evenly to its edge. Measured (seed 7, midnight, full, the 50% radius of the bright disc): ×8 old 0.54.33 92 px,
   the Lambert ball 76 px, now 91 px; ×1 12 / 11 / 13 px. The bake's rim was already filled (outside texels the
   mean albedo, `render/moon.rs` `pack`): no dark ring. Left: the baked albedo's maria read faint next to the old
   procedural ones (the full moon near white), a look question for the owner.
   Owner's answers, 2026-11-12: a fractional month ("Sounds good": 8.25 days); "keep darker seas (add a dark moon
   aria dirt block)"; "Yes for now, let's just do earth gravity out to moon. We likely need an atmosphere
   transition now between sky thinning to space. Clouds shouldn't extend past the earth atmosphere either."; the
   moon "should match previous size for now. We'll play by ear a bit when we travel there."
   **Built 2026-11-12** (branch `claude/moon-month-maria-wgxts3`): the month is 8.25 days
   (`shared/look/daynight.json` `moon.cycleDays`), so each full moon comes a quarter day later on the clock and over
   four months both faces get one near their midnight (`dwell-look` celestial test); the maria's ground is a new
   darker block `dwell:moon_mare_dust` (prototype content: the `mare_regolith` palette ramp, a painterly tile like
   the moon dust's; appended, no state id moves), named by `grids.json` `maria.block`, and the far bake's colour is
   now each ground block's `color` (no separate maria albedo), so the bake is darker over the maria by the same
   data. Owner's note on the PR: the baked maria were too faint after #682's shading, so the mare block's `color`
   (`#555459`) is 0.55 × the moon dust's luma, the old sky moon's seas (`1 − moon.maria`); a test pins the bake's
   maria/highland contrast at 0.55. Tests on both faces (`--test grids`, 26).

28. **An atmosphere: the sky thinning to space** (owner 2026-11-12: "We likely need an atmosphere transition now
   between sky thinning to space. Clouds shouldn't extend past the earth atmosphere either."). With flight to the
   moon, climbing should fade the sky's scattering to black space with stars by day, the horizon glow becoming a
   thin limb; clouds and plumes stay below the atmosphere's top (data); the moon has none.
   **Built 2026-11-12** (branch `claude/atmosphere-rendering-data-q5axre`; DAY_NIGHT.md §15):
   `shared/look/atmosphere.json` (base 3 km, top 80 km above sea level, face-local; a scale-height curve;
   space colour; stars by day in space; the limb; the sun whiter and sharper), the sky shader's
   `dwell_space` and its CPU twin, the fog thinning to none, clouds' and plumes' tops refused above the
   top (clouds.mjs) and run-time plumes cut there; today's sky exactly on the ground (a golden pin), both
   faces. **Left:** the browser check (local, a GPU), the home world drawn from far (M-6's bake).

29. **Thicker, more realistic distance haze** (owner 2026-11-12: "I think our atmospheric distance fog should be a
   little thicker, it doesn't seem quite realistic in farther mountains, hills looking whiter. What do you
   propose?"). Today (shared/look/fog.json) the haze reaches half at 16 km at sea level, density 0.95, scale
   height 1800 m, fading far terrain to the horizon's (pale, warm) colour. Proposed: aerial perspective — far
   land fades toward a bluer air colour (the sky a little above the horizon, warmer toward the sun, bluer away),
   loses contrast and saturation before it disappears, the haze half-distance shorter (~10 km) and the scale
   height lower (~1200 m) so valleys haze and peaks stand clearer. **Built as data, defaults unchanged
   (2026-11-11, branch `claude/haze`):** fog.json gained `air` (`elevationDeg`: fade toward the sky that far
   above the horizon; `sunTint` with `warm`/`cool`) and `veil` (`saturation`, `contrast`, `halfM`: far land
   greys and its darks fill with the air before the haze hides it), all 0 today (golden test pins today's
   output), and `presets` B `thicker` (10 km, 1200 m), C `aerial` (B + air 4° up + veil 0.45/0.35), D
   `aerial-sun` (C + sunTint 1), drawn with `?haze=<name>` (DAY_NIGHT.md §4). A 4×4 contact sheet (cone at
   8 km from 3000 m at noon, a valley with hills at noon, the cone at 16:48 toward and away from the sun)
   showed the differences are **modest at these values**: C greys and blues the far ridges in the valley
   most convincingly; the lower scale height makes the cone seen from 3000 m slightly *clearer*, not
   thicker (the eye is above most of the haze); D's tint is barely visible and warms noon hills toward
   dust. The white below the cone in those shots is the cloud deck, not the haze. **Waiting on the owner:**
   pick a preset (or ask for a stronger C: e.g. elevationDeg 8, saturation 0.6) and its values move to the
   top level of fog.json.

   Variants built (0.54.x, `?haze=thicker|aerial|aerial-sun`; defaults unchanged). Owner, 2026-11-12, on the sheet:
   **"The clouds make those screenshots hard to read."**; **"Also don't we just have an atmospheric distance fog
   that thins as you ascend? Something like that?"**; **"Shouldn't that be what's tuned, your screenshots seem to
   describe a lot of other effects?"** So: tune only the existing height fog's three numbers (fog.json
   `distanceM`, `density`, `heightM`); shoot a few settings from ground level and a hilltop (not from 3000 m,
   above the haze) with clouds off; the extra aerial-perspective mechanisms stay off.
   **Shots ready (2026-11-12, branch `claude/haze-tune`):** presets over the height fog's numbers only,
   `?haze=fog-11k` (11 km / 1400 m), `fog-8k` (8 km / 1200 m), `fog-6k` (6 km / 1000 m), density 0.95 throughout,
   no air or veil; defaults unchanged, no new shader work (frame cost as today). A 4×4 contact sheet, clouds off:
   the mixed-forest valley at 25 m looking east at the hills and the cones (noon), 300 m over the steppe across
   the ranges toward the tall cone 14 km off (noon), and the tall cone from ~8 km at ground level (noon and
   16:48). **Waiting on the owner:** pick A–D (or a value between); its numbers move to the top of fog.json.
   **Owner, 2026-11-12: "Yeah let's go with D on haze."** Done: fog.json's half distance 6 km (was 16 km) and scale
   height 1 km (was 1.8 km), density 0.95; the pinned fog values updated.
30. **Desktop frame rate on the M1 and hidden graphics settings (P0)** (owner 2026-11-12: "It seems like we're
   hiding some performance settings. I don't see 'tree' options in our settings for graphics. I think cards are
   killing my m1 mac too." / "Running 26fps." / "GPU at 31ms"). The desktop default preset is High (leaf cards
   on, high shadows, shafts, bloom, medium volumetric clouds, distanceM 256). Found: the Leaf cards switch exists
   but sits under Settings → View, not Graphics; several costly features have no setting at all (distant/LOD
   shadows, tall grass, far trees, hit shards, far herds). Next (local, after item 29's variants): measure each
   feature's GPU cost on the M1 at High; a Trees group in Settings → Graphics (leaf cards and the rest) and a
   setting for every renderer tier feature (an audit test that none is hidden); cut what's large; reconsider the
   desktop default for M1-class GPUs.
   **Performance target (owner, 2026-11-12, "you can record this"): "make the game run on most hardware smooth,
   and pretty, 'most hardware' uses my m1 MacBook Air as the baseline. So while you might be able to eek out some
   extra graphical fidelity on ultra. I want the core experience to be High on M1 MacBook Air."** Every visual
   feature is budgeted against High at 60 fps on an M1 Air; extra fidelity goes in Ultra.
   Owner on the cards and trees: "I think the internal leaf cards may be sucking up a lot of unneeded power and I
   think the leaf cards on the back sides of trees and farther out trees are rendering which is unnecessary. I
   also wonder if we should do billboards (maybe from above billboards too, possibly a few different angles so
   we can switch between them like old Mario kart did with carts, and some trees too)... We're going to want to
   have dense forests and so we really need to optimize." Plan: (1) measure; (2) cull cards inside the crown
   (faces whose way out is blocked by more leaves) and on the crown's far side (facing away from the camera),
   shorten the cards' range; (3) multi-angle impostors for far trees (views around and from above, baked per
   species/stage/variant, switched by view angle, lit by a baked normal), replacing the far tree meshes, and
   instancing for dense forests.
   **Round 1 (2026-11-12, BUGS.md #78; `claude/high-on-m1`):** measured on this repo's M1 at High
   (docs/DEV_TOOLS.md "High on the M1"): the broadleaf spot 15.5–15.7 ms GPU, open ground 9.2; leaf cards 0.6 ms,
   the cutout meshes together (leaves' cutout, tall grass, ladders, cards) 4.5–4.7, full detail 256 → 128 m 2.4–2.7,
   sun shadows 1.4–1.7, water 0.8–0.9, bloom 0.4–0.6, the rest ≤ 0.3. Done: steps (2)'s culls (cavity faces,
   far-side cards in the vertex shader, cards past their fade, fade 24–40 m) took the cards to 0.29 ms; Leaf cards in
   Settings → Graphics → Trees; an audit test for tier functions without a setting. The mixed forest read 17.05 ms after them, so High's full detail
   went 256 → 192 m (Ultra keeps 352): 13.88 ms there. Next: a Tall grass setting (a mesher flag; the renderer's cutout switch also hides
   leaves and ladders), a dense-forest spot that reproduces the owner's 31 ms, then (3) impostors.

   Owner, 2026-11-12, on this M1 Air: **"Im seeing on this m1 Mac that im getting sub-60FPS when flying on High. Im
   wondering if the new trees and volumetric clouds are at fault!"** **Measured (2026-11-12, BUGS.md #89,
   `claude/fly-high-m1`; docs/DEV_TOOLS.md "Flying at High on the M1", scenario `high-fly`):** flying at level 4
   from the starter's mixed forest east, `gpu.frame` 8.8 ms over open ground and 13.4–15.0 mean, p95 15.6–17.3
   (up to 18.5) over the forests; `frame.cpu` p95 ≤ 10.5. The volumetric clouds are not at fault (Medium → Low:
   no change), nor streaming (uploads, meshing, LOD; a 1 MiB desktop upload budget changed ≤ 1.8 ms and was not
   adopted); the forest's near pass is (cutout leaves and tall grass, their shadows: the costs #78 found standing).
   Speed effects −0.5 ms, shadows Low ~−2 over forest. No cut made in the time box. Next: the Tall grass
   setting (mesher flag) and impostors (item 31) are the levers; a vsync run of `high-fly` for the dropped-frame
   count; a faster flight (level 6+) for streaming.
   Owner, 2026-11-12: **"Ensure all of our graphics have settings in our graphics menu."** Every visual feature gets a
   Graphics setting (with preset values), not only the ones with a renderer tier function: tall grass density, far
   trees / impostors, leaf cards and chamfer, hit shards and particles, far herds, distant shadows (its own),
   volumetric clouds and plumes, lava crust/glow, bloom, haze extras, water tiers, light shafts, reflections, view
   distance; the audit test widened from `render/*_for` to every per-feature switch the renderer and mesher read.
   **Built (2026-11-12, cloud; the GPU measurement outstanding, local):** Settings → Graphics in seven groups
   (Display, Lighting, Sky & weather, Water, Terrain & trees, Effects, Distance); new settings with switches where
   there were none, each preset keeping what it drew before (all on / 100 %): Tall grass 0–100 % (the mesher's
   `ChunkFlags::plants`), Far trees (the LOD sections' trees), Distant shadows (its own switch at the Shadows
   tier), Hit shards 0–100 %, Particles 0–100 % (chips, sparks, leaves, steam), Far herds; stored in the
   append-only `dwell.features` record (an older record reads with the defaults). `graphicsAudit.test.ts` now
   covers four declared lists — the renderer's tier functions, the look data files, the mesher's switches and
   the client's CPU-tiered effects — each naming a setting or a written reason (the fog's haze, ambient and flood
   light baked at mesh time, the LOD blend, dev previews, …; lava's glow is Bloom's, the vent plumes are drawn by
   the clouds' pass). Next (local): measure each new switch's GPU cost on the M1 at High (forest-perf) and decide
   which presets should cut them (Low and Mobile are the candidates for tall grass, far trees and distant
   shadows); chamfered leaves have no switch (the mesh shape, not a per-pixel cost).
   Owner, 2026-11-12: **"It would be sweet if we could see in realtime on graphics menu exactly or approximately what %
   of performance is being used by each setting."** Plan: the renderer's GPU pass timings (the `gpu.*` telemetry,
   timestamp queries where the browser allows them) mapped to the settings that drive each pass (data: setting →
   passes), shown live beside each setting as ms and % of the frame budget (16.7 ms at 60 fps); settings without
   their own pass (e.g. tall grass, cards inside the terrain pass) estimated by a quick on/off A/B when the menu
   opens, labelled "≈"; a total bar; works without timestamp queries by falling back to the A/B estimate. Queued
   (local, after the setting-for-everything work lands).
31. **Far trees as multi-angle impostors** (owner 2026-11-12: "billboards (maybe from above billboards too,
   possibly a few different angles so we can switch between them like old Mario kart did with carts, and some
   trees too)" and "The farther distant thoughts on billboards are because our low LOD trees look pretty bad and
   not very much like our closer trees, so it's a rough transition."). Far trees drawn as impostors baked from the
   real tree models (each species, growth stage and seed variant; views around the tree and from above,
   albedo + normal + depth so they're lit by the sun and fog like the real ones), switched or blended by view
   angle, crossfaded with the near trees with no pop; replaces the LOD's blob trees (lod_trees.rs) and makes
   dense forests cheap (one quad a tree, instanced). **Status (cloud, 2026-11-12):** the design is
   [`proposals/TREE_IMPOSTORS.md`](proposals/TREE_IMPOSTORS.md) (ring of 8 × 2 views + 1 from above, 3 stages ×
   4 seeds per species with the tint at draw, ~640 KB a tree at desktop, a dithered hand-over, instances from tree
   lists, phases I-1–I-5, open questions for the owner); phase I-1, the bake, is built
   (`dwell_worldgen::impostor`, data `shared/trees/impostors/impostors.json`, preview sheet
   `docs/proposals/tree-impostors-bake.png`). Next: I-2 tree lists per section (cloud-able), then I-3 drawing
   (local, GPU, measured on the M1 at High). Waiting on the owner: the proposal's questions.
   **I-3's CPU half built (2026-11-12, local):** the hand-over per Far trees tier as data (`impostors.json`
   `far.tiers`: `on` an impostor from 96 m over a 24 m fade, `off` none), the crossfade's shares, the dithered
   view pick, the baked stage/variant per tree and the view direction through `FaceFrame` (tests on both faces).
   **Not built:** the GPU half (atlas upload, instance buffer, the billboard shader with the baked normals and
   depth offset); the far trees are still `lod_trees.rs`'s.
   **I-2's tree lists built (2026-11-12, local, `claude/forest-edge-impostors`):** `TerrainGenerator::far_trees(lo,
   hi)` lists the species trees anchored in a rectangle (each tree in one list, by its anchor: four quarters'
   lists are the square's, tested on both faces in `forest_world`), and `impostor::Config::instances` turns a list
   into instances (the tree's foot, its baked layer `species × stages × variants`, a quarter turn from its seed).
   Felled trees are not yet left out (the growth records are the server's). Next (local, one session): the GPU
   draw on these lists — the bake run at load in a worker, the atlas array, an instance buffer per LOD section,
   the billboard shader (`Config::dither`'s view pick, the baked normals, depth offset), `Far::share`'s
   crossfade, then the LOD sections stop drawing tree cells. The forest edge says it is also the frame-rate
   lever (item 32): the near pass there is bound by the leaves' triangles, and `lod_trees.rs` draws the same
   chamfered cells past 96 m.
   **I-3's GPU draw built (2026-11-12, local, `claude/impostors-gpu`):** the far trees are impostors
   (client `render/impostors.rs`, ARCHITECTURE.md §6.6): the atlas baked by the job workers and uploaded per
   layer, instances per 64 m tile from `far_trees` to 1024 m, `imp_fs` (dithered view, baked normals, the scene's
   sun/ambient/fog, received shadows), the near leaves' complementary dither (`TREE_LEAF_FADE`), and chunks wholly
   past the hand-over and the plants' fades (90 m) submit no cutout draw; the Far trees setting picks the tier,
   `?impostors=0` (bench config `impostors-off`) the old `lod_trees.rs` path. **Hand-over: 48 m over 16 m** (data),
   the nearest measured; 96 m saved nothing at the edge (leaves dropped in the vertex stage still cost their
   vertices, so the cut is by chunk). `forest-perf` on the M1 Air at High, `gpu.frame` p95 ms, before
   (`impostors-off`) → after: **inside 15.5 → 15.5, edge 25.8 → 23.4, above 15.6 → 12.3** (`gpu.near` p95: 13.0 →
   11.0, 22.5 → 18.6, 13.1 → 8.5). A `frag_depth` depth offset to the baked depth was built first: with it (tiles
   unsorted) the edge's `gpu.near` was 32.4 ms p95, without it and tiles drawn nearest first 24.9 (both at the
   96 m hand-over; it turns off early depth on ~70 k overlapping quads), so it was dropped. Look (shots in the PR): forests reach the horizon with their crowns instead of bare trunks on flat
   green; at 48 m the impostors read paler and noisier than the near crowns (no ambient occlusion baked, nearest
   texel sampling, no mips) and near trunks (logs drawn to full detail) show under them.
   **Left (the next session):** the edge is still **23.4 ms p95, ~7 ms over 16.7**: the cutout chunks within 90 m
   (the tall grass's fade keeps them submitted; leaves alone could stop at 64 m) — split grass and leaves into
   separate meshes or draws so leaf chunks stop at the hand-over; the impostors' own cost (not yet isolated:
   add a `view` switch); the look at the hand-over (bake the near leaves' AO/darkening into the albedo, mips per
   layer, the trunks handed over too); felled trees left out of the tiles; shadows cast (I-5); I-4 (delete
   `lod_trees.rs`); the phone run (32 px atlas, ~10 MB).
   **BUGS.md #95 fixed (2026-11-12, cloud, `claude/far-ground-holes`):** the LOD section mesher meshes the
   ground as if its trees weren't there (`lod_mesher.rs` `mesh_section_above`: tree cells cleared for the ground
   pass and the surface search, drawn whole over it by `lod_trees` when Far trees are on), so the impostors stand
   on closed ground; the near chunks unchanged. Test: `far_ground_under_trees` (every column's top covered, trees
   on or off, with and without surface data; trees off = the treeless ground's mesh exactly), both faces, red →
   green. Left: the in-game look from high up at night (a local check).

   Owner, 2026-11-12, on the 48 m hand-over side-by-sides: **"Distant trees looks solid handover."**
32. **A tall dense forest for performance reference** (owner 2026-11-12: "We should have a tall dense forest
   biome/climate for performance reference on trees."). A biome preset (tall, closely spaced mature trees — an
   old-growth temperate rainforest of tall pines and broadleaf, thick undergrowth) placed by climate (cool and very
   wet), plus a fixed test world and bench spot that stand in the densest forest, so every tree/leaf/impostor change
   is measured there at High on the M1 Air. **Built (2026-11-12, cloud), the GPU run outstanding (local):** the
   `preset:old_growth` biome (6–13 °C, 2200 mm and up; the old-growth species `dwell:old_pine` 18–26 m and
   `dwell:old_oak` 9–14 m, mostly mature, ~220 a hectare by the `old_growth` placement, crowns interleaving
   by a negative `spacing`; tall grass nearly everywhere), in the Realistic world by climate and the
   starter's cool wet ground (3.5 % of its land; its terrain changed there, a new line for the owner to raise; the default unchanged); `test:forest`, old-growth
   everywhere; the spots `forest:inside`, `forest:edge`, `forest:above` and the scenario `forest-perf` at
   `?preset=high`, a 16.7 ms frame budget report-only until item 30 (docs/DEV_TOOLS.md "The tree performance
   reference"). Next (local): run `forest-perf` on the M1 Air for the first numbers, look at the stand, and
   tune the spots if a view misses the densest trees.

   Owner, 2026-11-12, on the Realistic/forest video: **"The realistic forest video didn't show the forest for half of
   it. And it was slow frame rate in the forest though it looks really pretty."** Next: the forest's frame rate at its
   edge and inside (24.7 ms at the edge on the M1 at High; the near pass's cutout leaves and tall grass, sun
   shadows), then the impostors (item 31); re-record the forest video after, with most of it in the forest, recorded
   frame-exact (the screencast, not real time) so a slow page doesn't show as a slow video.

   **The forest's frame, measured (2026-11-12, M1 Air, `forest-perf` at High, `--unthrottled`, `gpu.frame`
   p50 / p95 ms):** before inside 17.0 / 18.8, edge 25.2 / 26.4, above 14.6 / 15.8. The edge's ablation
   (`forest-edge-ablate`): the cutout meshes (leaf cards + tall grass) off 24.6 → 9.8 (−14.8 ms, `gpu.near`
   21.9 → 7.7), sun shadows off −3.3, full detail 192 → 96 m −1.8. **Cut:** tall grass past its fade (88 m) was
   drawn out to the full-detail distance and discarded per fragment; any plant past its fade is now dropped
   whole in the vertex stage, as leaf cards were. After: inside **14.7 / 15.4** (under 16.7), edge 24.3 / 25.7
   (unchanged: its cutout cost is the cards and grass within their fades, seen from just over the crowns),
   above 14.5 / 15.8. **Outstanding:** the edge, ~9 ms over.
   **The edge split (2026-11-12, local, `claude/forest-edge-impostors`; docs/DEV_TOOLS.md "The tree performance
   reference"):** leaf cards off −0.5 ms, tall grass off (`?tallgrass=0`, a new dev override) −0.6 at the edge
   (−1.3 inside): neither is the edge's 14.8 ms; the cutout **leaf blocks** are. The near pass there is bound by
   geometry, not pixels (render scale 0.5 → 1: `gpu.near` +17 % for 4× the pixels). A cutout depth prepass
   (alpha-tested depth first, colour on equal depth) was built and measured: +6–8 ms at every spot (edge 23.8 →
   31.4), not adopted. Also fixed: `forest-perf`'s 16.7 ms `gpu.frame` budget was never checked (the GPU
   timings are samples, not per-frame series: "not recorded"). Next: fewer leaf triangles — impostors from
   nearer than 96 m (item 31) and a plainer leaf mesh with distance (no chamfer, merged faces past the cards'
   fade) — measured with `forest-edge`.
   **Leaves past the hand-over not drawn (2026-11-13, local, `claude/forest-edge-perf`):** the mesher orders a
   chunk's cutout triangles with its tree leaf faces last (`dwell_mesh::tree_leaves_from`), so a chunk past the
   impostors' hand-over end (`fromM` + `fadeM` + 2 m, the tier's data) but within the plants' 90 m draws its grass
   and cards alone (`render::impostors::cutout_draw`; tests pin the split and the data's switch distance). No
   look change (those leaves were dropped by the shader). `forest-perf` on the M1 Air at High, `--unthrottled`,
   `gpu.frame` p50 / p95 ms, two baselines → after: inside 13.9 / 18.1 and 13.2 / 16.8 → 13.5 / 16.6, **edge
   24.2 / 27.0 and 23.1 / 25.6 → 23.4 / 25.4**, above 10.3 / 12.5 → 10.4 / 12.5: the edge's drawn triangles
   fall (~5.4 M → 4.0 M) but its frame is within run-to-run noise of the second baseline, so the leaves
   between 66 and 92 m were not its cost. Still **~6.5 ms over** at p50. Measured, not adopted: the hand-over
   at 32 m (data) edge 21.5 / 24.3 (−1.7 ms, a visible hand-over nearer than the owner's approved 48 m).
   The edge's frame now: `gpu.near` 14.5 (the leaf blocks within 66 m), `gpu.shadows` 7.1 p50 (the nearest
   cascade, redrawn every frame, ~1.5 M caster triangles: the canopy's leaves), composite 4.2, clouds 3.8.
   Next levers: the leaves' shadow casters (a plainer leaf caster mesh, or leaves in the near cascade only within
   a distance), a plainer leaf mesh past the cards' fade (no chamfer, merged faces). Also seen: the bench's
   shots in `test:forest` came out flat grey at every spot (clouds off too), so they show nothing to compare;
   and `flood_tests::open_terrain_takes_the_full_sky::a` fails on main (3 dark vertices), unrelated.
   **Round 2, the leaf shadows (2026-11-13, local, `claude/forest-shadows`):** the grey shots were your own body:
   the bench's `spot` shot stands the player at the camera, and a placed camera drew the whole body around it, so
   every `test:forest` number before this measured a view covered by its inside, and the body, a moving caster,
   redrew a far cascade every `refreshFrames` with the canopy's leaves in it (the edge's `gpu.shadows` 7.1 ms was
   that, not the nearest cascade). Fixed: a placed camera within 1 m of your eyes draws you as first person
   (`own_body_draw`, test red → green), and a far cascade is drawn again only for moving casters touching its box
   (a player anywhere in the world redrew every far cascade every 4 frames at High; `moving_casters`, test red →
   green). No look change; real shots now. `forest-perf`, M1 Air, High, `--unthrottled`, p50 / p95 ms, covered →
   real: `gpu.frame` inside 13.6 / 17.0 → 14.1 / 15.4, **edge 21.5 / 25.7 → 20.6 / 25.4**, above 10.3 / 12.6 →
   12.5 / 14.2; `gpu.shadows` edge 1.0 / 9.0 → 0.8 / 1.1 (inside 3.0 / 6.1 → 0.7 / 1.2). With the shadows ~1 ms,
   no leaf caster change was made (a plainer caster or a near-only leaf distance would save under 1 ms). The
   edge's real frame: `gpu.near` 16.0 (the leaf blocks), `composite` 6.5, `clouds` 5.4 p50; next levers are those.
   **Round 3, the leaves' vertices (2026-11-13, local, `claude/forest-near`):** the near pass's cost is the vertex
   data its ~4 M leaf triangles read, so the terrain and cutout vertices are packed: the normal as snorm8x4 and the
   baked light, flood light (and a cutout's bare flag) as unorm8x4, one float's slot each (`render::packed_lit`;
   terrain 84 → 64 bytes, cutout 92 → 72; test `terrain_and_cutout_vertices_pack_normal_and_light_into_bytes`, both
   faces' meshes, every value within one 8-bit step). No look change: the shaders clamp light to 0–1 and read bare
   as over or under ½; the same-spot shots differ only where the clouds drift and the wind moves cards and grass
   between runs. `forest-perf`, M1 Air, High, `--unthrottled`, `gpu.frame` p50 / p95 ms, before → cutout packed →
   both packed: inside 13.4 / 14.1 → 13.0 / 13.7 → **12.6 / 13.0**, **edge 20.5 / 21.1 → 19.7 / 20.2 → 18.7 /
   19.3**, above 12.4 / 12.8 → 12.5 / 12.9 → **12.0 / 12.4** (`gpu.near` edge 16.0 → 15.2 → 14.3). Profiled, not
   changed: hidden leaf faces inside a canopy were already culled (no face between two leaf cells), and a leaf's
   pieces can't share vertices (each takes its own face's leaned normal and tint); `clouds` at High is the
   Medium tier already (quarter resolution, 10 steps), and the GPU's pass timestamps overlap on the M1 (near +
   composite + clouds + far sum to ~32 ms of a 20 ms frame), so `clouds` 5.6 and `composite` 6.7 at the edge
   are not their own cost: measure them by switching them off (`--compare`), not by their pass times. The edge
   is still **~2.6 ms over** at p95. Next: the cutout's vertex outputs (26 floats a vertex to the tiler: tile,
   fade and draw could be flat or packed), the colour as unorm8x4 (its tints can pass 1: needs a range),
   then `clouds`/`composite` measured by `--compare` with them off.
   **Round 4, the cutout's colour and outputs (2026-11-13, local, `claude/forest-r4`):** clouds measured by switching
   them off at the edge (a `view` step, same page): `gpu.frame` 18.6 / 19.5 → 18.3 / 19.4 → back on 18.7 / 19.5, so
   the clouds' 5.3 ms pass time is ~0.3 ms of real frame (they overlap the near pass). The composite measured by
   its parts (`--compare forest-edge`, new configs `shafts-off`, `bloom-off`): `gpu.frame` p95 18.26 / 18.27 /
   18.40 ms, `composite` 6.4 in all three, so neither is a cost at High and the composite's ~6 ms pass time is the
   near pass's overlap, not its own work. Built: the cutout's colour as unorm8x4 over a scale (`scaled_rgb`: rgb / (1 + 3a), tints up to 4, a colour within 0–1 keeps plain
   8-bit rounding; cutout 72 → 64 bytes a vertex; the terrain's colour stays floats, its lava faces code a stop in
   it), and the cutout's tile and fade passed flat (the same at every corner, test
   `cutout_tiles_and_fades_are_the_same_at_a_triangles_corners`; 6 of the 22 interpolated floats). No look change
   (same-spot edge shots before/after differ by the clouds' drift and the wind). `forest-perf`, M1 Air, High,
   `--unthrottled`, `gpu.frame` p50 / p95 ms, before (round 3 / this session's base) → after: inside 12.6 / 13.0 →
   **12.4 / 12.9**, **edge 18.7 / 19.3, 18.6 / 19.5 → 18.3 / 18.9** (`gpu.near` 14.1 → 13.8), above 12.0 / 12.4 →
   12.2 / 12.7 (noise). The edge is still **~2.2 ms over** at p95; what is left there is the near pass's ~4 M leaf
   triangles themselves. Next levers: fewer leaf triangles past the cards' fade (merged faces, no chamfer), or the
   terrain's colour packed too (its lava code needs a byte of its own).
33. **Licensed music replaces the generated music** (owner 2026-11-12: "I downloaded a big music set as a Unity
   asset, I'd like the casual game music setup in a prototype and make it play randomly in the game. Probably
   needs compression. It'll replace the current music which is AI generated."). The pack: *Andrew Sitkov Music*
   (Unity Asset Store, `~/Library/Unity/Asset Store-5.x/Andrew Sitkov Music/AudioMusic/`; the download was not
   finished at 23:10, the folder empty). Plan: extract the casual-game tracks from the .unitypackage, compress
   (AAC/Opus at a music bitrate, loudness-matched), a music list as data (tracks, moods, gain, fade), a prototype
   page to audition the set, the game's music player shuffling them (no repeats back to back, gaps and fades as
   data) in place of the current tracks; licence notes as for the Imphenzia samples (AUDIO.md §2b).
   Owner (2026-11-12): "Did you give me a sample of the music yet". **Built (branch `claude/casual-music`):**
   20 candidates from the casual packs (Mega *Casual Game Music*, Massive *Casual Game Music Pack* and its
   acoustic versions, *Simple and Positive*), AAC 96 kbps at −18 LUFS, 24.2 MB; the playlist as data
   (`shared/music/music.json` `playlist`: shuffle, 20–60 s gaps, cross-fade 0) played by
   `client/src/audio/playlist.ts`; Starry Sunshine removed; the audition page
   `docs/prototypes/music-audition.html` (https://claude.ai/artifact/Ccdtsp5eiQYF4b9uzspyYe). **Waiting on the owner:** the
   picks (the page's JSON), the level and the gap.
   Owner (2026-11-12): "Im following the Minecraft pattern where they play randomly not sequentially." (the
   shuffle was already random; the difference is the silence between tracks, ours 20–60 s, Minecraft's minutes)
   → "At random time." Owner (2026-11-13): "Are there more serious sounding ones we could play at night in the
   casual set" (the tracks are being picked locally); the gaps confirmed: "Yeah 3-10 minutes." **Built (branch
   `claude/music-random-times-kf03fi`):** gaps a random 3–10 minutes; the game's first track after a random
   30–180 s wait (`playlist.contexts`, the menu's 0: music at once; a track carried over from the menu plays on);
   tracks' optional `times` (`day`, `night`, default both) drawn by the listener's daylight against
   `playlist.dayAt`, the menu as day, a playing track finishing when day turns to night (AUDIO.md §2a, §6a).
   **Waiting on the owner:** which tracks are night ones (none is marked yet).
   Owner (2026-11-12), of Casual Classic #1: "I like that one". Then, after the night candidates' previews: "I
   really only like Night: 1, 4, 5 for both day and night." **Built (branch `claude/music-picks`):** the playlist
   is those four, each for day and night: Casual Classic #1, *Close Your Eyes And Fly* Variation #5 (UPDATE 1.28,
   Relaxing Themes), *Lyric Fantasy Theme #4* (Massive *Lyric Fantasy Background Music Pack*) and *Dream On*
   (*Beautiful and Emotional Music Pack*), encoded as before (−18 LUFS, AAC 96 kbps, 1.0–1.9 MB each, 5.6 MB
   together); the other 19 removed. Casual Classic #1 is kept from the earlier remark; one entry to drop.
   Owner (2026-11-13), after the round-3 piano and electronic previews: "I want these songs from the music you
   shared. Piano: 2, 4, 5 day or night. Electronic: 1 for night only." **Built (branch `claude/music-picks-2`):**
   the playlist is eight: the four above plus *Quiet Theme #1* (Fire VS Ice), *Positive and Calm Orchestral Pack
   2 Theme #5* and *Sentimental Moment* (Sentimental and Romantic) for day and night, and *Lyric & Peaceful Space
   Theme #2* for nights only (`times: ["night"]`), encoded as before (1.0–1.7 MB each).

34. **The in-game Chisel editor and the Textures tab extras** (owner 2026-11-12: "these all seem solid yes"): open
   Chisel's biome sliders inside a running world with the terrain around the player regenerating as they tune
   (Phase 56 B7); the Textures tab adding new materials and blocks by data, editing water and sky colours, and
   painting item and model textures (B6's rest). Queued.
   Brief (2026-11-13): "in a local world (creative, or a dev/creator toggle — say which), open Chisel's biome
   sliders as an in-game panel; a change becomes a world edit (saved with the world, the existing edit path)
   and the terrain around the player regenerates — the chunks within a radius re-generated and re-meshed
   (player edits kept on top …), LOD refreshed; debounced; undo/reset. Multiplayer: host only (say so)."
   **First slice built (branch `claude/chisel-in-game-phase-56-2jgr7e`):** the game menu's **Chisel** page in
   local worlds, offered in **creative** mode (no separate dev toggle): per biome the `detail`, `shaping` and
   `banks` sliders, Undo, Reset, a 400 ms debounce; a change retunes the world: the config saved in the world
   file (settings `worldgen`) and the world record, the maps re-wrapped (no recompute; a ground-moving edit is
   refused: a new world), the chunks within 3 chunks of the player generated again with the player's edits kept,
   sent whole and their LOD sections recomputed. **Host only**: refused while guests are in the world.
   Outstanding: the far LOD and far terrain follow only on reopening, edited chunks outside the radius keep
   the old ground, the water grid isn't re-read, the browser check (cloud session: no GPU), the Textures tab
   extras.
35. **Swords as weapons** (owner 2026-11-12: "we'll want to bring in the swords from the model pack we got the
   tools from as weapons."). Answers: animals only for now (no PvP); a timed swing arc like the axe that hits what
   it crosses at contact, with damage, knockback and durability per sword as data (reusing the swing and
   clip_probe systems); in the creative inventory now, adventure recipes later. The models from the owner's
   *3D Low Poly Tools Weapons Containers* pack (NexusArcadeStudios), as the tools were. Queued.
   Owner, 2026-11-12: **"For the sword we'll use a perpendicular vs parallel defense attack gameplay. Wandering
   NPCs will get swords. We'll use the Wii Resort sword fighting model, you can only hold the sword vertical or
   horizontal, and you can choose to block or not. Attacking drops block, if you hit enemy sword within an angle
   threshold of perpendicular you're blocked, if within an angle of parallel it connects."** So: a sword is held
   in one of two stances, vertical or horizontal (the same two swings as the axe); holding block raises the
   sword in its stance; attacking drops the block for the swing; a strike that meets the defender's raised sword
   within a threshold of perpendicular to it is blocked (a clash, no damage), within a threshold of parallel it
   slips past and connects; angles and thresholds as data. NPCs carry swords and fight by the same rules (item 38).
   **Built (server, data, tests; 2026-11-12):** a `weapon` tool kind (shared/tools: `weapon` damage, knockbackM,
   maxTargets, durability, `guard` stances/thresholds/recoil/front arc; a fixture weapon by data alone), the sword
   item (prop `katanas_swords`, the creative inventory's Weapons group); the server sweeps its arc at contact and
   strikes creatures only (never players), damage and knockback by data, one wear a creature struck; creatures
   have health, a hit radius and drops (shared/creatures), react by `near.hurt` (deer flee; wolves fight, flee when
   badly hurt), die and drop; a defender API (`Creature::guard`: stance, raised, attacking drops it) and the
   blade-angle clash (blocked / glanced / connected, a block recoils the striker). ARCHITECTURE.md "Weapons".
   **The model (local, 2026-11-12):** the pack has no straight sword: its `katanas-swords` FBX is one katana
   blade and five colour variants of one handle, stacked. The `sword` prop is the blade on the first handle,
   1 m, its grip measured on the handle (a grip off the handle fails the generator), held in the fist
   (ITEMS.md §1a′). **Outstanding (local):** slash
   clips of its own fitted with `clip_probe --fit` (it plays the axe's chop clips, with the axe's timings, for
   now); a clash's sound, sparks and recoil on the clients (a message for the outcome); the player's block input
   and raised pose (players can't be struck yet); NPCs raising their block and swinging on their own (the
   villager holds the sword as its guard from its spawn, lowered, and turns on its striker: item 38's
   `fightBack`; the sword in its drawn hand is item 38's renderer work); wolves biting back (no creature damage
   to players yet). The sword in the villager's hand is drawn (item 38).
   to players yet).
   **Round 2 (owner's brief, 2026-11-12):** a strike outcome event behind a capability (`weapons.clash`) with
   the clash's sound, sparks and the striker's recoil on clients; the player's block input (hold right-click / a
   gamepad button / a touch button, bindings as data) raising the sword in the stance, attacking drops it, the
   server knowing the player's guard and telling others; minimal NPC combat AI by data rules (face the attacker,
   pick a stance random/counter, raise the block on the wind-up, strike back on the recovery, give up after N
   seconds); NPCs never hurt players (open question for the owner: should NPCs be able to hurt players?).
   **Built (server, wire, client logic, tests; 2026-11-12):** `Swordplay` (clash outcomes to capable clients
   within `guard.clash.hearM`, raised guards of players and NPCs), `ChopAction::Guard`; the clash's sound
   (`guard.clash.sound`, the metal `axe_clank`), sparks (`guard.clash.sparks`, chips by data) and recoil;
   Place held past `guard.raiseAfterS` raises the block (a tap still turns the stance; the existing Place
   bindings of mouse, controller and touch), the stance's raised pose named by data (`raisedPose`); NPCs'
   `npc.fight` rules (`stance` counter/random, `raiseOnWindup`, `strikeBack`, `strikeWithinM`, `cooldownS`,
   `giveUpS`); an NPC's blow is resolved against the player's block but never hurts (owner's question open).
   Found along the way: the villager was 2 rows (1 m) tall, so a level slash at eye height passed over it; now 4.
   **Round 3 (local, 2026-11-12, combat visuals):** the raised block drawn: `raisedPose` names a role
   (`guard_level`, `guard_vertical`) each character plays from its clips (`biped_guard_*`, `fp_guard_*`,
   first guesses), laid over the gait while no swing plays, on your arms and body and on other players and
   NPCs from the server's guards (CHARACTERS.md §11.11); the hint per tool kind as data (BUGS.md #83).
   Seen in game: the vertical block reads (the blade upright before the view); **the level block does not
   yet** (the blade still leans up, not across: the hand's roll limit, ±40°, stops it: needs an arm turn,
   fitted with `clip_probe --fit`). Not seen: a clash's sparks in a duel (the villager did not turn on a
   single strike in the time box) and the third-person raised poses. Found: BUGS.md #85 (a NaN look kicks
   the player). **Outstanding:** fit the level block; NPCs' own swings drawn (the server sends no NPC swing
   to clients yet); a duel shot with sparks; the owner's answer on NPCs hurting players.
   **Round 4 (local, 2026-11-12, combat visuals 2):** the level block fitted with `clip_probe --fit` (an
   arm turn; the probe now poses guard roles as the game does, `clip_probe.rs`
   `the_level_block_lies_across_the_view_and_the_body`): seen in game, the blade lies level across the
   view. NPC swings on the wire behind the `npcs.swings` capability (an NPC mid-swing is sent in strike pose
   3 level / 4 vertical, drawn with the rig's chop clips; older clients get its idle pose). Dev `npcsFight()`
   turns every NPC on the player. **Not seen:** a duel. With `npcsFight` and dev `edit('break')` swings two
   metres off, the villager faced the player but never raised its block or struck back, so there is no
   shot of sparks, a connecting hit or the third-person poses. Next: check whether `edit('break')` starts
   a server-side weapon swing the fight rules can see (a native duel test through `LocalCore`).
   **Round 5 (local, 2026-11-12, combat visuals 3):** the native duel through `LocalCore` passes on main
   (`game_local.rs` `a_sword_swing_through_the_game_meets_a_villagers_block_and_it_strikes_back`): the
   block rises in the wind-up, the slash is blocked, and the villager strikes back. Round 4's browser miss
   was the shot's timing, not a bug: the whole exchange lasts under a second, longer than a screenshot
   takes to come back. Filmed with a screencast instead (2026-11-12). Seen: the villager raises its
   sword upright in the player's wind-up (its third-person raised pose), then swings back with the rig's
   chop clips (`npcs.swings`). The clash's sparks show, but **about 1.5 m off to the side of the blades,
   in the air** (the clash point looks wrong; to check next). Your own body isn't drawn from a placed
   camera, so the player's side of the duel isn't seen in third person.
   **Clash point (cloud, 2026-11-12; owner's brief: "the sword clash's sparks appear ~1.5 m to the side of
   the blades … the closest point between the two blade segments; both faces"):** fixed, BUGS.md #86: the
   point was the middle of the eye and the defender's body (or the two eyes), never on the blades. Each
   stance's raised blade is now data (`guard.stances.*.blade`, grip and tip from the feet, fitted to the
   raised clips and kept in step by a `clip_probe` test), and the point is where the swing's arc meets it.
   Not yet seen in game (a duel screencast on the local machine).
36. **A showcase video of the deer and wolves** (owner 2026-11-12: "have you observed the animals simulation? Make
   some clips of the deer/wolves behavior, really showcase it and try to get good angles. Keep it under 2 minutes
   of video. Splice it together into a single video."). Queued (local, after item 30).

   Made 2026-11-12: 85.6 s (sunrise herd with its stag: dolly, orbits, a follow, a drone rise; a wolf pack on a
   cliff at evening). Not in it: a chase or deer fleeing (on day 0 every pack is kilometres from any herd and
   players can't scare deer), far herds (none seen from above, unconfirmed), grazing (no grazing pose). Bugs found:
   BUGS.md (a herd popping up in size, deer overlapping, a wolf in a cliff, `?at=` into the ground).

   Owner, 2026-11-12: "fix four bugs found filming the deer and wolves" (BUGS.md #79–#82): births or a double
   spawn; no spacing; creature movement not keeping the body radius out of solid cells; `?at=`/teleport into the
   ground. Fixed (cloud, after 0.54.101): births paced by data (`near.mate.restS`, `groupEveryS`; no double spawn),
   bodies kept apart (`bodyRadiusM`, `limits.gapM`/`pushMps`) and out of walls, a buried player lifted onto the
   ground above.
37. **First-person arms from the player model** (owner 2026-11-12: "I thought we were going to swap out the AI
   generated first person character model? We discussed this yesterday."). The Peasant (the owner's Asset Store
   model) became other players' body and your shadow in 0.52.35 (CHARACTERS.md §11.5), but "Still rigid: your
   first-person arms … drawing them at the eye needs its own view and clips (later)" — the swap was never queued,
   so it was missed. Now: draw your first-person arms and hands from the Peasant's own mesh (its arm vertices by
   their skin weights), driven by the first-person clips (the swings fitted with clip_probe) mapped onto its arm
   bones, held items on its `hand.R`; the rigid arms retired. Priority: next after item 30. **Built
   (0.54)**: the arm cut from the body mesh by skin weight (`models.json`
   `firstPerson`), posed by the `fp_*` clips through the retarget map and scaled to the rig's reach, the
   held item on `hand.R`; `?fparms=rigid` keeps the old arms for comparison (CHARACTERS.md §11.10). Round 2: the fist round
   a handle as data (`grips`), the arm turned about the hand to show more forearm, and the held item on
   other players' skinned hands turned once, not twice.
   Owner, 2026-11-12: **"Im noticing my hands look like the new model, but my legs/body doesn't? What's up with
   that?"** Your own body seen in first person (CHARACTERS.md §5.1: legs, feet, torso when looking down or sliding)
   is still the old rigid parts (§11.5 "Still rigid"); only the arms moved to the Peasant. Next: draw your own body
   in first person from the Peasant too (the skinned body minus head, the arm cut hidden where the fp arms draw,
   the same clips as your shadow), the rigid body retired; `?fparms=rigid` keeps both old for comparison.
   **Built (0.54, cloud)**: your own body is the Peasant's skinned mesh less its head and arms, cut by skin
   weight (`models.json` `firstPerson.body`: `hide` the neck, head and left arm, plus the fp arm's joints),
   posed by your shadow's rig pose and placed as the rigid body was (`back`, `lookDown`, `clear`, `near`);
   no held item on it; `?fparms=rigid` draws the old arms and body together (CHARACTERS.md §11.12). Sliding,
   the Peasant's shorter legs come into view from 30° down (the rigid ones from 15°). In-game look still to
   be checked locally on a GPU.
38. **Wandering NPCs** (owner 2026-11-12: "Another item to queue up, wandering NPCs. They'll use the same player
   character model."). People using the Peasant model who wander the world like the animals do. Owner's answers
   (2026-11-12): wander, idle (look around, sit), turn to look at a nearby player, step aside when bumped — no
   talking or trading yet; a few near players by a data rule (count, spacing, biomes), simulated only nearby, like
   the animals; colour variations of the Peasant per NPC (palette remaps, seeded, data). Owner, later
   2026-11-12 (recorded in item 35): "Wandering NPCs will get swords." — for the NPCs: a held item slot by data
   (the sword, per NPC kind) drawn on the Peasant's `hand.R`, a disposition by data (neutral by default), a
   combat hook in the vocabulary; combat AI waits for the swords PR (a minimal fight-back once it is in).
   Built (cloud, 0.52.x): the villager as a creature with an `npc` block (idles stand/look/sit, look-at,
   sidestep, held, disposition, combat hook `fightBack`, colour parts), rules' `spacingM` and `biomes`, the
   server's behaviour, the `creatures.npcs` capability's NPC part on `Creatures`, the client's pose roles and
   colours; CHARACTERS.md §11.11. **Drawn (local, 2026-11-12):** each villager in its variant's colours (a per-body palette remap
   in the skinned shader) with its sword on the Peasant's `hand.R`, fist closed (CHARACTERS.md §11.11).
   **Round 2 (local, 2026-11-12):** NPCs posed from the rig's clips by role, as players are (`npc_rig_pose`),
   the sword in its level hold; `__dwell.spawnNpcs(n, radius, variant)` puts villagers round the player in a
   local world. Left: sit and look-around clips (the rig has none: both play idle); the NPC's raised guard
   and swings drawn from the server's fight. Combat with the swords built in item 35's round 2 (`npc.fight`: an NPC blocks its foe's wind-up by
   its stance rule, strikes back on the recovery, gives up after `giveUpS`; it never hurts a player).
   Owner, 2026-11-12: **"Hat gets its own colours"** — split the villager's hat into its own colour slot
   (hat colours: browns, greys, straw), separate from the hair, as data in the NPC variants. Not done in the
   combat-visuals PR: the Peasant GLB paints its hat, its hair and its boots with the same palette stop
   (`hair_brown`'s base, 2754 vertices from the feet to the hat's top), so a remap of one recolours all
   three; a hat slot needs the hat's vertices painted with a ramp of their own at import
   (shared/models `import.mjs`, by mesh or joint), then a `colours` part `hat` in shared/creatures. Queued. **Built (round 5, 2026-11-12):** models.json `repaint` (shared/models/coat.mjs
   `applyRepaint`, run by `import.mjs --coat`, checked by gen.mjs and `gen.test.mjs`) repaints the Peasant's
   hair_brown vertices past the head (brim) or above the hair (crown) to `wool_yellow`. That is a ramp
   the model uses nowhere else, so the hat now has its own slot. The villager's `hat` colour part picks
   straw, browns or greys (`wool_yellow`, `leather`, `bark`, `soil`, `flint`, `stone`), and the body
   remap now takes six pairs (`MAX_REMAP` 6). Seen in a group shot of eight villagers: each hat's colour
   is now its own, separate from the hair. The hat's band keeps the pack's orange triangle pattern.

   Owner, 2026-11-12: **"Villagers should be able to hurt players."** A villager's sword strike that connects on a
   player damages them (through the damage rules: a new cause, knockback, the player's raised guard blocks it by the
   same angle rule); modes without player damage (creative) are unaffected. **Built (cloud, 2026-11-12):** an
   NPC's blow past the player's block (glanced or connected) takes the weapon's `damage` through the damage
   rules, a new cause `Weapon` (6; `Fall` to a client whose `CausesHello` doesn't name it, as `Burn`), and
   pushes the player by the weapon's new `knockbackMps`/`liftMps` (shared/tools); a block recoils the NPC
   unhurt; creative neither hurts nor pushes; the clash goes to clients as before. Tests (both faces,
   `tests/weapons.rs`): an unguarded hit hurts and pushes, a raised guard across the blade blocks, creative
   takes none, an older client hears `Fall`. Not yet: the client's own hurt look for a weapon (it shows the
   generic damage flash), wolves biting players.
39. **Livelier animals** (owner 2026-11-12, after the video had no chase or fleeing — on day one every pack is
   kilometres from any herd and players can't scare deer): chose **deer flee from players** (they notice a player
   within a distance by data, closer when crouching, and run off; wolves keep their distance unless hungry),
   **packs roam toward herds** (hungry packs travel toward the nearest herd, so hunts happen near players), and
   **a grazing pose and more idle behaviour** (deer lower their heads to graze, look up, flick ears; wolves sit
   and lie down). **Built (cloud, 2026-11-12; ECOLOGY.md §8.2)**: `near.players` as data (notice distance by the
   player's stance — walking, crouching, still, running —, `flee` as a herd calming after `calmS`, or `avoid`;
   bold when hungry), a hunter's far `roam` (hungry packs head for the island's nearest herd) and near
   `hunt.trackM` (a near pack walks to a herd past its sense, then the chase), and `near.idles` (graze, look up,
   ear flick, stand; sit, lie) picked by weight for data seconds, with eating's and sleep's poses; tests on both
   faces and a fixture species by data alone. **Poses drawn (local, 2026-11-12):** the GLBs have only
   idle/walk/run clips, so each pose is joint turns on the idle clip as data (models.json `poses`, `poseBlendS`;
   CHARACTERS.md §11.6), sent as the `creatures.poses` capability's part of `Creatures` (after the NPCs'); the
   deer's graze, look up and ear flick and the wolf's sit, lie and ear flick are first guesses to tune by eye.
   Left: a new video. **Dev (2026-11-12):** `__dwell.spawnHerd('deer'|'wolf', size, offset)` puts a herd or
   pack by the player through the near ecology (a generated world: the flat world has no ecology). Tried
   in game: the deer bolt from the player at once (item 39's flee), and when the player moves off far
   enough for them to calm they fold back into a far record, so a camera can't film them idling near the
   player yet; the wolves stayed and idled (one read as sitting from a low camera). The poses are still
   unjudged first guesses: filming them needs a placed camera near a herd with the player out of its
   notice distance (a dev switch to make animals ignore the player, or `near.players` off) — next.
   **Round 4 (2026-11-12):** dev `animalsIgnorePlayers(true)` (the near animals see no player; a
   both-faces test) and `animalPose(name)` (every near animal drawn in one pose). Seen in game: +x on the
   deer's neck joints *raises* the head, so the first guesses were inverted; graze is now negative (heads
   down toward the grass, not quite to it) and lookUp positive (heads raised a little, alert). Both read now.
   **The wolves' sit and lie don't read:** a side view of a forced `sit` shows a standing wolf, so the leg
   turns have no visible effect. The joint names or axes need checking against the dog rig. Next.
   **Round 5 (2026-11-12):** the wolf GLB's joints listed: `spine.004` is the hips (the hind legs and the
   chest hang from it), and `spine`…`spine.003` is the **tail** (the first `sit` turned the tail). A pose
   can now lower the body (`$drop`, metres; models.json, `Grip::drop`, tested in `gen.test.mjs`), because
   turns alone can't bring the hips or belly down. Sit: the hips turned 35° front-up, the hind legs folded
   and the body dropped 0.3 m. Lie: all four legs folded and dropped 0.5 m. Seen in game: **the wolf reads
   as sitting** (chest up, rump on the ground, front legs straight) **and as lying down** (belly on the
   ground, head up).
   Owner on the first video: **"The recording was a really good start. I'd like to record at 9:16 1080/1920."** —
   the next video (after this item lands) is portrait 1080×1920, framed for portrait, with a chase and fleeing.

40. **Showcase recordings** (owner 2026-11-12: "We have a lot of visual features, list features that we could
   showcase with recordings. Ideally recordings are 1 minute or less. Don't include any text or anything in the
   video."). Made with the `record` skill (`.claude/skills/record/`): 9:16 at 1080×1920, h264 30 fps, a minute or
   less, no text of any kind, no music, clouds off unless the shot is about clouds, footage deleted after sending.
   **Showcase audio** (owner, 2026-11-13: "showcases shouldn't have music, but sound effects and ambiance are good";
   asked whether that meant the recordings: "Yes it's for the recordings"). Built 2026-11-13: every take records the
   game's own sound, effects and ambience, never the music or the UI (`__dwell.audioTap({ music: false })`, a parallel
   tap of the mix, the speakers unchanged); the record driver saves it beside the frames, take.mjs muxes it into each
   clip (aligned to the first frame, 48 kHz AAC), splice.py cuts, cross-fades and fades it with the picture
   (time-stretched on slow-motion parts, muted on time-lapses past 2×); a manifest's `audio` is on by default, per
   shot and per part overrides; theater mode's take takes `audio: true`. Outstanding: the first re-record with sound
   on the owner's machine (a browser check; the cloud session that built it had none), and the owner's listen.
   Video 1, the bifacial world (16), recorded 2026-11-12 (55 s, 7 clips; at the rim of seed 7 the edge is open sea on
   both faces, so the player flies over the edge rather than walking; BUGS.md #99 fixed for the third-person shots).
   Waiting on the owner to pick which come first. The list (suggested first: 1, 10, 3, 6, 11, 2 — the most striking
   that work today; 14 and 15 after item 39 and the swords/NPC work land; 5 after the moon trip is playable):
   Owner, 2026-11-12: **"Preferably some of each recording should be gameplay, player perspective."** — every
   video mixes cinematic shots with first-person gameplay (the arms and held item, actually playing).
   **Owner, 2026-11-12: "Start on oldest features first for showcasing."** Order: the oldest features first — the
   bifacial world (16), water and underwater (7, 17), rivers to the sea (18), day to night (3), building (13), dynamite
   (12), the world's scale (20), biomes (6), then the newer ones. Recorded with theater mode, 9:16, no text.
   1. Volcano and lava — the summit lava lake at night with bloom, crust plates and orange seams, steam where lava
      meets water, a lava river cooling from bright to dark.
   2. Rainier-style peak — a drone rise up the cone past snow lines, cleavers and the glacier cap, ranges fading
      into the haze.
   3. Day to night — a time-lapse from golden sunset to night, the moon rising with its glow, stars appearing.
   4. The moon up close — spyglass crescent, half and full, crater rims lit along the terminator.
   5. Flying to space (and the moon) — the sky thinning to black with stars by day; later the flight to the moon
      and landing at ⅙ g (after M-7c, the client side).
   6. Biomes tour — low passes over the red mesa canyons and arches, pine forest, palm coast, meadow tall grass,
      steppe, the old-growth forest.
   7. Water — rivers to the sea, waves on the coast, the underwater view up through the surface.
   8. Volumetric clouds — through and above the deck at sunrise (after the cloud lighting rework, item 13).
   9. A tree growing — sapling to mature oak, pine and palm in time-lapse, the trunk thickening.
   10. Chopping and felling — axe bites into a notch, chips, the tree tipping, the V gash, the trunk settling.
   11. Digging and mining — shovel scoops, pickaxe hits shattering stone into shards, cracks spreading.
   12. Dynamite — a fuse, the blast, blocks flung as pieces, a chain reaction.
   13. Building — placing blocks and a saved structure from its ghost preview.
   14. Deer and wolves — a grazing herd, deer bolting from the player, a wolf pack hunting (after item 39).
   15. Villagers — wandering in their outfits, turning to look at you, a sword duel showing block vs connect
       (after the NPC look and swords round 2).
   Owner, 2026-11-12: **"Some of these would look good as full timelapses, or partial timelapses. There's older
   features we haven't showcased yet too like the bifacial world, The underwater. It's probably ideal to include
   the player in the videos when possible. Rivers flowing from mountains to the sea."** So: time-lapse where it
   suits (a whole clip sped up, or a normal-speed shot that speeds up mid-way: day to night, tree growth, lava
   cooling, grass spreading, clouds); the player in frame when possible (a third-person camera on the player's
   body, or first person with the arms and held item); and three more:
   16. The bifacial world — flying out past the rim and over the edge to face B, gravity turning, the other
       face's sky, the world seen from beneath.
   17. Underwater — diving in a lake or the sea, the surface from below with the sky through it, light shafts;
       with the water transition itself (owner 2026-11-12: "Water transition as part of the underwater showcase."):
       the camera passing through the surface going in and coming back out.
   18. Rivers from mountains to the sea — a long follow down a river from the peaks through valleys to its
       mouth (partly time-lapsed).
   19. Multiplayer (owner 2026-11-12: "Multiplayer we haven't showcased.") — two or three players in one world
       (a friend world or a local server, each a browser page): building together, one chopping while another
       carries, a sword duel between players' stances (no damage between players), seen from a third camera.
   20. World scale, measuring a mountain (owner 2026-11-12: "A video with mountains showing the full world scale
       showing like a measuring of a tall mountain. (Text could be in that as part of the "measure" effect, just
       no description or titles)."): the player at the foot of the tallest peak (~5000 m), a measuring line or
       ruler rising up the mountain with height numbers ticking as part of the effect (the only text allowed),
       pulling back to the whole range and the island's scale. The text exception is the measuring effect's
       numbers only — no titles or descriptions. Owner: "Measure in meters" (e.g. 1000 m, 2000 m … ticking up).

   Video 1 (bifacial) owner review, 2026-11-12: **"That bifacial video is pretty bad. The cool part of the bifacial is
   how the world looks from the side far away showcasing the world outline. What it looks like to go down a portal.
   Etc. give me a shot plan before doing that."** — every showcase gets a shot plan approved before recording.
   Shot plan v2 (bifacial): (1) the outline edge-on from far beyond the rim, a slow push in; (2) a crane from
   edge-on over face A and under to face B; (3) first person walking to a portal; (4) **jump** down the shaft (owner:
   **"Dont climb down the ladder, jump through the hole. And after you go past the glittery particles it should switch
   to showing the other side as the player flies up out of the hole."**) — past the portal's motes the view flips
   and the player rises out on face B; (5) out on face B, a drone rise back to the edge-on outline. Defaults while
   unanswered: face A golden hour, face B night; clouds on for the far shots, off for the portal.
   v2 review, owner: **"The camera shouldn't flip on the wide shot, show it's upside down."** — the wide shot of the
   player coming out on face B keeps face A's up (the camera never takes face B's frame), so face B and the player
   rising out of its hole read upside down, hanging under the disc. Clouds off in every shot (owner: they "might look
   v4 review, owner: **"The camera is upside down for the out of the hole shot. I just wanted the not upside down for
   the initial wide shot so you could tell it was a different side without the disorientation."** — only the
   initial wide (the crane from face A round under to face B) holds face A's up; the out-of-the-hole shot on face B
   uses face B's own up (upright, `up: "auto"`).
   v5 (one-command tool #853; full record ~5 min, a one-shot fix 2 min 34 s): owner **"Bifacial v5 looks great saved
   it."** — video 1 done; its clip cache cleared. Next: video 2 (water and underwater), shot plan first.
   Showcase audio (owner 2026-11-12: "Can you record audio output from an application directly? Instead of trying to
   record via microphone?" → explained: the game's WebAudio master can feed a recorder inside the page; owner:
   **"Yeah showcases shouldn't have music, but sound effects and ambiance are good."**). Build: the recorders capture
   the game's mix with the music bus muted (effects + ambience kept) — the in-game recorder as a second track, the
   screencast takes via a MediaRecorder of the master (minus music) written alongside the frames and aligned in the
   splice; the manifest's audio on by default for showcases. Queued after video 2 and the Realistic steps.
   Video 2 plan given (coast glide, river to the sea, wading in, the transition, underwater swim, surfacing at
   sunset). Owner on the underwater look pass (#860), 2026-11-12: **"Water looks good"** — approved; video 2
   recorded with `take.mjs water` (manifest `.claude/skills/record/videos/water/`, seed 7's coast at ~11700, 19500). Owner sent a look reference: **"Underwater reference photo from the game abzu for pretty underwater with
   transparent surface, light rays etc."** (ABZÛ: bright turquoise water with a long clear view, a bright see-through
   surface from below with the sun's disc and Snell's window, strong volumetric light shafts, caustics, a colourful
   lit floor). Before recording video 2: an underwater look pass toward that reference (WATER_RENDERING.md W4's
   pieces exist: fog, Snell's window and TIR, god rays, caustics — tune and strengthen them), the owner's look
   check, then the video.
   Video 2 v1 review, owner 2026-11-12: **"Water video for waterline should go straight down slowly into the water.
   Should do sunset for the water surface fly over. Water should include a river follow. The water video should
   include sound."** So v2: the wade replaced by a camera going straight down slowly through the surface; the
   coast fly-over at sunset; a real river follow (a camera tracking down a river through stepped pools); the
   game's sound (effects and ambience, no music) on every shot. Recorded 2026-11-12 (57 s, 5 shots: sunset coast,
   a seed-7 hill river followed down its valley — it ends in a lake, no seed-7 river reached the sea in the scan —,
   the waterline crane, the underwater swim, treading water at sunset; sound lifted 26 dB in the splice). Waits on
   the owner's review.
   **Video 2 v2 approved and saved** (owner, 2026-11-13: "I've saved the water video, looks good. Good to move on to
   the next.").
   weird", confirmed by the v2 comparison).
   **Built 2026-11-12:** a keyframe `up` ("a" | "b" | "auto", default auto) holds a face's up, roll and look frame
   across the midplane (THEATER_MODE.md §3.1; presets and the keyframe editor's Up take it); the next take uses
   `"up": "a"` for the face-B wide.
   Owner: **"Also it looked like the player flew out of the hole instead of physics carrying it out of the hole. Like
   was flying."** — no flight in the portal shot: momentum alone carries the player up out of face B's hole (v2
   turned flight on at the top of the rise). **"Face b should be night."** — keep face B at night.
   Portal exit (owner's answer, 2026-11-12): **land beside the hole** — today a player rises ~12 m out of the far
   face's hole, falls back and bounces between the faces forever; the portal gives a small sideways push (or slows
   the fall-back) so the player arcs out and lands on the far face's ground beside the hole.
   **Built 2026-11-12** (BIFACIAL_WORLD.md §12.3): the shaft's **exit push**, a steady crosswind across its top
   2 m (`midplane.json` `portals.exit`: 30 m/s², `minRise` 10 m/s so a jump and the ladder never take it), along the
   player's facing; however fast the rise, the player lands ~6 m from the axis (2–4 m past the rim). Momentum in the
   shared controller (`step_portal`, from `VoxelWorld::portals` on the server and in the prediction: no replays).
   A far opening the swing can't reach, or a rise into the monument's arches, falls back and comes out on a later
   swing (seed 0: 12 of 16 drops out on the first, all within 3 crossings). For the portal shot: face between the
   pillars (a diagonal) when jumping in.
   Owner refinement for (4)/(5), 2026-11-12: **"Like a farther out non first person view of the other side."** — after
   the motes, cut to a third-person camera farther out on face B (15–40 m from the portal, looking at the hole) as the
   player flies up out of it, not first person; the jump stays first person on face A.
   Owner, 2026-11-12: **"Evaluate the far shots, they might look weird with the clouds on. Also the clouds look
   ridiculous, at least the volumetric ones. Look like crazy triangles."** — clouds off for every shot unless on
   clearly reads better. Checked: from beyond the rim both tiers draw a cloud layer in the void under the camera
   (BUGS.md #101), so v2 is clouds off throughout.
   Owner, 2026-11-12: **"fix BUGS.md #101 (clouds draw in the empty space beyond the disc's rim) and #102 (a bright
   horizontal band in face B's sky in every far shot)"** — clouds only over the disc (masked to the world's radius
   plus a margin as data, fading out near the rim), and the band's source found and fixed. Done (after 0.54.191):
   the deck and the volume stop at the rim (`clouds.json` `deck.rimMarginM`/`rimFadeM`); the band was the
   atmosphere's limb, now faded out past the rim (`atmosphere.json` `limbFadeM`). The look from the far camera is
   still to be checked on the Mac; far shots can take clouds on again once #100's shapes are in.
   **Video 1 v2 recorded 2026-11-12** (48.7 s, 6 clips, 17.8 MB): (1) the disc's outline from 160 → 118 km out,
   9–13 km up, a slow push; (2) an orbit-crane from over face A through the midplane to under face B at night; (3)
   first person walking between the monument's pillars to the hole and dropping in (no ladder); (4) the fall past the
   motes at 0.35× speed; (5) a hard cut to a camera 38 m out over face B looking down at the hole as the player flies
   up out of it, hovers, flies out and lands; (6) a drone rise from the player on face B past the monument and the
   coast to the whole disc from beneath. Paths kept with the plan (the record skill's lessons). Seed 7's portal
   at (1780.5, 20690.5) (`?spawn=portal`).
   **Video 1 v3 recorded 2026-11-12** (51.4 s, 6 clips, 17.9 MB, clouds off throughout, no flight): (1) the outline
   push as v2; (2) the crane over face A and under to face B with `"up": "a"`; (3) first person walking to the hole
   and dropping in; (4) the motes at 0.35×; (5) a hard cut to a wide 22 m out on face B with `"up": "a"`: face B's
   ground overhead, the monument's pillars hanging down, the player rising head-down out of the hole by momentum
   alone, then falling back in; (6) a drone rise from face B's hole (face A's up held) out under the disc and round
   the rim to the face-A outline, closing the loop on shot 1. Not in it: the landing beside face B's hole — in the
   browser the exit push never acts on face B (BUGS.md #107; natively the same shaft lands 6 m out), so the clip
   ends as the player falls back. Paths in `.claude/skills/record/paths/bifacial-v3/`.
   **#107 fixed 2026-11-12:** walking in, the player fell into the shaft's wall and the push (then under the opening,
   in the shaft) went into it; the push now acts in the 2 m over the opening. The browser check is for the next take.
   **Video 1 v4 recorded 2026-11-12** (54.3 s, 6 clips, 18.8 MB, clouds off, no flight): v3's shots with the landing.
   Browser check passed: 5 of 5 drops rose ~12 m out of face B's hole and landed 7.3 m from the axis on the first
   swing. Shot (5) is a camera 14 m out and 13.5 m up on the landing diagonal (`"up": "a"`): the player rises
   head-down, arcs out and stands on face B's ground overhead beside the hole; (6) the drone rise starts from that
   camera. Paths in `.claude/skills/record/paths/bifacial-v4/`.
41. **Theater mode** (owner 2026-11-12: "We should use full camera tools to do the things you were doing and expand
   this tool set as you go, we could call it theater mode. Even make it available ingame as a feature that could be
   toggled on for any world type."). Answers: **both** presets (pick a target and a move: orbit, dolly, track,
   rise, pan, with sliders) and keyframes (free-fly the camera, drop keyframes with position, look, time of day and
   speed, choose the move between them, preview and play; paths saved as data to replay and share); **record to a
   file** in the browser (the canvas without the HUD, 9:16 or 16:9, chosen frame rate, downloadable); in
   multiplayer **their camera only** (the world keeps running, their character stays put, time of day and
   time-lapse apply to their view only); **anyone, any world** (a Settings toggle or a key; the host can disable it
   for guests). The `record` skill drives the same tools (a bench `camera` step plays a path and records it), and
   the tool set grows with each video. In progress: design + logic (cloud), then the UI and recording (local).
   **T-1 built (2026-11-12):** the design (`docs/THEATER_MODE.md`, phases T-1 … T-5) and the camera path engine
   `crates/dwell-theater` (paths as JSON: keyframes, the moves cut/linear/ease/orbit/track/dolly/crane/pan and
   easings, the viewer's time of day and speed ramps; presets as data in `shared/theater/presets.json`; tests on
   both faces), wired into the client (`Game::step_theater`), `__dwell.theater`, the bench's `theater` step (the
   name `camera` was taken) and the record skill. Not yet run in a browser (cloud session, no GPU). Next, locally:
   T-2 the mode and editor, T-3 recording; then T-4 the creative viewpoint (designed in THEATER_MODE.md §7: the
   `theaterViewpoint` ability, server-checked) and T-5 the MCP server.
   **T-2 and T-3 built (2026-11-12, `client/src/theater/`):** F9 / Settings → Theater / the right stick press / a
   touch button toggle it (key and tunables in `shared/theater/theater.json`); the free camera (`theaterFly`, both
   faces); the HUD switch with an audit test; the toolbar's Presets, Keyframes (timeline, save/load, export/import)
   and Record (9:16 or 16:9, 720–1440p, 24/30/60 fps, MP4 or WebM download) panels; `__dwell.theaterMode`.
   Outstanding: the host's `theaterGuests` world setting (protocol), following a creature by id, the offline
   frame-exact recording mode, a playtest on a phone.
   Owner, 2026-11-12: **"For players to use it would be cool if it had an MCP server so they could use an LLM agent to
   handle the recording complexities, not sure if that would help us at all but it's a to have."** A nice-to-have,
   after the in-game tools: an MCP server exposing theater mode's operations (find subjects, set time of day,
   place/preview/play a path or preset, start/stop recording, list saved paths) so a player's LLM agent can direct
   shots. It would help us too: our own agents could record through the same tools instead of bench scripts.
   Likely shape: a small local MCP server (stdio) that talks to the open game page over a local WebSocket the page
   opens only when the player turns it on; nothing reachable from outside the machine.
   Owner, 2026-11-12: **"There may be a feature we need for theater mode that it uses the camera's position for
   simulation, perhaps only available in creative mode. Sets us free from the player but can't be used for cheating
   in adventure."** So: in creative (a mode ability as data), the theater camera becomes a simulation viewpoint —
   the server streams chunks and LOD around it, spawns and simulates creatures and NPCs near it, and runs the near
   ecology and water there — so a shot can be far from the player's body; in adventure the world stays centred on
   the body and the camera only sees what's already loaded near the player (no scouting). The server accepts the
   viewpoint only where the world's mode allows it. Owner: **"Still want theater mode for adventure, just without
   camera-relative sim."** — theater mode itself stays available in every mode; only the camera viewpoint is creative-only.
   **T-4 built (2026-11-12, cloud):** the creative simulation viewpoint (THEATER_MODE.md §7.1): the
   `theaterViewpoint` ability (creative only, `shared/modes/modes.json`), the `theater.viewpoint` capability and
   the `TheaterViewpoint` message (the playing path's eye, at most 4 a second, a clear when it ends); the server uses
   it only in a mode with the ability (checked on the server, never trusted from the client), within 4096 m of the
   body: chunks streamed around it from the same budget, creatures spawned and kept, the near ecology, lava and
   water stepped there; animals don't flee it. Tests on both faces (creative streams and simulates, adventure
   ignores, no capability ignored, a clear ends it). Not yet run in a browser. Next, locally: T-2, T-3.

42. **The sun as a real world** (owner 2026-11-12: "Add this feature to the queue, the sun is an actual world like
   the moon. Follow similar concepts as the moon. The sun is a volcano world. Constant flowing lava, oceans of
   lava."). Queued. Built on the moon's systems (MOON_WORLD.md): a grid with its own frame in grids.json, a
   generator (volcanic: cones and rifts, lava oceans and rivers always flowing — the lava fields of VOLCANOES.md
   kept molten by the world's heat, basalt and obsidian crusts), the far-body renderer drawing it from its bake as
   today's sun disc (same size in the sky, its glow and sunlight unchanged — the bake shapes the surface, the lava
   gives the light), its place in the celestial model (the sun's path today), flight and capture like the moon.
   Owner's answers (2026-11-12): **"The sun's doesn't really light the earth world or moon world, just
   conceptually."** (sunlight stays today's light model; the sun world's lava doesn't light other worlds);
   **"Players can land there."**; **"Lava has the same function on the sun as the earth world."** (same damage,
   burn, flow and cooling rules); **"Its gravity will be the same as earth world for now."** Queued after the moon's
   client side (M-7c) so it reuses the finished travel.
   **First slice S-1..S-3 built (2026-11-13, brief: "the sun as a grid in the grids data with its own frame, size
   and place on the sun's existing path … a volcanic generator by data … the far-body bake so the sky's sun disc
   is drawn from the sun world's surface (its glow kept) … capture/release and travel by the moon's rules if
   they generalise by data (say what doesn't)"; docs/proposals/SUN_WORLD.md):** `dwell:sun` in grids.json
   (`sunPath` frame on today's sun path at 954.7 km, 25 km radius at the disc's 1.5°, gravity 1), the `volcanic`
   generator (lava oceans, rift rivers, cones with crater lakes, basalt and obsidian; both faces), its bake
   shading the sky's sun disc (`sun.surface`; size, glow and sunlight unchanged), travel by the moon's rules
   natively. **Left:** the sky disc's look on a GPU (a local check); constant lava flow on the sun (lava fields
   on its rivers and lakes, damage there), its sky, spawns, its LOD.
   **Kept additive (orchestrator, 2026-11-13: "to keep this additive (no new line), gate the sun's travel on the
   client: add a capability (e.g. "grid.sun", or a grid count in the capability exchange) and have the server
   never capture a client that doesn't declare it into the sun's grid (it flies on as before, the sun stays a far
   body for it); a test that an old client (no capability) near the sun isn't captured and a new one is"):** a
   grid's optional `capability` in grids.json (the sun's `grid.sun`); the server captures only sessions that
   declare it, and the client's travel map holds only those grids.
   **The sun's sky** (owner, 2026-11-13: "Sun and moon should be space, the same as night sky. Could perhaps just
   make space a more general skybox and have 'day' be a closer atmosphere skybox so it can fade out as you leave
   earth."): the general sky is built by the moon session (claude/moon-m5), not here; the sun only declares in its
   grid data that it has no atmosphere, once that field exists.

## Done

- **Underwater audio (owner 2026-11-12: "swimming underwater sounds like swimming on the water surface, when
  underwater sounds should be muffled, high pass filter maybe?"; and "Swimming in the surface is too loud and high
  even then, like a kid learning to swim splashing."):** BUGS.md #104. Strokes by the head's depth (a soft, deep
  `swim_under` with the head under, a calm, filtered `swim` at the surface, one a second); the world's buses muffled
  by a low-pass with the eye under water, eased at the crossing, plus an underwater bed; all data (AUDIO.md §3a). The
  listening check is the owner's.

- **Tall grass steps (owner 2026-11-12: "Walking through tall grass is really loud and obnoxious. Bad sound effect for
  it I think, it's also playing really fast. ... the feet are moving really fast, way faster than normal legs
  step/walk. And there's an occasional crackly sound too, are there 3 SFX playing while walking on grass?"; brief: one
  footstep per step plus at most a soft brush, never per tick or per cell, a voice limit per sound, the walk/sprint
  clip at ground speed ÷ stride, ~1.8–2 steps a second):** BUGS.md #103. A rustle on each footstep only (data
  `brush`), a softer `leaf_brush`, per-sound `voices`/`minGapS`, strides walk 2.5 m / run 2.8 m and the gait clips
  two strides a loop. The listening and the look on a GPU are the owner's.

- **dwell-mesh's face-B parity test green (owner's brief 2026-11-12: "a_chunk_meshes_the_same_face_local_on_either_face::b
  fails on main (\"cutout plants\"), reported by two sessions … fix the cause at the source so face B's plants are face
  A's mirror … keep the density setting's behaviour"):** BUGS.md #98. Not the Tall grass density (6578783: cleared,
  the test failed before it, and `plant_kept` hashes world cells only for a 0–100 % thinning); a leaf card's packed
  normal drifted a grid step on face B's mirror. The packing's grid is now even, so face B's cards are face A's
  mirror to the bit.

- **Main's tests green again (owner 2026-11-12: "make main's tests green again (tests/fixes only)"):**
  four stale expectations, no code bugs. `checks.ts`'s areas lacked `shared/look/bloom.mjs` (render) and
  `src/theater/` (ui); the textures test's known palette deviations gained obsidian's near-black shadow
  and lava's self-lit top (the lava look); the client NPC test expected four colour parts, five since
  the hat got its own (8a5f8a1). `hosting.test.ts` and `hostPanel.test.ts` failed only without the
  wasm package (`npm run build:wasm`).
  **Still red, found along the way (not fixed here):** `dwell-client`'s
  `game_local.rs` `a_long_trip_over_the_terrain_keeps_the_near_ring_at_full_detail` (BUGS #54's test): a
  near chunk waits 277 frames (bar 120) at frame 4619, chunk (63, 14, 224); deterministic, and the same at
  every commit back to 588e896, so older than this checkout's recent history; needs its own look.

- **Axe swing directions and animation tools (owner 2026-11-11: "The axe animations are still trash.
  They don't swing in the right direction at all. Make yourself some tools to properly observe
  animations"):** the clip probe (`cargo run -p dwell-client --example clip_probe`, DEV_TOOLS.md "Clip
  probe and clip strip": a clip's points over time through the game's own posing, in first person,
  top, side, front and third-person views, as a table, CSV or an SVG panel sheet) showed the level
  strike going bottom → top and left → right on screen and both wind-ups taking the head behind the
  eye. The arms' chop clips were refitted to head targets (level right → left, vertical top → bottom,
  through the crosshair at contact) and the body got its own chop clips; pinned by
  `clip_probe::chop_swing_tests` (red on the old clips, both faces in the world). The bench's
  `clip-strip` step (K frames of one swing from first person and two third-person cameras, one
  contact sheet) is for the local browser check, not run yet. Outstanding: that browser check; remote
  players still see a `break` (the wire's swing byte has no chop kind).
- **"Starting the graphics" frozen (BUGS.md #71):** not a main-thread task: on a cold Metal shader
  cache the GPU process compiled the renderer's 38 pipelines for ~9 s with the page painting no frame.
  Now the compile is its own stage (`shaders`), the pipelines are lazy (`render/pipeline.rs`) and
  warmed one a frame behind a queue fence while the loading screen is up, tiers that are off never
  built: the longest no-paint gap after `create` 9.0 s → 0.68 s, playing 15.1–15.6 → 14.1 s cold.
- **Starter world on iPhone (P0, 0.52.68, BUGS.md #70):** the tab reloaded while loading the terrain.
  Every generator of a many-biome world built its own whole-map biome grids (20 sets in a job worker);
  now one per map and process (`terrain::shared_biomes`), and the LOD coarsens by its overshoot so a
  phone's loading view fits its 96 MB within seconds. Bench (`memory-phone --touch`): starter 1,467 →
  850 MB at load, default 746 → 676. Owner to check on the iPhone 17. Then (pending release) the grids
  shrunk exactly: stored by their exceptions over the sea (`grid::SparseGrid`), the drainage's height
  copy read from the world map, scratch dropped early: 53.6 → 15.9 MB a process natively
  (`tests/starter_memory.rs`, with the starter's terrain pinned bit for bit). Left: re-measure the page
  on the bench; one copy per process remains. The pin was re-recorded 2026-11-11: it had been taken on code from
  before the steppe's `undergrowth` 1 → 0.75, so it failed on main (stale, not a float difference: with
  the steppe at 1 the old hashes match).

- **Terrain leftovers, round 1** (0.52.66): arches from erosion-grown fins (a biome's `arches.fins`, the
  fin rule in T4's cut-back; sites slide to a fin within 12 m) and arches in a chosen rock
  (`arches.rock`); the starter's mesa 1 arch / 15 overhangs → 10 / 6 (12 km square, face A). The mesa's
  red layers at height: `dwell:red_sandstone` as sandstone's face and cap, and a biome's own stack shows
  its bed on walls and standing tops at any height (grey band stone over ~700 m before; ~64 % → ≥ 85 %
  of high mesa tops show the beds). Not yet seen in game (no screenshots this round).

- **Starter world** (0.52.54, WORLD_GENERATION.md): `test:starter`, a candidate default from the
  systemic terrain layers and eleven biome presets placed by climate, for the owner to react to at
  `?worldgen=test:starter`; fixture worlds may carry their own biomes. Left: in-game screenshots of the
  tour, per-cone heights for the arc, palms. Next: the leaves (item 5). 0.52.63 (leftovers round 1): the biome
  balance (broadleaf up, boreal and steppe down), the pick's `sharpen` (less mesa/badlands speckle), the
  boreal spot's pines, the arc's cones falling off (`arcDecay`), Chisel's "Open
  starter world", the tour printing a mesa spot (its strongest sample), a
  biome's `decorations` read. 0.52.64 (round 2): the New world form's **World** (Default, or a world
  file offered as data, `newWorld`: Starter), recorded as the world's `worldgen` so it reopens and guests
  get it; a world's `cliffs` (the terrain volume's strength, 0 none) and the starter without cliffs (seen
  in game at 1, 0.5 and 0 at the steppe and the broadleaf spots: stone and dirt walls across the forest at
  1 and 0.5 alike; off, the hillsides read as grassy slopes with oaks); the tour spots looked at in game
  (the volcano view from 15 km east, facing west: the cone fills the view over red badlands, a black
  vertical seam on it; the wetland spot is a grassy slope with no water in view: the river is ~150 m
  off; the mesa spot stands on red ground under a grey stone ridge). Left: a wetland tour spot by its
  water, a mesa spot that reads as a mesa, the page stalling on a fresh load at a far spot when a long
  script waits on it (twice; a new tab loaded), spacing per biome, players' structures as sources, the
  in-game editor, the textures tab extras. 0.52.65 (round 3, the section's last): spacing per biome
  (a biome's `spacing`: placement → blocks, the anchor's biome's; default unchanged); the tour prints a
  wetland spot by its water (`at=-14900.5,36,-20300.5`: a wide grassy plain with oaks under the range; the
  water lies behind, the look can't be turned by link) and a mesa spot under 700 m with walls (`at=
  14500.5,668,400.5`: inside a red canyon, a wall fills the view; higher mesa columns show the grey
  province rock, not the red stack); the volcano's dark column narrowed (BUGS.md #69: in the LOD, not a
  skirt, not a height spike). **The biome section is done** except, for the owner's design input: the
  in-game Chisel editor; the textures tab extras (new materials and blocks by data, water and sky
  colours, item and model textures); players' saved structures as placement sources (plan in ADR 0074);
  BUGS.md #69.

- **LOD bugs** (leftovers section 3, after 0.52.92): BUGS.md #68 fixed — the holes round the tall peak's
  crater were the renderer's far pass clipping terrain over a fixed 1024 m top from above it
  (`far_pass_near`, `drawn_top`); `?loddebug=levels` tints sections by level. #69 narrowed to the
  distant shadows (gone with `?lodshadows=off`; a world-fixed streak along the low sun on the cone's
  z, not the sweep's max mips; suspect a thin caster on that line); still open.

- **Underwater at night** (0.52.53, WATER_RENDERING.md §16.1): Snell's window shows the sky pass's own
  sky (one function: the stars, the moon, the sun, the clouds), no white-cyan glow over it, fewer cells;
  the water seen from inside takes the moonlight's grey as the scene above does. Owner, 2026-11-11:
  "Check the underwater update at night, and you should be looking from just outside the water up
  compared to just under the water up, because that's the mismatch."
- **Underwater surface** (0.52.49): Snell's window as data (`underwater.json` `window`), the refracted
  sky, the sun and moon discs, glow by the sky's light; total internal reflection outside it. Not yet
  seen in frame: the night moon disc and the sunset sun disc from below. Owner, 2026-11-11: "We need to
  make underwater surface layer look transparent like the surface and look more accurate to what light
  is shining down."
- **Creative breaking fix** (0.52.51): in creative anything but a tool breaks every block in one hit
  (predicted); a tool keeps its hits; adventure unchanged.
- **Tree Growth prototype** (0.52.50, and the owner's later edits in 0.52.52): oak, pine and palm as
  data, saplings growing in height, trunk width and crown together; a 1.8 m player for scale, seed
  variety in height, crown and tint, and block-grid palms as the default (a toggle shows the free
  lean).
- **Tool grip** (0.52.52): the hand's grip as data (props.json `hand`: the fist's centre and grip axis
  in the hand bone's frame), each tool's handle axis measured from its GLB (`handle`), the handle laid
  through the fist and rolled by data (`hold.roll`); the lean toward the crosshair is the hand's pose
  (the `tool` hold); full size. The stone pickaxe and the wooden shovel wear like the stone axe (32
  hits; creative doesn't wear, metal tools are unlimited).

## Smaller items, after the queue

- **The full-sky flood test on main (2026-11-13, `claude/flood-sky-test`), done.** Owner: "`cargo test -p dwell-mesh --lib
  flood_tests::open_terrain_takes_the_full_sky` fails on main (face a: 3 dark vertices) … find which merge broke it, decide
  whether the 3 dark vertices are a real lighting bug or a fixture made stale by an intended change. Check face b too."
  A stale fixture, not a lighting bug: a first-parent bisect of #823..#885 puts it at #873 (resurrection shrines), which
  moved every spawn to a shrine, so the test's window around `spawn_point()` slid onto the shrine's levelled ground. The
  3 vertices are a ramp at sky 12–13 straight under a tree's crown, correct shade. That window also has no open
  terraces: with the ramp-corner fix the test was written for (`Flood::shaped`) disabled, every vertex it darkens there
  is under the same crown, so no filter that lets crown shade through could tell the two apart. The test now centres on `spawn_search_point()`, the open ground it was written for: green on both
  faces, and red again (face A, 13 dark) with `Flood::shaped` disabled, as face B was before #873. Face B passed on main.

- **Downhill water's steps (BUGS.md #92; owner, 2026-11-12: "I'm seeing some edge triangles and thin lines at slope
  transitions on downhill water.")** — fixed 2026-11-12 (`claude/water-steps`, WATER_RENDERING.md §18): the open
  cells at a river's ½ m steps are step cells whose corners are shared with the reaches above and below, so a river
  stepping on the diagonal is one continuous sloped surface. Next (local): the owner's spot in the browser.
- **Leaves rubber-band (BUGS.md #94, 2026-11-12, `claude/leaves-rubberband-dy5tni`), done.** Owner: "seems to be a mismatch
  between server and client for leaves collision. Jumping into the leaves of a tree makes you rubberband falling to
  the ground endlessly until you back out of the leaves." The server's buried-player lift took leaves (solid but
  passable) for ground; it now uses the terrain collision's own test. Tests in `netcode` and `buried` (both faces).
- **Realistic and forest look (2026-11-12, `claude/realistic-forest-look`), seen in Chrome on the GPU** (Realistic
  seeds 1–3 face A, `test:forest`; stills, a contact sheet and a 9:16 video handed to the owner). What reads: the big
  map (M, widest level) shows the plates' story at a glance — one continent a seed, snow-capped ranges, a green wet
  coast on one side of each range and a red-brown dry interior on the other (the rain shadow), hotspot chains; in
  world, a range behind a coastal plain, red badlands with mesas under a jagged range on the dry side, the
  tall-broadleaf rainforest at seed 1's spawn; the old growth at ground level (tall pines and oaks to a beach edge) is
  the best-looking forest yet. Issues, for later:
  - **Land is one continent in an empty ocean** (~15–20 % of the disc each seed) and the island arcs are
    **uniform-width green ribbons** with no relief or breaks — they read as ropes, not arcs of islands.
  - **Range cores are flat plateaus** on the F4 height overlay and in the LOD (broad slabs, few ridges or valleys);
    steep far LOD faces show **moiré stripes** (seed 3, the cone across the strait).
  - **Dry interiors dominate** (more brown than green on all three seeds), and lowland grass reads as one flat lawn
    at LOD, with no forest patches beyond the near range.
  - **The rainforest is open, not closed:** tall-broadleaf "lollipops" spaced on lawn read as savanna from above
    (seed 1 spawn); it wants a closed canopy and undergrowth like the old growth's.
  - **Far old-growth trees lose their crowns at LOD** (`lod_trees.rs`'s coarse cells): beyond the full-detail range
    the forest becomes lawn dotted with brown trunk stubs — a hard edge from 150 m up (`forest:above`) and across the
    canopy (`forest:edge`). Item 31's impostors (I-2/I-3) are the fix.
  - **`forest-perf` at High on the owner's Mac** (first GPU run): GPU frame p50 14.7 ms inside, **24.7 ms at the
    edge** (≈ 40 fps), 14.4 ms above; CPU ~2 ms everywhere. The edge view (the whole canopy at once) is over budget.
  - The F4 overlay (height only) and the big map (M, materials) are drawn in different orientations; the big map has
    no climate or plate overlays yet (q8).
  - A `?at=` y at the forest stand's ground (40.5, −39.5) put the player inside the ground or a crown twice; the
    bench's `above` spots land right, so use `--spot` or the world's own spawn for the forest.

- **Chisel's world knobs take every key gen.mjs takes (owner 2026-11-12: "fix a failing test on main": `state.test.ts`
  "opens the starter world as a valid edit" failed with "tectonics.lavaVents is not a world knob"; "make Chisel
  accept every key the generator's world config accepts — ideally derived from the same schema … so new keys never
  break it again"). Done (`claude/chisel-world-knobs`): `validateWorldKnobs` (shared/worldgen/biomes.mjs) takes the
  knob groups' list keys from `TERRAIN_LISTS` (`WORLD_LISTS`, checked by the same checker gen.mjs uses, over
  terrain.json's group), as the tectonics' ranged keys already came from `TERRAIN`; a test validates every world
  file's knob-group overrides as a Chisel edit. Still open on main: `textures.test.ts` "flags colours off the art
  direction" lists `obsidian.shadow` and `lava.light` off the palette (an art-direction call for the owner).
- **Chop check (2026-11-12, `claude/chop-check`), items 24–26 seen in Chrome on the GPU** (`fell-tree` and scratch
  scenarios on `?world=flat`):
  - **One swing after the fall wipes the gash (item 24).** At the 11th swing the stump is an upright ramp low at the
    notch and the trunk's foot an upside-down ramp, as built; a 12th swing at the same aim (likely: players keep
    clicking until they see it fall) hits the stump's `dwell:log_ramp`, which the axe `clears`, and leaves a flat
    stump. `fell-tree` swung 14 times, which is why its stump shots looked flat; it now stops at 11 and adds a
    `fell-gash-profile` shot. Decide: should the axe still clear a stump's fell wedge (or only a lying one / only
    with a deliberate aim, or after a delay)?
  - **The gash isn't a V while the trunk lies there.** The trunk falls toward the notch and lands flat, so its
    foot's slope faces sideways or down; the two ramps read as a V only in the instant of the fall. If the owner
    wants the V to read afterwards, it is on the stump alone (a ramp), not a pair.
  - **Bites grow down the cell, not across the log (item 26).** The first bite is head-sized (~0.14 m wide, about half
    the ½ m cell's height); by the 3rd and 6th it is a slot the cell's full height, still ~0.14 m wide, with a V
    deepening in profile. It reads as one tall narrow slot rather than bites joining across into a notch;
    on a ½ m log one aim never widens it sideways.
  - **No grain cracks seen round the bites** in any close shot (1, 3, 6 swings) on the dark bark; check the wood
    `cracks` set's stages against the bites' fraction, or their contrast on bark.
- **Visual pass 2 (2026-11-12), today's merges seen in Chrome on the GPU** (bench scenarios in a scratch
  file, `?world=flat` and `starter:meadow-grass`; shots named below):
  - **Damage cracks don't show in game (item 18, BLOCK_DAMAGE.md §6.3).** Stone struck 2 and 4 times
    with the stone pickaxe (it breaks on the 5th, the stone tier's 1.5×) and dirt dug 2 and 4 times with
    the shovel: the carve shows, but no crack strokes anywhere, not round the dent, not on the struck
    cell's other faces. A pixel diff of `stone-0` against `stone-2`/`stone-4` (same view) changes only
    inside the dent (none ≥ 4/255 on the cell's top or the face round it). Native tests pass
    (`damage_mesh` overlay per vertex, `terrain_vertices_carry_the_crack_bytes`), so the loss is
    between the worker's mesh and the screen: check the `cracks` stream after the job codec, the
    Unorm8x4 attribute and `crack_shade`'s slot in the browser. Shots: `stone-4-near.png`,
    `stone-zoom-0-2-4.png`, `diff-stone-4.png`, `dirt-sheet.png`.
    **Fixed (`claude/cracks-and-fell-bugs`, BUGS.md #74):** the game's chunks append their ground items
    to the opaque mesh after the crack stream was padded, so it fell short and the client dropped it whole;
    the items now pad it. Seen in the browser after: crack strokes on the struck stone's face and top.
  - **The stone carve reads as dark spiky wedges** (`stone-4-near.png`): the shards' facets
    in shade are near-black blades, sharper than the painterly look; the dirt bowl reads well
    (`dirt-4-low.png`). From above and close the bowl's lit far wall can read as a post
    (`dirt-2-close.png`).
  - **Felling not reached with the bench's swings (item 24).** A one-cell log column on the flat
    world: 4 level swings aimed high (pitch −3°) cut nothing visible; 4 low (−18°) a kerf; 12 more
    alternating high/low and 4 vertical left a thin kerf and one vertical slit, no notch opening and no
    fall (`fell-probe-sheet.png`, `c-alt-12.png`). The gash itself is pinned natively
    (`felled_from_each_side_…`); the in-game look waits on a reliable fell: a bench step that
    swings at a cell's two bands (as the test does), or a check that the bench's swings land deep.
    **Fixed (`claude/cracks-and-fell-bugs`, BUGS.md #75):** every swing at one aim landed in the same
    quarter-metre band and hacked one binding kerf; a swing at the deeper band now lands in the open one,
    so one aim notches, and the stone axe's `biteM` 0.1 makes it 11 swings (ADR 0067 §2 amended). Bench
    `fell-tree` chops a column at one aim and shoots the stump from the notch side.
  - **Camera sway works, subtly (item 17):** measured on the bursts, the level chop turns the view
    ~18 px (≈1.3°) right in the wind-up and back left through the strike; the vertical pitches up
    ~16 px in the wind-up. The bursts' ~100 ms frames miss the contact kick. `clip-strip` holds the
    arms only (`holdSwing`), so it shows no sway; a blue blob at the bottom of first person mid-swing
    (the off hand's sleeve?) at 0.446 s (`level-sheet.png`) is worth a look.
  - **Meadow tall grass 0.54 (item 21): good.** Tufts stand on the ½ m ramps as on flat grass, in the
    grass top's colour (not pale); they read soft at a distance (`meadow-0.png`, `meadow-1.png`).
  - Not checked: item 25's contact timing (not merged), remote chop (two clients).

- **Visual pass (2026-11-11), found in game** (spots `starter:*` in `client/bench/spots.json`, found by
  `starter_world` `feature_spots`; shots named `starter-<spot>.png`):
  - **Tall grass nearly absent in game: not a loss (checked 2026-11-11, `claude/tall-grass-missing`).**
    Counted at the meadow spot three ways, they agree: the generator (`tall_grass_counts`, 7914),
    the game's own generator path (`TerrainGenerator::with_map` over the starter's map, the same 7914),
    the server's chunks round a player after 1 and 10 s (`dwell-sim` `starter_grass`: 938 of 938 within
    48 m; grass growth only adds) and the page (`__dwell.voxel`: 294 tufts in a 40 x 30 m patch 30-60 m
    ahead, as natively). The spot itself stands in a gap: tall grass grows in patches (decorations.json
    `meadow_grass`: wavelength 40 m, coverage 0.45, density 0.55, on flat `dwell:grass` only, so the ~40%
    of the meadow's columns that are 1/2 m grass ramps hold none), and the 20 x 20 m round the spot has 0
    tufts over 108 flat grass and 152 ramp tops, natively as in game. Open for the owner (tuning, not a
    bug; changing the decoration changes the default world's terrain, a compatibility break): denser or
    wider patches, tufts on grass ramps (`dwell:tall_grass` already allows `placeOn` ramps), and the
    tufts reading pale against the grass (whitish-green blades at a patch edge).
  - **The steppe had no tall grass at all: fixed (data).** Its lee's wetness (~0.01) is under
    `dwell:meadow_grass`'s moisture envelope ([0.05, 1], fade 0.3), so at `undergrowth` 1 its climate
    share was 0 (0 over 50545 grass cells). The starter's steppe now has `undergrowth` 0.75: a floor of
    a quarter, 2591 tufts over the same 50545 (~5%; the meadow ~16%); `the_steppe_keeps_sparse_tall_grass`
    (red at 1, green at 0.75).
  - **Arches:** `starter-arch-2` reads (a hole through a red wall, sky beyond); `arch-1`'s opening sits at
    the foot of a ~100 m fin with a pale sandstone heap in front of it, `arch-3`'s is a slit in a long
    wall (the "fin" is a wall, not a narrow fin). Arches are tunnels through broad walls more than
    spans on fins.
  - **Rainier summit ice reads saturated blue** (`starter-rainier-summit`): every surface on the summit
    is glacier ice in a strong blue; from afar (`starter-rainier-flank`) cleavers, snow and the ice cap
    read well. On the glacier edge (`starter-glacier-edge`) ice and snow barely differ.
  - **Leaf tints barely show** (`starter-leaf-tints`, `-pines`): crowns look nearly alike from 30 m
    (oak lightness 0.72–1.18 in data); needs an on/off comparison or wider variety.
  - **Sandy soil spots land on shores:** `feature_spots`' sand-edge search finds lake and sea shores
    (`starter-steppe-sand`), not sand under the steppe's grass; a better search is needed to see T3b's
    `soilBlocks`.
  - **The lava river is a sheet** (`lava-river-day`, face B, rate ×30 for 45 s): a lava pond with a crust
    in a shallow valley, no channel (as the lava queue item says).
  - **Bench: an `above` spot can find ground far too low** at a high spot: `starter:tundra-snowline` as
    `above` (openY 1500) found ground at 1200 m under a 1272 m column and shot from inside the hill
    (`starter-tundra-snowline-BUG-above.png`); the spot is an `at` now. `openY` caps at 2000, so spots
    over 2000 m (Rainier, the glacier) must be `at` spots.
- **BUGS.md #68:** holes and walls in the far terrain on a 4800 m cone seen from its summit (parked
  after two rounds; leads in BUGS.md).
- **Arches:** the openings in the LOD and in Chisel's 3D view; caprock lips; an arch on a fin seen in
  game (fins and the rock filter built, 0.52.66).
- **Terrain leftovers (0.52.66 round 1 left):** in-game screenshots of an arch
  on a fin, the red mesa and a snowy biome edge, and of T3b in the starter (its biomes' snow lines, sand
  under the steppe's grass, the grass thinning into the dry lee; built without screenshots). T3b is built
  (ADR 0075 T3b): a biome's own snow line (`snow`), soil blocks by soil type (`soilBlocks`), tall grass
  and ground items by climate (`undergrowth`); the starter opts in.
- **Pieces follow-ups (0.52.46):** the worn axe not yet seen in game.
- **Block damage follow-ups:** scoops and shards now modelled as dents (`damage.look.depth`, lit by their
  modelled normals; BLOCK_DAMAGE.md §6); in-game before/after screenshots and the owner's look still to do.
- **Peaks (Rainier detail, built; ADR 0075 T5):** a broken summit (rim notches, crags, the highest point
  off-centre), cleavers at the column level that survive the 100 m map, and cirques under steep
  headwalls with ice floors, as opt-in `tectonics` keys on `test:tall_peak` and the starter.
  Outstanding: the in-game look (screenshots from the summit and from afar, on a local GPU); the far
  terrain's LOD levels coarser than the cirques and crags may smooth them.
- **Chisel:** biome decorations and spacing per biome; players' saved structures as placement sources;
  the in-game editor and a biome choice in the New world form.

43. **Model part painter** (owner 2026-11-12, after bug #96, villagers' faces and hair taking clothes/hat colours:
   **"You should have tooling for debugging this and setting up a model so it can be retextured."**). Answers: a
   **standalone prototype page** (docs/prototypes, published like the Character Lab); parts **painted per vertex,
   saved as data** (a part map per model: each vertex → part — skin, hair, shirt, trousers, hat, eyes, …; seeded
   from the skin weights and colours, corrected by hand with a brush). The page shows the model with its parts in
   distinct colours (front/side/back, rotate), paints vertices into parts, previews the creature's colour variants
   (creatures.json `colours`), and exports the part map file the game reads (the recolour by part, not by colour).
   Order: bug #96's fix lands the part map format and the game reading it; then the page.
   **First half built with bug #96's fix (after 0.54.171):** the part map format (`shared/models/parts/<model>.json`,
   models.json `parts`), `shared/models/parts.mjs` (`--seed` from rules: mesh island near a point, colour ramp, joint
   weight, height; `--render` front/side/back PNGs; `--stats`), gen.mjs's checks (vertex count, known parts, every
   NPC colour part a part of its model) and the game recolouring by part (CHARACTERS.md §11.11). Bug #96's owner
   follow-ups, 2026-11-12: "Oh maybe not all of them, but their hair and faces are really crazy looking." / "I think
   some of the clothes and hat vertices are being assigned incorrectly (like they should be face and hair)."
   **Built (the painter page):** `docs/prototypes/model-part-painter.html` (listed in docs/prototypes/README.md; the
   orchestrator publishes it and adds the link): loads a GLB and a part map (file picker, drop, or the Peasant from
   the repo), shows the parts flat-shaded in distinct colours, real colours, a creature's colour variants recoloured
   by part (`dwell_models::recolour`'s rule) or a joint's skin weights; front/side/back/top, orbit; a brush (screen px,
   front faces only), fill island, fill same colour within the brush; parts added and renamed (≤ 6), counts;
   undo/redo; the other detail levels' mapped parts (read-only); exports the part map exactly as `parts.mjs` writes it
   (refused when the vertex count isn't the GLB's `detail_0`). `node --test shared/models/painter.test.mjs` runs the
   page's core against `parts.mjs` (the Peasant's export byte for byte). Left: the owner repaints the Peasant's face
   and hair with it.
   **Outfit patterns and the eyes** (owner 2026-11-13): **"I also want to improve the outfit texturing of the human
   model. Alternating patterns around the hat, not on the brim. and also around the waist we have some of that now.
   Also I think we lost the eyes, maybe other features in the low poly ization? Or it may not have had eyes?"**
   Findings (branch `claude/outfit-patterns`, 2026-11-13): the Peasant's mesh has **no eye, brow or mouth geometry**
   (`detail_0` is the pack's full mesh, ratio 1: the face is one smooth skin surface of ~70 front triangles; the head's
   islands are the body, the hair cap and the hat). Whatever eyes the pack had were **painted in its texture** and
   were lost at the import's paint step (`blender_import.py` `paint`: each face takes the median of five texture
   samples, snapped to a palette base; an eye smaller than a face loses the vote), not by the decimation and not by
   the part map. Nothing on the face is darker than `skin_deep` at any level; the nose and lip shading
   (`skin_tan`) survived. The pack isn't in the repo, so its texture was not checked to prove it had eyes. The
   **default level (60 %) draws the face as ~10 triangles** and the full mesh's facets are wide quads across the
   face, so **painting eyes onto existing faces doesn't work** (tried: a dark wedge under the fringe at 60 %,
   "sunglasses" at 100 %): eyes need **small geometry** (a few quads a side on the head joint, by data, added to
   every level). The waist: the belt (`hair_brown`/`heartwood`, no part) is a ring at y 0.87–0.92 m mostly
   inside the tunic; a band painted on the tunic at 60 % paints long spikes (its triangles run hip to chest).
   **Built so far:** `shared/models/decor.mjs` (bands of alternating colours on a part's ring, by height and
   radius, and marks; by facet, every level; `node --test shared/models/decor.test.mjs`), not yet applied to the
   Peasant. **Left:** eyes as geometry; the hat band and a waist band wired into models.json `decor` once the eyes
   land, with renders for the owner.

44. **Trees on slopes hang over the edge** (owner 2026-11-12: **"I notice a lot of trees on slopes are hanging out over
   the edge. Perhaps trees should always have flats under them? Don't limit trees to flats, since they might not
   spawn on wide sections of terrain? Check implications of this and possible fixes."**). Finding: placements
   (`placements.rs`) anchor a tree at its anchor column's top + 1 and test the slope only by the height field's
   gradient over ±2 m (`maxSlopeDeg` up to 44°); the other trunk columns (thick old-growth trunks) and ½ m ramps under
   the trunk are not levelled, so a trunk's downhill side stands over a ramp or a drop. Options, analysis and the
   owner's pick follow. Options shown (https://claude.ai/artifact/ChMv9xuaABdEGfzRPVLDM9): today floating, A trunk
   reaches down, B level a pad, C flare + small fill. Owner: **"For now let's just do level pad, can just be the the
   tree width, 1m."** — fill the cells under the trunk's footprint only (no ring) up to the trunk's base; a change
   to the terrain a seed generates, so a new compatibility line (0.55, the owner raises it). **Built
   (branch `claude/tree-pads`):** a placement's `pad` ("trunk", the default for tree species, set on
   the pines' rule; or "none") fills each trunk column (the structure's lowest cells) up to under the
   base with the column's ground and makes ramps there full; the ground outside the trunk is unchanged,
   the LOD's tree level the same (`tests/tree_pads.rs`, both faces). Left: the owner raises
   `package.json` and `line.json` to 0.55 (the goldens and the starter pin are re-recorded here).

45. **Floating trees drift, leaves let water through** (owner 2026-11-12: **"Also, trees they are floating in water should
   never settle, it causes them to block up rivers randomly instead of floating downstream. And leaves also should
   let water flow through freely."**). A body floating in water (a felled trunk, a crown) never settles back into
   blocks while it floats; it keeps drifting with the current until it beaches on dry ground (then settles as
   today). Leaf blocks are open to the water grid (water flows through their cells as through air, the leaves
   kept), in place and on bodies. **Built (2026-11-12):** a body afloat (a cell in a liquid, lighter than it on the whole) of a
   kind whose `settlesAfloat` (bodies.json, default false) is false never settles; it settles only `beachSeconds`
   (2 s, felling.json) out of the water, then by the usual sleep or timeout (BUILT_BODIES.md §6a; waterlogging
   later hooks in where `afloat` is decided). Leaves (any `passable` block with a `flooded` state) are open to
   the water grid's flow, drawn flooded under the water, never replaced (WATER_SURFACE_GRID.md §2d); bodies'
   cells were never in the grid. Generated water levels unchanged (no new compatibility line). Tests red → green:
   a trunk in a 1 m/s river drifts 40 s without settling (both faces), a trunk moved onto the bank settles, a
   natural raft afloat is predicted without snaps, water beyond a wall of leaves stands as with none (both
   faces).

46. **Animals easier to find, and more of them** (owner 2026-11-12: **"Im having a hard time finding animals, are they
   visible from high up?"**). Today each face's island starts with 8 deer herds (6 head) and 2 wolf packs (4), caps 40
   and 8; far herds are dots fading out by 700 m (wolves 550 m). Answers: **see herds from farther**, **more
   animals**, **a way to find them for testing**, and **"Even more animals. What's a reasonable amount for a real
   world this size?"** Real densities: deer ~5–20 per km² in good range, grey wolves ~1 per 50–100 km² (packs of
   4–8). A face's island (~20–30 km radius, ~1,000–2,000 km² of land) would hold ~5,000–20,000 deer and 20–40 wolves
   (4–8 packs). Plan: deer at the low end of real (~2–4/km² of grazing land, herds of 4–12: a few hundred herds,
   capped by the grazing's carrying capacity), wolves at real (~1 per 60 km²: 4–8 packs), as data; far herds
   visible to ~2.5 km and growing with distance; a test/dev command to list herds and go to the nearest.
   **Built** (2026-11-12, docs/ECOLOGY.md §3, §9, §11): a species' `start` is a density over its range
   (`perKm2`: deer 3 a km² of grazing land in herds of 10, wolves 1 per 60 km² of land in packs of 5,
   `minGroups` for small islands); deer `maxGroups` 40 → 600, wolves cap 7 → 8 (packs of 4–8). Seed 1:
   ~590 herds (5,900 deer) and 7 packs a face at the start, settling at ~6,900 deer (3.5/km²) and 50–65
   wolves in 8 packs over two simulated years; a 5-day step ~2.5 ms a face (the step's O(n²) scans cut),
   one step a server tick; the record ~137 KB a face. Far herds fade 1.8–2.5 km (wolves 1.5–2.2 km), each
   head at least 3 px on screen (`far.minPx`), stood past the loaded chunks on the generator's ground;
   the server sends each client its nearest groups first. The finder: `__dwell.herds(kind?)`,
   `__dwell.goToHerd(kind?, back?)` (local worlds) and the bench step `herd` (scenario `herds`). Near
   creature limits (`limits.max` 64, `spawnChunks`) unchanged: at ~3 deer a km² a player meets a herd or
   two at a time. Not yet: in-game screenshots and the frame cost from the bench (no GPU in the cloud).

47. **Wind only when flying** (owner 2026-11-12: "let's disable the wind for now it doesn't seem right like should just be
   for flying or high altitude only." / "Wind SFX"). Built: the wind bed reads the eye's height above the ground
   (silent below 12 m, full at 60 m) instead of altitude above the sea.

48. **Leaves flat against logs** (owner 2026-11-12: **"can we make it so the inside leave edges make a flat side against
   the logs, so it doesn't leave quite so much gapping where they connect to the tree. Currently they're slanted edges
   all the way around in all cases, but where it touches the tree it would look better if it was flat"**). Answers:
   **"Flat leaf applied when a full Face touching and any new leaf touching leaf cases that need to be adjusted to fit
   together properly."** — a leaf block's side that shares a full face with a log is flat (no chamfer); and wherever
   that change leaves leaf-to-leaf seams that no longer meet (a flat side next to a chamfered neighbour), adjust them
   so the crown fits together without gaps or overlaps. **Built (2026-11-12):** block data `leafFlush: true`
   (the log; synced as look tag `0x93`) makes a leaf's face against it flush: drawn flat and whole, no chamfer
   on that side, the edge cuts running into it squared off (`dwell-mesh` `mesher/leaves.rs` `flush_pieces`;
   the far trees' `lod_trees.rs` alike; bodies mesh through the same path). Leaf-to-leaf seams stay closed by
   the existing gap fill (a test sums the crown's area vectors to zero). Tests on both faces with `dwell:log`
   and the fixture's `test:post` by data alone; a tree-dense chunk meshes no slower (≈196 vs ≈211 ms).
   Docs: TREE_GROWTH.md §10, BLOCK_REGISTRY.md.

49. **Release queue starved by CI** (2026-11-12): every notes-only PR started a full CI run (e2e shards), and the Release
   runs queued behind ~40 runs for ~40 minutes. Owner: **"Yeah go ahead and cancel the CI ones and that makes sense
   to drop PRs for notes only updates."** Done: the stale CI runs of merged/closed branches cancelled; CI skips
   docs-only changes (`paths-ignore` docs/** and *.md); notes (BUGS/WORK_QUEUE entries) ride along with the next real
   change instead of their own PR (CI already cancels superseded runs per branch).

50. **Fast re-records: one command per video, shots kept until approved** (owner 2026-11-12: "I would guess that would
   take about 3 minutes to re-record. What are you expecting and why might it be slower currently?" → explained:
   ~20 min because every shot is re-recorded, an agent drives each step, camera spots found by trial; owner:
   **"Yeah build thst"**). Keep each finished shot as a compressed clip until the owner approves the video (raw
   frames still deleted); `take.mjs <video> [--only s5]` records the named shots from the committed plan and paths
   and re-splices with no agent in the loop; an agent only for new camera spots. Owner (2026-11-12): **build this
   before the v5 re-record** (v5 was stopped); the bifacial manifest must make shot 5's camera and `up: "auto"` easy to
   set (only the initial wide holds face A's up; the out-of-the-hole shot is upright in face B's frame).
   **Built 2026-11-12:** `.claude/skills/record/videos/bifacial/video.json` (v4's shots: s1, s2, the `portal` take with
   parts s3–s5 and shot 5's `camB`/`fov`/`up` as args, s6) and `take.mjs <video> [--only s5] [--splice-only]
   [--dry-run]`, `--clean <video>`: records the named, missing and stale shots into a per-shot clip cache (frames
   encoded and deleted right after capture), reuses the rest, splices with `splice.py`. Tests without a browser
   (`take.test.mjs`, in CI): the manifest validates, shot selection and the cache, and the generated plan equals v4's
   hand-written one. Not yet run against the real browser (the first use is the v5 re-record, on the Mac).

51. **The axe behaves like the other tools** (owner 2026-11-12: **"Also the axe doesn't behave like the other tools. I
   think it should. What about how it's designed is different currently?"**). Explained: today the axe on a log uses a
   separate chopping system (ADR 0067: no hits-to-break; a notch cut across the trunk in bands; after ~11 swings, 5
   metal, the tree above falls as a body; the log block never breaks), while every other tool uses block damage
   (BLOCK_DAMAGE.md: hits to break per tool, each hit carves a bite of ½ the block ÷ hits, chips fly, cracks spread,
   at 50 % the block breaks and drops). Owner's answer: **"Like the others, felling on top"** — logs take block
   damage like stone or dirt (hits to break by tool, bites, chips, cracks; the block breaks and drops a log), and
   breaking a trunk's base log fells the tree above as a body, as today. **Built 2026-11-12** (cloud, branch
   `claude/axe-like-tools`), waiting on the owner's look in game: the log's `damage` data (stone axe 10 hits,
   metal 5), each axe hit a block-damage hit (a mark, a wear, its share of the 8 wood), the last breaking the log
   and felling the trunk above as one body toward the side struck; a lying log the same hits (bucking); the log
   drawn with the axe's head-sized bites at its marks grown by the bite rule, the wood cracks by the fraction. No
   new cuts (no notch, kerfs or gash wedges); older saves' cuts still load and draw, the next hit carrying their
   progress into damage. Additive: no new compatibility line (docs/BLOCK_DAMAGE.md §12, ADR 0067 note).

52. **Big worlds keep their mountains and rivers** — **Quick fix built 2026-11-12** (branch `claude/notes-batch-2`,
   one PR; needs the owner to raise the 0.56 line before it merges: it changes 2×–10× worlds' terrain, 1× is the same
   bits, its goldens and hydrology digests unchanged). `hydrology.json` `bigWorlds` (generated `BIG_WORLDS`,
   applied by `HydrologyParams::sized` above 1×): the ranges' coast distances × the radius scale (they rise across
   the island, not in a ring by its coast), ridges eroded as at the profile's cell (`ridge_cell`), the least lake
   × scale² and its depth × scale, river lips one for one to 100 m³/s then by √Q. Measured (seed 0, face A,
   `examples/big_terrain.rs`, table in BIG_MAP.md §5.1): 10× before max 1 427 m, 0.0 % above 1 500 m, 0.76
   peaks/1 000 km², a 6.5 km channel, 5 888 lakes (10.5 %); after 2 692 m, 8.1 %, 2.83, 807 m, 1 lake (0.03 %); 1×
   2 613 m, 6.4 %, 1.92, 67 m, 3 lakes. Tests `tests/big_terrain.rs` (4×, 10×, both faces vs 1×), red on the old
   rules → green; `map_build_budget`'s uplift bound 4× → 8× (ranges across the island: ~1 000 segment tests a
   cell at 10×, the map still ~2 s). **Left:** the uplift's ranges sampled per column at full resolution (the
   world map would carry its spines to the chunk generator: a map-format change), and (2) below, the regional
   100 m maps (small rivers and lakes, R1–R4). Original item:
   **Big worlds keep their mountains and rivers** (owner 2026-11-12: "any idea why 10x worlds are super flat with
   massive rivers and lakes? And no mountains? Are they just stretching out the noise or something like that?" →
   explained: the whole-map grids keep ~1 M cells, so a 10× cell is 1 km; ranges narrower than a few cells vanish,
   rivers are carved a cell wide, the lake fill is per cell, the island tunables don't scale; BUGS #77. Owner:
   **"Yes feel free to add the big terrain fix to the queue."**). Plan (BIG_MAP.md §5): (1) quick — features in
   metres, not cells (range widths, river widths by discharge, lake fill per metre), the island shape and the number
   of ranges scaled with the radius, so bigger islands get more mountains, not flatter ones; (2) the regional 100 m
   maps on demand around the player (25.6 km squares, seams agreed). Changes big worlds' terrain: the 0.56 line
   (the owner raises it), 1× unchanged.

53. **Map markers you place yourself** — **Built 2026-11-13** (branch `claude/map-markers`; needs the owner's
   look in the browser: the picker, the minimap icons and the world waypoints were not drawn on a GPU here).
   Built: the data, generator and validator; the record (`dwell_protocol::markers`, golden vector); the
   world's `markers` table; the `markers.private` capability and `Markers` 0x6C; the big map's place / edit /
   drag / delete; the minimap; the world waypoints (a HUD part); spawn and last-death auto markers. **Left:**
   a list view of the markers; portals (the `portal` kind is data, nothing notes one until portals exist);
   a marker placed on the big map takes the player's height on that face, not the ground's under the click. Owner 2026-11-12:
   **"Let's use non-Unicode vector style symbols to let you annotate your own map so you can find your way
   around."** / **"Add that to the queue."** Answers: shown on the **big map (M), the minimap and in the world** (a
   waypoint over its spot with its distance); **private to the player** (saved with the world for that player); a
   marker carries **an icon from a set, a colour, a short label**; plus **auto markers** (spawn, last death, portals
   once seen). Icons are vector (SVG paths or the game's own vector draw), never emoji or Unicode glyphs. Design:
   [MAP_MARKERS.md](MAP_MARKERS.md) — the icon set, palette and auto kinds as data (`shared/ui/markers.json` →
   `markers.gen.ts`), a player's markers as one record per device key in the world's optional `markers` table,
   synced to that player only behind the `markers.private` capability.

54. **Resurrection shrines** (owner 2026-11-12: **"And then we want to add 'resurrection shrines' these should work like
   Guild Wars. You respawn at the nearest shrine when you die."** / **"We'll have to spread these pretty thick across
   the map likely."**). Answers: shrines **~2 km apart**; respawn at the **nearest shrine to where you died, on the face
   you died on**; shrines **show on the map**, are **a built structure with a glow**, and the map can **toggle them off
   (filters)**. Placing a structure changes the terrain a seed generates: a new compatibility line, raised in the same
   PR (RELEASES.md "Who raises the line"). Status in docs/SHRINES.md. The shrine's design (owner, 2026-11-12): **"The
   shrine should be a 8x8m terraced and filled stone circle with stepping pillars forming a half circle. Only the
   tallest pillars (half of them) are connected at the top. Forming a quarter circle."** Built as data
   (`shrines.json` `design`: radius, steps, pillar count, heights, lintel, light), generated into the structure file:
   an 8 × 8 m round platform filled to the ground and terraced in ½ m steps, pillars on one half of its rim stepping
   up toward the arc's middle, the tallest half (a quarter circle) joined by a lintel, the light at the centre; the
   open half turned by a seeded quarter turn per site. Owner, 2026-11-12: **"Not sure how easy this is to do, but prefer to
   rotate it so it faces out from hillsides."** So each shrine is turned by its ground: the pillar arc uphill, the open
   half facing downhill (the height field's gradient over the footprint, snapped to the four quarter turns, the
   structure being drawn on the block grid), the seeded turn on flat ground (`flatBelowDeg`). Owner, 2026-11-12:
   **"Perhaps we use the original spawn point we've been setting to then target the nearest shrine to that point for
   initial spawn, so all spawns are at shrines."** So the spawn search runs as before and the player stands up at the
   shrine nearest its point on that face (face A's spawn and face B's `?spawn=b`); when none lies within `spawnWithin`
   m a shrine is built at the search's point itself (deterministic: the point is the seed's), so a spawn always has
   one. Portal and moon spawns keep theirs (they stand at a portal's rim or on another world by design).
   **Fixes after a browser screenshot pass (2026-11-13, brief):** four bugs, BUGS.md #115–118, fixed on branch
   `claude/map-shrine-fixes`: (1) "Shrine clutter on the big map: zoomed out, shrine icons are drawn only in a square
   patch around the view centre, a dense overlapping glowing block … Fix: shrines over the whole visible map, icons
   thinned/clustered by zoom (no overlap; e.g. hide below a zoom or merge into a count), data-driven thresholds";
   (2) "The spawn waypoint shows '1 m' stuck to the top screen edge while standing on the spawn: hide a waypoint
   within a few metres (data), and don't edge-clamp a point that is below/at the camera"; (3) "No pending pin while
   the marker picker is open: draw a temporary pin at the clicked spot until saved/cancelled"; (4) "New world → More
   options repeats World type, Seed and Spawn (already shown above) and that copy has no Realistic choice: remove the
   duplicates (one source of each field)".
   **Follow-ups from a browser check of those fixes (2026-11-13, brief, M1 GPU, seed 0):** BUGS.md #119–122, fixed on branch
   `claude/map-fixes-2`: (1) "Shrine clusters (#115) not seen: zoomed fully out (whole disc, 20 km scale) no shrine
   icons or cluster counts appear at all … at that zoom show clusters with counts instead of nothing"; (2) "The zoom
   buttons jump between 20 m, 500 m and 20 km — too coarse; steps of ~2× (data), wheel too"; (3) "After 'Center on
   me' and a zoom, the player arrow was off the view: zoom about the centred player"; (4) "The picker shows 'Delete'
   for a marker not yet saved: hide it (Cancel only)".
   **Pillars with gaps (owner, 2026-11-13, after a screenshot where the stepped pillars read as one solid wall and the
   quarter-circle lintel didn't stand out):** **"Yes, shrine pillars have gaps."** Brief: the pillars on the back
   semicircle stand apart with at least one full cell of air between neighbours at every height (pillar width as data,
   spacing derived from the arc and the count; fewer pillars by data if 9 don't fit); the lintel spans only the tallest
   half's tops (a quarter circle), bridging the gaps so it reads as an arch; the terraced filled dais and the torches
   kept. Built on branch `claude/shrine-gaps` (`design.pillars.width` 1 m; a gap check in `gen.mjs` fails a count or
   width that leaves no air): 9 pillars don't fit the 3.5 m arc with gaps and 7 can't make the joined half a quarter
   circle, so **5 pillars**, 45° apart, 2.5–3.5 m tall (`lowest` 2.5, `rise` 0.5), the tallest 3 joined by a lintel
   rising from top to top over the gaps (air under it), the middle pillar ½ m off the axis (a 1 m pillar can't centre
   on the 8 m grid's axis). Changes the terrain seeds generate: the 0.59 line, carried by `claude/terrain-059`
   (orchestrator, 2026-11-13: that branch merges this one and raises 0.59 for both worldgen changes).
   Owner, 2026-11-13 (with the 0.59 terrain batch): **"Notched ramps seems fine. I've saved the water video, looks good.
   Good to move on to the next."** The shrine gaps branch (`claude/shrine-gaps`) is merged into the 0.59 batch (branch
   `claude/terrain-059-batch-v81i64`), so its shrine change ships on the new line.

## Future (needs owner answers)

- **Waterlogging** (owner 2026-11-12: "Eventually we'll want to make it so some blocks if they don't move for a little
  while or 'move enough' they'll get waterlogged, sink to the bottom and then settle."): floating bodies of
  certain blocks soak up water after resting still for a while or travelling far enough, sink, and settle on the
  bed — per-block data (soak time, distance).

- **Tall grass hides you** (owner 2026-11-12: "I like the hide mechanic idea, save this for later."): tall grass
  hides a crouched player from animals and NPCs (and deer bed down in it).

- **Geography presets for Realistic worlds** (owner 2026-11-12: "Let's hold that for a possible future feature."):
  Mixed (today), Archipelago (oceanic plates only plus more hotspots: Hawaii-style chains and Japan-style arcs),
  Continental (mostly continental plates); plate counts scaling with world size so big worlds get real continents.
  Today every seed has at least one oceanic and one continental plate (plates.json `kinds.min`).

- **A looser body in the chops** (owner 2026-11-12: "We can save loosening the stiff body for later"): the
  hips and legs turning into a level chop seen from behind, not the chest alone.

- **Active volcanoes, lava and eruptions** — docs/proposals/VOLCANOES.md (owner 2026-11-11), all 5 questions answered.
  Phase V1 (lava as a fluid) moved into the queue (item 8); the later phases (volcano states, eruptions,
  partly destroyed cones) stay here.

- **A 10× map** — docs/proposals/BIG_MAP.md (owner 2026-11-11: "we will test out a 10x size map soon (could
  start whenever)"). Study written: 10× area (158 km radius) runs natively today (62 s, 871 MB a face here)
  once the radius is one value (M0–M2, a desktop test); a phone needs coarse whole-disc grids and 100 m
  regions on demand (R1–R6). Owner's answers (2026-11-11): 10× **radius** (500 km); a desktop test first, phones after the regional maps;
  the size a **world setting**; the plates decide the land; "Perhaps even scope it as a 2x, 4x, 8x or 10x" —
  sizes 1×/2×/4×/8×/10× as data. At 10× radius the whole-map grids run at a coarser cell (~1 km) until the
  regional maps. **M0 built:** the size is a per-world setting (optional tag, today's worlds unchanged) and the
  LOD levels, big map cells, hydrology cell (≤ 1 M cells at every size), island shape and uplift counts derive
  from it. **M1 built:** the LOD root (level 15) holds every size; protocol v23, so it needs a new
  compatibility line (the owner's call). **M2a built (native):** the generators, the disc and build volume
  (`WorldBounds`), the LOD and the server read the world's size; a saved 2× world reopens at 2×; 1× unchanged;
  a 2× world creates natively in 4.6 s at 176 MB (1×: 3.7 s, 164 MB). **M2b built (the browser):** New world →
  More options → World size (desktop pages only; hidden on touch) and the dev link `?size=2x`; the size saved
  in the world's settings and its menu record, every worker builds its maps and generators at it, the client's
  bounds, streaming, LOD and big map follow the join's size; 1× unchanged. **Checked on the M1's bench
  (2026-11-11, `?size=`, seed 0):** 2× plays — maps 3.0 s, spawn search 2.9 s, playing at 11.2 s (1×: 2.1, 1.5,
  8.7 s), every big map level right over the 200 km disc, no gap at the old 50 km rim, a 3.8 km flight at
  128 m/s at 60 fps (frame.cpu p95 9 ms); 4× plays — maps 7.3 s, playing at 25.3 s. The F4 height map's
  widest zoom showed only the 1× disc (fixed). Open: the page's memory grows with the area (BUGS.md #76:
  non-mesh 749 → 979 → 1 819 MB at 1×/2×/4×) and the bigger islands lose their mountains to a speckle of
  lakes (#77). **Next: the owner tests 2× on the desktop**; #76 before 8× and 10×.

- **Sharing saved structures between players** (owner 2026-11-11: "eventually shareable").

## Waiting on the owner

- Palm fronds: leaf cards or blocks (after the Tree Growth prototype).

## Decided and not to be revisited

- **Crafting proposal shelved** (2026-11-11): "That's a lot of assumptions I don't think I want to do
  any of those crafting question things for now." Only the pieces and combine recipes were built.
- **Pieces:** 8 rock shards → a stone slab, 8 dirt clumps → a dirt slab, 8 wood chunks → a log slab;
  rocks still found in grass and dirt at random; sticks from leaves. Stone tools: 2 sticks + 2 rock
  shards + 2 grass fibers. Stone axe durability 32.
- **Volcanoes:** dormant for now, eruptions later.
- **Tool wear** (2026-11-11): "Tools don't wear in creative, stone pick wears too like axe, same with
  shovel": the stone axe, stone pickaxe and wooden shovel 32 hits each; metal tools unlimited.
- **Rocks from plain dirt** (2026-11-11): confirmed, as built in 0.52.46.
- **Backpack:** 30 slots.
