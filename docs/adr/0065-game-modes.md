# 0065 — Game modes as data: creative and adventure, per world

**Status:** Accepted (2026-11-07; built in 0.50.30). Moves `docs/FUTURE.md`'s "Game modes: adventure
and creative" into the architecture. Amends the brushes' mode table (`docs/BRUSHES.md` §2), which
moves here.

## Context

The owner wants two game modes, adventure and creative (2026-10-19), and the brushes are already
gated by a one-mode table in `shared/brushes/brushes.json` (every world creative). FUTURE.md asked
for an ADR (modes as data, per world or per player, what each gates) before building them. The
owner's answers (2026-11-07):

- **Per world**, saved with it; the host can change it later.
- **Modes are a data file** (`shared/modes/modes.json`): each mode lists what it allows by named
  abilities, a small fixed vocabulary in code, so new modes or new switches are data.
- **Creative:** no damage at all (no fall, blast or crush damage, nothing), flight, unlimited blocks,
  the brush tools (in every terrain world, not only the sandbox).
- **Adventure:** takes damage (falls included), no brush tools, no flight. **Unlimited blocks stay**
  in adventure too for now: items and block drops are designed next ("Then we'll talk about items,
  block drops etc.").
- **Reach** is not mode-dependent: "creative has a more normal reach for block placement, keep that
  as is". The block reach stays 5 m in every mode; the brushes' ranged reach is the brushes'.
- **Defaults:** existing and new worlds are creative; the New world form offers the mode, with
  adventure marked "experimental".
- **Server-authoritative:** the server enforces the mode (damage, flight, brush strokes refused in
  adventure); the client hides what the mode doesn't allow and predicts consistently (flight off in
  adventure must not desync the controller: the server's flight check is what counts).

Who adds modes: the owner or a mod, by editing `shared/modes/modes.json` (a tool could). What is
fixed in code: the abilities and where each is checked. Scope: per world.

## Decision

1. **Data.** `shared/modes/modes.json` holds `formatVersion`, `defaultMode` and `modes`: each a
   `name` (how worlds refer to it), a `label`, an optional `description` and `experimental` flag,
   and `abilities`, a list of names from the fixed vocabulary **`damage`, `flight`, `brushes`,
   `unlimitedBlocks`, `map`, `mapRevealed`** (`dwell_protocol::modes::Ability`). `shared/modes/gen.mjs` validates it (a
   bad entry fails naming the mode and the problem: an unknown ability or field, a name twice, a
   `defaultMode` that names no mode) and emits `crates/dwell-protocol/src/modes.gen.rs` (the
   server, the sim core and the client) and `client/src/ui/modes.gen.ts` (the New world form). CI
   regenerates both and fails on a diff, and runs `shared/modes/gen.test.mjs`, which adds a mode by
   data alone. A Rust test checks the generator's vocabulary against `Ability`'s.
2. **Per world, a world setting.** The mode is a `WorldSettings` field (ADR 0054), tag **`0x82`
   `MODE`**, the mode's name as UTF-8. It is **optional** (top bit set): a build before modes skips
   it and plays the world as it always did (creative), so it needs no new compatibility line. A
   world saved before modes has no field and reads as `defaultMode` (creative), whatever the
   server's default for new worlds; a name this build doesn't know (a newer build's mode) reads as
   the default too. A new world takes `ServerConfig::mode` (the New world form's choice, by name
   through the local-world worker; creative by default).
3. **The server enforces it.** Every damage cause goes through one `Server::damage`, which does
   nothing in a mode without `damage` (falls, blasts and falling pieces; a blast's knockback is not
   damage and still pushes). Flight is `flight` and the flight policy: the server clears the fly
   bit otherwise. A region edit that holds a brush stroke's `KEEP` cells is refused
   (`RegionCheck::NotPlaceable`) in a mode without `brushes`, and the region reach there is the
   default 65 m. The out-of-world kill (a body far outside the world) is a respawn, not damage, and
   stays in every mode. `unlimitedBlocks` is in both modes and checked nowhere yet: blocks are not
   used up anywhere until items and drops exist.
4. **The client follows it.** Welcome's flags say what this player may do now: `FLIGHT` (the mode
   and the policy), `BRUSHES` (the mode), and a new **`FLIGHT_POLICY` (8)**, the policy alone
   (older clients ignore the bit). The client hides the brush tool and the flight toggle by them, as
   before, so in adventure it never predicts flight the server would refuse.
5. **The host changes it.** `Server::set_mode(name)` (a name not in the data is refused) applies at
   once: the server's checks read it, every joined client is sent `WorldSettings` again, and the
   world saves it with its next save. A client with modes takes `WorldSettings` after join and
   recomputes flight (`FLIGHT_POLICY` and the mode's `flight`) and brushes; an older client ignores
   it (its control handler drops it after the handshake) and goes on with its join-time flags,
   which the server's checks override. `LocalCore::set_mode` and the local world's `setMode` (wasm)
   expose it to the integrated server.
6. **Brushes.** `shared/brushes/brushes.json`'s `modes` and `defaultMode` are gone (the generator
   says where they moved); it keeps the brush reach as `reachM`. Whether a world has brushes is its
   mode's `brushes` ability.

## Consequences

- A new mode is an entry in `modes.json`; a new switch is an `Ability` variant, its name in the
  generator, and the check where it applies.
- Tests: the data's validation and a fixture mode by data alone (`shared/modes/gen.test.mjs`,
  `dwell_protocol::modes`); the settings round trip with the optional tag, a pre-modes world and a
  newer build's mode (`world_settings.rs`); creative takes no fall or blast damage and adventure
  does, on both faces (`dwell-sim` `tests/modes.rs`, `tests/explosives.rs`); adventure refuses
  brush strokes and flight on the server (`tests/modes.rs`, `tests/netcode.rs`); the host's change
  applies to players in the world (`tests/modes.rs`, `tests/netcode.rs`, the client session's
  `a_mode_change_after_join_…`). Tests that are about damage now run in adventure.
- **Not built yet:** a host control to change the mode in game (the server, `LocalCore` and the
  worker's `setMode` are ready; the pause menu or host panel needs the button), and the dedicated
  server's `--mode` flag. Those are UI and CLI on a finished system.
- Reversing it: drop the `MODE` field's reader; worlds keep the field, which builds without it
  skip.
