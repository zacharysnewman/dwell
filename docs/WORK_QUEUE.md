# Work queue

The owner's ordered queue of work, with the decisions behind each item, so it survives a session
ending. Update it in the same change that starts, finishes, re-orders or re-scopes an item. The phase
status lives in `docs/IMPLEMENTATION_PLAN.md`; this file is the order and the owner's words.

Last updated: 2026-11-11 (0.52.68).

## Next, in order

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
   (WATER_RENDERING.md §17.1–17.2). **Next:** heat per cell; then the water grid's columns as the fields' water and V2's vents.
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
   every server (fixed). The ecology is 12.5 MB for both faces; its ~3 MB record is now written only when
   it changed (a local world saves every 5 s). Next: a smaller island, grazing applied to tall grass as
   chunks load.
12. **Players' saved structures as placement sources** (owner 2026-11-11): per player (their library, usable
   in any world they create); placed copies editable, not protected; 32³ for now (may change), or larger
   structures as a grid of 32³ tiles. Sharing between players: Future.
13. **Volumetric clouds** (owner 2026-11-11: "volumetric clouds in the sky. These clouds will likely be
   useful for weather sim, and volcano smoke. So they need to be performant."). **Built (branch
   `claude/volumetric-clouds`, CLOUDS.md §8):** a ray-marched layer in its own reduced-resolution pass
   with temporal accumulation and a depth-aware upsample; tiers as data (High ½ opt-in, Medium ¼ the
   desktop default, Low the 2D deck); plumes with a colour by data; the cheap deck kept for Snell's
   window and reflections. Bench from above: High +1.5 ms (budget 1.5), Medium +0.9 ms (budget 0.8).
   Next: trace the clouds pass's fixed cost; plumes as run-time uniforms for a volcano or weather
   system.

Cloud sessions: logic-only items can run as `claude --cloud` sessions in parallel (verified 2026-11-11).

14. **Tall grass grows and spreads** (owner 2026-11-11: "I want tall grass to steadily grow and spread. It
   should probably have a max depending on climate/biome."). **Built** (ADR 0075 T3c, docs/ECOLOGY.md §10):
   `shared/worldgen/grass.json` (the rule as data: the block, the ground, the decoration whose climate
   envelope sets the cap, the max density, spread and regrowth a day, reach, open air above, the catch-up
   bound) → `grass.gen.rs`; `dwell_worldgen::grass` (the face-local step); the server's grass step every 5 s
   over loaded chunks, each growing the whole world days it missed (at most 20); a player's block wins;
   eaten grass regrows; the far forage's most is scaled by the same climate share. A per-world switch in an
   optional `grass_growth` table: on for new worlds, off for worlds saved before it (their meadows stay as
   they were). Not yet: a setting in the world menu.

## Done

- **Starter world on iPhone (P0, 0.52.68, BUGS.md #70):** the tab reloaded while loading the terrain.
  Every generator of a many-biome world built its own whole-map biome grids (20 sets in a job worker);
  now one per map and process (`terrain::shared_biomes`), and the LOD coarsens by its overshoot so a
  phone's loading view fits its 96 MB within seconds. Bench (`memory-phone --touch`): starter 1,467 →
  850 MB at load, default 746 → 676. Owner to check on the iPhone 17; left: smaller grids (f16, the
  drainage's copy of the heights), one copy per instance.

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

## Future (needs owner answers)

- **Active volcanoes, lava and eruptions** — docs/proposals/VOLCANOES.md (owner 2026-11-11), all 5 questions answered.
  Phase V1 (lava as a fluid) moved into the queue (item 8); the later phases (volcano states, eruptions,
  partly destroyed cones) stay here.

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
