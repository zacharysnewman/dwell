// The record skill's showcase sound (WORK_QUEUE item 40, owner: "showcases shouldn't have music, but sound
// effects and ambiance are good"): the in-page capture the driver runs during a take, the alignment of that
// capture to the screencast's first frame, and the ffmpeg arguments that mux it into a shot's clip. Plain
// functions, no browser: take.test.mjs tests them.

/** The rate and codec every clip and the final video carry (AAC at 48 kHz, stereo). */
export const AUDIO_RATE = 48000;
export const AUDIO_CODEC = ['-c:a', 'aac', '-b:a', '192k', '-ar', String(AUDIO_RATE), '-ac', '2'];

/**
 * The page-side expression that starts recording the game's sound: `__dwell.audioTap` (effects and ambience,
 * never the music or UI) into a MediaRecorder; the wall-clock start (epoch seconds, the screencast frames'
 * clock) is kept for the alignment. Resolves to the start, or null when the page has no audio.
 */
export function audioStartExpr(opts = { music: false, ui: false }) {
  return `(async () => {
    const tap = globalThis.__dwell?.audioTap?.(${JSON.stringify(opts)});
    if (!tap) return null;
    const mime = ['audio/webm;codecs=opus', 'audio/webm', 'audio/ogg;codecs=opus'].find((t) => MediaRecorder.isTypeSupported(t)) ?? '';
    const rec = new MediaRecorder(tap.stream, { ...(mime ? { mimeType: mime } : {}), audioBitsPerSecond: 192000 });
    const chunks = [];
    rec.ondataavailable = (e) => { if (e.data.size > 0) chunks.push(e.data); };
    const started = new Promise((r) => { rec.onstart = () => r((performance.timeOrigin + performance.now()) / 1000); });
    rec.start(1000);
    const startS = await started;
    globalThis.__recAudio = { rec, chunks, tap, startS, mime: mime || rec.mimeType, buses: tap.buses };
    return { startS, buses: tap.buses };
  })()`;
}

/** The page-side expression that stops it: resolves to {bytes (base64), mime, startS, stopS, buses} or null. */
export function audioStopExpr() {
  return `(async () => {
    const a = globalThis.__recAudio;
    if (!a) return null;
    globalThis.__recAudio = null;
    await new Promise((r) => { a.rec.onstop = r; a.rec.stop(); });
    const stopS = (performance.timeOrigin + performance.now()) / 1000;
    a.tap.close();
    const buf = new Uint8Array(await new Blob(a.chunks).arrayBuffer());
    let bin = '';
    for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000));
    return { bytes: btoa(bin), mime: a.mime, startS: a.startS, stopS, buses: a.buses };
  })()`;
}

/**
 * Where the audio sits against the clip: `offsetS` is the audio's start minus the first frame's (both epoch
 * seconds); positive means the sound began after the first frame (pad silence before it), negative that it
 * began before (trim its head). `durationS` is the clip's length, frames ÷ rate.
 */
export function audioAlignment(frameTimes, audio, rate) {
  if (!frameTimes.length) throw new Error('no frames to align the audio to');
  return { offsetS: audio.startS - frameTimes[0], durationS: frameTimes.length / rate };
}

/**
 * The audio filter for one clip: resampled to 48 kHz stereo, aligned to the first frame (a delay, or the head
 * trimmed), padded with silence and cut to the clip's length.
 */
export function alignFilter({ offsetS, durationS }, input = '1:a', out = 'a') {
  const f = [`aresample=${AUDIO_RATE}`, 'aformat=sample_fmts=fltp:channel_layouts=stereo'];
  if (offsetS > 0.0005) f.push(`adelay=delays=${Math.round(offsetS * 1000)}:all=1`);
  else if (offsetS < -0.0005) f.push(`atrim=start=${(-offsetS).toFixed(3)}`, 'asetpts=PTS-STARTPTS');
  f.push('apad', `atrim=0:${durationS.toFixed(3)}`);
  return `[${input}]${f.join(',')}[${out}]`;
}

/**
 * ffmpeg's arguments to encode a shot's frames to its clip (the splice's timeline), with its sound muxed in
 * when `audio` is given ({file, offsetS, durationS}).
 */
export function encodeArgs({ frames, rate, out, fps, crf = 16, preset = 'veryfast', audio = null }) {
  const a = ['-v', 'error', '-y', '-framerate', rate.toFixed(3), '-i', frames];
  if (audio) a.push('-i', audio.file, '-filter_complex', alignFilter(audio), '-map', '0:v', '-map', '[a]');
  a.push(...(fps ? ['-r', String(fps)] : []), '-c:v', 'libx264', '-crf', String(crf), '-preset', preset, '-pix_fmt', 'yuv420p');
  a.push(...(audio ? AUDIO_CODEC : ['-an']), out);
  return a;
}

/**
 * What a splice part does with its shot's sound: `"keep"` (the default at speed 1), `"stretch"` (time-stretched
 * to the part's speed, pitch kept), `"mute"` (silence). `"auto"` (or none) keeps it at speed 1, stretches it
 * for a speed in [0.25, 2] (slow motion, a gentle speed-up) and mutes a time-lapse past 2× or a crawl under ¼.
 */
export const PART_AUDIO = ['auto', 'keep', 'stretch', 'mute'];
export function partAudio(mode = 'auto', speed = 1) {
  if (mode === 'mute') return 'mute';
  if (mode === 'keep' || mode === 'stretch') return speed === 1 ? 'keep' : 'stretch';
  if (speed === 1) return 'keep';
  return speed >= 0.25 && speed <= 2 ? 'stretch' : 'mute';
}
