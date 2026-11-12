# Work queue

The owner's ordered queue of work, with the decisions behind each item, so it survives a session
ending. Update it in the same change that starts, finishes, re-orders or re-scopes an item. The phase
status lives in `docs/IMPLEMENTATION_PLAN.md`; this file is the order and the owner's words.

**Rule (owner, 2026-11-11: "This is why we need to be documenting these as we go"):** every request, answer
or decision the owner gives goes into this file **in the same turn it arrives**, before or alongside starting
the work, quoting the owner's words; never only in a session's memory or an agent's brief. An item nobody
recorded doesn't exist for the next session.

Last updated: 2026-11-11 (0.52.108).

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
   on both faces, not wired into any world (goldens unchanged). The Chisel plates overlay moved to R2. Next:
   R2 (uplift from boundaries); q2's bands, q6 and q8 still open. q5 (2026-11-11): no new rock blocks
   for now; R5 uses today's blocks (stone, basalt, sandstone, red sandstone, sand, dirt) named in data, so
   new ones can replace them later.

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
   Left: the owner's look in game; grass growth still regrows tufts on flat grass only.
22. **Settings and version buttons look like web links** (owner 2026-11-11: "I thought we had settings and
   version buttons changed queued, is that true?" — it wasn't recorded anywhere; then "The buttons should be
   aligned to match the design of the others instead of looking like web links"). Restyle them as the menu's
   own buttons, aligned with the others.

23. **Expandable sections in the main menu** (owner 2026-11-12: "Our main menu has some expandable sections that
   don't have good UI best practices to indicate they can expand and collapse. Improve these."). Built
   (0.54.x): New world's More options, Versions' Earlier versions and About share one disclosure style: a
   chevron right when closed and down when open, a hover and pressed background, a focus ring; About is a
   real disclosure button (aria-expanded, aria-controls). The world card's ⋯ menu keeps its button.

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
  compatibility line (the owner's call). Next: M2 (the generators read the world's size; 2× first).

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
