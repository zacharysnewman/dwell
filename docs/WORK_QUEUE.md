# Work queue

The owner's ordered queue of work, with the decisions behind each item, so it survives a session
ending. Update it in the same change that starts, finishes, re-orders or re-scopes an item. The phase
status lives in `docs/IMPLEMENTATION_PLAN.md`; this file is the order and the owner's words.

Last updated: 2026-11-11 (0.52.51).

## Running

2. **Tree Growth prototype update** (0.52.50, docs prototype, merge after 1): oak, pine and palm as
   data; saplings start thin and short with a small crown of the same shape and grow in height, trunk
   width (octagon steps) and crown together; the chosen leaf look (option 5) with the game's tiles.
   Republish to the Tree Growth link.

## Next, in order

3. **Creative breaking fix (bug)** — done in 0.52.51 (creative's `instantBreaks` mode ability,
   docs/BLOCK_DAMAGE.md §7). Owner: "I can't break blocks like stone with my fists in creative,
   that's not right. I should be able to break any block while holding anything or nothing with the
   exception of tools." Creative, no tool held (bare hand or any non-tool item): any block breaks in
   one hit. A tool held: that tool's damage rules. Adventure unchanged (bare hands can't break stone).
   Red → green test.
4. **Tool grip.** Owner: "The tools are obviously held incorrectly … can you properly align them to the
   hand?" Each tool's handle through the closed fist along the palm, in first person and in other
   players' hands; full size kept (0.52.44: "the original size … was much more accurate"). Screenshots
   of all six.
5. **Leaves, option 5, with matching far trees.**
   - Leaf Lab option 5 at its defaults ("#5 looks really good with defaults"): canopy normals 75 %,
     chamfer 0.28 m, leaf cards density 55 % and size 0.95 m. Leaves stay blocks. Fix the dark gaps at
     chamfer joins. Data-driven; the CPU tier without cards.
   - Cards on near trees only ("Cards could only show on near trees"), faded out by distance (data,
     about 48–64 m to start).
   - Far trees: "We do need better low rez trees the ones directly beyond high detail look nothing like
     the main trees." The LOD's trees match the near ones (octagon trunks, the same crown shape at
     coarser detail, the same colours and canopy lighting), no pop at the hand-over.
6. **Tree species and growth in game.** Owner: "trees start with small trunks, thin, same shape, but
   shorter, smaller leaves … it should grow over time" and "Oak trees …, Pine trees …, and Palm
   trees." Species as data (`shared/trees`), growth stages stored, per climate (vegetation.json),
   from the Tree Growth prototype once the owner has looked at it. Open: palm fronds (cards vs blocks).
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

## Done

- **Underwater surface** (0.52.49): Snell's window as data (`underwater.json` `window`), the refracted
  sky, the sun and moon discs, glow by the sky's light; total internal reflection outside it. Not yet
  seen in frame: the night moon disc and the sunset sun disc from below. Owner, 2026-11-11: "We need to
  make underwater surface layer look transparent like the surface and look more accurate to what light
  is shining down."
- **Creative breaking fix** (0.52.51): in creative anything but a tool breaks every block in one hit
  (predicted); a tool keeps its hits; adventure unchanged.

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

- Should the stone pickaxe wear too (only the stone axe has durability, 32 hits)?
- Should tools wear in creative (`creativeWear` is false)?
- Plain dirt now also drops random rocks ("grass and dirt"): confirm.
- Palm fronds: leaf cards or blocks (after the Tree Growth prototype).

## Decided and not to be revisited

- **Crafting proposal shelved** (2026-11-11): "That's a lot of assumptions I don't think I want to do
  any of those crafting question things for now." Only the pieces and combine recipes were built.
- **Pieces:** 8 rock shards → a stone slab, 8 dirt clumps → a dirt slab, 8 wood chunks → a log slab;
  rocks still found in grass and dirt at random; sticks from leaves. Stone tools: 2 sticks + 2 rock
  shards + 2 grass fibers. Stone axe durability 32.
- **Volcanoes:** dormant for now, eruptions later.
- **Backpack:** 30 slots.
