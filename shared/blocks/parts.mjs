// Multi-cell blocks of the block registry (docs/BLOCK_REGISTRY.md, "Multi-cell blocks"; LIGHTS.md
// §2): a block whose `extends` names the block that fills the next cell (`{ "up": "dwell:torch_upper" }`,
// face-local up), and that block's `partOf` naming it back. Placing the first puts both; breaking
// either part clears the other (dwell_world::edit::edit_writes). The directions are a fixed
// vocabulary (`up` only, for now). gen.mjs validates them here; parts.test.mjs shows bad entries
// failing with a message.

/** The directions a part may extend in. */
export const PART_DIRECTIONS = ['up'];

/**
 * Checks every block's `extends` and `partOf` against the others (`blocks`: `{ id, def }`), and
 * returns `{ id → { extendsUp, partOf } }`. Throws naming the block on a dangling or mismatched
 * partner.
 */
export function checkParts(blocks) {
  const fail = (id, m) => {
    throw new Error(`shared/blocks: ${id}: ${m}`);
  };
  const byId = new Map(blocks.map((b) => [b.id, b]));
  const out = new Map();
  for (const { id, def } of blocks) {
    let extendsUp = null;
    let partOf = null;
    if (def.extends !== undefined) {
      const e = def.extends;
      if (e === null || typeof e !== 'object' || Array.isArray(e)) fail(id, 'extends must be an object of direction → block id');
      for (const k of Object.keys(e)) {
        if (!PART_DIRECTIONS.includes(k)) fail(id, `extends.${k}: directions are ${PART_DIRECTIONS.join(', ')}`);
      }
      if (typeof e.up !== 'string') fail(id, 'extends.up must name a block');
      const part = byId.get(e.up);
      if (!part) fail(id, `extends.up names unknown block ${e.up}`);
      if (part.def.partOf !== id) fail(id, `extends.up names ${e.up}, whose partOf must be ${id} (got ${JSON.stringify(part.def.partOf)})`);
      if (part.def.extends !== undefined) fail(id, `extends.up names ${e.up}, which extends further (one part a block, for now)`);
      extendsUp = e.up;
    }
    if (def.partOf !== undefined) {
      if (typeof def.partOf !== 'string') fail(id, 'partOf must name a block');
      const owner = byId.get(def.partOf);
      if (!owner) fail(id, `partOf names unknown block ${def.partOf}`);
      if (owner.def.extends?.up !== id) fail(id, `partOf names ${def.partOf}, whose extends.up must be ${id}`);
      if (def.placeable || def.palette) fail(id, 'a part is placed with its block: it may not be placeable on its own');
      if (def.properties && Object.keys(def.properties).length) fail(id, 'a part has no properties (one state)');
      partOf = def.partOf;
    }
    out.set(id, { extendsUp, partOf });
  }
  return out;
}
