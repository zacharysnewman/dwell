import { describe, expect, it } from 'vitest';
import { formatMemory, memoryTotal, type MemoryReport } from './memory';

const MB = 1048576;

describe('memory line (F3)', () => {
  const local: MemoryReport = {
    coreBytes: 64 * MB,
    serverBytes: 180 * MB,
    worldgenBytes: [16 * MB, 16 * MB, 16 * MB],
    meshBytes: 120 * MB,
    screenBytes: 30 * MB,
    jsHeapBytes: 210 * MB,
  };

  it('adds up the WebAssembly memories and the GPU, not the JavaScript heap', () => {
    expect(memoryTotal(local)).toBe((64 + 180 + 48 + 120 + 30) * MB);
    expect(formatMemory(local)).toBe(
      'memory 442 MB · core 64 · world 180 · worldgen 3×16 · GPU 120 + screen 30 · JS 210',
    );
  });

  it('leaves out what is not there: no local world, no heap figure (Safari)', () => {
    const remote = {
      ...local,
      serverBytes: null,
      jsHeapBytes: null,
      worldgenBytes: [8 * MB, 24 * MB],
    };
    expect(formatMemory(remote)).toBe(
      'memory 246 MB · core 64 · worldgen 8+24 · GPU 120 + screen 30',
    );
  });
});
