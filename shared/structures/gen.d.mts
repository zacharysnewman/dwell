// Types for gen.mjs, for the client's tests (an exported structure passes the generator).
import type { Registry, ValidStructure } from './format.mjs';

export * from './format.mjs';

/** Validates every `*.json` in `dir` as the generator does (names match files, namespaces). */
export function loadDir(
  dir: string,
  registry: Registry,
  options: { test: boolean },
): (ValidStructure & { text: string; hash: string })[];
