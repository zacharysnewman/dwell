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
