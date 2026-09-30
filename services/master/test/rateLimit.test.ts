import { describe, expect, it } from 'vitest';
import { RateLimiter } from '../src/rateLimit';

const spec = { burst: 3, perSecond: 1 };

describe('rate limiter', () => {
  it('allows a burst, then refills over time', () => {
    const limits = new RateLimiter();
    expect([1, 2, 3, 4].map(() => limits.take('a', spec, 0))).toEqual([true, true, true, false]);
    expect(limits.take('a', spec, 500)).toBe(false);
    expect(limits.take('a', spec, 1000)).toBe(true);
    expect(limits.take('a', spec, 1000)).toBe(false);
    // Never above the burst, however long the wait.
    expect([1, 2, 3, 4].map(() => limits.take('a', spec, 1e9))).toEqual([true, true, true, false]);
  });

  it('keeps buckets apart', () => {
    const limits = new RateLimiter();
    for (let i = 0; i < 3; i++) limits.take('a', spec, 0);
    expect(limits.take('a', spec, 0)).toBe(false);
    expect(limits.take('b', spec, 0)).toBe(true);
  });

  it('forgets the least recently used bucket past its capacity', () => {
    const limits = new RateLimiter(2);
    for (let i = 0; i < 3; i++) limits.take('a', spec, 0);
    limits.take('b', spec, 0);
    limits.take('c', spec, 0);
    expect(limits.size).toBe(2);
    // 'a' was dropped, so it starts full again.
    expect(limits.take('a', spec, 0)).toBe(true);
  });
});
