// Join codes (ARCHITECTURE.md §10.3): six characters from an alphabet without look-alikes
// (no I, L, O, 0, 1), shown as "KQ7-XM4". 31^6 ≈ 887 million codes; guessing is rate-limited per
// IP. A friend world's code names its Room Durable Object.

export const CODE_ALPHABET = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789';
export const CODE_LENGTH = 6;

/** A random code (without the dash). */
export function newCode(random: () => number = Math.random): string {
  let code = '';
  for (let i = 0; i < CODE_LENGTH; i++) {
    code += CODE_ALPHABET[Math.floor(random() * CODE_ALPHABET.length)] ?? 'A';
  }
  return code;
}

/**
 * The code a player typed or a link carried, or null. Case, spaces and dashes don't matter;
 * characters outside the alphabet (I, L, O, 0, 1 and the rest) make it invalid.
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
