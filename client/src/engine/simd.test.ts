import { describe, expect, it } from 'vitest';
import { NEEDS_SIMD } from '../ui/startupError';
import { hasWasmSimd, SIMD_PROBE } from './simd';

describe('the WebAssembly SIMD check', () => {
  it('validates its probe where SIMD is supported (Node has it)', () => {
    expect(WebAssembly.validate(SIMD_PROBE)).toBe(true);
    expect(hasWasmSimd()).toBe(true);
    // The probe is a real module, not just bytes that validate.
    const run = new WebAssembly.Instance(new WebAssembly.Module(SIMD_PROBE));
    expect(run).toBeInstanceOf(WebAssembly.Instance);
  });

  it('says no for a browser that rejects SIMD, has no WebAssembly, or throws', () => {
    expect(hasWasmSimd({ validate: () => false })).toBe(false);
    expect(hasWasmSimd(null)).toBe(false);
    expect(
      hasWasmSimd({
        validate: () => {
          throw new Error('old');
        },
      }),
    ).toBe(false);
  });

  it('names the browsers that have it', () => {
    expect(NEEDS_SIMD).toMatch(/too old/);
    for (const b of ['Safari 16.4', 'Chrome 91', 'Firefox 89']) expect(NEEDS_SIMD).toContain(b);
  });
});
