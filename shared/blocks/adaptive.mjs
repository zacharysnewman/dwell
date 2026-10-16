// The adaptive rule table (docs/ADAPTIVE_BLOCKS.md §2, ADR 0024): what shape an adaptive top block
// takes from its 8 horizontal neighbours, each higher, the same height or lower. The table is data:
// `adaptive.json` holds per-neighbourhood overrides on top of the five rules, and this module
// builds the 3⁸ = 6,561 entries from both, validates them and runs the exhaustive no-holes test.
// `gen.mjs` turns the result into the Rust table; `adaptive.test.mjs` tests it.
//
// A neighbourhood is written as its 3 × 3 grid, rows north to south and each row west to east,
// separated by "/": "+" higher, "=" the same, "-" lower, "." the block itself — e.g. "+==/=.-/===".
// Its index is Σ codeₖ · 3ᵏ over the neighbours in reading order (NW, N, NE, W, E, SW, S, SE) with
// codes 0 same, 1 lower, 2 higher.

import {
  ADAPTIVE_SHAPE_NAMES,
  FACING_TURNS,
  SYMMETRIC_SHAPES,
  GAME_CORNER_SHAPES,
  ADAPTIVE_CORNER_SHAPES,
  RIDGE_SHAPES,
  rotateCorners,
  namedShape,
} from "./shapes.mjs";

export const SAME = 0;
export const LOWER = 1;
export const HIGHER = 2;
export const NEIGHBOURHOODS = 6561;
/** Neighbour offsets [dx, dz] in index order: NW, N, NE, W, E, SW, S, SE (north = −z). */
export const OFFSETS = [
  [-1, -1],
  [0, -1],
  [1, -1],
  [-1, 0],
  [1, 0],
  [-1, 1],
  [0, 1],
  [1, 1],
];
const SYMBOL = ["=", "-", "+"];
const FACING_OF_TURNS = ["east", "south", "west", "north"];

/** The 8 codes of a neighbourhood index. */
export function codesOf(index) {
  const out = [];
  let rest = index;
  for (let k = 0; k < 8; ++k) {
    out.push(rest % 3);
    rest = Math.floor(rest / 3);
  }
  return out;
}

export function indexOf(codes) {
  return codes.reduce((s, c, k) => s + c * 3 ** k, 0);
}

/** "+==/=.-/===" for a neighbourhood index. */
export function aroundOf(index) {
  const c = codesOf(index).map((v) => SYMBOL[v]);
  return `${c[0]}${c[1]}${c[2]}/${c[3]}.${c[4]}/${c[5]}${c[6]}${c[7]}`;
}

/** The neighbourhood index of an "around" string, or an error message. */
export function parseAround(text) {
  if (typeof text !== "string" || !/^[=+-]{3}\/[=+-]\.[=+-]\/[=+-]{3}$/.test(text))
    return { error: `around "${text}": expected three rows of "+", "=", "-" with "." in the middle, like "+==/=.-/==="` };
  const chars = text.replace(/[/.]/g, "");
  return { index: indexOf([...chars].map((ch) => SYMBOL.indexOf(ch))) };
}

/**
 * The five rules (ADAPTIVE_BLOCKS.md §2) on one neighbourhood: { shape, turns, rule } where `rule`
 * is the rule that chose the whole shape (R4, R5) or "corners" (R1–R3), as the Shape Playground's
 * `rulesFor` computes it.
 */
