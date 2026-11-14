# Camera patterns — distilled from "Camera Angles & Movements: A Mechanical Spec for AI Agents"

The record skill's reference for planning shots (owner, 2026-11-13, WORK_QUEUE item 40: "Make a 60s or less cinematic
trailer for our game. Use these patterns (document the patterns)"). Source: the owner's Claude Doc "Camera Angles &
Movements: A Mechanical Spec for AI Agents", https://claude.ai/artifact/LgmAqzqGrNoyx9fn9QWHTU (rev 18, 2026-11-13),
read in full and distilled here; the last part maps each pattern to Dwell's theater and record tools and lists the
gaps. Use it with [SKILL.md](SKILL.md): pick a pattern by intent (§10), solve its numbers (§1–§5), then write it as a
theater path or a take in a manifest. Doc sections, in order: Purpose and scope; Camera model; Shot sizes; Camera angles; Camera
movements; Compound moves; Film juice: camera; Film juice: subjects and blocking; Cuts and editing (+ Transitions);
Trailers and extra polish; Intent-to-shot mapping; Shot description schema; Reference shots; Validation checks.

---

## 0. Scope and conventions
- Every shot = constraints on 7 numbers (position x,y,z; yaw, pitch, roll; focal length) relative to a subject,
  plus a narrative purpose. Applies to 3D engines, virtual production, robotic rigs, gen-video prompts. Lighting,
  colour, cuts, transitions, trailer editing only as far as they shape shot planning.
- All values are defaults; ranges are where a shot still reads as that shot (outside → a different shot).

