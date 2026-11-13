// @vitest-environment happy-dom
// Theater mode's panel (owner, 2026-11-13: "Improve the UI on the theater mode. It's really bad, it
// doesn't even have a proper X button or responsive layout."): the X and Esc close it through the
// panels' owner (which locks the mouse again), the layout follows the viewport, every control has
// an accessible name, the controller works it, and at 360 px nothing is wider than the screen.

import { describe, expect, it } from 'vitest';

import { LAYOUT_CLASS, PadNav, accessibleName, focusables, layoutFor, timerText } from './layout';
import { type Subject, type TheaterMode, type TheaterModeClient, nearestSubjects } from './mode';
import { fakeClient, rig } from './panelRig';

describe('the theater panel', () => {
  it('opens through the panels; the X closes it and locks the mouse again', () => {
    const { theater, panels, locks, bar } = rig();
    theater.enter();
    expect(panels.panel).toBe('theater');
    expect(theater.active).toBe(true);
    expect(bar.hidden).toBe(false);
    const x = bar.querySelector<HTMLButtonElement>('#theater-close');
    expect(x?.tagName).toBe('BUTTON');
    expect(x?.getAttribute('aria-label')).toBe('Close theater mode');
    // It sits in the header, last: the top right.
    expect(x?.parentElement?.lastElementChild).toBe(x);
    x?.click();
    expect(panels.panel).toBe('none');
    expect(theater.active).toBe(false);
    expect(bar.hidden).toBe(true);
    expect(locks.length).toBe(1);
  });

  it('closes on Esc; Esc first stops a playing path and stays open', () => {
    const { theater, panels, calls } = rig();
    theater.enter();
    expect(panels.escape(10_000)).toBe(true);
    expect(theater.active).toBe(false);
    theater.enter();
    bar(theater).querySelector<HTMLButtonElement>('.tb-primary')?.click(); // ▶ Preview a preset
    expect(calls).toContain('play');
    expect(theater.busy).toBe('playing');
    expect(panels.escape(20_000)).toBe(true);
    expect(calls).toContain('stop');
    expect(theater.active).toBe(true);
    expect(panels.panel).toBe('theater');
    expect(panels.escape(30_000)).toBe(true);
    expect(theater.active).toBe(false);
  });

  it('shows Pause while a path plays, and the header alone', () => {
    const { theater, calls } = rig();
    theater.enter();
    bar(theater).querySelector<HTMLButtonElement>('.tb-primary')?.click();
    const pause = bar(theater).querySelector<HTMLButtonElement>('.tb-pause');
    expect(pause?.hidden).toBe(false);
    expect(bar(theater).dataset.busy).toBe('playing');
    pause?.click();
    expect(calls).toContain('pause');
    expect(pause?.textContent).toBe('Resume');
    pause?.click();
    expect(calls).toContain('resume');
  });

  it('switches its layout class by the viewport width and orientation', () => {
    expect(layoutFor(1440, 900)).toBe('docked');
    expect(layoutFor(390, 844)).toBe('sheet-bottom');
    expect(layoutFor(844, 390)).toBe('sheet-side');
    expect(layoutFor(800, 1000)).toBe('sheet-bottom');
    const { theater, bar } = rig();
    theater.enter();
    for (const [w, h, l] of [
      [1440, 900, 'docked'],
      [360, 740, 'sheet-bottom'],
      [740, 360, 'sheet-side'],
    ] as const) {
      expect(theater.relayout(w, h)).toBe(l);
      for (const [k, c] of Object.entries(LAYOUT_CLASS))
        expect(bar.classList.contains(c), `${c} at ${String(w)}×${String(h)}`).toBe(k === l);
    }
  });

  it('names every control', () => {
    const { theater, bar } = rig();
    theater.enter();
    for (const d of bar.querySelectorAll('details')) d.open = true;
    const controls = [...bar.querySelectorAll<HTMLElement>('button, input, select, summary')];
    expect(controls.length).toBeGreaterThan(20);
    for (const c of controls)
      expect(accessibleName(c), `${c.tagName}.${c.className} has no name`).not.toBe('');
    expect(bar.getAttribute('role')).toBe('dialog');
    expect(bar.getAttribute('aria-labelledby')).toBe('theater-title');
  });

  it('is worked by the controller: the d-pad moves the focus, A presses, B closes', () => {
    const { theater, bar } = rig();
    theater.enter();
    const nav = new PadNav();
    const pad = (...down: number[]) => ({
      buttons: Array.from({ length: 16 }, (_, i) => ({ pressed: down.includes(i) })),
    });
    const list = focusables(bar);
    nav.step(pad(13), bar);
    expect(document.activeElement).toBe(list[0]);
    nav.step(pad(), bar);
    nav.step(pad(13), bar);
    expect(document.activeElement).toBe(list[1]);
    nav.step(pad(13), bar); // held: no repeat
    expect(document.activeElement).toBe(list[1]);
    // To the Record tab and press it.
    const recordTab = [...bar.querySelectorAll<HTMLElement>('.tb-tab')].find(
      (t) => t.dataset.panel === 'record',
    );
    recordTab?.focus();
    nav.step(pad(), bar);
    nav.step(pad(0), bar);
    expect(recordTab?.getAttribute('aria-selected')).toBe('true');
    nav.step(pad(), bar);
    expect(nav.step(pad(1), bar)).toBe('close');
    // In the mode's own frame B leaves it.
    theater.frame(0, pad(1) as unknown as Gamepad);
    theater.frame(16, pad() as unknown as Gamepad);
    theater.frame(32, pad(1) as unknown as Gamepad);
    expect(theater.active).toBe(false);
  });

  it('a closed Advanced shows only its summary to the controller', () => {
    const { theater, bar } = rig();
    theater.enter();
    const more = bar.querySelector('details');
    expect(more).not.toBeNull();
    const inside = more?.querySelector('button');
    expect(focusables(bar)).not.toContain(inside);
    if (more) more.open = true;
    expect(focusables(bar)).toContain(inside);
  });

  it('times a take as m:ss', () => {
    expect(timerText(0)).toBe('0:00');
    expect(timerText(7_900)).toBe('0:07');
    expect(timerText(65_000)).toBe('1:05');
  });

  it('lists the creatures nearest first and follows the picked one by id', () => {
    const subjects: Subject[] = [
      { id: 4, name: 'deer', npc: false, at: [30, 50, 0] },
      { id: 9, name: 'villager', npc: true, at: [5, 50, 0] },
    ];
    expect(nearestSubjects(subjects, [0, 50, 0]).map((s) => s.id)).toEqual([9, 4]);
    const { theater, bar } = rig(subjects);
    theater.enter();
    // Presets → Target: Creature, pick the deer, Preview: the preset aims at {creature: 4}.
    let subject = '';
    (theater as unknown as { host: { client: TheaterModeClient } }).host.client.theaterPreset = (
      name,
      _p,
      s,
    ) => {
      subject = s;
      return fakeClient().client.theaterPreset(name, '', '');
    };
    const chip = [...bar.querySelectorAll<HTMLButtonElement>('.tb-chip')].find(
      (c) => c.textContent === 'Creature',
    );
    chip?.click();
    const who = bar.querySelector<HTMLSelectElement>('select[aria-label="Creature to follow"]');
    expect([...(who?.options ?? [])].map((o) => o.value)).toEqual(['', '9', '4']);
    if (who) {
      who.value = '4';
      who.dispatchEvent(new Event('change'));
    }
    bar.querySelector<HTMLButtonElement>('.tb-primary')?.click();
    expect(JSON.parse(subject)).toEqual({ creature: 4 });
  });
});

