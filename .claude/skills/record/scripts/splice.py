#!/usr/bin/env python3
"""Splice recorded clips into one video (the `record` skill).

python3 splice.py plan.json [--dry-run] — plan: {"out", "size": [w, h], "fps", "title", "subtitle", "audio"?: true
(the clips carry the game's sound: effects and ambience, never music),
"clips": [{"src": frame folder or mp4, "from": s, "to": s, "speed"?: k (0.5 = half speed),
"vf"?: extra ffmpeg filters (e.g. a grade, "eq=gamma=1.4"), "cut"?: true (a hard cut into this clip, no
cross-fade: to split one take into parts at different speeds), "audio"?: "keep" | "stretch" | "mute" (with the
plan's audio: the clip's sound as is, time-stretched to its speed with the pitch kept, or silence)}]}.
Frame folders come from driver.mjs (f00000.jpg … plus times.json); they're turned into an mp4 at their measured
frame rate first. The sound cuts, cross-fades and fades out with the picture. Writes `out`, and a still per clip
next to it. --dry-run prints the ffmpeg commands as JSON and runs nothing (take.test.mjs checks the graphs)."""
import json, os, subprocess, sys

RATE = 48000
AUDIO = ['-c:a', 'aac', '-b:a', '192k', '-ar', str(RATE), '-ac', '2']
AFMT = f'aresample={RATE},aformat=sample_fmts=fltp:channel_layouts=stereo'


def atempo(k):
    """A chain of atempo filters for speed k (each step within ffmpeg's 0.5–2 range)."""
    steps = []
    while k > 2: steps.append(2.0); k /= 2
    while k < 0.5: steps.append(0.5); k /= 0.5
    steps.append(k)
    return ','.join(f'atempo={s:.6g}' for s in steps)


def segment_args(src, a, b, k, vf, audio, out):
    """ffmpeg's arguments for one clip's segment: cut [a, b), at speed k, scaled; its sound kept, stretched to k,
    or silence (audio None: no sound track at all)."""
    d = (b - a) / k
    args = ['-ss', str(a), '-t', str(b - a), '-i', src]
    if audio is None:
        return args + ['-vf', vf, '-an', '-c:v', 'libx264', '-crf', '20', out]
    if audio == 'mute':
        args += ['-f', 'lavfi', '-t', f'{d:.3f}', '-i', f'anullsrc=r={RATE}:cl=stereo']
        af = f'[1:a]{AFMT},atrim=0:{d:.3f}[a]'
    else:
        af = f'[0:a]{AFMT}' + (f',{atempo(k)}' if k != 1 else '') + f',apad,atrim=0:{d:.3f}[a]'
    return args + ['-filter_complex', f'[0:v]{vf}[v];{af}', '-map', '[v]', '-map', '[a]',
                   '-c:v', 'libx264', '-crf', '20', *AUDIO, out]


def concat_args(paths, audio, out):
    """A group of hard-cut segments joined end to end (picture and sound)."""
    ins = sum((['-i', p] for p in paths), [])
    n = len(paths)
    labels = ''.join(f'[{i}:v]' + (f'[{i}:a]' if audio else '') for i in range(n))
    graph = f'{labels}concat=n={n}:v=1:a={1 if audio else 0}[v]' + ('[a]' if audio else '')
    return [*ins, '-filter_complex', graph, '-map', '[v]', *(['-map', '[a]', *AUDIO] if audio else []),
            '-c:v', 'libx264', '-crf', '18', out]


def join_graph(durations, fade, audio, gain_db=0):
    """The cross-fades between the groups (xfade, and acrossfade for the sound: both shorten the video by `fade`
    a join) and the fade out of the last second; `gain_db` lifts the whole sound (the tap records the game's mix
    quietly, ~-50 dB mean), with a limiter so it can't clip. Returns (filter graph, total seconds)."""
    parts, prev, aprev, t = [], '[0:v]', '[0:a]', durations[0]
    for i in range(1, len(durations)):
        parts.append(f'{prev}[{i}:v]xfade=transition=fade:duration={fade}:offset={t - fade:.3f}[x{i}]')
        if audio:
            parts.append(f'{aprev}[{i}:a]acrossfade=d={fade}:c1=tri:c2=tri[ax{i}]')
            aprev = f'[ax{i}]'
        prev, t = f'[x{i}]', t + durations[i] - fade
    parts.append(f'{prev}fade=t=out:st={t - 1:.3f}:d=1[out]')
    if audio:
        lift = f'volume={gain_db}dB,alimiter=limit=0.9,' if gain_db else ''
        parts.append(f'{aprev}{lift}afade=t=out:st={t - 1:.3f}:d=1,atrim=0:{t:.3f}[aout]')
    return ';'.join(parts), t