export function ruleShape(index) {
  const n = codesOf(index);
  const at = (dx, dz) => {
    if (dx === 0 && dz === 0) return SAME;
    return n[OFFSETS.findIndex(([x, z]) => x === dx && z === dz)];
  };
  // Per corner (NW NE SE SW): its two edge neighbours and the diagonal.
  const corners = [
    [-1, -1],
    [1, -1],
    [1, 1],
    [-1, 1],
  ].map(([dx, dz]) => {
    const three = [at(dx, 0), at(0, dz), at(dx, dz)];
    const higher = three.includes(HIGHER);
    const lower = three.includes(LOWER);
    if (higher && !lower) return 2; // rise
    if (lower) return 0; // drop
    return 1; // rest
  });
  if (corners.every((v) => v === 0)) {
    // Edge neighbours east, south, west, north (turns 0..3).
    const card = [at(1, 0), at(0, 1), at(-1, 0), at(0, -1)];
    const higher = card.map((v) => v === HIGHER);
    const same = card.map((v) => v === SAME);
    if (higher.filter(Boolean).length === 1 && !same.some(Boolean))
      return { shape: "wedge", turns: (higher.indexOf(true) + 2) % 4, rule: "R5" };
    const count = same.filter(Boolean).length;
    if (count === 0) return { shape: "half_pyramid", turns: 0, rule: "R4" };
    if (count === 1) return { shape: "half_ridge_end", turns: same.indexOf(true), rule: "R4" };
    if (count === 2 && same[0] === same[2])
      return { shape: "half_ridge", turns: same[0] ? 0 : 1, rule: "R4" };
    if (count === 2) {
      const k = [0, 1, 2, 3].find((i) => same[i] && same[(i + 1) % 4]);
      return { shape: "half_ridge_corner", turns: k, rule: "R4" };
    }
    if (count === 3) return { shape: "half_ridge_tee", turns: (same.indexOf(false) + 3) % 4, rule: "R4" };
    return { shape: "half_ridge_cross", turns: 0, rule: "R4" };
  }
  const cornered = { ...GAME_CORNER_SHAPES, ...ADAPTIVE_CORNER_SHAPES };
  for (const shape of Object.keys(cornered)) {
    for (let turns = 0; turns < 4; ++turns) {
      if (rotateCorners(cornered[shape], turns).join() === corners.join())
        return { shape, turns: SYMMETRIC_SHAPES.has(shape) ? 0 : turns, rule: "corners" };
    }
  }
  return { error: `no shape has the corners ${corners.join(" ")} (rules 1–3 for ${aroundOf(index)})` };
}

/** The facing name of a shape at `turns` (what data files say): "" for symmetric shapes. */
export function facingName(shape, turns) {
  if (SYMMETRIC_SHAPES.has(shape)) return "";
  if (shape === "half_ridge") return turns % 2 ? "north_south" : "east_west";
  return FACING_OF_TURNS[turns];
}

function turnsOfFacing(shape, facing) {
  if (SYMMETRIC_SHAPES.has(shape)) return facing === undefined || facing === "" ? { turns: 0 } : { error: `${shape} takes no facing` };
  if (shape === "half_ridge") {
    if (facing === "east_west") return { turns: 0 };
    if (facing === "north_south") return { turns: 1 };
    return { error: `half_ridge facing "${facing}": east_west or north_south` };
  }
  if (!(facing in FACING_TURNS)) return { error: `${shape} facing "${facing}": north, east, south or west` };
  return { turns: FACING_TURNS[facing] };
}

/**
 * Builds the table from the data (`adaptive.json`'s contents): an array of 6,561 entries
 * { shape, turns, rule, override } — or throws with every problem found, each naming its entry.
 */
export function buildTable(data) {
  const problems = [];
  if (!data || typeof data !== "object") throw new Error("shared/blocks/adaptive.json: not an object");
  if (data.rules !== "five-rules") problems.push(`rules "${data.rules}": the only rule set is "five-rules"`);
  const overrides = data.overrides ?? [];
  if (!Array.isArray(overrides)) problems.push("overrides: not a list");
  const table = [];
  for (let i = 0; i < NEIGHBOURHOODS; ++i) {
    const r = ruleShape(i);
    if (r.error) problems.push(r.error);
    table.push({ ...r, override: false });
  }
  const seen = new Map();
  (Array.isArray(overrides) ? overrides : []).forEach((o, k) => {
    const where = `overrides[${k}]${o?.around ? ` (${o.around})` : ""}`;
    const at = parseAround(o?.around);
    if (at.error) return problems.push(`${where}: ${at.error}`);
    if (seen.has(at.index)) return problems.push(`${where}: repeats overrides[${seen.get(at.index)}]`);
    seen.set(at.index, k);
    if (!ADAPTIVE_SHAPE_NAMES.includes(o.shape))
      return problems.push(`${where}: unknown shape "${o.shape}" (known: ${ADAPTIVE_SHAPE_NAMES.join(", ")})`);
    const t = turnsOfFacing(o.shape, o.facing);
    if (t.error) return problems.push(`${where}: ${t.error}`);
    table[at.index] = { shape: o.shape, turns: t.turns, rule: "override", override: true };
  });
  if (problems.length) throw new Error(`shared/blocks/adaptive.json:\n  ${problems.join("\n  ")}`);
  const holes = holesIn(table);
  if (holes.length)
    throw new Error(
      `shared/blocks/adaptive.json: ${holes.length} hole(s) between equal-height neighbours, e.g.\n  ${holes.slice(0, 5).join("\n  ")}`,
    );
  return table;
}

// Cell-face tags of the sides in profile order (+X −X +Z −Z) and the coordinate each runs along.
const SIDE_TAGS = [0, 1, 4, 5];
const RUN = [2, 2, 0, 0];

/**
 * The top of a shape's drawn side face at running coordinate s, from its baked polygons (cells):
 * the highest point of the polygons' outlines over s, or 0 where it has none.
 */
