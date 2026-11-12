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

The running order across the items below; each line names its item, where the owner's words and details are.

1. **High at 60 fps on the M1 Air (item 30, P0)** — running locally: measure each feature at High, cull leaf cards
   inside crowns and on the far side, pull their range in, a Trees group and a setting for every costly feature.
2. **Distance haze tuning (item 29)** — only the height fog's three numbers, ground-level and hilltop shots, clouds off.
3. **Cloud lighting (item 13)** — shaded bases and self-shadowing, matched against the old 2D deck.
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
   Next (local): the in-game look side by side with the old deck (sun high, low sun, from above, night) and the
   bench before/after at the §8.3 views; tune by the data.
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
   new ones can replace them later.

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
   Realistic is not in the World list yet (R6), so the Sky field shows once it is. **O3 not built** (the climate
   reading the orbit needs it carried to the map workers): next.
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
   own buttons, aligned with the others.

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
   BLOCK_DAMAGE.md §6.4 "Round 2", ADR 0067 note.
26b. **A felled trunk landing on its chopper flings them (BUGS.md #52)** (the owner's brief, 2026-11-12: fix the
   cause, "we shouldn't be capping player kick like that" (2026-11-08): no cap on the kick; a trunk hitting a
   player at speed must still knock them as physics says). **Built 2026-11-12** (branch `claude/pinch-fling`):
   a piece's contact pressing a player down takes the piece's friction (`physics::PinchGrip`, a Rapier contact
   hook; PLAYER_CONTROLLER.md §6.6), so the frictionless capsule is pinned, not squeezed out; side hits stay
   frictionless and uncapped. The repro is un-ignored (0.4–0.5 m, was 155–160 m; both faces).

27. **The moon as a real bifacial world** (owner 2026-11-12: "we're going to make the moon a real bifacial world
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
   wondering if the new trees and volumetric clouds are at fault!"** Next (local, after theater mode's UI): measure
   flying at High (sky and cloud pass cost at altitude, tree meshing/uploads while streaming, the clouds' volume
   pass vs the deck), cut what's large.
   Owner, 2026-11-12: **"Ensure all of our graphics have settings in our graphics menu."** Every visual feature gets a
   Graphics setting (with preset values), not only the ones with a renderer tier function: tall grass density, far
   trees / impostors, leaf cards and chamfer, hit shards and particles, far herds, distant shadows (its own),
   volumetric clouds and plumes, lava crust/glow, bloom, haze extras, water tiers, light shafts, reflections, view
   distance; the audit test widened from `render/*_for` to every per-feature switch the renderer and mesher read.
   In progress (cloud).
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

33. **Licensed music replaces the generated music** (owner 2026-11-12: "I downloaded a big music set as a Unity
   asset, I'd like the casual game music setup in a prototype and make it play randomly in the game. Probably
   needs compression. It'll replace the current music which is AI generated."). The pack: *Andrew Sitkov Music*
   (Unity Asset Store, `~/Library/Unity/Asset Store-5.x/Andrew Sitkov Music/AudioMusic/`; the download was not
   finished at 23:10, the folder empty). Plan: extract the casual-game tracks from the .unitypackage, compress
   (AAC/Opus at a music bitrate, loudness-matched), a music list as data (tracks, moods, gain, fade), a prototype
   page to audition the set, the game's music player shuffling them (no repeats back to back, gaps and fades as
   data) in place of the current tracks; licence notes as for the Imphenzia samples (AUDIO.md §2b).

34. **The in-game Chisel editor and the Textures tab extras** (owner 2026-11-12: "these all seem solid yes"): open
   Chisel's biome sliders inside a running world with the terrain around the player regenerating as they tune
   (Phase 56 B7); the Textures tab adding new materials and blocks by data, editing water and sky colours, and
   painting item and model textures (B6's rest). Queued.
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
   same angle rule); modes without player damage (creative) are unaffected. In progress (cloud).
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

## Done

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

## Future (needs owner answers)

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
