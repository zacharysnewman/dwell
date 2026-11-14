# 0054. World settings sent at join

- Status: Accepted (the owner, 2026-11-03: "Can we build it in a way that it won't change the API in
  the future?"); built in 0.42.0 (protocol v22)
- Date: 2026-11-03
- Design: [`DAY_NIGHT.md`](../DAY_NIGHT.md) §2, [`WATER_FLOW.md`](../WATER_FLOW.md) §4.1

## Context

The owner asked for a 20-minute day. The clock (day and night length, start hour) was compiled into
every build and only the world tick was sent, so two builds that disagreed on it drew different
skies on one server: changing it was a compatibility change, and so would every later world-wide
setting (the water speed) have been.

## Options

1. **Keep it compiled**, and open a new compatibility line each time it changes.
2. **Fields in `Welcome`.** A layout change each time a setting is added.
3. **A world-settings message of tagged fields** (the block registry's pattern, ADR 0038), sent
   before `Welcome` and saved in the world's meta in the same form: a tag with the top bit set is
   optional and skipped by a build that doesn't know it; any other must be understood.

## Decision

Option 3. `WorldSettings` (0x54) carries the clock (required) and the water speed (optional); the
client draws the sky from the world's clock; the server keeps the fields it doesn't know and saves
them back. A new world takes this build's clock (`daynight.json`) and the server's water speed.

## Consequences

- One compatibility change now (0.42.0, protocol v22); after it, a build's default clock or water
  speed changes only new worlds, and a new setting that an older build can do without is an
  optional field, not a new line.
- A world keeps its own day length; a host can choose one (a host setting for it is later).

## Amendment (0.52.38, ADR 0074 B7): a required tag that is sent, not saved

The first required tag after the clock: `WORLDGEN` (2), the u64 hash of the world's worldgen config
(a Chisel edit or a fixture world). It is required because an older build can't make that terrain:
skipping it would generate the seed's own ground under the host's, so it refuses instead. The server
adds it to the fields it sends from its world's maps and never saves it (setting the known fields drops
it). Its value is too long for a field (`MAX_FIELD_BYTES`, 1 KB), so the tag carries only the hash and
a client that knows it asks for the text (`WorldgenRequest` → `WorldgenConfig`); an older build never
sends the request, so it never sees the new message. A default world sends no new tag: its join is
byte-identical.

## Amendment (BIG_MAP.md M0, 2026-11-11): the world's size, optional

`WORLD_SIZE` (0x83, `u16` radius scale; the named sizes are `constants.json` `worldSizes`): the owner
chose the radius as a world setting so a big world sits beside today's on the same line. Optional, and
written only for a size above 1×, so a default world's fields and join are byte-identical; a world
without it is 1×. An older build skips it and would draw today's disc, but no build before M1 can
hold a bigger LOD root, so a big world is a dev world until then.
