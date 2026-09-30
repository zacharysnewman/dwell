import { describe, expect, it } from 'vitest';
import { formatCode, normalizeCode } from './joinCode';

describe('join codes', () => {
  it('read regardless of case, spaces and dashes, and reject look-alikes', () => {
    expect(normalizeCode('kq7-xm4')).toBe('KQ7XM4');
    expect(normalizeCode(' KQ7 XM4 ')).toBe('KQ7XM4');
    expect(normalizeCode('KQ7XM')).toBeNull();
    expect(normalizeCode('KO7XM4')).toBeNull();
    expect(normalizeCode('KQ1XM4')).toBeNull();
    expect(formatCode('KQ7XM4')).toBe('KQ7-XM4');
  });
});
