// What `npm run sfx:render` renders (docs/AUDIO.md §7), from the sound data alone: every set's
// every event, every event's, action's and UI sound, a 10-step walk and run per surface, break
// and place variations per set, and landings soft to hard. Pure, so the plan is tested.
import type { SoundData } from './types';

export interface RenderPlay {
  sound: string;
  /** Seconds into the render. */
  at: number;
  intensity?: number;
  pan?: number;
  gain?: number;
  bus: 'effects' | 'ui';
}

export interface RenderJob {
  /** File name without `.wav`. */
  name: string;
  plays: RenderPlay[];
  seconds: number;
  seed: number;
  /** Sensible audible length, seconds. */
  minSeconds: number;
  maxSeconds: number;
}

const local = (id: string): string => id.slice(id.indexOf(':') + 1);
/** Seconds of tail after the last play (reverb). */
const TAIL = 0.8;

export function renderPlan(data: SoundData, walkSpeed = 5, runSpeed = 8): RenderJob[] {
  const jobs: RenderJob[] = [];
  const single = (name: string, sound: string, bus: 'effects' | 'ui', intensity = 1): void => {
    jobs.push({
      name,
      plays: [{ sound, at: 0.02, intensity, bus }],
      seconds: 2.5,
      seed: 1,
      minSeconds: 0.01,
      maxSeconds: 2.0,
    });
  };
  for (const [set, events] of Object.entries(data.sets)) {
    for (const [event, sound] of Object.entries(events))
      single(`set_${local(set)}_${event}`, sound, 'effects');
  }
  for (const e of data.events) if (e.sound) single(`event_${e.name}`, e.sound, 'effects');
  for (const [name, sound] of Object.entries(data.actions)) single(`action_${name}`, sound, 'ui');
  for (const [kind, sound] of Object.entries(data.ui)) single(`ui_${kind}`, sound, 'ui');

  const sequence = (
    name: string,
    sound: string,
    times: number[],
    extra: (i: number) => Partial<RenderPlay>,
  ): void => {
    const last = times[times.length - 1] ?? 0;
    jobs.push({
      name,
      plays: times.map((at, i) => ({ sound, at: at + 0.02, bus: 'effects', ...extra(i) })),
      seconds: last + TAIL + 1,
      seed: 7,
      minSeconds: last,
      maxSeconds: last + 2,
    });
  };
  const steps = (gait: 'walk' | 'run', speed: number): number[] => {
    const g = data.gaits[gait] ?? { stride: 2, gain: 1 };
    return Array.from({ length: 10 }, (_, i) => (i * g.stride) / speed);
  };
  for (const [set, events] of Object.entries(data.sets)) {
    const name = local(set);
    if (events.step) {
      for (const [gait, speed] of [
        ['walk', walkSpeed],
        ['run', runSpeed],
      ] as const) {
        const gain = data.gaits[gait]?.gain ?? 1;
        sequence(`steps_${name}_${gait}_x10`, events.step, steps(gait, speed), (i) => ({
          pan: i % 2 ? 0.12 : -0.12,
          gain,
        }));
      }
    }
    if (events.break) sequence(`break_${name}_x4`, events.break, [0, 0.6, 1.2, 1.8], () => ({}));
    if (events.place) sequence(`place_${name}_x4`, events.place, [0, 0.5, 1.0, 1.5], () => ({}));
    if (events.land) {
      const levels = [0.1, 0.4, 0.7, 1];
      sequence(`land_${name}_soft_to_hard`, events.land, [0, 0.7, 1.4, 2.1], (i) => ({
        intensity: levels[i] ?? 1,
      }));
    }
  }
  return jobs;
}
