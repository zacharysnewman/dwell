// Chisel's Textures tab, the page side (ADR 0074 B6): a block material picked from
// shared/textures/materials.json, its layers as sliders and palette pickers, the palette's colours
// with the art direction's warnings, the tile, a 3 × 3 repeat and the block on a cube painted live by
// the build's painter (textures.ts), and Export (download and copy) of the files' JSON.
import {
  LAYER_KINDS,
  STROKE_KINDS,
  RAMP_STOPS,
  imageRows,
  resolveColor,
  resolveMaterial,
  type Layer,
} from '../../../shared/textures/paint.mjs';
import {
  TILE,
  blockMaterials,
  buildTextures,
  colourRefs,
  exportMaterials,
  exportPalette,
  farColour,
  hex,
  layerFields,
  paintTile,
  paletteWarnings,
  rangeOf,
  tilesOf,
  validateTextures,
  type TextureEdit,
  type TextureView,
} from './textures';

function el(
  tag: string,
  props: Record<string, unknown> = {},
  ...kids: (Node | string)[]
): HTMLElement {
  const e = document.createElement(tag);
  Object.assign(e, props);
  e.append(...kids);
  return e;
}
const button = (text: string, onclick: () => void, title = '') =>
  el('button', { textContent: text, onclick, title });

/** What the tab needs from the page: the edit, the view, and where to report a change. */
export interface TexturesHost {
  edit: () => TextureEdit;
  setEdit: (e: TextureEdit) => void;
  view: () => TextureView;
  setView: (v: TextureView) => void;
  /** After any edit: store it, update the URL, and repaint the 3D section if it is on. */
  changed: () => void;
  download: (name: string, text: string) => void;
}

/** The tab's preview canvases (in the page's preview area). */
export interface TexturePreview {
  tile: HTMLCanvasElement;
  repeat: HTMLCanvasElement;
  cube: HTMLCanvasElement;
  status: HTMLElement;
}

function tileCanvas(tile: Uint8Array): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = TILE;
  c.height = TILE;
  c.getContext('2d')?.putImageData(
    new ImageData(new Uint8ClampedArray(imageRows(tile)), TILE, TILE),
    0,
    0,
  );
  return c;
}

/** Paints the picked material into the preview: the tile, 3 × 3, and a block in three-quarter view. */
export function drawPreview(p: TexturePreview, e: TextureEdit, material: string) {
  const problems = validateTextures(e);
  if (problems.length) {
    p.status.textContent = problems.join('\n');
    p.status.className = 'sub bad';
    return;
  }
  const tile = paintTile(e, material);
  const src = tileCanvas(tile);
  const t = p.tile.getContext('2d');
  if (t) {
    t.imageSmoothingEnabled = false;
    t.clearRect(0, 0, p.tile.width, p.tile.height);
    t.drawImage(src, 0, 0, p.tile.width, p.tile.height);
  }
  const r = p.repeat.getContext('2d');
  if (r) {
    r.imageSmoothingEnabled = false;
    const s = p.repeat.width / 3;
    r.clearRect(0, 0, p.repeat.width, p.repeat.height);
    for (let y = 0; y < 3; y++) for (let x = 0; x < 3; x++) r.drawImage(src, x * s, y * s, s, s);
  }
  const b = p.cube.getContext('2d');
  if (b) {
    b.clearRect(0, 0, p.cube.width, p.cube.height);
    b.imageSmoothingEnabled = false;
    const s = p.cube.width * 0.375;
    const a = s * 0.866;
    const h = s * 0.5;
    const cx = p.cube.width / 2;
    const cy = p.cube.width / 8;
    // Each face the unit square mapped onto the cube, darkened like the game's face tints (only
    // where the tile has texels, so cut-outs stay open). The sides use the same tile: the block's
    // own side tile is a separate material (pick it to see it).
    const face = (m: [number, number, number, number, number, number], shade: number) => {
      b.save();
      b.setTransform(...m);
      b.drawImage(src, 0, 0, 1, 1);
      if (shade < 1) {
        b.globalCompositeOperation = 'source-atop';
        b.fillStyle = `rgba(28, 32, 52, ${(1 - shade).toFixed(2)})`;
        b.fillRect(0, 0, 1, 1);
      }
      b.restore();
    };
    face([a, h, -a, h, cx, cy], 1);
    face([a, h, 0, s, cx - a, cy + h], 0.78);
    face([a, -h, 0, s, cx, cy + 2 * h], 0.62);
  }
  const m = resolveMaterial(e.materials, material);
  const tiles = tilesOf(e.materials, material);
  p.status.className = 'sub';
  p.status.textContent = `${material} · ramp ${String(m.ramp)} · far colour ${hex(farColour(tile))}${tiles.length ? ` · paints ${tiles.join(', ')}` : ''}`;
  document.body.dataset.texture = `${material}:${hex(farColour(tile))}`;
}

