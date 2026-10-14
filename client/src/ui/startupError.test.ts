import { describe, expect, it } from 'vitest';
import { NEEDS_WEBGPU, startupMessage } from './startupError';

describe('the start-up error screen', () => {
  it('says WebGPU is needed when the browser has none', () => {
    expect(startupMessage(new Error('anything'), false)).toBe(NEEDS_WEBGPU);
  });

  it('says so too when WebGPU exists but gives no adapter or device', () => {
    const noAdapter = new Error(
      'no WebGPU adapter: No suitable graphics adapter found; webgpu found no adapters',
    );
    expect(startupMessage(noAdapter, true)).toBe(NEEDS_WEBGPU);
    expect(startupMessage(new Error('no WebGPU device: lost'), true)).toBe(NEEDS_WEBGPU);
  });

  it('names any other error', () => {
    expect(startupMessage(new Error('boom'), true)).toBe('Dwell failed to start: boom');
  });
});
