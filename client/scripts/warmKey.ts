// The warm client's pure parts (scripts/warmClient.ts): which sources key the cached build, its
// port and how many builds are kept. No Node imports, so Vitest covers them.

/**
 * The sources the built client reads, as git pathspecs from the repository root: the Rust
 * workspace, the shared data, the client's page sources and config. Docs, Markdown, the bench's
 * data and scripts, and the e2e specs are left out: changing them doesn't rebuild.
 */
export const SOURCE_PATHSPECS: readonly string[] = [
  'Cargo.toml',
  'Cargo.lock',
  'rust-toolchain.toml',
  'crates',
  'shared',
  'client',
  ':(exclude,glob)**/*.md',
  ':(exclude)client/bench',
  ':(exclude)client/scripts',
  ':(exclude)client/e2e',
  ':(exclude,glob)**/*.test.ts',
];

/** The long-lived preview's port (the bench's own `vite preview` uses 4173). */
export const WARM_PORT = 4174;

/** Builds kept in the cache (each ~tens of MB). */
export const CACHE_KEEP = 3;

/** The cache entries to delete: all but the `keep` newest, never `current`. */
export function staleEntries(
  entries: readonly { name: string; mtimeMs: number }[],
  keep: number,
  current: string,
): string[] {
  return [...entries]
    .sort((a, b) => b.mtimeMs - a.mtimeMs)
    .slice(keep)
    .map((e) => e.name)
    .filter((n) => n !== current);
}