function refColour(e: TextureEdit, ref: string, ramp: string | undefined): string {
  try {
    return hex(resolveColor(e.palette, ref, ramp));
  } catch {
    return 'transparent';
  }
}

/** A palette reference picker (one colour), its chip showing the colour. */
function colourPicker(
  e: TextureEdit,
  ramp: string | undefined,
  value: string,
  set: (v: string) => void,
) {
  const sel = el('select') as HTMLSelectElement;
  for (const r of colourRefs(e.palette))
    sel.append(el('option', { value: r, textContent: r, selected: r === value }));
  if (!colourRefs(e.palette).includes(value))
    sel.append(
      el('option', { value, textContent: `${value} (not in the palette)`, selected: true }),
    );
  const chip = el('span', { className: 'chip' });
  chip.style.background = refColour(e, value, ramp);
  sel.onchange = () => {
    set(sel.value);
    chip.style.background = refColour(e, sel.value, ramp);
  };
  return el('span', { className: 'pick' }, chip, sel);
}

/** A slider and number box over a layer's number (or each end of a [min, max] pair). */
function numberRow(
  label: string,
  range: readonly [number, number, number],
  get: () => number,
  set: (v: number) => void,
) {
  const [min, max, step] = range;
  const r = el('input', {
    type: 'range',
    min: String(min),
    max: String(max),
    step: String(step),
    value: String(get()),
  }) as HTMLInputElement;
  const box = el('input', {
    type: 'number',
    step: String(step),
    value: String(get()),
  }) as HTMLInputElement;
  const apply = (v: number) => {
    if (!Number.isFinite(v)) return;
    set(v);
    r.value = String(v);
    box.value = String(v);
  };
  r.oninput = () => {
    apply(Number(r.value));
  };
  box.onchange = () => {
    apply(Number(box.value));
  };
  return el('div', { className: 'row' }, el('span', { textContent: label }), r, box);
}

