# 0069 — Stamina and oxygen: vitals as data, simulated in the controller, adventure only

**Status:** Accepted (2026-11-09; built in 0.50.54). Spends the axe swing's `staminaCost` (ADR 0067),
unused until now. Adds two mode abilities to ADR 0065's vocabulary.

## Context

Adventure (ADR 0065) hurts players but nothing tires them: they sprint forever and tread water
forever, and can stay under water as long as they like. The owner's design (2026-11-09):

- **Stamina** is spent by sprinting, swimming and treading water, and by axe swings (the tool's
  `staminaCost`). "Your 'bobbing' to the surface force in the water is canonically you 'treading
  water' so when you run out of stamina you sink. Can't sprint when out of stamina. Perhaps have like
  a 25% charge required to do any stamina intensive actions (but that's only after you run out)
  going from 25% to 24% works normally, doesn't stop you." It comes back when you stop spending.
- **Oxygen** drains while the head is under water; at none, the player takes steady damage until
  they surface, and surfacing fills it again quickly.
- **Adventure only**, as mode abilities; creative has neither: no draining, no sinking.
- **HUD:** a stamina bar and an air bar by the health bar, shown only when not full; hideable in
  Settings → On screen; placed by the HUD layout data so nothing overlaps.

Who adds to it: the owner or a mod, by editing a data file (a tool could): the rates, the
threshold, and which activities spend stamina at what rate. What is fixed in code: the activities
(a small vocabulary: `sprint`, `swim`, `tread`) and where each is recognised, and what exhaustion
gates. Scope: per build (the data), switched per world by the mode.

## Decision

1. **Data.** `shared/vitals/vitals.json` holds the tunables (`docs/ARCHITECTURE.md` §7.4):

   | field | value | meaning |
   |---|---|---|
   | `stamina.max` | 100 | stamina points when rested |
   | `stamina.drains` | `sprint` 12/s, `swim` 6/s, `tread` 4/s | points a second each activity spends |
   | `stamina.regenPerSecond` | 20 | points a second regained while nothing is spent |
   | `stamina.regenDelaySeconds` | 1.0 | how long after the last spend regen starts |
   | `stamina.recoverFraction` | 0.25 | the share of `max` an exhausted player needs back |
   | `stamina.exhaustedSinkSpeed` | 1.2 m/s | how fast an exhausted swimmer sinks |
   | `oxygen.capacitySeconds` | 20 | seconds of air with the head under water |
   | `oxygen.refillPerSecond` | 10 | seconds of air regained a second with the head out |
   | `oxygen.headFraction` | 0.9 | the capsule's submerged share at which the head is under |
   | `oxygen.drownDamage` | 10 | health lost each `drownIntervalSeconds` with no air |
   | `oxygen.drownIntervalSeconds` | 1.0 | the drowning damage's period |

   `shared/vitals/gen.mjs` validates it (a bad entry fails naming the field: an unknown activity
   or field, a rate out of range, an activity twice) and emits `crates/dwell-protocol/src/vitals.gen.rs`
   (the controller, the server, the client) and `client/src/ui/vitals.gen.ts` (the HUD). CI
   regenerates both and fails on a diff, and runs `shared/vitals/gen.test.mjs`, which adds an
   activity's rate by data alone. A Rust test checks the generator's activity vocabulary against
   `dwell_protocol::vitals::Activity`.
2. **Modes.** Two new abilities, **`stamina`** and **`oxygen`** (`Ability::Stamina`,
   `Ability::Oxygen`), both in adventure. Without `stamina` nothing drains, nothing exhausts and
   nobody sinks from it; without `oxygen` air never drains and nobody drowns. A world's mode change
   (ADR 0065's `set_mode`) switches them at once.
3. **The rule (`dwell_protocol::vitals::Vitals::step`), one pure function** used by the server and
   the client's prediction alike, each tick: the activity spent (at most one: sprinting on the
   ground, swimming with any move input, treading water with none) drains its rate; a tick without
   one counts toward the regen delay and, past it, regains `regenPerSecond`. Reaching 0 makes the
   player **exhausted**; while exhausted, stamina-heavy actions are refused until stamina is back to
   `recoverFraction × max`, which ends exhaustion. A rested player may spend below 25 % freely (25 →
   24 % is fine): the lockout follows only from hitting 0. Oxygen drains 1 s a second with the head
   under (`submerged ≥ headFraction`) and refills at `refillPerSecond` otherwise.
4. **In the controller, so prediction agrees.** `PlayerController::vitals` holds stamina, oxygen,
   exhaustion and the ticks since the last spend; `Players::set_vitals(stamina, oxygen)` switches
   the rule per world (the server from its mode; the client's predictor from Welcome/WorldSettings'
   mode). The controller gates on it (PLAYER_CONTROLLER.md §6.13): exhausted, the run input reads as
   not held (no sprint, no sprint slide), and in the water the buoyancy ("treading") is off, swim-up
   is ignored and the body sinks at `exhaustedSinkSpeed`, still able to swim sideways (which spends,
   so it stays exhausted). Rested, a swimmer floats as before. The step runs after the layers in the
   same tick on both sides, from the same inputs, so the prediction replays it identically.
5. **Axe swings.** A swing draws its tool's `swing.staminaCost` on the server when it lands
   (`Chop::Swing`); an **exhausted player's swing is refused** (no cut, no chips: "blocked", rather
   than a weak swing, which would need a second bite rule). The client doesn't start a swing while
   its predicted vitals are exhausted. The swing's cost isn't predicted: the server's `Vitals`
   message corrects the predicted stamina a round trip later (item 7).
