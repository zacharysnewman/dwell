// What the HUD says (ARCHITECTURE.md §5): the centre message is for states that stop play
// (joining, death); background work such as terrain loading is a small status in a corner.

export interface HudTextInput {
  dead: boolean;
  /** The session has spawned the player. */
  active: boolean;
  terrainReady: boolean;
  terrainError: string | null;
}

export interface HudText {
  center: string;
  status: string;
}

export function hudText(s: HudTextInput): HudText {
  if (s.dead) return { center: 'You died — respawning…', status: '' };
  if (!s.active && !s.terrainError) return { center: 'Joining…', status: '' };
  return {
    center: '',
    status: s.terrainError
      ? `Terrain unavailable: ${s.terrainError}`
      : s.terrainReady
        ? ''
        : 'Loading terrain…',
  };
}
