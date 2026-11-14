import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { THEATER, parseTheaterConfig } from './config';
import { HUD_ELEMENTS, HUD_RULES, setHud, shownWhenClean } from './hud';
import { KeyframeEditor, MIN_GAP_S, type PathJson } from './keyframes';
import { flyMove, wheelSpeed } from './mode';
import { extensionOf, fitOnScreen, pickMime, recordSize, takeTracks } from './record';
import { type KeyValue, PathStore, STORE_KEY, exportName, parseImported } from './store';

const SRC = join(__dirname, '..');

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return n === 'pkg' ? [] : sources(p);
    return n.endsWith('.ts') && !n.endsWith('.test.ts') && !n.endsWith('.gen.ts') ? [p] : [];
  });
}

const at = (x: number): Omit<Parameters<KeyframeEditor['add']>[0], 't'> => ({
  position: [x, 40, 0],
  look: [x, 0],
});

describe('the HUD switch', () => {
  it('hides every HUD part the sources put on the page, and only keeps the canvas and the bar', () => {
    // The audit: every place a source builds a HUD part on `document.body` (a class given
    // `document.body`, or an append to it) and every element index.html's body starts with is
    // listed in HUD_ELEMENTS. A new HUD part fails here until it is listed (and so is a child of
    // `body`, which the switch's one rule hides).
    const found = new Set<string>();
    for (const f of sources(SRC)) {
      const text = readFileSync(f, 'utf8');
      for (const m of text.matchAll(/new (\w+)\(\s*document\.body/g)) found.add(m[1] ?? '');
      if (/(doc|document)\.body\.append\(/.test(text)) {
        const name = f.split('/').at(-1)?.replace('.ts', '') ?? '';
        found.add(name);
      }
    }
    const html = readFileSync(join(SRC, '..', 'index.html'), 'utf8');
    const body = html.slice(html.indexOf('<body'));
    for (const m of body.matchAll(/^\s{4}<(\w+) id="([\w-]+)"/gm))
      if (m[2] !== 'view') found.add(`index.html #${m[2] ?? ''}`);
    const listed = HUD_ELEMENTS.map((e) => e.split(' ')[0]);
    // Modules appending something that is not a HUD part: a download link, a drag ghost.
    const notHud = new Set(['record', 'structuresPanel', 'creativeInventory', 'mode', 'panelRig']);
    for (const name of found) {
      if (notHud.has(name)) continue;
      const short = name.startsWith('index.html') ? name : name;
      expect(
        listed.includes(short) || HUD_ELEMENTS.includes(short as never),
        `${name} is not in HUD_ELEMENTS`,
      ).toBe(true);
    }
    // The rule hides every child of body but the canvas and the bar (and the bar in a take).
    // The rules live in style.css (the page's CSP forbids inline style): they must be there.
    const css = readFileSync(join(SRC, 'style.css'), 'utf8');
    for (const rule of HUD_RULES) expect(css, rule).toContain(`${rule} {`);
    expect(HUD_RULES[0]).toContain(':not(#view):not(#theater-bar)');
    for (const id of ['net-status', 'perf-status', 'build-info', 'theater-open', 'hotbar'])
      expect(shownWhenClean(id, false)).toBe(false);
    expect(shownWhenClean('view', true)).toBe(true);
    expect(shownWhenClean('theater-bar', false)).toBe(true);
    expect(shownWhenClean('theater-bar', true)).toBe(false);
  });

  it('switches the class, the take and the arms together', () => {
    const classes = new Set<string>();
    const arms: boolean[] = [];
    const doc = {
      body: {
        classList: {
          toggle: (c: string, on: boolean) => (on ? classes.add(c) : classes.delete(c)),
        },
      },
    } as unknown as Document;
    const client = { arms: (on: boolean) => arms.push(on) };
    setHud(doc, client, false, true);
    expect([...classes].sort()).toEqual(['theater-clean', 'theater-take']);
    setHud(doc, client, false, false);
    expect([...classes]).toEqual(['theater-clean']);
    setHud(doc, client, true);
    expect(classes.size).toBe(0);
    expect(arms).toEqual([false, false, true]);
  });
});

describe('the keyframe editor', () => {
  it('adds keys at the end, between the selected and the next, and splits their segment', () => {
    const e = new KeyframeEditor('shot', 5, 'ease');
    e.add(at(0));
    e.add(at(1));
    e.add(at(2));
    expect(e.keys.map((k) => k.t)).toEqual([0, 5, 10]);
    expect(e.segments).toHaveLength(2);
    e.setMove(0, 'orbit');
    e.selected = 0;
    const i = e.add(at(9));
    expect(i).toBe(1);
    expect(e.keys.map((k) => k.t)).toEqual([0, 2.5, 5, 10]);
    expect(e.segments.map((s) => s.move)).toEqual(['orbit', 'orbit', 'ease']);
    expect(e.selected).toBe(1);
  });

  it('moves a key only between its neighbours, the first held at 0', () => {
    const e = new KeyframeEditor();
    for (let x = 0; x < 3; x++) e.add(at(x));
    e.move(1, 100);
    expect(e.keys[1]?.t).toBeCloseTo(10 - MIN_GAP_S);
    e.move(1, -3);
    expect(e.keys[1]?.t).toBeCloseTo(MIN_GAP_S);
    e.move(0, 4);
    expect(e.keys[0]?.t).toBe(0);
    e.move(2, 30);
    expect(e.duration).toBe(30);
  });

  it('deletes keys, joining segments, and starts the path at 0 again', () => {
    const e = new KeyframeEditor();
    for (let x = 0; x < 3; x++) e.add(at(x));
    e.setMove(1, 'pan');
    e.remove(0);
    expect(e.keys.map((k) => k.t)).toEqual([0, 5]);
    expect(e.segments.map((s) => s.move)).toEqual(['pan']);
    e.remove(1);
    expect(e.segments).toHaveLength(0);
    e.remove(0);
    expect(e.keys).toHaveLength(0);
    expect(e.selected).toBeNull();
  });

  it('writes a path and reads it back', () => {
    const e = new KeyframeEditor('dusk');
    e.add({ ...at(0), fov: 50, timeOfDay: 0.7 });
    e.add({ ...at(1), speed: 3 });
    e.setMove(0, 'crane');
    const p = e.toPath();
    expect(p).toMatchObject({ formatVersion: 1, name: 'dusk', segments: [{ move: 'crane' }] });
    expect(p.keyframes[0]).toEqual({
      t: 0,
      position: [0, 40, 0],
      look: [0, 0],
      fov: 50,
      timeOfDay: 0.7,
    });
    const f = new KeyframeEditor();
    f.load(JSON.parse(JSON.stringify(p)) as PathJson);
    expect(f.toPath()).toEqual(p);
  });

  it('writes a held up on every key once one holds it, and reads it back (WORK_QUEUE 40)', () => {
    const e = new KeyframeEditor('under');
    e.add(at(0));
    e.add({ ...at(1), up: 'a' });
    e.add(at(2));
    const p = e.toPath();
    expect(p.keyframes.map((k) => k.up)).toEqual(['auto', 'a', 'auto']);
    const f = new KeyframeEditor();
    f.load(JSON.parse(JSON.stringify(p)) as PathJson);
    expect(f.keys.map((k) => k.up)).toEqual([undefined, 'a', undefined]);
    expect(f.toPath()).toEqual(p);
    // Left out, a key carries the one before's, as the engine reads it.
    f.load({
      formatVersion: 1,
      name: 'x',
      keyframes: [{ t: 0, position: [0, -40, 0], up: 'a' }, { t: 2 }],
      segments: [],
    });
    expect(f.keys.map((k) => k.up)).toEqual(['a', 'a']);
    expect(() => {
      f.load({
        formatVersion: 1,
        name: 'x',
        keyframes: [{ t: 0, position: [0, 0, 0], up: 'down' }],
        segments: [],
      });
    }).toThrow(/up must be/);
    // Without one, no key writes it.
    expect(new KeyframeEditor().toPath().keyframes).toEqual([]);
    const plain = new KeyframeEditor();
    plain.add(at(0));
    expect(plain.toPath().keyframes[0]).not.toHaveProperty('up');
  });

  it('reads a hand-written path whose keys carry fields over', () => {
    const e = new KeyframeEditor();
    e.load({
      formatVersion: 1,
      name: 'pan',
      keyframes: [
        { t: 0, position: [1, 2, 3], look: [0, 0] },
        { t: 4, look: [90, 0] },
      ],
      segments: [],
    });
    expect(e.keys[1]?.position).toEqual([1, 2, 3]);
    expect(e.segments).toEqual([{ move: 'ease' }]);
    expect(() => {
      e.load({ formatVersion: 1, name: 'x', keyframes: [{ t: 0 }], segments: [] });
    }).toThrow(/position is required/);
  });
});

function memory(): KeyValue & { map: Map<string, string> } {
  const map = new Map<string, string>();
  return { map, getItem: (k) => map.get(k) ?? null, setItem: (k, v) => map.set(k, v) };
}

describe('the path store', () => {
  it('saves and loads a path, replacing by name', () => {
    const kv = memory();
    const s = new PathStore(kv);
    const e = new KeyframeEditor('a');
    e.add(at(0));
    e.add(at(1));
    s.save(e.toPath(), { seed: '7' }, 1);
    expect(s.get('a')?.path).toEqual(e.toPath());
    expect(s.get('a')?.seed).toBe('7');
    e.add(at(2));
    s.save(e.toPath(), {}, 2);
    expect(s.list()).toHaveLength(1);
    expect(s.get('a')?.path.keyframes).toHaveLength(3);
    s.remove('a');
    expect(s.list()).toHaveLength(0);
  });

  it('keeps unknown fields and records it cannot read when it rewrites (append-only)', () => {
    const kv = memory();
    const future = { name: 'a', savedAt: 1, path: { formatVersion: 1 }, rating: 5 };
    kv.setItem(STORE_KEY, JSON.stringify([future, { weird: true }, 42]));
    const s = new PathStore(kv);
    s.save({ formatVersion: 1, name: 'a', keyframes: [], segments: [] }, {}, 9);
    s.save({ formatVersion: 1, name: 'b', keyframes: [], segments: [] }, {}, 10);
    const raw = JSON.parse(kv.map.get(STORE_KEY) ?? '[]') as Record<string, unknown>[];
    expect(raw).toHaveLength(4);
    expect(raw[0]).toMatchObject({ name: 'a', savedAt: 9, rating: 5 });
    expect(raw[1]).toEqual({ weird: true });
    expect(raw[2]).toBe(42);
    expect(s.list().map((p) => p.name)).toEqual(['b', 'a']);
  });

  it('survives an unreadable store, and exports and imports a path', () => {
    const kv = memory();
    kv.setItem(STORE_KEY, '{not json');
    expect(new PathStore(kv).list()).toEqual([]);
    expect(exportName('volcano at dusk!')).toBe('volcano-at-dusk.camera.json');
    expect(exportName('///')).toBe('shot.camera.json');
    const p = parseImported(
      '{"formatVersion":1,"name":"x","keyframes":[{"t":0,"position":[0,0,0]}]}',
    );
    expect(p.segments).toEqual([]);
    expect(() => parseImported('{"formatVersion":2,"keyframes":[{}]}')).toThrow(/formatVersion 2/);
    expect(() => parseImported('nope')).toThrow(/not JSON/);
  });
});

describe('recording', () => {
  it('sizes the canvas to the aspect at the height, both sides even', () => {
    const tall = THEATER.record.aspects.find((a) => a.name === '9:16');
    const wide = THEATER.record.aspects.find((a) => a.name === '16:9');
    if (!tall || !wide) throw new Error('theater.json lost an aspect');
    expect(recordSize(tall, 1920)).toEqual({ width: 1080, height: 1920 });
    expect(recordSize(tall, 1080)).toEqual({ width: 608, height: 1080 });
    expect(recordSize(wide, 1080)).toEqual({ width: 1920, height: 1080 });
    expect(recordSize(wide, 720)).toEqual({ width: 1280, height: 720 });
    const fit = fitOnScreen({ width: 1080, height: 1920 }, { width: 1600, height: 960 });
    expect(fit.height).toBe(960);
    expect(fit.width).toBe(540);
    expect(fit.left).toBe(530);
  });

  it('records the first type the browser has, named by its container', () => {
    expect(pickMime(THEATER.record.mimeTypes, (t) => t.startsWith('video/webm'))).toBe(
      'video/webm;codecs=vp9',
    );
    expect(pickMime(['video/x'], () => false)).toBe('');
    expect(extensionOf('video/mp4;codecs=avc1')).toBe('mp4');
    expect(extensionOf('video/webm')).toBe('webm');
  });

  it('a take with sound records the tap’s audio track beside the canvas, with an audio codec', () => {
    const video = { getTracks: () => ['v'] };
    const tap = { getAudioTracks: () => ['a'] };
    expect(takeTracks(video, tap)).toEqual(['v', 'a']);
    expect(takeTracks(video, null)).toEqual(['v']);
    expect(pickMime(THEATER.record.audioMimeTypes, (t) => t.startsWith('video/webm'))).toBe(
      'video/webm;codecs=vp9,opus',
    );
    expect(THEATER.record.audioMimeTypes.slice(0, -1).every((t) => /,(opus|mp4a)/.test(t))).toBe(
      true,
    );
  });
});

describe('the free camera and its data', () => {
  it('moves along the held keys at the speed, diagonals no faster', () => {
    expect(flyMove(new Set(['KeyW']), 8, 0.5, false)).toEqual([4, 0, 0]);
    const d = flyMove(new Set(['KeyW', 'KeyD']), 8, 0.5, false);
    expect(Math.hypot(...d)).toBeCloseTo(4);
    expect(flyMove(new Set(['KeyW']), 8, 0.5, true)[0]).toBe(4 * THEATER.fly.runFactor);
    expect(flyMove(new Set(['KeyW', 'KeyS']), 8, 1, false)).toEqual([0, 0, 0]);
    expect(wheelSpeed(8, -1)).toBeCloseTo(8 * THEATER.fly.wheelStep);
    expect(wheelSpeed(8, 1e6)).toBe(THEATER.fly.minSpeedMps);
  });

  it('fails a bad theater.json with a message', () => {
    const good = JSON.parse(
      readFileSync(join(SRC, '..', '..', 'shared', 'theater', 'theater.json'), 'utf8'),
    ) as Record<string, unknown>;
    expect(() => parseTheaterConfig(good)).not.toThrow();
    const bad = structuredClone(good) as { keyframes: { moves: string[] } };
    bad.keyframes.moves.push('teleport');
    expect(() => parseTheaterConfig(bad)).toThrow(/unknown move teleport/);
    const noAspect = structuredClone(good) as { record: { defaultAspect: string } };
    noAspect.record.defaultAspect = '4:3';
    expect(() => parseTheaterConfig(noAspect)).toThrow(/defaultAspect 4:3/);
  });
});
