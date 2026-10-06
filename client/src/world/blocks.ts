// The block registry (docs/BLOCK_REGISTRY.md, ARCHITECTURE.md §6.1): namespaced blocks with typed
// properties, canonical state strings and dense runtime state ids. The tables are generated
// (blocks.gen.ts, from shared/blocks/*.json) and mirror the C++ registry id for id; this is the
// lookup and string layer over them (C++: block_registry.h).
import {
  BLOCK_DEFS,
  PIECE_NEAREST,
  REGISTRY_HASH,
  SHAPES,
  STATE_DEFS,
  type BlockDef,
  type MaterialLook,
  type MaterialTextures,
  type ShapeDef,
  type ShapeFace,
  type StateDef,
} from './blocks.gen';

export { BLOCK_DEFS, PIECE_NEAREST, REGISTRY_HASH, SHAPES, STATE_DEFS };
export type { BlockDef, MaterialLook, MaterialTextures, ShapeDef, ShapeFace, StateDef };

export const STATE_COUNT = STATE_DEFS.length;

const BY_STATE = new Map<string, number>(STATE_DEFS.map((s) => [s.state, s.id]));
const BY_BLOCK = new Map<string, BlockDef>(BLOCK_DEFS.map((b) => [b.id, b]));

/** Runtime state id by name, for code that needs a fixed block (`stateId('dwell:water')`). */
export function stateId(canonical: string): number {
  const id = BY_STATE.get(canonical);
  if (id === undefined) throw new Error(`unknown block state ${canonical}`);
  return id;
}

/** The canonical string of a state id (`dwell:stone`); undefined for an unknown id. */
export function stateString(id: number): string | undefined {
  return STATE_DEFS[id]?.state;
}

/**
 * Parses `ns:name` or `ns:name[k=v,…]`: any key order, missing properties take their defaults (the
 * first declared value). Returns an error message instead of an id for an unknown block, property or
 * value, a repeated key or malformed text.
 */
export function parseState(text: string): { id: number } | { error: string } {
  const open = text.indexOf('[');
  let blockId = text;
  let list = '';
  if (open >= 0) {
    if (!text.endsWith(']')) return { error: `missing ']' in "${text}"` };
    blockId = text.slice(0, open);
    list = text.slice(open + 1, -1);
  }
  const block = BY_BLOCK.get(blockId);
  if (!block) return { error: `unknown block "${blockId}"` };
  const index = block.properties.map(() => 0);
  const given = new Set<string>();
  if (open >= 0 && list !== '') {
    for (const pair of list.split(',')) {
      const eq = pair.indexOf('=');
      if (pair === '' || eq < 0) return { error: `expected key=value: "${pair}"` };
      const key = pair.slice(0, eq);
      const value = pair.slice(eq + 1);
      const k = block.properties.findIndex((p) => p.name === key);
      const prop = block.properties[k];
      if (!prop) return { error: `${blockId} has no property "${key}"` };
      if (given.has(key)) return { error: `repeated property "${key}"` };
      given.add(key);
      const v = prop.values.indexOf(value);
      if (v < 0) return { error: `${blockId}.${key} has no value "${value}"` };
      index[k] = v;
    }
  }
  // Mixed radix, the last property varying fastest (the generator's order).
  let offset = 0;
  block.properties.forEach((p, k) => {
    offset = offset * p.values.length + (index[k] ?? 0);
  });
  return { id: block.first + offset };
}

/** The value of a state's property, if the block has it. */
export function stateProperty(id: number, property: string): string | undefined {
  return STATE_DEFS[id]?.values[property];
}

/** The same block with `property` set to `value` (rotation, flooding); undefined if there is none. */
export function withProperty(id: number, property: string, value: string): number | undefined {
  const s = STATE_DEFS[id];
  const block = s && BLOCK_DEFS[s.block];
  if (!s || !block) return undefined;
  const prop = block.properties.find((p) => p.name === property);
  if (!prop?.values.includes(value)) return undefined;
  const values = { ...s.values, [property]: value };
  let offset = 0;
  for (const p of block.properties) {
    offset = offset * p.values.length + p.values.indexOf(values[p.name] ?? '');
  }
  return block.first + offset;
}

/** FNV-1a 64 over the canonical strings (each followed by "\n") in id order; equals REGISTRY_HASH. */
export function computeRegistryHash(): bigint {
  let h = 0xcbf29ce484222325n;
  const encoder = new TextEncoder();
  for (const s of STATE_DEFS) {
    for (const byte of encoder.encode(`${s.state}\n`)) {
      h ^= BigInt(byte);
      h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
    }
  }
  return h;
}