function bar(t: TheaterMode): HTMLElement {
  return t.bar;
}

describe('the phone’s flying controls', () => {
  it('map the stick and the Up and Down buttons onto the camera’s move', async () => {
    const { THEATER } = await import('./config');
    const { stickTilt, touchFly } = await import('./touch');
    const r = THEATER.touch.stickRadiusPx;
    const f = THEATER.touch.verticalFactor;
    // Pushed up (screen y negative) at full tilt: forward at the flying speed.
    expect(touchFly(stickTilt(0, -r), false, false, 10, 0.5)).toEqual([5, 0, 0]);
    // Right: sideways.
    const right = touchFly(stickTilt(r * 3, 0), false, false, 10, 0.5);
    expect(right[1]).toBeCloseTo(5);
    // Half way: half speed; a diagonal beyond the radius no faster than straight.
    expect(touchFly(stickTilt(0, r / 2), false, false, 10, 1)[0]).toBeCloseTo(-5);
    const diag = touchFly(stickTilt(r, -r), false, false, 10, 1);
    expect(Math.hypot(diag[0], diag[1])).toBeCloseTo(10);
    // Up and Down: at the vertical share; both held cancel.
    expect(touchFly(null, true, false, 10, 1)).toEqual([-0, 0, 10 * f]);
    expect(touchFly(null, false, true, 10, 1)[2]).toBeCloseTo(-10 * f);
    expect(touchFly(null, true, true, 10, 1)[2]).toBe(0);
  });

  it('show on a touch screen while the mode is on and nothing plays, and fly the camera', () => {
    const { theater } = rig();
    (theater as unknown as { host: { touchDevice: () => boolean } }).host.touchDevice = () => true;
    const moves: number[][] = [];
    const host = (theater as unknown as { host: { client: TheaterModeClient } }).host;
    host.client.theaterFly = (a, b, c) => {
      moves.push([a, b, c]);
      return [0, 50, 0, 0, 0];
    };
    theater.enter();
    theater.relayout(390, 844);
    theater.frame(0, null);
    const el = document.getElementById('theater-touch');
    expect(el?.hidden).toBe(false);
    expect(el?.classList.contains('tb-sheet-bottom')).toBe(true);
    const up = el?.querySelector<HTMLButtonElement>('.tt-up');
    expect(up?.getAttribute('aria-label')).toBe('Fly up');
    up?.dispatchEvent(new Event('pointerdown'));
    moves.length = 0;
    theater.frame(100, null);
    expect(moves.at(-1)?.[2]).toBeGreaterThan(0);
    up?.dispatchEvent(new Event('pointerup'));
    theater.exit();
    expect(el?.hidden).toBe(true);
  });
});

