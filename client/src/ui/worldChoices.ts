// The worlds the New world form offers besides the build's terrain (0.52.64): the world files in
// shared/worldgen/worlds/*.json that carry `newWorld` (a label and a line, checked by gen.mjs). A
// world made from one records the file's name as its worldgen config (`WorldMeta.worldgen`), so it
// reopens on the same ground and its guests get the config at join (`WorldgenRequest`, 0.52.38).

/** A world file the New world form offers. */
export interface WorldChoice {
  /** The world file's name (`test:starter`): the world's `worldgen`. */
  name: string;
  label: string;
  description: string;
}

interface WorldFile {
  name?: unknown;
  newWorld?: { label?: unknown; description?: unknown };
}

/** The offered worlds among `files`, by label. */
export function choicesOf(files: readonly WorldFile[]): WorldChoice[] {
  const out: WorldChoice[] = [];
  for (const f of files) {
    const n = f.newWorld;
    if (typeof f.name !== 'string' || !n || typeof n.label !== 'string') continue;
    out.push({
      name: f.name,
      label: n.label,
      description: typeof n.description === 'string' ? n.description : '',
    });
  }
  return out.sort((a, b) => a.label.localeCompare(b.label));
}

/** The build's offered worlds. */
export const WORLD_CHOICES: readonly WorldChoice[] = choicesOf(
  Object.values(
    import.meta.glob<WorldFile>('../../../shared/worldgen/worlds/*.json', {
      eager: true,
      import: 'default',
    }),
  ),
);

/** `name` when it is one of `choices` (a route's `worldgen`), else '' (the build's terrain). */
export function worldChoice(name: string, choices: readonly WorldChoice[] = WORLD_CHOICES): string {
  return choices.some((c) => c.name === name) ? name : '';
}
