// Join codes (ARCHITECTURE.md §10.3), as the master makes them (services/master/src/codes.ts):
// six characters without look-alikes (no I, L, O, 0, 1), shown as "KQ7-XM4".

export const CODE_ALPHABET = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789';
export const CODE_LENGTH = 6;

/**
 * The code a player typed or a link carried, or null. Case, spaces and dashes don't matter;
 * characters outside the alphabet make it invalid.
 */
export function normalizeCode(text: string): string | null {
  const code = text.toUpperCase().replace(/[\s-]/g, '');
  if (code.length !== CODE_LENGTH) return null;
  for (const ch of code) if (!CODE_ALPHABET.includes(ch)) return null;
  return code;
}

/** "KQ7XM4" → "KQ7-XM4". */
export function formatCode(code: string): string {
  return `${code.slice(0, 3)}-${code.slice(3)}`;
}
