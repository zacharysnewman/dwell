# Proposal: chopping, redesigned (aim, rhythm, feedback)

**Status:** recorded 2026-11-10 from the owner's direction; replaces parts of ADR 0067 (the stone axe
and chopping) when adopted. Follows the tools update (0.51.24).

## 1. The owner's direction (2026-11-10)

> "We should discuss the tree chopping system. It's pretty bad, unintuitive and doesn't have good visual
> feedback."
>
> "1) Where to aim. The chop animation also doesn't reflect what you're seeing at all.
> 2) Perhaps at the beginning we could just have a decent chopping animation, you can hold chop to
> continue chopping. The way it works in real trees is it's a series of higher/lower chops. Can you
> find some good animation reference points for chopping? First person swings reference points from
> Dying Light."
>
> On where the blade lands: "I'm not sure if it should land directly on your crosshair or if that
> should be the end of the sweep from the side or the top" → agreed: **the crosshair is where the blade
> bites; each swing travels into that point from its own side.**

Also: a cut log turns square instead of octagonal (0.51.x; the carve is still a cube, ADR 0072).

## 2. What is there today (ADR 0067)

A level or vertical swing (two holds); a sweep along an arc finds the first wood it crosses; each good
swing bites a V-notch deeper on the side hit, by how square to the grain the swing's plane is; swings
past the notch cut kerfs; past the felling point the cut cell splits into wedges and the trunk tips
toward the notch (0.51.0). Feedback: the carved notch, chips, a hit-stop, sounds.

Why it feels bad: the aim is indirect (where an arc crosses the trunk), the swing plane and bite
maths are invisible to the player, the animation doesn't land where the cut appears, and nothing says
how close the tree is to falling.

## 3. References

**Real felling with an axe** (timber-faller and felling guides): the notch is two cuts — a level cut
and a ~45° cut — meeting in a wedge; the chip between them pops out. Strokes **alternate** between the
two and stay even so both faces of the V grow together. Notch between knee and waist height, on the
side the tree should fall, to about a third of the way through; then a cut from the opposite side, a
little above the notch, leaving a hinge that steers the fall.
([awesomeaxes](https://www.awesomeaxes.com/how-to-notch-a-tree-with-an-axe/),
[themanual](https://themanual.com/outdoors/how-to-fell-a-tree),
[knivesandtools](https://www.knivesandtools.com/en/ct/how-to-fell-a-tree.htm))

**First-person melee (Dying Light):** no frame breakdown is published; Techland hand-animates first
person and draws on film, wrestling and martial arts
([3dvf interview](https://3dvf.com/en/redaction/dying-light-2-stay-human-our-interview-with-techland-about-the-gut-feeling-update-focused-on-combat-animations-and-physics/));
later coverage credits small hand shifts and weight transfers for momentum. The usual beats of a
weighty first-person swing, to tune against footage:
1. **Wind-up** — arms pull back over the shoulder, a small camera roll the other way.
2. **Strike** — fast (~0.1 s).
3. **Impact** — a short hit-stop (~0.05–0.08 s), a small camera kick toward the hit, the axe stuck in
   the wood for a beat.
4. **Recovery** — a tug to free it, back to ready.

## 4. The design

1. **Aim:** the crosshair is the bite point. A faint **ghost notch** shows on the bark where the next
   chop will land; once a notch is started, the aim **locks onto it** (it can't drift to a new cut).
2. **Hold to chop:** holding the button chops on a rhythm, **alternating a high strike** (from upper
   right, ~45° down) **and a low strike** (level, from the side), both ending with the blade in the
   wood at the crosshair. Each pair knocks out a visible chip and deepens the V a step. A tap is one
   strike (the next of the pair).
3. **The animation matches what you see:** the axe head is placed by IK so it arrives exactly at the
   aimed bark point, on the cut's plane; the chip flies from there; the camera kick points at it.
4. **Felling feedback:** past ~⅓ depth the tree **creaks** and shows **cracks** at the hinge; it
   **sways** a little with each hit; the fall goes toward the notch, the hinge snapping (wedges,
   0.51.0).
5. **Simplify:** drop the level/vertical hold choice and the grain-angle bite maths; the notch side
   decides the fall. A back cut is optional (see §6).
6. **Octagon:** the notch is carved into the octagonal log (clip the octagon per step along the
   grain; caps and bark ring as `cap_pieces`), so a cut log stays octagonal.
7. **Data:** the rhythm, strike timings, hit-stop, kick, notch step size, creak/sway thresholds as data
   on the tool (`shared/tools/tools.json`), tunable without code.

## 5. Phases (≤ 45 min each)

1. **C1:** aim — the bite point from the crosshair, the ghost notch, the aim lock; the sweep replaced.
2. **C2:** hold-to-chop rhythm (alternating high/low), the notch growing a chip per pair, data.
3. **C3:** the first-person animation landing on the bite point (IK), the four beats, camera kick.
4. **C4:** felling feedback (creak, cracks, sway) and the octagonal notch carve.

## 6. Questions

1. Should the player also make a **back cut** from the far side (real technique, more skill), or does
   the notch alone fell the tree once it is deep enough (simpler)?
2. How many chops should a typical tree take (e.g. 6–10 strikes)?
3. Should **where** on the trunk you start the notch matter (height, side) beyond choosing the fall
   direction?
4. Footage: are there Dying Light clips you'd like the swing timed against?
