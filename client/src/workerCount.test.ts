import { describe, expect, it } from 'vitest';
import { workerCount } from './workerCount';

describe('workerCount', () => {
  it('is at most 4 a pool (2 on phones) and 2 fewer than the cores', () => {
    expect(workerCount('', 8, false)).toBe(4);
    expect(workerCount('', 8, true)).toBe(2);
    expect(workerCount('', 4, false)).toBe(2);
    expect(workerCount('', 2, false)).toBe(1);
    expect(workerCount('', undefined, false)).toBe(2);
  });

  it('takes ?workers=N from 1 to 8 and ignores anything else', () => {
    expect(workerCount('?workers=2', 8, false)).toBe(2);
    expect(workerCount('?seed=0&workers=6', 8, true)).toBe(6);
    expect(workerCount('?workers=0', 8, false)).toBe(4);
    expect(workerCount('?workers=9', 8, false)).toBe(4);
    expect(workerCount('?workers=2.5', 8, false)).toBe(4);
    expect(workerCount('?workers=many', 8, false)).toBe(4);
  });
});