function sideTopAt(shape, k, s) {
  let top = 0;
  for (const f of shape.faces) {
    if (f.tag !== SIDE_TAGS[k]) continue;
    const n = f.pts.length;
    for (let i = 0; i < n; ++i) {
      const a = f.pts[i];
      const b = f.pts[(i + 1) % n];
      const lo = Math.min(a[RUN[k]], b[RUN[k]]);
      const hi = Math.max(a[RUN[k]], b[RUN[k]]);
      if (s < lo - 1e-9 || s > hi + 1e-9) continue;
      const y =
        hi - lo < 1e-9 ? Math.max(a[1], b[1]) : a[1] + ((b[1] - a[1]) * (s - a[RUN[k]])) / (b[RUN[k]] - a[RUN[k]]);
      top = Math.max(top, y);
    }
  }
  return top * 2; // halves
}

/**
 * The exhaustive adjacency test over every pair of neighbourhoods of two blocks of the same height
 * side by side (east–west and north–south, 3¹⁰ pairs each). For the shared side:
 * - **No hole:** the baked side faces must match the side profiles the meshers cull by, at the
 *   side's ends and middle (both are piecewise linear between them, so that is exact) — then
 *   wherever one block's side stands above the other's it is drawn, because a side is culled only
 *   where the neighbour's covers it everywhere.
 * - **No step:** the two surfaces meet at the side's two corners. (They may differ at its middle:
 *   a ridge's end stands half a block above a slope beside it, and shows its side.)
 * Returns a description per failure (at most `limit`).
 */
export function holesIn(table, limit = 20) {
  const baked = table.map((e) => namedShape(e.shape, e.turns));
  const profiles = baked.map((s) => s.sides);
  const holes = [];
  for (let i = 0; i < baked.length && holes.length < limit; ++i) {
    for (let k = 0; k < 4; ++k) {
      const drawn = [0, 0.5, 1].map((s) => sideTopAt(baked[i], k, s));
      if (drawn.some((v, j) => Math.abs(v - profiles[i][k][j]) > 1e-9)) {
        holes.push(`${aroundOf(i)}: ${describe(table[i])}'s side ${k} is drawn as ${drawn} but culled as ${profiles[i][k]}`);
        break;
      }
    }
  }
  // East pair: A's E neighbour is B (same); B's W is A. A's NE/SE are B's N/S; A's N/S are B's NW/SW.
  // South pair likewise, rotated.
  const pairs = [
    // [A's side, B's side (opposite), A neighbour index of B, map B's neighbours from A's]
    { a: 0, b: 1, ab: 4, shared: [[2, 1], [7, 6], [1, 0], [6, 5]], ba: 3, own: [2, 4, 7] },
    { a: 2, b: 3, ab: 6, shared: [[5, 3], [7, 4], [3, 0], [4, 2]], ba: 1, own: [5, 6, 7] },
  ];
  for (const p of pairs) {
    for (let ia = 0; ia < NEIGHBOURHOODS; ++ia) {
      const ca = codesOf(ia);
      if (ca[p.ab] !== SAME) continue;
      // B's neighbourhood: its own side's three are free; the rest come from A.
      for (let free = 0; free < 27; ++free) {
        const cb = new Array(8).fill(0);
        cb[p.ba] = SAME;
        // shared: [A's neighbour, B's neighbour]
        for (const [fromA, toB] of p.shared) cb[toB] = ca[fromA];
        const f = [free % 3, Math.floor(free / 3) % 3, Math.floor(free / 9)];
        p.own.forEach((k, j) => (cb[k] = f[j]));
        const ib = indexOf(cb);
        const sa = profiles[ia][p.a];
        const sb = profiles[ib][p.b];
        if (sa[0] !== sb[0] || sa[2] !== sb[2]) {
          holes.push(
            `${aroundOf(ia)} (${describe(table[ia])}) and ${aroundOf(ib)} (${describe(table[ib])}) to its ${p.a === 0 ? "east" : "south"}: a step at a shared corner (${sa} against ${sb})`,
          );
          if (holes.length >= limit) return holes;
        }
      }
    }
  }
  return holes;
}

/** How many entries each rule decided, and how many are overridden (the generator reports it). */
export function summary(table) {
  const out = {};
  for (const e of table) out[e.rule] = (out[e.rule] ?? 0) + 1;
  return out;
}

/** The names of the shapes and facings the table uses, for the vectors file. */
export function describe(entry) {
  const f = facingName(entry.shape, entry.turns);
  return f ? `${entry.shape} ${f}` : entry.shape;
}

export { RIDGE_SHAPES };