function layerEditor(
  e: TextureEdit,
  ramp: string | undefined,
  layer: Layer,
  i: number,
  layers: Layer[],
  host: TexturesHost,
  redraw: () => void,
) {
  const box = el('details', { open: i < 3 });
  const kind = el('select') as HTMLSelectElement;
  for (const k of LAYER_KINDS)
    kind.append(el('option', { value: k, textContent: k, selected: k === layer.kind }));
  kind.onchange = () => {
    layer.kind = kind.value;
    host.changed();
    redraw();
  };
  box.append(
    el('summary', {}, `${String(i + 1)}. ${layer.kind}`),
    el(
      'div',
      { className: 'bar' },
      kind,
      button(
        '↑',
        () => {
          if (i > 0) layers.splice(i - 1, 0, ...layers.splice(i, 1));
          host.changed();
          redraw();
        },
        'Move up',
      ),
      button(
        '↓',
        () => {
          if (i < layers.length - 1) layers.splice(i + 1, 0, ...layers.splice(i, 1));
          host.changed();
          redraw();
        },
        'Move down',
      ),
      button(
        '×',
        () => {
          layers.splice(i, 1);
          host.changed();
          redraw();
        },
        'Delete the layer',
      ),
    ),
  );
  // Colours: a stroke's list, or a fill's / gradient's / edge light's own.
  if (STROKE_KINDS.includes(layer.kind)) {
    const colors = Array.isArray(layer.colors) ? (layer.colors as string[]) : [];
    const row = el('div', { className: 'bar' }, el('span', { textContent: 'colours' }));
    colors.forEach((c, j) => {
      row.append(
        colourPicker(e, ramp, c, (v) => {
          colors[j] = v;
          host.changed();
        }),
      );
    });
    row.append(
      button(
        '+',
        () => {
          colors.push('@light');
          layer.colors = colors;
          host.changed();
          redraw();
        },
        'Add a colour',
      ),
    );
    if (colors.length > 1) {
      row.append(
        button(
          '−',
          () => {
            colors.pop();
            host.changed();
            redraw();
          },
          'Remove the last colour',
        ),
      );
    }
    box.append(row);
  } else {
    for (const k of layer.kind === 'gradient' ? ['from', 'to'] : ['color']) {
      box.append(
        el(
          'div',
          { className: 'bar' },
          el('span', { textContent: k }),
          colourPicker(e, ramp, typeof layer[k] === 'string' ? layer[k] : '@base', (v) => {
            layer[k] = v;
            host.changed();
          }),
        ),
      );
    }
  }
  for (const k of layerFields(layer.kind)) {
    const range = rangeOf(layer, k);
    const v = layer[k];
    if (!range) continue;
    if (Array.isArray(v) && v.length === 2 && v.every((x) => typeof x === 'number')) {
      const pair = v as [number, number];
      box.append(
        numberRow(
          `${k} min`,
          range,
          () => pair[0],
          (x) => {
            pair[0] = x;
            host.changed();
          },
        ),
        numberRow(
          `${k} max`,
          range,
          () => pair[1],
          (x) => {
            pair[1] = x;
            host.changed();
          },
        ),
      );
    } else if (typeof v === 'number') {
      box.append(
        numberRow(
          k,
          range,
          () => layer[k] as number,
          (x) => {
            layer[k] = x;
            host.changed();
          },
        ),
      );
    } else {
      // An unset field: a button that sets it at its range's start (or the middle for opacity).
      box.append(
        el(
          'div',
          { className: 'row' },
          el('span', { textContent: k }),
          button('set', () => {
            layer[k] = k === 'opacity' ? 1 : range[0];
            host.changed();
            redraw();
          }),
          el('span'),
        ),
      );
    }
  }
  return box;
}

function paletteEditor(
  e: TextureEdit,
  host: TexturesHost,
  redraw: () => void,
  only: string | undefined,
) {
  const warnings = paletteWarnings(e.palette);
  const box = el('div');
  const flagged = new Map(warnings.map((w) => [w.ref, w.message]));
  const ramps = Object.entries(e.palette.ramps).filter(([n]) => !only || n === only);
  for (const [name, ramp] of ramps) {
    const row = el(
      'div',
      { className: 'bar' },
      el('span', { className: 'ramp-name', textContent: name }),
    );
    for (const s of RAMP_STOPS) {
      const ref = `${name}.${s}`;
      const c = el('input', {
        type: 'color',
        value: ramp[s],
        title: `${ref} ${ramp[s]}${flagged.has(ref) ? ` — ${String(flagged.get(ref))}` : ''}`,
      }) as HTMLInputElement;
      if (flagged.has(ref)) c.className = 'warn';
      c.oninput = () => {
        ramp[s] = c.value;
        host.changed();
      };
      c.onchange = redraw;
      row.append(c);
    }
    box.append(row);
  }
  const list = el('div', { className: 'about' });
  if (warnings.length) {
    list.append(
      `${String(warnings.length)} art-direction warning${warnings.length > 1 ? 's' : ''} (ART_DIRECTION.md §2):`,
    );
    for (const w of warnings)
      list.append(el('div', { className: 'warn-line', textContent: `${w.ref}: ${w.message}` }));
  } else list.append('No art-direction warnings.');
  box.append(list);
  return box;
}

