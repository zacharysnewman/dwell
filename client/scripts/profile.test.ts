import { describe, expect, it } from 'vitest';
import { formatProfile, profileOf } from './profile.ts';

describe('profileOf', () => {
  it('sums self time per function and thread from profile chunks', () => {
    const trace = {
      traceEvents: [
        { ph: 'M', name: 'thread_name', pid: 1, tid: 7, args: { name: 'CrRendererMain' } },
        { ph: 'M', name: 'thread_name', pid: 1, tid: 9, args: { name: 'v8:ProfEvntProc' } },
        // The profile starts on its thread; its chunks are emitted from the profiler's.
        { ph: 'P', name: 'Profile', pid: 1, tid: 7, id: '0x1', args: {} },
        {
          name: 'ProfileChunk',
          pid: 1,
          tid: 9,
          id: '0x1',
          args: {
            data: {
              cpuProfile: {
                nodes: [
                  { id: 1, callFrame: { functionName: '(root)' } },
                  { id: 2, parent: 1, callFrame: { functionName: 'lod_update' } },
                  { id: 3, parent: 1, callFrame: { functionName: '(idle)' } },
                  { id: 4, parent: 2, callFrame: { functionName: 'hash_one' } },
                ],
                samples: [2, 2, 3],
              },
              timeDeltas: [0, 1000, 2000],
            },
          },
        },
        // A later chunk of the same profile only refers to its nodes.
        {
          name: 'ProfileChunk',
          pid: 1,
          tid: 9,
          id: '0x1',
          args: { data: { cpuProfile: { samples: [2, 4] }, timeDeltas: [500, 0, 250] } },
        },
      ],
    };
    const [main] = profileOf(trace);
    expect(main.thread).toBe('CrRendererMain');
    const lod = main.functions.find((f) => f.name === 'lod_update');
    // 1 ms + 2 ms (each sample lasts until the next), then 0 ms from the later chunk.
    expect(lod?.selfMs).toBeCloseTo(3);
    // hash_one's 0.25 ms (its chunk's last sample uses its own delta) is inside lod_update.
    expect(main.functions.find((f) => f.name === 'hash_one')?.selfMs).toBeCloseTo(0.25);
    expect(lod?.totalMs).toBeCloseTo(3.25);
    expect(main.totalMs).toBeCloseTo(3.25);
    expect(formatProfile([main])).toContain('lod_update');
  });

  it('reads a trace that is a bare event list', () => {
    expect(profileOf([])).toEqual([]);
  });
});
