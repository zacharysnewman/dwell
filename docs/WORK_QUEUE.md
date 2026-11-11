# Work queue

The owner's ordered queue of work, with the decisions behind each item, so it survives a session
ending. Update it in the same change that starts, finishes, re-orders or re-scopes an item. The phase
status lives in `docs/IMPLEMENTATION_PLAN.md`; this file is the order and the owner's words.

Last updated: 2026-11-11 (0.52.52).

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

## Done

- **Starter world** (0.52.54, WORLD_GENERATION.md): `test:starter`, a candidate default from the
  systemic terrain layers and eleven biome presets placed by climate, for the owner to react to at
  `?worldgen=test:starter`; fixture worlds may carry their own biomes. Left: in-game screenshots of the
  tour, per-cone heights for the arc, palms. Next: the leaves (item 5). 0.52.63 (leftovers round 1): the biome
  balance (broadleaf up, boreal and steppe down), the pick's `sharpen` (less mesa/badlands speckle), the
  boreal spot's pines, the arc's cones falling off (`arcDecay`), Chisel's "Open
  starter world", the tour printing a mesa spot (its strongest sample), a
  biome's `decorations` read. Left: the cliffs layer in the starter (weaker or off), tour spots for the volcano
  view, a wetland and a mesa, in-game screenshots of the spots;
  the New world form's biome/world choice.

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
- **Arches:** erosion growing real fins so arches are common; arches in a chosen rock only; the
  openings in the LOD and in Chisel's 3D view; caprock lips.
- **Pieces follow-ups (0.52.46):** the Combine buttons say "Stone"/"Log", not "slab"; the wood chunk's
  icon is a plain dot; a dead player's dropped tools lose their wear; the worn axe not yet seen in game.
- **Block damage follow-ups:** scoops read as dark stains (flat decals); remote players' leaf brushing
  and stick snaps are silent.
- **Peaks:** a more broken summit, deeper ridges and cirques (Rainier).
- **Chisel:** biome decorations and spacing per biome; players' saved structures as placement sources;
  the in-game editor and a biome choice in the New world form.

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
