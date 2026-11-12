#!/usr/bin/env python3
"""Splice recorded clips into one video (the `record` skill).

python3 splice.py plan.json — plan: {"out", "size": [w, h], "fps", "title", "subtitle",
"clips": [{"src": frame folder or mp4, "from": s, "to": s, "speed"?: k (0.5 = half speed),
"vf"?: extra ffmpeg filters (e.g. a grade, "eq=gamma=1.4"), "cut"?: true (a hard cut into this clip, no
cross-fade: to split one take into parts at different speeds)}]}. Frame folders come from driver.mjs
(f00000.jpg … plus times.json); they're turned into an mp4 at their measured frame rate first.
Writes `out`, and a still per clip next to it."""
import json, os, subprocess, sys

plan = json.load(open(sys.argv[1]))
W, H = plan.get('size', [1080, 1920]); FPS = plan.get('fps', 30); X = plan.get('fade', 0.8)
out = plan['out']; work = os.path.splitext(out)[0] + '_seg'; os.makedirs(work, exist_ok=True)
V = f'scale={W}:{H}:force_original_aspect_ratio=increase,crop={W}:{H},fps={FPS},format=yuv420p,setsar=1'

def run(*a): subprocess.run(['ffmpeg', '-v', 'error', '-y', *a], check=True)

def as_mp4(src):
    if src.endswith('.mp4'): return src
    times = json.load(open(os.path.join(src, 'times.json')))
    fps = (len(times) - 1) / max(times[-1] - times[0], 1e-6) if len(times) > 1 else FPS
    mp4 = src.rstrip('/') + '.mp4'
    if os.path.exists(mp4) and os.path.getmtime(mp4) >= os.path.getmtime(os.path.join(src, 'times.json')):
        return mp4  # already encoded (a take split into several clips, or a re-splice)
    run('-framerate', f'{fps:.3f}', '-i', os.path.join(src, 'f%05d.jpg'), '-c:v', 'libx264', '-crf', '16', '-pix_fmt', 'yuv420p', mp4)
    return mp4

segs = []
clips = [(as_mp4(c['src']), c.get('from', 0), c['to']) for c in plan['clips']]
speeds = [float(c.get('speed', 1)) for c in plan['clips']]
extra = [c.get('vf') for c in plan['clips']]
cuts = [bool(c.get('cut')) for c in plan['clips']]
filters = subprocess.run(['ffmpeg', '-hide_banner', '-filters'], capture_output=True, text=True).stdout
if plan.get('title') and 'drawtext' in filters:
    font = next((f for f in ['/System/Library/Fonts/Supplemental/Georgia.ttf', '/System/Library/Fonts/Helvetica.ttc'] if os.path.exists(f)), None)
    esc = lambda s: s.replace(':', r'\:').replace("'", r"\'")
    vf = f"{V},trim=0:3,setpts=PTS-STARTPTS,eq=brightness=-0.25:saturation=0.8"
    vf += f",drawtext=fontfile={font}:text='{esc(plan['title'])}':fontsize={W // 18}:fontcolor=white:x=(w-tw)/2:y=(h-th)/2-10"
    if plan.get('subtitle'):
        vf += f",drawtext=fontfile={font}:text='{esc(plan['subtitle'])}':fontsize={W // 40}:fontcolor=white@0.85:x=(w-tw)/2:y=(h/2)+{W // 20}"
    title = os.path.join(work, 'title.mp4')
    run('-ss', str(clips[0][1]), '-t', '3', '-i', clips[0][0], '-vf', vf, '-an', '-c:v', 'libx264', '-crf', '20', title)
    segs.append((title, 3))
    cuts.insert(0, False)
for i, (src, a, b) in enumerate(clips):
    seg = os.path.join(work, f'{i:02d}.mp4')
    k = speeds[i]
    vf = (f'setpts=PTS/{k},' if k != 1 else '') + V + (f',{extra[i]}' if extra[i] else '')
    run('-ss', str(a), '-t', str(b - a), '-i', src, '-vf', vf, '-an', '-c:v', 'libx264', '-crf', '20', seg)
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
    ins = sum((['-i', p] for p, _ in group), [])
    labels = ''.join(f'[{i}:v]' for i in range(len(group)))
    run(*ins, '-filter_complex', f'{labels}concat=n={len(group)}:v=1:a=0[v]', '-map', '[v]', '-c:v', 'libx264', '-crf', '18', joined)
    segs.append((joined, sum(d for _, d in group)))
inputs = sum((['-i', p] for p, _ in segs), [])
parts, prev, t = [], '[0:v]', segs[0][1]
for i in range(1, len(segs)):
    parts.append(f'{prev}[{i}:v]xfade=transition=fade:duration={X}:offset={t - X:.3f}[x{i}]')
    prev, t = f'[x{i}]', t + segs[i][1] - X
parts.append(f'{prev}fade=t=out:st={t - 1:.3f}:d=1[out]')
run(*inputs, '-filter_complex', ';'.join(parts), '-map', '[out]', '-r', str(FPS), '-c:v', 'libx264', '-crf', str(plan.get('crf', 21)),
    '-preset', 'slow', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', out)
mb = os.path.getsize(out) / 1048576
print(f'{out}: {t:.1f} s, {mb:.1f} MB' + ('  (over 30 MB: re-encode with "crf": 28)' if mb > 30 else ''))
for i, (p, d) in enumerate(stills):
    run('-ss', str(d / 2), '-i', p, '-frames:v', '1', os.path.splitext(out)[0] + f'_still_{i:02d}.jpg')