describe('frame-exact takes', () => {
  it('stamp frame i at i / fps and write a WebM any player reads', async () => {
    const { ID, muxWebm, readElements, vint } = await import('./webm');
    const { exactTimestampUs } = await import('./record');
    // A slow page or a fast one: frame i is at i / fps either way.
    expect(exactTimestampUs(0, 30)).toBe(0);
    expect(exactTimestampUs(30, 30)).toBe(1_000_000);
    expect(exactTimestampUs(7, 24)).toBe(291_667);
    expect([...vint(5)]).toEqual([0x85]);
    expect([...vint(200)]).toEqual([0x40, 200]);
    const frames = Array.from({ length: 70 }, (_, i) => ({
      timestampUs: exactTimestampUs(i, 30),
      key: i % 60 === 0,
      data: Uint8Array.from([i, i, i]),
    }));
    const file = muxWebm(
      { codec: 'V_VP8', width: 608, height: 1080 },
      frames,
      exactTimestampUs(70, 30),
    );
    const top = readElements(file);
    expect(top.map((e) => e.id)).toEqual([ID.EBML, ID.Segment]);
    const header = readElements(file, top[0]?.start, (top[0]?.start ?? 0) + (top[0]?.size ?? 0));
    const docType = header.find((e) => e.id === ID.DocType);
    expect(
      new TextDecoder().decode(
        file.subarray(docType?.start, (docType?.start ?? 0) + (docType?.size ?? 0)),
      ),
    ).toBe('webm');
    const seg = top[1];
    const parts = readElements(file, seg?.start, (seg?.start ?? 0) + (seg?.size ?? 0));
    const clusters = parts.filter((e) => e.id === ID.Cluster);
    expect(parts.slice(0, 2).map((e) => e.id)).toEqual([ID.Info, ID.Tracks]);
    expect(clusters.length).toBe(2); // a cluster per key frame
    // Every frame is a block, at its own time: cluster time + the block's offset.
    const times: number[] = [];
    for (const c of clusters) {
      const inside = readElements(file, c.start, c.start + c.size);
      const tc = inside.find((e) => e.id === ID.Timecode);
      const base = tc
        ? (file[tc.start] ?? 0) * (tc.size > 1 ? 256 : 1) +
          (tc.size > 1 ? (file[tc.start + 1] ?? 0) : 0)
        : 0;
      for (const b of inside.filter((e) => e.id === ID.SimpleBlock)) {
        const rel = ((file[b.start + 1] ?? 0) << 8) | (file[b.start + 2] ?? 0);
        times.push(base + rel);
      }
    }
    expect(times.length).toBe(70);
    expect(times).toEqual(frames.map((f) => Math.round(f.timestampUs / 1000)));
  });
});