def main(argv):
    dry = '--dry-run' in argv
    plan = json.load(open([a for a in argv if not a.startswith('--')][0]))
    W, H = plan.get('size', [1080, 1920]); FPS = plan.get('fps', 30); X = plan.get('fade', 0.8)
    audio = bool(plan.get('audio'))
    out = plan['out']; work = os.path.splitext(out)[0] + '_seg'
    if not dry: os.makedirs(work, exist_ok=True)
    V = f'scale={W}:{H}:force_original_aspect_ratio=increase,crop={W}:{H},fps={FPS},format=yuv420p,setsar=1'
    commands = []

    def run(*a):
        commands.append(['ffmpeg', '-v', 'error', '-y', *a])
        if not dry: subprocess.run(commands[-1], check=True)

    def as_mp4(src):
        if src.endswith('.mp4') or dry: return src if src.endswith('.mp4') else src.rstrip('/') + '.mp4'
        times = json.load(open(os.path.join(src, 'times.json')))
        fps = (len(times) - 1) / max(times[-1] - times[0], 1e-6) if len(times) > 1 else FPS
        mp4 = src.rstrip('/') + '.mp4'
        if os.path.exists(mp4) and os.path.getmtime(mp4) >= os.path.getmtime(os.path.join(src, 'times.json')):
            return mp4  # already encoded (a take split into several clips, or a re-splice)
        run('-framerate', f'{fps:.3f}', '-i', os.path.join(src, 'f%05d.jpg'), '-c:v', 'libx264', '-crf', '16', '-pix_fmt', 'yuv420p', mp4)
        return mp4

    def has_audio(src):
        if dry: return True
        r = subprocess.run(['ffprobe', '-v', 'error', '-select_streams', 'a', '-show_entries', 'stream=index', '-of', 'csv=p=0', src],
                           capture_output=True, text=True)
        return bool(r.stdout.strip())

    segs = []
    clips = [(as_mp4(c['src']), c.get('from', 0), c['to']) for c in plan['clips']]
    speeds = [float(c.get('speed', 1)) for c in plan['clips']]
    extra = [c.get('vf') for c in plan['clips']]
    cuts = [bool(c.get('cut')) for c in plan['clips']]
    sound = [(c.get('audio', 'keep') if has_audio(clips[i][0]) else 'mute') if audio else None for i, c in enumerate(plan['clips'])]
    filters = '' if dry else subprocess.run(['ffmpeg', '-hide_banner', '-filters'], capture_output=True, text=True).stdout
    if plan.get('title') and 'drawtext' in filters:
        font = next((f for f in ['/System/Library/Fonts/Supplemental/Georgia.ttf', '/System/Library/Fonts/Helvetica.ttc'] if os.path.exists(f)), None)
        esc = lambda s: s.replace(':', r'\:').replace("'", r"\'")
        vf = f"{V},trim=0:3,setpts=PTS-STARTPTS,eq=brightness=-0.25:saturation=0.8"
        vf += f",drawtext=fontfile={font}:text='{esc(plan['title'])}':fontsize={W // 18}:fontcolor=white:x=(w-tw)/2:y=(h-th)/2-10"
        if plan.get('subtitle'):
            vf += f",drawtext=fontfile={font}:text='{esc(plan['subtitle'])}':fontsize={W // 40}:fontcolor=white@0.85:x=(w-tw)/2:y=(h/2)+{W // 20}"
        title = os.path.join(work, 'title.mp4')
        run(*segment_args(clips[0][0], clips[0][1], clips[0][1] + 3, 1, vf, 'mute' if audio else None, title))
        segs.append((title, 3))
        cuts.insert(0, False)
    for i, (src, a, b) in enumerate(clips):
        seg = os.path.join(work, f'{i:02d}.mp4')
        k = speeds[i]
        vf = (f'setpts=PTS/{k},' if k != 1 else '') + V + (f',{extra[i]}' if extra[i] else '')
        run(*segment_args(src, a, b, k, vf, sound[i], seg))
        segs.append((seg, (b - a) / k))
    stills = list(segs)
    # A clip with "cut" joins the one before it with a hard cut (concatenated), then the groups cross-fade.
    groups = []
    for i, sd in enumerate(segs):
        if cuts[i] and groups: groups[-1].append(sd)
        else: groups.append([sd])
    segs = []
    for g, group in enumerate(groups):
        if len(group) == 1: segs.append(group[0]); continue
        joined = os.path.join(work, f'cut{g:02d}.mp4')
        run(*concat_args([p for p, _ in group], audio, joined))
        segs.append((joined, sum(d for _, d in group)))
    inputs = sum((['-i', p] for p, _ in segs), [])
    graph, t = join_graph([d for _, d in segs], X, audio, plan.get('gainDb', 0))
    run(*inputs, '-filter_complex', graph, '-map', '[out]', *(['-map', '[aout]', *AUDIO] if audio else ['-an']),
        '-r', str(FPS), '-c:v', 'libx264', '-crf', str(plan.get('crf', 21)),
        '-preset', 'slow', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', out)
    if dry:
        print(json.dumps({'commands': commands, 'seconds': round(t, 3)}))
        return
    mb = os.path.getsize(out) / 1048576
    print(f'{out}: {t:.1f} s, {mb:.1f} MB' + ('  (over 30 MB: re-encode with "crf": 28)' if mb > 30 else ''))
    for i, (p, d) in enumerate(stills):
        run('-ss', str(d / 2), '-i', p, '-frames:v', '1', os.path.splitext(out)[0] + f'_still_{i:02d}.jpg')


if __name__ == '__main__':
    main(sys.argv[1:])
