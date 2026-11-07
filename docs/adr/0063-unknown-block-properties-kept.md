# 0063. Unknown block properties stay on the block: adding a property or value is not breaking

- Status: Accepted (the owner, 2026-11-06: "shouldn't builds that don't recognize a block property just
  ignore it, leave it alone, let it stay on the block?" — yes); built in 0.50.0, with flooded leaves
  as its first user
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

## As built (0.50.0)

- `dwell_blocks::parse_state` registers the alias (`register_alias`, `Origin::Alias`): a one-state
  block entry under the full string (so payloads and the workers' extension carry it by name), whose
  row is its base's under that name and points at the base's block, so `block_of`, `state_property`
  and the rules read the base. `alias_base` / `base_state` give the base. `with_property` on an alias
  changes its base (the unknown part dropped: a rule rewriting the block replaces it).
  `parse_known_state` (and `state`, `require`) stay strict, for the build's own content, where an
  unknown property is a typo.
- World files: a palette string that parses is the build's state (missing properties at their
  defaults) or an alias; only an unknown whole block is a placeholder (ADR 0038 decision 8).
- Registry sync: the server serves its aliases; the client maps a server state its build names
  differently to an alias of its known part (or, missing a property, to its build state) when the
  server's rules are the base's, with no difference listed, and its own base state places the first
  such server state (the new property at its default). A server state whose rules differ is adopted as
  before (a synced copy).
- **Deviation:** aliases are not cleared per world or session: like the other registered states they
  live for the process, and the same string keeps its id (a reconnect or a second world registers
  nothing new).
