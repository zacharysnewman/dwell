// Token buckets for the master's rate limits (ARCHITECTURE.md §10.3, §11): per signing key and per
// IP. Pure (time is passed in) so it runs the same in tests and in the Directory Durable Object.

export interface BucketSpec {
  /** Most requests allowed at once. */
  burst: number;
  /** Requests refilled per second. */
  perSecond: number;
}

/**
 * Join-code lookups per IP (on top of the signed limits): a few mistyped codes are fine, guessing
 * one of ~887 million codes is not.
 */
export const JOIN_LIMIT: BucketSpec = { burst: 20, perSecond: 0.5 };

/** Limits for signed requests: a burst of 30, then 1 per second, per key and per IP. */
export const SIGNED_LIMITS: { key: BucketSpec; ip: BucketSpec } = {
  key: { burst: 30, perSecond: 1 },
  ip: { burst: 60, perSecond: 2 },
};

interface Bucket {
  tokens: number;
  at: number;
}

/** Buckets by name, oldest dropped past `maxBuckets` so memory stays bounded. */
export class RateLimiter {
  private readonly buckets = new Map<string, Bucket>();

  constructor(private readonly maxBuckets = 10_000) {}

  /** Takes `cost` tokens from the named bucket if it has them. */
  take(name: string, spec: BucketSpec, now: number, cost = 1): boolean {
    const b = this.buckets.get(name) ?? { tokens: spec.burst, at: now };
    b.tokens = Math.min(spec.burst, b.tokens + (Math.max(0, now - b.at) / 1000) * spec.perSecond);
    b.at = now;
    const allowed = b.tokens >= cost;
    if (allowed) b.tokens -= cost;
    // Re-inserted so the map's order is least recently used first.
    this.buckets.delete(name);
    this.buckets.set(name, b);
    if (this.buckets.size > this.maxBuckets) {
      const oldest = this.buckets.keys().next().value;
      if (oldest !== undefined) this.buckets.delete(oldest);
    }
    return allowed;
  }

  get size(): number {
    return this.buckets.size;
  }
}
