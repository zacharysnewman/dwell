# 0063. Unknown block properties stay on the block: adding a property or value is not breaking

- Status: Proposed (the owner, 2026-11-06: "shouldn't builds that don't recognize a block property just
  ignore it, leave it alone, let it stay on the block?"); planned for the 0.50.0 line, with flooded
  leaves as its first user
- Date: 2026-11-06
- Amends: ADR 0038 (block registry sync), ADR 0014 / `docs/RELEASES.md` §3 (what is breaking)

## Context

Block states are named by their full string, `ns:name[k=v,…]`, in world files' palettes and at join
(ADR 0038), and the parser fills a missing property with its default, so a world from before a property
existed loads in a build that has it. The other way round fails: a build that doesn't know a property
or a value rejects the state (`"has no property"`), so an older build on the same compatibility line
can't open a newer build's world, and an older client can't map a newer server's states. That makes
adding any property or value a breaking change, a new compatibility line each time.

The owner wants new properties to be routine (flooded leaves now; mods adding properties to blocks
later), and a build that doesn't know one to "ignore it, leave it alone, let it stay on the block".

## Options

1. **Drop what isn't known:** read the state as its known part (unknown keys and values to defaults).
   Simple, but an older build that saves the world loses the newer build's values for good.
2. **Keep it on the block (chosen):** read the state as its known part for behaviour, but keep its full
   name, so it is saved and sent back exactly as it was.
3. **Keep breaking:** every new property a new compatibility line.

## Decision

Option 2.

- **An alias state.** A build that meets a state it can't parse exactly (an unknown property, or an
  unknown value of a known one) at load, at join or in a structure registers it at runtime as an
  **alias**: a new state id whose name is the full string as read and whose behaviour (shape, collision,
  look, sounds, rules) is its **base**, the state its known part names (unknown keys dropped, unknown
  values to the property's default). Aliases of the same string share one id.
- **Kept on the block:** a cell holding an alias is drawn, collided with and broken as its base, and
  saved and sent as the alias's full name, unchanged. It changes only when the block itself is replaced
  (broken, placed over, or rewritten by a rule acting on the base): what replacing a block means.
- **Registry sync (ADR 0038):** the client maps a server state it doesn't know to an alias the same way.
- **Unknown blocks** (a whole `ns:name` a build doesn't know) are not covered: still breaking.
- **Mods:** a property added to another namespace's block is namespaced (`mymod:wet`), so it never
  collides with the block's own.
- **What stays breaking:** removing or renaming a property or a value, changing an existing state's
  shape or rules, changing worldgen, unknown blocks.

## Consequences

- Adding a property (with a default meaning "as before") or a value is not breaking once every build on
  the line has aliases: the build that introduces aliases starts a line (0.50.0), and the first new
  property (`flooded` on leaves) can ride in it.
- A build needs a growable state table at runtime (ids past the generated ones), cleared per world or
  session.
- An older build shows an alias as its base: flooded leaves look like plain leaves there, and keep
  their `flooded=true` when saved.
- Reversing it: stop registering aliases (unknown states fail to parse again).