6. **Drowning.** With no air left, the server deals `drownDamage` every `drownIntervalSeconds`
   through `Server::damage` (so creative, without `damage`, is never hurt either) with a new cause
   **`Drown` (4)**, until the head is out.
7. **The wire: optional, as ADR 0066/0067 (no new compatibility line).**
   - Welcome gains flag **`VITALS` (128)**: this server simulates vitals.
   - C→S **`VitalsHello`** (91), control channel, only to a server with `VITALS`: this client
     understands vitals and the `Drown` cause.
   - S→C **`Vitals`** (92), a datagram with each snapshot, only to a client that said hello and only
     while the mode has `stamina` or `oxygen`: the input seq it is as of, stamina, oxygen (f32) and
     flags (exhausted, the rule's stamina and oxygen switches). The predictor compares it with its
     history at that seq and, if it differs (beyond 0.5 point or 0.1 s, or exhaustion), patches that
     tick's vitals and replays to the present, as a knockback is replayed.
   - A `Drown` damage or death event goes only to clients that said hello; the others are sent
     `Fall`, a cause they know (an older client would refuse the message).
   An older client on a newer server plays without the gating predicted: the server's controller
   still gates, so its sprints are corrected and it sinks when exhausted, as it would with any other
   misprediction. A newer client on an older server sees no flag and predicts no vitals.
8. **Save: a new table, append-only.** `vitals`, keyed by the device key: a version byte (1),
   stamina and oxygen (f32) and the exhausted flag. Loaded at join (a player new to the world, or a
   record this build can't read, starts rested and full); a respawn starts rested. Older builds don't
   open the table and keep it. Player records are unchanged.
9. **HUD.** `#vitals`, two short bars (stamina, air) just above the health bar, each faded out when
   full (and air when not breathing water); a `vitals` part in `shared/ui/hud.json` (Settings → On
   screen hides it) and a fixed part in `shared/ui/layout.json`, so the hotbar is placed above it.
   The client's `hud()` reports `vitals: { stamina, oxygen, exhausted }` (fractions) when the mode
   has them, from the predicted controller.

## Consequences

- A new stamina-spending movement is an `Activity` variant, its name in the generator and where the
  controller recognises it; its rate is data. Another tool's cost is its `staminaCost`.
- Tests: the data's validation and an activity's rate by data alone (`shared/vitals/gen.test.mjs`);
  the rule's rates, regen delay and the 25 % rule (`dwell_protocol::vitals`); the controller on both
  faces (an exhausted swimmer sinks and a rested one floats, sprint refused when exhausted, oxygen
  drains under water and refills out of it, creative unaffected) and the predictor matching the
  server (`dwell-physics`); the server's drowning damage, the axe's cost and refusal, the mode switch
  and the save (`dwell-sim` `tests/vitals.rs`); the wire's round trips.
- Not built: food or other ways to restore stamina faster; a weaker swing instead of a refused one;
  breath sounds. Those are content on this system.
- Reversing it: drop the abilities from adventure; the table and messages stay harmless.
