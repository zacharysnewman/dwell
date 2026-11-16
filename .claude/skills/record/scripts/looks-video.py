#!/usr/bin/env python3
"""Terrain-looks timelapse (owner, 2026-11-16): the stills of each version's look (look-shot.mjs), oldest to
newest, kept where the look changed, spliced into one portrait video on a speed curve.

  python3 looks-video.py --out video.mp4 [--min-diff 1.0] [--first 0.35] [--last 2.5] [--hold 3]
                         [--fade 0.3] [--sheet sheet.png] DIR [DIR ...] [--set DIR[,DIR] ...]

Several `--set`s play back to back (owner, 2026-11-16: "both sets of screenshots back to back. A 2 second pause
on the last shot of each"): each set is chosen and eased on its own, its last still held `--hold` seconds.

Each DIR holds `<version>-<view>.png`; every PNG is ordered by its version. A still is kept when its mean
difference from the last kept one (grey, at 270×480) is at least `--min-diff`; the newest is always kept.
The kept stills are shown from `--first` seconds each, easing (ease-in, quadratic) to `--last`, the newest held
`--hold` seconds more, each cross-fading into the next over `--fade`. No text (the owner's showcase rule).
`--sheet` writes a contact sheet of the kept stills with their versions (for choosing, not for the video).
"""
import argparse
import glob
import os
import subprocess

from PIL import Image, ImageChops, ImageDraw, ImageStat


def version_key(path):
    v = os.path.basename(path).split('-')[0]
    return tuple(int(x) for x in v.split('.'))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('dirs', nargs='*')
    ap.add_argument('--set', action='append', default=[])
    ap.add_argument('--out', required=True)
    ap.add_argument('--min-diff', type=float, default=1.0)
    ap.add_argument('--first', type=float, default=0.35)
    ap.add_argument('--last', type=float, default=2.5)
    ap.add_argument('--hold', type=float, default=3.0)
    ap.add_argument('--fade', type=float, default=0.3)
    ap.add_argument('--size', default='1080x1920')
    ap.add_argument('--sheet')
    a = ap.parse_args()
    w, h = (int(x) for x in a.size.split('x'))
    sets = [d.split(',') for d in a.set] + ([a.dirs] if a.dirs else [])
    kept, durations = [], []
    for dirs in sets:
        stills = sorted({p for d in dirs for p in glob.glob(os.path.join(d, '*.png'))}, key=version_key)
        these, last = [], None
        for i, p in enumerate(stills):
            small = Image.open(p).convert('L').resize((270, 480))
            diff = 1e9 if last is None else ImageStat.Stat(ImageChops.difference(small, last)).mean[0]
            if diff >= a.min_diff or i == len(stills) - 1:
                these.append(p)
                last = small
        print(f'{len(these)} of {len(stills)} stills kept:', ' '.join(os.path.basename(p).split('-')[0] for p in these))
        n = len(these)
        for i in range(n):
            t = i / max(1, n - 1)
            durations.append(a.first + (a.last - a.first) * t * t + (a.hold if i == n - 1 else 0))
        kept += these
    if a.sheet:
        cols = 6
        rows = (len(kept) + cols - 1) // cols
        sheet = Image.new('RGB', (cols * 180, rows * 340), 'white')
        draw = ImageDraw.Draw(sheet)
        for i, p in enumerate(kept):
            x, y = (i % cols) * 180, (i // cols) * 340
            sheet.paste(Image.open(p).convert('RGB').resize((180, 320)), (x, y + 20))
            draw.text((x + 4, y + 4), os.path.basename(p).split('-')[0], fill='black')
        sheet.save(a.sheet)
        print('sheet', a.sheet)
    n = len(kept)
    # One ffmpeg pass: each still a looped input of its duration (plus the fade), chained with xfade.
    cmd = ['ffmpeg', '-y', '-loglevel', 'error']
    for p, d in zip(kept, durations):
        cmd += ['-loop', '1', '-t', f'{d + a.fade:.3f}', '-i', p]
    chains, prev, offset = [], '0:v', 0.0
    scale = f'scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},fps=30,format=yuv420p'
    for i in range(n):
        chains.append(f'[{i}:v]{scale}[s{i}]')
    prev = 's0'
    for i in range(1, n):
        offset += durations[i - 1]
        out = f'x{i}'
        chains.append(f'[{prev}][s{i}]xfade=transition=fade:duration={a.fade}:offset={offset:.3f}[{out}]')
        prev = out
    total = sum(durations)
    chains.append(f'[{prev}]fade=t=out:st={total - 0.6:.3f}:d=0.6[v]')
    cmd += ['-filter_complex', ';'.join(chains), '-map', '[v]', '-t', f'{total:.3f}',
            '-c:v', 'libx264', '-crf', '20', '-preset', 'medium', '-pix_fmt', 'yuv420p', a.out]
    subprocess.run(cmd, check=True)
    print(f'{a.out}: {total:.1f} s, {n} looks')


if __name__ == '__main__':
    main()
