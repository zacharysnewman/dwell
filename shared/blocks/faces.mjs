// Face-tile rules of block data (docs/PAINTERLY_TEXTURES.md §2, "Side tiles from the block above"):
// a block's optional `sideUnder` maps a block id to the tile its side faces take when that block is
// directly above it, e.g. dirt's `{ "dwell:grass": "dirtSkirt" }` (the turf's skirt hanging over a
// dirt face below a grass slope). gen.mjs validates it here and writes it to the registry rows;
// the meshers apply it (dwell-mesh mesher.rs).

/**
 * The block's rules, validated, as [{ above, side }] in data order (the first match wins).
 * `id` names the block in messages; `ids` holds every block id of the registry.
 */
export function sideUnderRules(id, def, ids) {
  const rules = def.sideUnder;
  if (rules === undefined) return [];
  if (rules === null || typeof rules !== 'object' || Array.isArray(rules)) {
    throw new Error(`${id}: sideUnder must be an object of block id → side tile`);
  }
  if (!def.textures) throw new Error(`${id}: sideUnder needs textures (it replaces the side tile)`);
  const entries = Object.entries(rules);
  if (!entries.length) throw new Error(`${id}: sideUnder is empty (leave it out)`);
  return entries.map(([above, side]) => {
    if (!ids.includes(above)) throw new Error(`${id}: sideUnder names unknown block ${above}`);
    if (typeof side !== 'string' || !/^[A-Za-z][A-Za-z0-9_]*$/.test(side)) {
      throw new Error(`${id}: sideUnder.${above} must be a texture tile name (got ${JSON.stringify(side)})`);
    }
    return { above, side };
  });
}

/** Every texture tile a block draws with: its top, side and bottom, and its `sideUnder` tiles. */
export function blockTiles(def) {
  const t = def.textures;
  if (t === undefined) return [];
  const base = typeof t === 'string' ? [t, t, t] : [t.top, t.side, t.bottom];
  return [...base, ...Object.values(def.sideUnder ?? {})];
}
