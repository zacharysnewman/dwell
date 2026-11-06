# 0060. Older worlds open in newer builds, after a warning

- Status: Accepted (the owner, 2026-11-06: "make it so I can still play older version worlds with
  newer versions it just shows a warning you have to confirm so it's obvious when it's happening");
  built 0.48.2; amends ADR 0014
- Date: 2026-11-06
- Design: [`RELEASES.md`](../RELEASES.md) §5 ("Older worlds in newer builds")

## Context

ADR 0014 locked every world to its compatibility line: a build opened a world only on the line of
the version that last saved it, and the launcher always picked the newest build of that line. A
player who wanted a newer line's features on an old world had no way to try it. The owner asked for
one, with a warning that makes it obvious, and answered the design questions (2026-11-06):

- **Unexplored land** generates with the newer build's terrain (a build has only its own
  generator), accepting seams where it meets saved land.
- **The original:** the warning offers both, to play a copy (the original stays on its version)
  or to upgrade the world itself.
- **Where:** the player picks the build on the version page; playing an older line's world in it
  prompts. Play from the main menu still opens a world in its own line's newest build.
- **Scope:** browser worlds only (local and friend worlds). Joining a host on another line still
  can't work: the protocols differ between lines.

## Decision

- **The rule** (`canUpgradeWorld`, `can_upgrade_world`, `shared/version/vectors.txt` `upgrade`):
  a build may open a world from another line after the warning when both versions are stable
  releases and the build is newer. Never a newer world, never in or from a dev build, never a save
  from before versioned releases.
- **The warning** (`client/src/ui/upgradePrompt.ts`): the world's version and line and the build's,
  what changes (new terrain where unexplored, placeholders for blocks this build lacks, no way
  back once saved), and three choices: Play a copy (default), Upgrade this world, Cancel (focused).
- **A copy** (`WorldIndex.copy`, `copyWorldFile`): a new world entry named "<name> (from
  <version>)" with the same seed, generator and spawn, and the world file copied in OPFS; then the
  copy plays. **An upgrade** plays the world itself.
- **The local core opens it only when told** (`LocalWorld`'s `upgrade`, `world_open_error`): the
  page passes the player's confirmation, so a file from another line is still refused otherwise.
- **This build's generator** (`dwell_world::current_generator`): a world saved under a retired or
  unknown terrain generator opens on this build's terrain (it fell back to the flat world); flat,
  sandbox and the retired playground keep theirs. The index records it on play (`touch`).
- **The version page** lists, for each build from 0.48.2 (`UPGRADE_SINCE`), the older line's
  worlds it opens "with a warning".

## Consequences

- An old world gains the newer line's features; the price is visible: seams at the edge of the
  explored land, placeholders for removed blocks, and (for an upgrade) no way back to its version.
  Copies keep the original playable.
- No migration exists beyond what the storage already does on open (missing tables created, the LOD
  cache rebuilt for a new registry, unknown states kept as placeholders): a future format that
  needs one adds it to `WorldDb::setup`, and the warning still applies.
- Dedicated servers still refuse another line's world (`world_version_error`); extending this to
  them is a command-line flag on the same `world_open_error`.
- Reversing it: drop the prompt and `canUpgradeWorld`; worlds already upgraded stay on their new
  line.
