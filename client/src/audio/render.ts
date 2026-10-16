// Offline renders in a browser (docs/AUDIO.md §7; `npm run sfx:render` loads this into Chromium):
// one job's plays through the real synth and mixer in an OfflineAudioContext, returned as a WAV
// and its statistics.
import { buildMixer, DEFAULT_VOLUMES, soundInput } from './mixer';
import { SOUND_DATA } from './sounds.gen';
import { makeNoise, playSound, startLoop } from './synth';
import { Variation } from './variation';
import { encodeWav, renderStats, type RenderStats } from './wav';
import type { RenderJob } from './renderPlan';

const RATE = 48000;

export interface RenderResult {
  name: string;
  wav: string;
  stats: RenderStats;
}

function base64(bytes: Uint8Array): string {
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000)
    s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

export async function renderJob(job: RenderJob): Promise<RenderResult> {
  const ctx = new OfflineAudioContext(2, Math.ceil(job.seconds * RATE), RATE);
  const mixer = buildMixer(ctx, DEFAULT_VOLUMES);
  // Renders are at full volume, so a check of the peak is a check of the loudest the game plays.
  mixer.setVolumes({ master: 1, effects: 1, ui: 1 });
  const noise = makeNoise(ctx);
  const variation = new Variation(job.seed);
  for (const p of job.plays) {
    const sound = SOUND_DATA.sounds[p.sound];
    if (!sound) throw new Error(`no sound ${p.sound}`);
    const input = soundInput(ctx, mixer, p.bus, sound.reverb ?? 0);
    const params = variation.draw(p.sound, sound, p.intensity ?? 1);
    if (p.loop) {
      // As the game drives it: an intensity update every 50 ms (sound.rs), then the stop.
      const { seconds, from, to } = p.loop;
      const voice = startLoop(ctx, noise, input, sound, params, p.at, from, {
        ...(p.gain !== undefined ? { gain: p.gain } : {}),
      });
      for (let t = 0.05; t < seconds; t += 0.05)
        voice.set(from + ((to - from) * t) / seconds, p.at + t);
      voice.stop(p.at + seconds);
      continue;
    }
    playSound(ctx, noise, input, sound, params, p.at, {
      ...(p.pan !== undefined ? { pan: p.pan } : {}),
      ...(p.gain !== undefined ? { gain: p.gain } : {}),
    });
  }
  const buffer = await ctx.startRendering();
  const channels = [buffer.getChannelData(0), buffer.getChannelData(1)];
  return {
    name: job.name,
    wav: base64(encodeWav(channels, RATE)),
    stats: renderStats(channels, RATE),
  };
}

declare global {
  interface Window {
    renderJob?: typeof renderJob;
  }
}
window.renderJob = renderJob;