## 1. Camera model
- World: right-handed, y up, metres, ground y = 0. Subject frame: anchor S (a person's **eyes**, default 1.6 m),
  forward F = facing direction.
- Symbols: P position; d = |P−S| (from shot size); h camera height (default = S height); yaw (0 = facing
  subject's front); pitch (+ up, − down); roll (around lens axis); f focal length, full-frame 36×24 mm equiv,
  default **35 mm**; vFOV from f; t normalized shot time 0→1.
- **vFOV = 2·atan(24 / 2f)** → 24 mm ≈ 53°, 35 mm ≈ 38°, 50 mm ≈ 27°, 85 mm ≈ 16°.
- **Frame height at subject H = 2d·tan(vFOV/2) = 24d / f** (so d = H·f/24).
- **Look-at rule:** orientation solved every frame so the lens axis passes through the framing target, placed by
  the rule of thirds; a person's eyes on the upper third line (~1/3 down from top).
- Lens character: **wide ≤ 28 mm** exaggerates depth/speed, distorts faces close (energy, space, unease);
  **normal 35–50 mm** ≈ human eye (neutral coverage); **long ≥ 85 mm** compresses depth, flatters faces, isolates on
  soft background (intimacy, surveillance, packed crowds).

## 2. Shot sizes (H = vertical slice filling frame; d for a 1.8 m person at 50 mm)
| Size | Covers | H m | d m @50 | Use | Example |
|---|---|---|---|---|---|
| ECU | eyes / one detail | 0.15 | 0.3 | peak emotion, crucial clue | sweat bead; finger on trigger |
| CU | head to top of shoulders | 0.35 | 0.7 | reading a face, reactions | hearing bad news on phone |
| MCU | head to mid-chest | 0.6 | 1.25 | default for dialogue | friends at a bar |
| MS | head to waist | 0.9 | 1.9 | gesture + expression | chef explaining while plating |
| Medium long / cowboy | head to mid-thigh | 1.3 | 2.7 | body language, hands at hips | standoff |
| Full / long (LS) | whole body, small margin | 2.1 | 4.4 | movement, costume, blocking | dancer's routine |
| Wide (WS) | subject + surroundings, subject ≈ 1/3 frame height | 6 | 12.5 | who is where | detective enters scene |
| EWS / establishing | location dominates, subject tiny/absent | ≥ 30 | ≥ 60 (often longer or wider lens) | place, scale, isolation | lone car on desert highway |
- **Headroom:** 5–10% of frame above head in CU–MS; ECU crops top of head, eyes on upper third.
- **Edges:** never crop at a joint (neck, elbow, waist, knee, ankle); shift H ~10% to land between joints.

## 3. Camera angles (static; solve h first, then pitch = atan((S_y − h)/horizontal distance); low camera → + pitch)
| Angle | Rule | Conveys | Example |
|---|---|---|---|
| Eye level | h = S_y ± 0.1 m; pitch ≈ 0° (±5°) | neutral, honest, equal | dialogue; job interview |
| Low | h below eyes; pitch +10° to +45° | power, menace, heroism, scale | villain out of a car; hero rising |
| Worm's-eye | h ≤ 0.3 m; pitch +45° to +90° | monumental scale, creature view | skyscrapers; giant over a soldier |
| High | h above eyes; pitch −10° to −45° | vulnerability, defeat, being watched | lost child; suspect in a cell |
| Bird's-eye / overhead | directly above S; pitch −90°; yaw free | pattern, choreography, fate, detachment | heist table plan; body on floor; chase in city grid |
| Aerial | h ≥ 30 m, often moving; pitch −15° to −90° | geography, scale, journeys | opening on a city; convoy over mountains |
| Dutch / canted | any of the above + roll 10°–45° (sign = direction of unease) | disorientation, madness, moral wrongness | drugged stumble; conspiracy reveal |
| OTS | P behind and 0.3–0.6 m beside foreground subject A at A's eye height; target B's eyes; A's shoulder + back of head fill 1/4–1/3 frame | connect two people; whose side we're on | negotiation; couple arguing |
| POV | P = character's eye; orientation = gaze; + their head motion (slight handheld) | audience inside a character | waking in hospital; keyhole |
| Profile | yaw ±90° | observation; two faces in one frame | rivals nose to nose |
| Three-quarter | yaw ±30°–45° | default flattering, depth on face | standard coverage of a speaker |
| Frontal | yaw 0° | confrontation, direct address, symmetry | confession to camera; Wes Anderson tableau |
- **Strength scales with magnitude:** 10° low = confident, 40° = threatening; use the smallest value that delivers.
- **180° rule:** line through two interacting subjects; all cameras in a scene stay on one side (screen direction
  consistent). Cross only on purpose, with a visible move or a neutral shot on the line.

## 4. Camera movements (functions of t; e(t) = easing)
Rotation-only (pan, tilt, roll, zoom): P fixed, **no parallax**. Translation (dolly, truck, pedestal, crane, arc):
P changes, near objects slide faster → physical, 3D feel.
| Move | Changes / locked | Equation | Speed | Use | Feel notes |
|---|---|---|---|---|---|
| Pan | yaw / P, pitch, roll, f | yaw₀ + Δyaw·e(t) | 5–30°/s | follow sideways, reveal what a character sees, connect two things (runner; phone → person ignoring it) | lead subject with 1/3 frame ahead; too fast strobes: wide shots < ~1 frame width per 7 s at 24 fps |
| Tilt | pitch / P, yaw, roll, f | pitch₀ + Δpitch·e(t) | 5–20°/s | reveal height, size up a person (skyscraper; boots → face) | end on the eyes; hold 0.5–1 s before/after |
| Roll | roll / all else | roll₀ + Δroll·e(t) | 5–45°/s | disorientation, into dream/drugged state (faint) | rare; motivated cause (impact, spin) |
| Zoom | f / P, orientation | f₀·(f₁/f₀)^e(t) (exponential) | 0.5–5 s for 2× | attention without moving; documentary/1970s; slow zoom as a lie lands; crash zoom | linear f feels accelerating; flattens space → observational not experiential |
| Dolly in/out | P along lens axis / orientation (or look-at), f | P₀ + Δd·e(t)·forward | 0.2–1 m/s | in: intensify, approach realization; out: isolate, reveal context, let go | slow push 5–10% closer over 5 s ≈ subliminal, very effective; foreground objects sell parallax |
| Tracking / follow | P beside/behind moving subject / relative offset | Subject(t)+offset (smoothed) | match subject | stay with a character through space; immersion (kitchen; car chase) | lag 0.2–0.4 s spring smoothing → operated, not glued |
| Truck | P sideways ⟂ lens axis / orientation, f | P₀ + Δx·e(t)·right | 0.2–1 m/s | reveal across a space; parallel to a walker (row of prisoners; lineup) | foreground wipes add depth |
| Pedestal | P vertical / orientation (pitch constant), f | h₀ + Δh·e(t) | 0.1–0.5 m/s | shift power within a shot; rise over an obstacle (rise from behind a table) | ≠ tilt: horizon line moves, perspective changes |
| Crane / jib | P on vertical-plane arc, often + tilt / f | pivot + R·[cos θ(t), sin θ(t)], look-at | 1–3 m/s at arm tip | grand openings/endings, scope, leaving a character (kiss → city; descend into ballroom) | start or end on stillness; crane up and away at scene end = "the story moves on" |
| Arc / orbit | P on horizontal circle / radius, h, look-at S | S + r·[sin θ, 0, cos θ] | 10–30°/s | heighten a moment, all sides, turning point (first kiss; hero surrounded) | spinning background behind still subject is the effect; slightly longer lens for separation |
| Handheld | small noise, 6 axes / intended path | path(t)+noise(t) | — | realism, chaos, urgency, documentary | layered noise, not per-frame jitter; operators drift and correct |
| Steadicam / gimbal | P on spline; orientation free / horizon (roll ≈ 0) | spline(t), soft-damped look-at | walking 1–1.5 m/s | long immersive walks; dreamy floating (nightclub to back room) | very low-frequency height drift 0.1–0.3 Hz, no jitter |
| Whip pan | yaw very fast / all else | over 0.1–0.3 s with motion blur | 300–1000°/s | energy, comedy, transition between spaces/times | blur hides a mid cut; ease out so it lands |
| Snap / crash zoom | f very fast / P, orientation | f ×2 to ×4 in 0.1–0.3 s | — | shock, comedy, homage (villain unmasked) | tiny overshoot + settle, 2–3% of f |
| Rack focus | focus distance / all else | d_near → d_far with e(t) | 0.5–2 s | redirect attention between depths (phone → hand) | needs shallow DOF (open aperture, long lens); ease in/out; slight overshoot reads human |
- **Terminology:** "dolly"/"tracking" often interchangeable; "push/pull" may mean dolly or zoom. Resolve: does P
  change (dolly/track) or only the lens (zoom)? Bare "push in" → physical dolly in (parallax, more cinematic).

## 5. Compound moves
- **Dolly zoom (Vertigo; Hitchcock 1958, Spielberg's Jaws beach 1975):** dolly one way, zoom the other, H constant:
  **f(t) = f₀·d(t)/d₀**. Dolly in + zoom out → background expands away (world opening, vertigo). Dolly out + zoom
  in → background rushes toward subject (dread, realization closing in). Use for one moment of internal shock
  (lifeguard spots attack; betrayal). Keep eyes locked to the same frame position every frame; **2× f over 2–4 s is
  plenty**; needs deep background detail.
- Push-in + tilt down (approach and examine: desk → note). Crane down + track (enter the world, join a character:
  rooftops → street). Arc + push (build to climax: circle and tighten on hero). Pan + truck = parallax pan (stationary
  subject feels alive: slide past someone at a window). Pull-out reveal (dolly out / crane up from CU: recontextualize,
  smiling face → funeral). Oner / long take (any chain, no cuts: real-time tension, virtuosity; restaurant back entrance).
- **Chaining rule (long takes):** never start/stop two moves on the same frame; overlap 0.3–1 s so one hands off.

## 6. Film juice — the camera
1. **Mass and easing:** never linear. Default e(t) = **smootherstep 6t⁵ − 15t⁴ + 10t³**. **Asymmetric** ease: in over
   first 20–30%, out over last 30–40% (slow landing = deliberate). Heavier rigs ease longer: crane 1–2 s ramps, dolly
   0.5–1 s, handheld 0.1–0.3 s. **Overshoot and settle** on fast moves (whip, snap zoom, a stopping follow): overshoot
   1–3% of the move, settle 0.2–0.5 s, damped spring ratio 0.6–0.8.
2. **Motivation:** by subject (start 2–6 frames after the subject moves, like an operator reacting; actor stands →
   tilt just after); by attention (push-in during the important line, not before); by emotion (drift speed = scene
   energy: slow creep dread, quick handheld panic). Unmotivated moves slow and rare (authorial: fate, memory, narrator).
3. **Depth and parallax:** layer foreground (out of focus, partly in frame) / midground subject / background;
   translation through three layers = strong parallax. Foreground wipes (pillar, plant, doorway near lens during a
   truck) add speed and can hide a cut. Shallow DOF f/1.4–2.8 for intimacy; deep f/8–16 for scope/ensemble.
4. **Noise:** layered smooth noise (2–3 Perlin/simplex octaves on rotation, a little on position). Handheld:
   0.3–1.5° amplitude, 0.5–2 Hz sway + 4–8 Hz micro-shake at 10–20% amplitude, reframing corrections every 2–5 s.
   Locked tripod: 0.05–0.1° at 0.1 Hz (perfect stillness looks synthetic). Scale to emotion: calm barely there,
   panic ×3 amplitude and higher frequency. Roll noise ≤ 30% of pan/tilt noise.
5. **Lens/sensor:** 180° shutter motion blur (1/48 s at 24 fps; fast pans without blur strobe). 24 fps for cinema
   (higher reads as video/sports). Focus pull lags subject and settles, never snaps. Vignette, slight barrel on
   wides, aperture-shaped bokeh if available.
6. **Framing in motion:** lead room ≥ 1/3 frame ahead of moving/looking subject (less = tension, on purpose).
   Reframe continuously but late: let the subject drift 5–10% off mark before correcting. **Hold 0.5–1.5 s** still
   before a move begins and after it ends. Every move lands on a frame that works as a still.

## 7. Film juice — subjects and blocking
1. **Subjects change shot size:** walk into a close-up (static camera, full → CU; confident); walk away to isolate
   (shrink into a wide; fired, long hallway). Rule: internal emotion → move the subject; change of audience
   perspective → move the camera.
2. **Staging in depth:** use z, not only left-right (wife in foreground CU, husband on phone deep behind). Nearer
   subject dominates; swap mid-scene for a power shift. Triangles beat lines; vary heights.
3. **Eyelines:** single = lens 5–15° off the gaze (closer = intimate, wider = observational). Shot/reverse:
   opposite sides of frame, same height. Looking into the lens breaks the fourth wall (direct address or a jolt
   only). Gaze directs attention; a glance motivates a pan or cut.
4. **Motion vs frame:** toward camera = threat/arrival; away = departure/loss/journey; L→R (Western) = progress;
   R→L = resistance/return; diagonal = energy, depth; entering frame = surprise; stillness amid motion =
   isolation/focus. Keep screen direction across cuts until the story turns them.
5. **Camera/subject counterpoint:** counter-move (opposite directions: fast parallax, fights); matched move (same
   speed: walk-and-talk, immersion); converging (push in as subject approaches: doubles intensity, climaxes);
   diverging (pull back as they approach: size steady, world revealed); hand-offs (follow A until they pass B, pick
   up B: party introductions, long takes).
6. **Performance timing:** start the move on the anticipation (weight shift, glance), not the action. Time camera
   arrivals to the beat (reaction, key word). Micro-motion: breath every 3–5 s, blink every 2–10 s, weight shifts.
   Marks missed by 2–5 cm; camera reframes.
7. **Multi-subject composition:** dirty single (sliver of other's shoulder) connects; clean single isolates.
   Two-shot balance: equal size/height = equals; unequal size/height/focus = power imbalance. Frames within frames
   (doorways, windows, mirrors) for entrapment, observation, separation.

## 8. Cuts and editing
| Cut | Mechanic | Use |
|---|---|---|
| Cut on action | cut mid-movement, continues from new angle | invisible continuity (door in wide → handle CU) |
| Match cut | shape/motion/composition matches | link ideas/times/places (bone → spaceship) |
| Smash cut | abrupt jump to the opposite | shock, comedy |
| Jump cut | same angle, time skipped | time passing, restlessness, vlog (packing in 6 jumps) |
| J-cut | next shot's audio before its picture | pull forward (train heard before station) |
| L-cut | current audio over next picture | linger, reaction |
| Cross-cut | alternate two simultaneous events | tension toward a meeting point |
| Shot / reverse shot | alternate speakers, same side of 180° line | dialogue |
| Insert / cutaway | brief detail / elsewhere | clue, info, rhythm |
| Hidden cut | during whip, foreground wipe or darkness | fake long take |
| Montage | short shots compressing time | training, travel, months |
| Hard cut to black | picture to black, often silence | finality, gut punch |
- **30° rule:** consecutive shots of same subject differ ≥ 30° angle or one size step, else reads as a mistake.
- Cut on motion hides the cut; cut on stillness makes it felt.
- **Shot length = pace:** 4–8 s drama, 1.5–3 s action, < 1 s trailer montage; shorten to build tension, then hold a
  long shot for the payoff.
- Move into move: same direction and similar speed across a cut = seamless; opposing = jarring (fights).
- Eye trace: shot B's point of interest where the viewer was looking at the end of A.

### Transitions (anything but a straight cut; few per film)
| Transition | Mechanic | Duration | Use |
|---|---|---|---|
| Fade from/to black | ramp from/to black | 0.5–2 s | start/end scene, act, film; long time jump |
| Fade to white | ramp to white | 0.3–1.5 s | death, dream, memory, explosion/flash |
| Dissolve (cross-fade) | A out while B in, overlapping | 0.5–2 s | gentle time passing, link related images |
| Match dissolve | dissolve between matching shapes | 1–2 s | poetic link (moon → ceiling lamp) |
| Wipe | line/shape sweeps | 0.3–1 s | playful, retro, serial |
| Iris | circle closes/opens on black | 0.5–1 s | silent-film homage, comedy, one detail |
| Whip-pan transition | whip out of A, into B same direction, cut in blur | 0.2–0.4 s total | energetic jumps in place/time |
| Object / body wipe | something fills the frame near lens; cut while covered | 0.1–0.5 s | seamless location change, fake long take |
| Push / zoom-through | push into dark area (mouth, doorway, eye), emerge in B | 0.3–1 s | going inside, travel into a world |
| Graphic match | last frame of A and first of B share shape/position | instant | strong linkage (clock → porthole) |
| Flash frame | 1–3 frames white/overexposed | 1–3 frames | trailers, memories, impacts |
| Speed ramp transition | ramp A to very fast, cut, ramp B down to normal | 0.3–0.8 s | music videos, action trailers, travel |
| Sound bridge | J/L-cut audio | 0.5–3 s | smooth scene change without visuals |
| Morph / form cut | object transforms into another | 0.5–1.5 s | stylized, surreal, tech |
- Rules: **default to a straight cut**, transition only when time, place or reality changes; **one transition
  language per film**; plan both sides (whip/object wipe needs matching motion, direction and speed at end of A and
  start of B); longer transition = bigger change (2 s fade > 0.5 s dissolve).

## 9. Trailers and polish
- **Structure (1.5–2.5 min, built around music):** Setup 0–30 s (calm, longer shots, world + hero, often one voice
  or line) → Disruption 30–60 s (inciting event, a sound hit, shots shorten) → Escalation 60–110 s (action montage,
  rising music, title cards between beats) → Drop or silence (a held shot / silence before climax) → Peak montage
  (fastest cuts, biggest moves, < 1 s each) → Title and button (title card, then a final short joke or scare).
- **Techniques:** cut to the beat (cuts on drum hits/bass drops; big moves start on downbeats); hit points (braam,
  slam + cut to black or hard visual change); speed ramps (normal → slow-mo at peak → back); black frames and title
  cards for rhythm; **hero shot** = low angle, slow motion, backlit, walking toward camera ("the poster in motion");
  silence 0.5–2 s before the hit; reverse and repeat (striking image early, same image in new context later).
- **Polish:** silhouettes/backlight; motivated light (window, lamp, fire; hard = drama, soft = warmth; leave shadows
  dark); colour contrast (warm subject on cool background, or one accent colour in a muted frame); negative space
  (loneliness, awe); symmetry (order, menace, stylized); atmosphere (haze, fog, dust, rain shape light, add layers);
  slow motion (shoot 48–120 fps, play 24; sparingly, peak moment); aspect 2.39:1 epic, 1.85:1 more intimate, 4:3
  vintage/claustrophobic; anamorphic (oval bokeh, horizontal flares, edge distortion); reveal by withholding (out of
  frame, out of focus, in shadow until the moment).

## 10. Intent → shot (pick by intent, then tune intensity by magnitude: angle steepness, move speed, lens length)
| Intent | Size | Angle | Move | Blocking | Lens |
|---|---|---|---|---|---|
| Establish a place | EWS/WS | high or aerial | slow crane, drone push, or locked | small/absent; one figure for scale | 24–35 |
| Character with power | MS → CU | low 15–25° | slow push-in or tilt up from feet | walks toward camera, stops on mark | 35–50 |
| Mystery | CU details → reveal | eye level | pan/truck across details; pull-out reveal | face withheld until end | 50–85 |
| Calm conversation | MCU singles + OTS | eye level, ¾ | locked, micro drift | seated/still, matched eyelines | 35–50 |
| Rising tension (conv.) | tighten each cut toward CU | eye level → slight Dutch | very slow push-ins on both singles | lean in, less movement | 50–85 |
| Revelation / shock | CU | eye level | fast push or dolly zoom | freeze, small reaction | 35–50 |
| Vulnerability, defeat | MS/WS | high 20–45° | crane up and away, or locked | small in frame, low to ground | 24–35 |
| Chaos, action | mixed, short | varied, Dutch allowed | handheld, whip pans | diagonals, entering/exiting | 18–35 |
| Romance, intimacy | CU, dirty singles | eye level, near eyeline | slow arc or gentle float | close, soft touches, stillness | 85+, shallow |
| Dread, being watched | WS from hiding | high or low + foreground clutter | slow creep, very slow push | unaware, back to camera | 85+ (voyeur) or 18 (creeping close) |
| Triumph | LS → WS | low | crane up, orbit | arms rise, faces up, toward camera | 24–35 |
| Ending / farewell | WS → EWS | eye level or high | pull-out, crane up, locked as subject leaves | walks away | 35 |
- **Contrast is the engine:** a shot's effect comes from the one before (calm locked wide → handheld CU hits harder
  than handheld throughout). Plan sequences: vary size ≥ one step between cuts; save the strongest angle/move for
  the peak beat.

## 11. Shot description schema (per shot; adapters turn it into keyframes)
Fields (example `sc04_sh07`, intent "revelation", 4.5 s, 24 fps): `shot_id`, `intent`, `beat`, `duration_s`, `fps`,
`size {start, end}` (MS → CU), `angle {vertical, pitch_deg, yaw_from_subject_front_deg (35), roll_deg}`,
`lens {focal_mm {start 35, end 70}, aperture 2.0, focus_target "maya.eyes"}`, `subjects[] {id, anchor, screen_pos
{x 0.62, y 0.33}, blocking[] {t, action, position | gaze_offset_deg}, micro_motion {breath_period_s 4, blink}}`,
`moves[] {type "dolly_zoom", direction "out_zoom_in", t_start 0.5, t_end 0.9, easing "smootherstep", lock
"maya.eyes"}`, `feel {rig, motivation, reaction_delay_frames 4, noise {profile "tripod_drift", amplitude_deg 0.08},
overshoot_pct, hold_start_s 1.0, hold_end_s 0.8, motion_blur_shutter_deg 180}`, `continuity {axis_side,
screen_direction}`. **Required:** shot_id, intent, duration_s, size, angle, lens.focal_mm, subjects (≥ 1 or an
explicit empty list for pure environment), moves (empty = locked).

## 12. Reference shots (style anchors)
Dutch: The Third Man (1949). Dolly zoom: Vertigo (1958), Jaws (1975). Steadicam oner: Goodfellas (1990) Copacabana.
Opening long take: Touch of Evil (1958, crane-heavy). Whole-film oner: 1917 (2019, hidden cuts). Low-angle villain
intro (below eyeline, steps toward lens, slow push/tilt up). Overhead in heist/crime films. Handheld chaos in
war/documentary (layered shake, late reframes, entries/exits).

## 13. Validation checks (fail = fix or justify)
- **Mechanics:** subject anchor in frame all shot (unless exit intended); start/end size within ±15% of H; no crop on
  a joint; camera never clips geometry or comes within 0.3 m of a subject; pan under strobing limit or blur on; zoom
  exponential; dolly zoom holds subject size within ±3%.
- **Feel:** no linear easing; every move motivated (subject/attention/emotion) or marked authorial; noise on every
  shot incl. locked, scaled to emotion; moves start 2–6 frames after the cue; holds ≥ 0.5 s at both ends; final
  frame a composed still.
- **Subjects:** lead room ≥ 1/3 (unless tension); eyelines match in shot/reverse; micro-motion present; staging in
  depth.
- **Sequence:** one side of the 180° line (or crossing shown); consistent screen direction; consecutive shots differ
  ≥ one size step or 30°; strongest angle/move saved for the peak.

---

## Mapping to Dwell's theater/record tools

Sources: `docs/THEATER_MODE.md`, `shared/theater/presets.json`, `.claude/skills/record/SKILL.md`, and a check of
`crates/dwell-theater/src/eval.rs` / `path.rs`.

**Dwell's vocabulary.** Keyframes `{t, position, look [yaw, pitch] | target (point, "player", {creature}, {npc}),
fov (vertical°, 5–150), timeOfDay, speed (0, 1000], up a|b|auto}` (fields carry forward). Segment moves: `cut`,
`linear`, `ease`, `orbit` (`around`, `degrees`), `track` (`target`, `offset` [0,2,−6], `smoothingS` 0.5), `dolly`
(`at`, `vertigo`), `crane` (`upEasing`), `pan`. Easings: linear, smooth (smoothstep), in/out/inOut × Quad/Cubic/Sine.
Presets: orbit (radiusM, heightM, degrees), dolly (fromM, toM, heightM, vertigo), track (distanceM, sideM, heightM,
smoothingS), rise = crane (backM, fromM, toM), pan (degrees, pitch); all take seconds, fov, timeFrom/timeTo, days,
up. Recording: in-game MediaRecorder (9:16/16:9, 24/30/60 fps, real time) or the record skill's frame-exact
screencast; `splice.py`: from/to, 0.8 s cross-fade (default), `cut: true` hard cut, `speed`/`speedFrom`→`speedTo`
ramps, `vf` grade, fade-out over the last second, audio keep/stretch/mute following cuts and fades.
**Owner's format overrides the spec:** 9:16 1080×1920, 30 fps (not 24), ≤ 1 min, no text/title cards, no music
(effects and ambience only). **The trailer (2026-11-13)** adds a 16:9 1920×1080 cut and one title card ("Let's do
vertical and widescreen. Title and game sound, no music."): a manifest's `aspects` and `title` (SKILL.md §0c).

**Unit conversions.** Dwell's `fov` is vertical degrees, so use the spec's table directly: 24 mm → fov 53, 35 → 38,
50 → 27, 85 → 16. Distance for a shot size: d = H / (2·tan(fov/2)) (= H·f/24). In 9:16 the same vFOV gives a much
narrower horizontal view than the spec's 3:2 full frame, so wides need a wider fov or more distance; portrait suits
CU–LS and tall subjects (the skill: frame tall, slower horizontal moves). Dwell's player is not 1.8 m with eyes at
1.6 m necessarily; measure from `d.player()` before solving H.

### Shot sizes and angles
| Pattern | In Dwell | Status |
|---|---|---|
| ECU → EWS sizes | keyframe `position` at d from H and `fov`; `target` the subject | doable by hand (no size solver; compute d) |
| Headroom / eyes on upper third | `target` puts the target on the **frame centre**; aim at a point ~(1/6 of H) below the eyes to lift them | workaround (no screen-position offset) |
| Eye level / low / worm's-eye / high | keyframe height vs the target's height; pitch follows from `target` | yes |
| Bird's-eye pitch −90° | look pitch clamps to ±90; a `target` straight below works | yes (check the yaw at the pole) |
| Aerial | `rise` preset, `ease`/`linear` drone passes (skill lessons) | yes |
| Dutch / canted (roll 10–45°) | no roll field in keyframes or presets | **no** (only a post-rotate+crop in `vf`) |
| OTS | position behind/beside A at A's eye height, `target` B (`{npc}`/`{creature}`/point) | by hand; only the player is directable as A |
| POV | first-person gameplay (the record skill's player shots), no theater path | yes |
| Profile / ¾ / frontal | yaw around the subject via keyframe position, or `orbit` with `degrees` to land at ±90/±35/0 | yes, by hand (no subject-relative yaw field; facing must be read from `d.creatures()` yaw / player look) |
| 180° rule, 30° rule | planning only | manual |

### Movements
| Pattern | In Dwell | Status / gap |
|---|---|---|
| Pan | `pan` segment / `pan` preset (degrees, pitch) | yes |
| Tilt | `pan` segment between keyframes differing in pitch (slerped look) | yes (no tilt preset) |
| Roll | — | **no** |
| Zoom | `fov` on two keyframes, P fixed (`pan` segment, same look) | partial: fov lerps **linearly in degrees** with the segment easing, not exponentially in f |
| Snap/crash zoom + overshoot | short fov segment (0.1–0.3 s) | partial: no overshoot/spring easing |
| Dolly in/out | `dolly` preset (fromM → toM, heightM) / `dolly` segment with `at` | yes |
| Slow push 5–10% over 5 s | dolly with fromM/toM ≈ 1.07 ratio, seconds 5 | yes |
| Tracking / follow | `track` (offset, `smoothingS`; spec's 0.2–0.4 s lag → set 0.2–0.4, default is 0.5–0.6) | yes, exponential lag (not a spring) |
| Truck | `ease`/`linear` sideways with the same `look` on both keyframes (or `target` for a parallax pan) | yes, no preset |
| Pedestal | `ease` vertical with the same look | yes, no preset |
| Crane / jib | `crane` (across with easing, up with `upEasing`), `rise` preset; descents = negative rise | approximate: an eased up/across, not a true pivot arc |
| Arc / orbit | `orbit` (radius, height, degrees; radius/height ease between keyframes) | yes |
| Handheld / tripod drift / steadicam float | — | **no** noise layer (perfectly smooth paths) |
| Whip pan (300–1000°/s, 0.1–0.3 s) | `pan` segment of 0.1–0.3 s | partial: **no camera motion blur** (the renderer's blur is the wind effect only), so it strobes; no overshoot |
| Rack focus / shallow DOF | — | **no** (no depth of field in the renderer) |
| Terminology "push in" | default to `dolly` | yes |

### Compounds, chaining, easing
| Pattern | In Dwell | Status |
|---|---|---|
| Dolly zoom (Vertigo) | `dolly` with `vertigo: 1` (needs keyframe i's fov; fov solved to hold the subject's size, tested) | yes |
| Push-in + tilt down | `dolly` with `at`, or `ease` with a lower target | yes |
| Crane down + track | `crane` segment then `track` segment | yes (sequential) |
| Arc + push | `orbit` with keyframe radii shrinking (radius eases between keyframes) | yes |
| Parallax pan (truck + pan) | `ease`/`linear` sideways with `target` on the subject | yes |
| Pull-out reveal | `dolly` out (fromM < toM) or `rise` | yes |
| Oner | one multi-segment path; hidden cuts via `cut` segments | partial (see overlap) |
| Overlapping moves 0.3–1 s | one move per segment, segments don't overlap; only orbit/crane combine axes inside one segment | **no** general overlap/blend |
| Smootherstep default | `smooth` is smoothstep; `inOutCubic`/`inOutSine` close | approximate (no quintic) |
| Asymmetric ease (in 20–30%, out 30–40%) | — | **no** asymmetric curve (only split into segments) |
| Overshoot and settle | — | **no** (no back/spring easing) |
| Hold 0.5–1.5 s at ends | two identical keyframes (a `pan`/`ease` segment with no change) or `warmS` + splice `from`/`to` trim | yes |
| Reaction delay 2–6 frames | manual (start the path after the cue) | manual |
| Lead room ≥ 1/3 | `track` `sideM`/offset, but the look stays on the target (centred) | **no** screen-space offset |
| Late reframe (drift 5–10%) | `smoothingS` lag approximates | partial |

### Subjects and blocking
- Player: scriptable (`d.press`, `d.look`, `d.fly`, teleport) → walk into a close-up, walk away to isolate,
  converging/diverging (dolly toward/away while the player walks), matched move (`track`). Creatures/NPCs: only
  aimed at (`{creature: id}`/`{npc: id}`), not directed; follow-by-id from the picker isn't built (THEATER_MODE §0).
  Hand-offs (follow A then B) = two `track` segments with different targets, no blended hand-off.
- Micro-motion, marks, eyelines: game-driven, not controllable. Counter-moves: a `linear` segment against the
  player's walk.

### Cuts, transitions, trailer
| Pattern | In Dwell | Status |
|---|---|---|
| Straight cut, cut on action, jump cut, montage, shot length pacing | splice `cut: true`, `from`/`to`; `cut` segments in a path | yes |
| Dissolve | splice default cross-fade (fixed 0.8 s) | yes, duration not per clip (fixed) |
| Fade out at end | splice fades the last second | yes |
| Fade from black at start | the manifest's `splice.fadeIn` (picture and sound) | yes (2026-11-13) |
| Fade to white, mid-video fade/cut to black | — | **no** (not in splice) |
| Match cut / graphic match / eye trace / move-into-move | planning (matching keyframes across shots; skill: start the next shot from the last camera) | manual |
| Speed ramps (incl. speed-ramp transition) | keyframe `speed` ramps (in-engine) and splice `speedFrom`/`speedTo`/`speed` | yes |
| Slow motion | splice `speed` < 1 (frame-duplication at 30 fps source) | partial: no high-fps capture / world slow-mo |
| Whip-pan transition, object wipe, push-through | possible by planning a fast `pan` / a pass behind a pillar into a `cut:true` | partial (no blur) |
| Wipe, iris, flash frame, morph | — | **no** |
| J/L-cut, sound bridge | audio follows the picture's cuts/fades; no independent offset | **no** |
| Title card | the manifest's `title` (the game's wordmark, `title.mjs`, fading in over the last shot, sized per aspect) | yes, the trailer only (showcases stay text-free) |
| Black frames, hit points, music cuts | music forbidden; black frames not built | no |
| Trailer structure | ≤ 1 min, so compress setup → disruption → escalation → held shot → peak → title | planning only (`videos/trailer/`) |
| Golden hour, backlight, silhouettes | `timeOfDay` / `timeFrom`→`timeTo`; aim into the sun at 0.265–0.3 or ~0.7–0.8 (seed 7 golden hour ~0.8) | yes |
| Time-lapse | `timeFrom/timeTo`, `days`, keyframe `speed` | yes (beyond the spec) |
| Atmosphere, colour contrast | world fog/water; `vf` grade in splice | partial |
| Aspect 2.39 / 1.85 / 4:3 | 9:16 or 16:9 only; letterbox via `vf` crop | partial (and owner format is 9:16) |
| One shot list in two aspects | the manifest's `aspects`: record a square, crop each aspect out of it; a part's `crop` moves an aspect's window where a subject would be cut | yes (2026-11-13) |
| 24 fps, 180° shutter | 24 fps recordable; owner wants 30; no shutter blur | partial |

### Validation checks vs Dwell
- Done by tests: no jumps at joins (per-frame move/turn bounded), dolly zoom holds size, frame-rate independence.
- Not available: subject-in-frame check, size ±15% check, **geometry clipping / 0.3 m clearance** (skill lessons:
  close orbits on hills pass through terrain and trees), pan strobing check, "no linear easing" lint (`orbit`,
  `track`, `linear` default to linear), noise presence, hold-length lint, 180°/30° sequence checks.

### What the tools can't do yet (summary)
1. Roll / Dutch angle and roll moves. 2. Handheld, tripod-drift and steadicam noise layers. 3. Depth of field and
rack focus. 4. Camera motion blur (180° shutter) — whip pans strobe. 5. Exponential zoom interpolation (fov is
linear in degrees). 6. Overshoot/spring, smootherstep and asymmetric easings. 7. Overlapping/blended moves across
segment joins (chaining rule). 8. Screen-space framing offsets (rule of thirds, lead room, OTS sliver) — `target`
always centres. 9. A shot-size solver (H → distance) and subject-relative yaw/angle parameters (¾, profile, low
15–25°) as presets. 10. Truck, pedestal, tilt and zoom presets (doable only as raw keyframes). 11. Transitions other
than cross-fade/hard cut/end fade/fade from black (to white, wipe, iris, flash frame), per-clip dissolve length, and
J/L audio offsets. 12. Directing creatures/NPCs as subjects; following a creature by id from the picker. 13. Path
validation for terrain clipping and the spec's sequence rules.
