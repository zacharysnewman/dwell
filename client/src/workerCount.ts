/**
 * Workers per job pool: generation and meshing each get this many (docs/DEV_TOOLS.md, "Flight
 * loading (M1 Air)"). At most 4 (2 on phones), and 2 fewer than the cores. `?workers=N` (1–8)
 * overrides it for a visit, to measure how the pools' size trades loading speed against the main
 * thread's and the GPU process's share of the cores.
 */
export function workerCount(search: string, cores: number | undefined, mobile: boolean): number {
  const asked = Number(new URLSearchParams(search).get('workers'));
  if (Number.isInteger(asked) && asked >= 1 && asked <= 8) return asked;
  return Math.max(1, Math.min(mobile ? 2 : 4, (cores || 4) - 2));
}
