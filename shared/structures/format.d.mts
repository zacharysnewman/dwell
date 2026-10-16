// Types for format.mjs (the structure format's rules), for the TypeScript client.

/** Block id → its properties, each with its values in declared order (the first the default). */
export type Registry = Map<string, Map<string, string[]>>;

export type Variant =
  | { op: 'rotate'; values: number[] }
  | { op: 'mirror'; values: string[] }
  | { op: 'choices'; choices: { key: string; options: [string, number][] }[] };

/** A validated structure: canonical states by palette character (null keeps), cells in layer order. */
export interface ValidStructure {
  name: string;
  kind: string;
  size: [number, number, number];
  anchor: [number, number, number];
  palette: Map<string, string | null>;
  /** One character per cell: y-slices bottom up, z-rows north to south, x west to east. */
  cells: string;
  variants: Variant[];
}

export const FORMAT: number;
export const MAX_EDGE: number;
export const KINDS: readonly string[];
export const VARIANT_OPS: readonly string[];
export const TOP_LEVEL: readonly string[];

export function registryFromVector(text: string): Registry;
export function registryFromBlockDefs(
  defs: readonly { id: string; properties: readonly { name: string; values: readonly string[] }[] }[],
): Registry;
export function canonicalState(registry: Registry, text: unknown): string;
/** Throws `<file>: <entry>: <why>` for the first problem. Unknown top-level fields are errors. */
export function validate(json: unknown, file: string, registry: Registry): ValidStructure;
export function canonicalText(s: ValidStructure): string;
export function structureHash(text: string): string;
