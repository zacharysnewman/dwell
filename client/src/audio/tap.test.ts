// The recording tap (`Mixer.tap`, `__dwell.audioTap`; the record skill's showcase sound, WORK_QUEUE item
// 40): a parallel graph that carries effects and ambience, never the music or the UI unless asked, and
// leaves the speakers' graph as it was. Checked on a fake AudioContext that records connections.
import { describe, expect, it } from 'vitest';
import { buildMixer, tapBuses } from './mixer';
import { SOUND_DATA } from './sounds.gen';
import type { Bus } from './types';

interface FakeParam {
  value: number;
  cancelScheduledValues(): void;
  setValueAtTime(): void;
  linearRampToValueAtTime(): void;
  exponentialRampToValueAtTime(): void;
}
interface FakeNode {
  kind: string;
  type?: string;
  outs: FakeNode[];
  gain: FakeParam;
  frequency: FakeParam;
  Q: FakeParam;
  connect(n: FakeNode): void;
  disconnect(n?: FakeNode): void;
}

const param = (): FakeParam => ({
  value: 0,
  cancelScheduledValues: () => undefined,
  setValueAtTime: () => undefined,
  linearRampToValueAtTime: () => undefined,
  exponentialRampToValueAtTime: () => undefined,
});

/** Just enough of a BaseAudioContext for buildMixer and a tap. */
class FakeContext {
  readonly sampleRate = 8000;
  readonly nodes: FakeNode[] = [];
  readonly destination = this.node('destination');
  node(kind: string): FakeNode {
    const n: FakeNode = {
      kind,
      outs: [],
      gain: param(),
      frequency: param(),
      Q: param(),
      connect(to) {
        this.outs.push(to);
      },
      disconnect(to) {
        this.outs = to ? this.outs.filter((o) => o !== to) : [];
      },
    };
    this.nodes.push(n);
    return n;
  }
  createGain(): FakeNode {
    return this.node('gain');
  }
  createBiquadFilter(): FakeNode {
    return this.node('biquad');
  }
  createWaveShaper(): FakeNode {
    return this.node('shaper');
  }
  createConvolver(): FakeNode {
    return this.node('convolver');
  }
  createBuffer(channels: number, length: number) {
    return { numberOfChannels: channels, length, copyToChannel: () => undefined };
  }
}

/** Whether sound into `from` reaches `to` along the recorded connections. */
function reaches(from: FakeNode, to: FakeNode, seen = new Set<FakeNode>()): boolean {
  if (from === to) return true;
  if (seen.has(from)) return false;
  seen.add(from);
  return from.outs.some((n) => reaches(n, to, seen));
}

const BUSES: Bus[] = ['effects', 'ambience', 'music', 'ui'];

function setup() {
  const ctx = new FakeContext();
  const mixer = buildMixer(ctx as unknown as BaseAudioContext);
  const bus = (b: Bus) => mixer[b] as unknown as FakeNode;
  const outs = () => new Map(ctx.nodes.map((n) => [n, [...n.outs]]));
  return { ctx, mixer, bus, outs };
}

describe('the recording tap', () => {
  it('carries effects and ambience by default, never the music or UI unless asked', () => {
    expect(tapBuses()).toEqual(['effects', 'ambience']);
    expect(tapBuses({ music: false })).toEqual(['effects', 'ambience']);
    expect(tapBuses({ music: true })).toEqual(['effects', 'ambience', 'music']);
    expect(tapBuses({ ui: true, ambience: false })).toEqual(['effects', 'ui']);
  });

  it('the tap excludes the music bus; the speakers still hear every bus', () => {
    const { ctx, mixer, bus, outs } = setup();
    const before = outs();
    const dest = ctx.node('mediaStreamDestination');
    mixer.tap(dest as unknown as AudioNode, tapBuses({ music: false }));
    expect(reaches(bus('effects'), dest)).toBe(true);
    expect(reaches(bus('ambience'), dest)).toBe(true);
    expect(reaches(bus('music'), dest)).toBe(false);
    expect(reaches(bus('ui'), dest)).toBe(false);
    // The room's reverb returns into the effects bus, so it is on the tap too.
    expect(reaches(mixer.reverb as unknown as FakeNode, dest)).toBe(true);
    for (const b of BUSES) expect(reaches(bus(b), ctx.destination)).toBe(true);
    // The tap never feeds the speakers, and the speakers' graph only gained the buses' new links.
    const tapNodes = ctx.nodes.filter((n) => !before.has(n));
    for (const n of tapNodes) expect(reaches(n, ctx.destination)).toBe(false);
    for (const [n, was] of before) {
      const added = n.outs.filter((o) => !was.includes(o));
      expect(n.outs.filter((o) => was.includes(o))).toEqual(was);
      for (const o of added) expect(tapNodes).toContain(o);
    }
  });

  it('the muffled buses go through the tap’s own underwater low-pass, the others straight in', () => {
    const { ctx, mixer, bus } = setup();
    const dest = ctx.node('mediaStreamDestination');
    mixer.tap(dest as unknown as AudioNode, ['effects', 'ambience', 'music', 'ui']);
    const lowpasses = ctx.nodes.filter((n) => n.kind === 'biquad');
    expect(lowpasses).toHaveLength(2);
    const tapLowpass = lowpasses.find((l) => reaches(l, dest));
    for (const b of BUSES) {
      const viaLowpass = bus(b).outs.some((o) => o === tapLowpass);
      expect(viaLowpass).toBe(SOUND_DATA.underwater.buses.includes(b));
    }
  });

  it('follows the master volume and closes cleanly', () => {
    const { ctx, mixer, bus } = setup();
    const dest = ctx.node('mediaStreamDestination');
    const tap = mixer.tap(dest as unknown as AudioNode, tapBuses());
    mixer.setVolumes({ master: 0.3, effects: 1, ui: 1, music: 1, ambience: 1 });
    const tapMaster = ctx.nodes.find(
      (n) =>
        n !== (mixer.master as unknown as FakeNode) && n.kind === 'gain' && n.gain.value === 0.3,
    );
    expect(tapMaster).toBeDefined();
    tap.close();
    for (const b of BUSES) expect(reaches(bus(b), dest)).toBe(false);
    for (const b of BUSES) expect(reaches(bus(b), ctx.destination)).toBe(true);
    tap.close(); // twice is harmless
  });
});
