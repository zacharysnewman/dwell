// Arguments of scripts/shot.ts (a headless in-game screenshot): parsed here, apart from the
// browser, so they are tested.

export interface ShotView {
  /** Feet position (m); null: where the world spawns the player. */
  at: [number, number, number] | null;
  /** View direction (degrees). */
  look: [number, number];
  /** Output PNG (a JSON summary is written beside it). */
  out: string;
}

export interface ShotOptions {
  views: ShotView[];
  seed: number;
  /** Generator: `terrain` (default) or `flat`. */
  world: string;
  /** Resolution scale (`?scale=`): software rendering is bound by pixels. */
  scale: number;
  width: number;
  height: number;
  /** Debug views: stand-ins in magenta, LOD sections tinted by level. */
  standins: boolean;
  lodcolors: boolean;
  /** Debug view: LOD skirts in red. */
  skirts: boolean;
  /** Wait for the view to settle (all loading done) at most this long (s), then shoot anyway. */
  settleSeconds: number;
  /** A served build to use instead of starting `vite preview` on dist/. */
  url: string | null;
}

export const SHOT_USAGE = `Usage: npm run shot -- [options]
  --at x,y,z         feet position (default: the spawn)
  --look yaw,pitch   view direction in degrees (default 0,0)
  --out file.png     output (default shot.png; a .json summary is written beside it)
  --views file.json  several views: [{ "at": [x,y,z], "look": [yaw,pitch], "out": "a.png" }, …]
  --seed N           world seed (default 0)      --world terrain|flat (default terrain)
  --standins         draw LOD stand-ins in magenta
  --lodcolors        tint LOD sections by level
  --skirts           draw LOD skirts in red
  --scale S          resolution scale (default 0.5)   --size WxH (default 960x540)
  --settle S         wait up to S seconds for loading to finish (default 120)
  --url URL          use a served build (default: vite preview of dist/)`;

const triple = (v: string, name: string): [number, number, number] => {
  const n = v.split(',').map((s) => (s.trim() === '' ? NaN : Number(s)));
  if (n.length !== 3 || n.some((x) => !Number.isFinite(x))) throw new Error(`${name}: x,y,z`);
  return [n[0] ?? 0, n[1] ?? 0, n[2] ?? 0];
};
const pair = (v: string, name: string): [number, number] => {
  const n = v.split(',').map((s) => (s.trim() === '' ? NaN : Number(s)));
  if (n.length !== 2 || n.some((x) => !Number.isFinite(x))) throw new Error(`${name}: a,b`);
  return [n[0] ?? 0, n[1] ?? 0];
};
/** A view file's [x, y, z] (or "x,y,z") as the text the flags take. */
const listText = (v: unknown): string =>
  Array.isArray(v) ? v.map(String).join(',') : typeof v === 'string' ? v : '';
const positive = (v: string, name: string): number => {
  const n = Number(v);
  if (!Number.isFinite(n) || n <= 0) throw new Error(`${name}: a positive number`);
  return n;
};

/** Parses the command line; `readViews` reads a --views file (JSON text). Throws on bad input. */
export function parseShotArgs(
  argv: readonly string[],
  readViews: (path: string) => string,
): ShotOptions {
  const o: ShotOptions = {
    views: [],
    seed: 0,
    world: 'terrain',
    scale: 0.5,
    width: 960,
    height: 540,
    standins: false,
    lodcolors: false,
    skirts: false,
    settleSeconds: 120,
    url: null,
  };
  let at: [number, number, number] | null = null;
  let look: [number, number] = [0, 0];
  let out = 'shot.png';
  let viewsFile: string | null = null;
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i] ?? '';
    const value = (): string => {
      if (i + 1 >= argv.length) throw new Error(`${flag} needs a value`);
      return argv[++i] ?? '';
    };
    switch (flag) {
      case '--at':
        at = triple(value(), flag);
        break;
      case '--look':
        look = pair(value(), flag);
        break;
      case '--out':
        out = value();
        break;
      case '--views':
        viewsFile = value();
        break;
      case '--seed':
        o.seed = Number(value());
        if (!Number.isInteger(o.seed) || o.seed < 0) throw new Error('--seed: a whole number');
        break;
      case '--world':
        o.world = value();
        if (o.world !== 'terrain' && o.world !== 'flat') throw new Error('--world: terrain|flat');
        break;
      case '--standins':
        o.standins = true;
        break;
      case '--lodcolors':
        o.lodcolors = true;
        break;
      case '--skirts':
        o.skirts = true;
        break;
      case '--scale':
        o.scale = positive(value(), flag);
        break;
      case '--size': {
        const [w, h] = value().split('x').map(Number);
        if (!w || !h || w <= 0 || h <= 0) throw new Error('--size: WxH');
        o.width = w;
        o.height = h;
        break;
      }
      case '--settle':
        o.settleSeconds = positive(value(), flag);
        break;
      case '--url':
        o.url = value();
        break;
      default:
        throw new Error(`unknown option ${flag}\n${SHOT_USAGE}`);
    }
  }
  if (viewsFile) {
    const list = JSON.parse(readViews(viewsFile)) as unknown;
    if (!Array.isArray(list) || list.length === 0) throw new Error('--views: a non-empty array');
    o.views = list.map((v: { at?: unknown; look?: unknown; out?: unknown }, n) => {
      if (typeof v.out !== 'string') throw new Error(`--views[${String(n)}]: "out" is required`);
      return {
        at:
          v.at === undefined || v.at === null
            ? null
            : triple(listText(v.at), `--views[${String(n)}].at`),
        look: v.look === undefined ? [0, 0] : pair(listText(v.look), `--views[${String(n)}].look`),
        out: v.out,
      };
    });
  } else {
    o.views = [{ at, look, out }];
  }
  return o;
}

/** The page's query string for a run: the world, the debug views and the first view's start. */
export function shotQuery(o: ShotOptions): string {
  const p = new URLSearchParams({ local: '1', world: o.world, seed: String(o.seed) });
  p.set('scale', String(o.scale));
  p.set('fly', '1');
  if (o.standins) p.set('standins', '1');
  if (o.lodcolors) p.set('lodcolors', '1');
  if (o.skirts) p.set('skirts', '1');
  const first = o.views.at(0);
  if (first?.at) p.set('at', first.at.join(','));
  if (first) p.set('look', first.look.join(','));
  return `?${p.toString()}`;
}