/** Fills `panel` with the tab's editor (re-run after a structural change). */
export function renderTextures(panel: HTMLElement, host: TexturesHost, preview: TexturePreview) {
  const e = host.edit();
  const v = host.view();
  const redraw = () => {
    renderTextures(panel, host, preview);
  };
  const pick = el('select') as HTMLSelectElement;
  for (const n of blockMaterials(e.materials))
    pick.append(el('option', { value: n, textContent: n, selected: n === v.material }));
  pick.onchange = () => {
    host.setView({ ...v, material: pick.value });
    host.changed();
    redraw();
  };
  const m = e.materials.materials[v.material];
  let ramp: string | undefined;
  try {
    ramp = resolveMaterial(e.materials, v.material).ramp;
  } catch {
    ramp = m?.ramp;
  }
  const rampPick = el('select') as HTMLSelectElement;
  for (const r of Object.keys(e.palette.ramps))
    rampPick.append(el('option', { value: r, textContent: r, selected: r === ramp }));
  rampPick.onchange = () => {
    if (m) m.ramp = rampPick.value;
    host.changed();
    redraw();
  };
  panel.append(
    el('h2', { textContent: 'Material' }),
    el('div', { className: 'bar' }, pick, el('span', { textContent: 'ramp' }), rampPick),
    el('p', {
      className: 'about',
      textContent:
        "Painted by the build's painter (shared/textures/paint.mjs): unchanged, the tile is the baked atlas's, texel for texel.",
    }),
  );
  if (m && !m.layers && m.like) {
    panel.append(
      el('p', { className: 'about', textContent: `Its layers come from "${m.like}".` }),
      button('Copy the layers here', () => {
        m.layers = JSON.parse(
          JSON.stringify(resolveMaterial(e.materials, v.material).layers ?? []),
        ) as Layer[];
        host.changed();
        redraw();
      }),
    );
  }
  if (m?.layers) {
    const layers = m.layers;
    panel.append(el('h2', { textContent: 'Layers' }));
    layers.forEach((l, i) => {
      panel.append(layerEditor(e, ramp, l, i, layers, host, redraw));
    });
    const add = el('select') as HTMLSelectElement;
    for (const k of LAYER_KINDS)
      add.append(el('option', { value: k, textContent: k, selected: k === 'dab' }));
    panel.append(
      el(
        'div',
        { className: 'bar' },
        add,
        button('Add layer', () => {
          const k = add.value;
          const base: Layer =
            k === 'fill'
              ? { kind: 'fill', color: '@base' }
              : k === 'gradient'
                ? { kind: 'gradient', from: '@shadow', to: '@base', direction: 'up' }
                : k === 'edgeLight'
                  ? {
                      kind: 'edgeLight',
                      edge: 'top',
                      color: '@light',
                      width: 2,
                      ragged: 1,
                      opacity: 0.6,
                    }
                  : {
                      kind: k,
                      count: 6,
                      colors: ['@light'],
                      width: [3, 5],
                      length: k === 'ring' ? [4, 12] : [8, 12],
                      angle: 0,
                      angleJitter: 20,
                      opacity: 0.5,
                    };
          layers.push(base);
          host.changed();
          redraw();
        }),
      ),
    );
  }
  const all = el('input', { type: 'checkbox' }) as HTMLInputElement;
  const pal = el('div');
  const fillPalette = () => {
    pal.replaceChildren(paletteEditor(e, host, redraw, all.checked ? undefined : ramp));
  };
  all.onchange = fillPalette;
  fillPalette();
  panel.append(
    el('h2', { textContent: 'Palette' }),
    el('label', { className: 'about' }, all, " every ramp (else this material's)"),
    pal,
    el('h2', { textContent: 'Export' }),
    el(
      'div',
      { className: 'bar' },
      button(
        'materials.json',
        () => {
          host.download('materials.json', exportMaterials(e));
        },
        'Download shared/textures/materials.json',
      ),
      button(
        'palette.json',
        () => {
          host.download('palette.json', exportPalette(e));
        },
        'Download shared/textures/palette.json',
      ),
      button(
        'Copy material',
        () => void navigator.clipboard.writeText(`"${v.material}": ${JSON.stringify(m, null, 2)}`),
        "Copy this material's JSON",
      ),
      button(
        'Reset textures',
        () => {
          host.setEdit(buildTextures());
          host.changed();
          redraw();
        },
        "Back to the build's materials and palette",
      ),
    ),
    el('p', {
      className: 'about',
      textContent:
        'Drop the files into shared/textures/ and run node shared/textures/gen.mjs: the bake paints these pixels.',
    }),
  );
  drawPreview(preview, e, v.material);
}
