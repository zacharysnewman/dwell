// The game menu: a button in the top-left corner (or Esc) opens a full-screen menu over the game
// with pages — Settings (the height fog, render/fog.ts; the detail settings, lod/detail.ts; the
// tone-mapping exposure and the sky, render/look.ts; ARCHITECTURE.md §5, §6.6), Controls (every key
// and touch control, ui/controls.ts), Debug (the debug tools as buttons, with their keys) and Host
// (a local world's hosting, ui/hostPanel.ts) — and Resume and Quit to main menu (Phase 5a). The
// settings are kept in this browser.
import { defaultDetail, detailLimits, sanitizeDetail, type DetailSettings } from './detailSettings';
import { DEFAULT_FOG, FOG_LIMITS, sanitizeFog, type FogSettings } from './viewSettings';
import { DEFAULT_EXPOSURE, EXPOSURE_LIMITS, sanitizeExposure } from './viewSettings';
import { CLOUD_QUALITIES, defaultClouds, sanitizeClouds, type CloudQuality } from './viewSettings';
import { DEBUG_KEYS, KEYBOARD, TOUCH, keysLabel, type Control } from './controls';

/** Slider steps: fine enough that a log-scaled slider moves smoothly. */
export const SLIDER_STEPS = 1000;

/** Slider position (0 to SLIDER_STEPS) to a value; `log` spreads the range evenly by ratio. */
export function sliderToValue(step: number, min: number, max: number, log: boolean): number {
  const t = Math.min(1, Math.max(0, step / SLIDER_STEPS));
  return log ? min * Math.pow(max / min, t) : min + (max - min) * t;
}

/** The slider position nearest to a value (the inverse of sliderToValue). */
export function valueToSlider(value: number, min: number, max: number, log: boolean): number {
  const v = Math.min(max, Math.max(min, value));
  const t = log ? Math.log(v / min) / Math.log(max / min) : (v - min) / (max - min);
  return Math.round(t * SLIDER_STEPS);
}

/** Metres for a label: "850 m", "1.5 km", "120 km", "16,384 km". */
export function formatMetres(m: number): string {
  if (m < 1000) return `${String(Math.round(m))} m`;
  const km = m / 1000;
  if (km < 10) return `${km.toFixed(1)} km`;
  return `${Math.round(km).toLocaleString('en-US')} km`;
}

export interface Settings {
  fog: FogSettings;
  detail: DetailSettings;
  /** Tone-mapping exposure (render/look.ts). */
  exposure: number;
  /** Sky rotation in degrees, 0–360 (dwell-look's sky_frame.rs; this browser only). */
  skyRotation: number;
  /** The sky's clouds: off, low or high (dwell-look's clouds.rs). */
  clouds: CloudQuality;
}

export function defaultSettings(mobile: boolean): Settings {
  return {
    fog: { ...DEFAULT_FOG },
    detail: defaultDetail(mobile),
    exposure: DEFAULT_EXPOSURE,
    skyRotation: 0,
    clouds: defaultClouds(mobile),
  };
}

/** The settings as JSON to copy and share (rounded: whole metres, density to 0.01). */
export function settingsJson(s: Settings): string {
  const rounded: Settings = {
    fog: {
      distanceM: Math.round(s.fog.distanceM),
      density: Math.round(s.fog.density * 100) / 100,
      heightM: Math.round(s.fog.heightM),
    },
    detail: {
      distanceM: Math.round(s.detail.distanceM),
      pixelError: Math.round(s.detail.pixelError * 10) / 10,
      memoryMb: Math.round(s.detail.memoryMb),
    },
    exposure: Math.round(s.exposure * 100) / 100,
    skyRotation: Math.round(s.skyRotation),
    clouds: s.clouds,
  };
  return JSON.stringify(rounded, null, 2);
}

const FOG_KEY = 'dwell.fog';
const DETAIL_KEY = 'dwell.detail';
const EXPOSURE_KEY = 'dwell.exposure';
const SKY_ROTATION_KEY = 'dwell.skyRotation';
/** Its own key (the stores are append-only: an older build ignores it, a newer one reads it). */
const CLOUDS_KEY = 'dwell.clouds';

/** A sky rotation in degrees from storage: 0–360, anything else 0. */
export function sanitizeSkyRotation(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(360, Math.max(0, value))
    : 0;
}

function load(key: string): unknown {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as unknown) : null;
  } catch {
    return null; // storage missing or blocked
  }
}

/** The settings kept in this browser, or the defaults. */
export function loadSettings(mobile: boolean): Settings {
  const defaults = defaultSettings(mobile);
  const fog = load(FOG_KEY);
  return {
    fog: fog ? sanitizeFog(fog) : defaults.fog,
    detail: sanitizeDetail(load(DETAIL_KEY), defaults.detail, detailLimits(mobile)),
    exposure: sanitizeExposure(load(EXPOSURE_KEY)),
    skyRotation: sanitizeSkyRotation(load(SKY_ROTATION_KEY)),
    clouds: sanitizeClouds(load(CLOUDS_KEY), mobile),
  };
}

function save(s: Settings): void {
  try {
    localStorage.setItem(FOG_KEY, JSON.stringify(s.fog));
    localStorage.setItem(DETAIL_KEY, JSON.stringify(s.detail));
    localStorage.setItem(EXPOSURE_KEY, JSON.stringify(s.exposure));
    localStorage.setItem(SKY_ROTATION_KEY, JSON.stringify(s.skyRotation));
    localStorage.setItem(CLOUDS_KEY, JSON.stringify(s.clouds));
  } catch {
    // Not kept: the settings still apply for this visit.
  }
}

interface SliderSpec {
  label: string;
  hint: string;
  min: number;
  max: number;
  log: boolean;
  format: (v: number) => string;
  get: (s: Settings) => number;
  with: (s: Settings, v: number) => Settings;
}

/** A setting picked from a few named options (a row of buttons). */
interface ChoiceSpec<T extends string> {
  label: string;
  hint: string;
  options: readonly { value: T; label: string }[];
  get: (s: Settings) => T;
  with: (s: Settings, v: T) => Settings;
}

/** The Look section's choices. */
const CHOICES: ChoiceSpec<CloudQuality>[] = [
  {
    label: 'Clouds',
    hint: 'Clouds in the sky; Low is lighter on the GPU',
    options: CLOUD_QUALITIES.map((q) => ({
      value: q,
      label: q.charAt(0).toUpperCase() + q.slice(1),
    })),
    get: (s) => s.clouds,
    with: (s, v) => ({ ...s, clouds: v }),
  },
];

const withFog = (key: keyof FogSettings) => (s: Settings, v: number) => ({
  ...s,
  fog: { ...s.fog, [key]: v },
});

/** The menu's sliders; the detail ranges depend on the device (lod/detail.ts). */
function sections(mobile: boolean): { title: string; sliders: SliderSpec[] }[] {
  const DETAIL_LIMITS = detailLimits(mobile);
  return [
    {
      title: 'Fog',
      sliders: [
        {
          label: 'Distance',
          hint: 'How far the haze reaches half strength at sea level',
          ...FOG_LIMITS.distanceM,
          log: true,
          format: formatMetres,
          get: (s) => s.fog.distanceM,
          with: withFog('distanceM'),
        },
        {
          label: 'Density',
          hint: 'The most the haze can hide the far distance (0: no fog)',
          ...FOG_LIMITS.density,
          log: false,
          format: (v) => `${String(Math.round(v * 100))}%`,
          get: (s) => s.fog.density,
          with: withFog('density'),
        },
        {
          label: 'Height',
          hint: 'How high the haze reaches: the air thins above it',
          ...FOG_LIMITS.heightM,
          log: true,
          format: formatMetres,
          get: (s) => s.fog.heightM,
          with: withFog('heightM'),
        },
      ],
    },
    {
      title: 'Detail',
      sliders: [
        {
          label: 'Full detail',
          hint: 'How far every block is drawn; farther uses more memory',
          ...DETAIL_LIMITS.distanceM,
          log: false,
          format: formatMetres,
          get: (s) => s.detail.distanceM,
          with: (s, v) => ({ ...s, detail: { ...s.detail, distanceM: v } }),
        },
        {
          label: 'Distant detail',
          hint: 'Largest step in the distant terrain, in pixels; smaller is sharper and slower',
          ...DETAIL_LIMITS.pixelError,
          log: true,
          format: (v) => `${v.toFixed(1)} px`,
          get: (s) => s.detail.pixelError,
          with: (s, v) => ({ ...s, detail: { ...s.detail, pixelError: v } }),
        },
        {
          label: 'Distant memory',
          hint: 'Memory for the distant terrain; past it, distant detail coarsens to fit',
          ...DETAIL_LIMITS.memoryMb,
          log: true,
          format: (v) => `${String(Math.round(v))} MB`,
          get: (s) => s.detail.memoryMb,
          with: (s, v) => ({ ...s, detail: { ...s.detail, memoryMb: v } }),
        },
      ],
    },
    {
      title: 'Look',
      sliders: [
        {
          label: 'Exposure',
          hint: 'How bright the picture is after tone mapping',
          ...EXPOSURE_LIMITS,
          log: true,
          format: (v) => `${v.toFixed(2)}×`,
          get: (s) => s.exposure,
          with: (s, v) => ({ ...s, exposure: v }),
        },
        {
          label: 'Sky rotation',
          hint: 'Turns the sky as a whole, day to night (this browser only)',
          min: 0,
          max: 360,
          log: false,
          format: (v) => `${String(Math.round(v))}°`,
          get: (s) => s.skyRotation,
          with: (s, v) => ({ ...s, skyRotation: v }),
        },
      ],
    },
  ];
}

/** A debug tool as a button in the Debug page (`key` matches an entry of DEBUG_KEYS). */
export interface DebugTool {
  key: string;
  run: () => void;
  /** Whether it is on now (a toggle), for the button's pressed state. */
  on?: () => boolean;
}

type Page = 'settings' | 'controls' | 'debug' | 'host';

const PAGE_TITLES: Record<Page, string> = {
  settings: 'Settings',
  controls: 'Controls',
  debug: 'Debug',
  host: 'Host',
};

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Keys as keycaps: alternatives ("C / Ctrl") with "or" between, `+` joining keys used together. */
function keycaps(keys: string): HTMLElement[] {
  return keys.split(' / ').map((alt, i) => {
    const group = el('span', 'keys');
    if (i > 0) group.append(el('span', 'keys-or', 'or'));
    // A phrase ("Drag on the left", "Left click") reads as words, not a key.
    for (const k of alt.split('+'))
      group.append(el('kbd', k.includes(' ') ? 'phrase' : undefined, k));
    return group;
  });
}

/** One control as a row: what it does, its keys, and when it applies. */
function controlRow(c: Control): HTMLElement {
  const row = el('div', 'control-row');
  const what = el('span', 'control-action', c.action);
  const keys = el('span', 'control-keys');
  keys.append(...keycaps(keysLabel(c)));
  row.append(what, keys);
  if (c.when) row.append(el('span', 'control-when', c.when));
  return row;
}

export class SettingsMenu {
  /** The whole menu (full screen when open). */
  private readonly panel: HTMLDivElement;
  private readonly button: HTMLButtonElement;
  private readonly subtitle = el('p', 'gm-sub');
  private readonly tabs = el('nav', 'gm-tabs');
  private readonly pagesBox = el('div', 'gm-pages');
  private readonly footer = el('footer', 'gm-foot');
  private readonly pages = new Map<Page, { tab: HTMLButtonElement; page: HTMLElement }>();
  private readonly debugButtons: { tool: DebugTool; button: HTMLButtonElement }[] = [];
  private readonly rows: { spec: SliderSpec; input: HTMLInputElement; value: HTMLSpanElement }[] =
    [];
  private readonly choices: { spec: ChoiceSpec<CloudQuality>; buttons: HTMLButtonElement[] }[] = [];
  /** Shows the JSON to copy by hand where the clipboard is unavailable. */
  private readonly fallback = document.createElement('textarea');
  private settings: Settings;
  /** Told when the menu opens or closes (the touch controls step aside while it is open). */
  onOpenChange: ((open: boolean) => void) | null = null;

  constructor(
    parent: HTMLElement,
    private readonly mobile: boolean,
    private readonly onChange: (settings: Settings) => void,
  ) {
    this.settings = loadSettings(mobile);
    const button = document.createElement('button');
    this.button = button;
    button.type = 'button';
    button.id = 'menu-button';
    button.setAttribute('aria-label', 'Menu');
    button.setAttribute('aria-expanded', 'false');
    button.setAttribute('aria-controls', 'settings-menu');
    button.title = 'Menu and settings (Esc)';
    button.append(el('span', 'menu-glyph'));

    this.panel = document.createElement('div');
    this.panel.id = 'settings-menu';
    this.panel.className = 'game-menu';
    this.panel.hidden = true;
    this.panel.setAttribute('role', 'dialog');
    this.panel.setAttribute('aria-modal', 'true');
    this.panel.setAttribute('aria-label', 'Game menu');

    const sheet = el('div', 'gm-sheet');
    const head = el('header', 'gm-head');
    const titles = el('div', 'gm-titles');
    titles.append(el('h1', 'gm-title', 'Paused'), this.subtitle);
    const close = el('button', 'gm-close');
    close.type = 'button';
    close.setAttribute('aria-label', 'Close menu');
    close.title = 'Close (Esc)';
    close.addEventListener('click', () => {
      this.setOpen(false);
    });
    head.append(titles, close);
    this.tabs.setAttribute('role', 'tablist');
    sheet.append(head, this.tabs, this.pagesBox, this.footer);
    this.panel.append(sheet);
    // A click on the dimmed game around the sheet closes the menu.
    this.panel.addEventListener('click', (e) => {
      if (e.target === this.panel) this.setOpen(false);
    });

    this.addPage('settings', this.settingsPage());
    this.addPage('controls', this.controlsPage());

    button.addEventListener('click', () => {
      this.setOpen(!this.isOpen);
      // Unfocused, so Space (jump) doesn't press it again.
      button.blur();
    });
    parent.append(button, this.panel);
    this.set(this.settings);
    this.show('settings');
  }

  get current(): Settings {
    return this.settings;
  }

  get isOpen(): boolean {
    return !this.panel.hidden;
  }

  setOpen(open: boolean): void {
    if (open === this.isOpen) return;
    this.panel.hidden = !open;
    this.button.setAttribute('aria-expanded', String(open));
    document.body.classList.toggle('menu-open', open);
    if (open) this.refreshDebug();
    this.onOpenChange?.(open);
  }

  /** The world or server being played, under the menu's title. */
  setSubtitle(text: string): void {
    this.subtitle.textContent = text;
  }

  /**
   * Makes the panel the game menu: Resume (closes it, then `onResume`, e.g. to lock the pointer
   * again) and Quit to main menu (`onQuit`, which saves and leaves).
   */
  addGameActions(onResume: () => void, onQuit: () => void): void {
    const resume = el('button', 'block-button', 'Resume');
    resume.type = 'button';
    resume.id = 'menu-resume';
    resume.addEventListener('click', () => {
      this.setOpen(false);
      onResume();
    });
    const quit = el('button', 'quiet-button', 'Quit to main menu');
    quit.type = 'button';
    quit.id = 'menu-quit';
    quit.addEventListener('click', () => {
      quit.disabled = true;
      quit.textContent = 'Saving…';
      onQuit();
    });
    const actions = el('div', 'menu-actions');
    actions.append(quit, resume);
    this.footer.prepend(actions);
  }

  /** Adds the Host page (local worlds, Phase 5c): `panel` is ui/hostPanel.ts's. */
  addHostPanel(panel: HTMLElement): void {
    panel.hidden = false;
    const page = el('section');
    page.append(
      el(
        'p',
        'gm-lead',
        'Open this world to friends: they join with the code or link while you play.',
      ),
      panel,
    );
    this.addPage('host', page);
    this.pages.get('host')?.tab.setAttribute('id', 'menu-host');
  }

  /** Adds the Debug page: each tool a button with its key (DEBUG_KEYS). */
  addDebugTools(tools: DebugTool[]): void {
    const page = el('section');
    page.append(
      el('p', 'gm-lead', 'Tools for looking under the hood. Each also has a key while you play.'),
    );
    const grid = el('div', 'debug-tools');
    for (const tool of tools) {
      const c = DEBUG_KEYS.find((d) => d.keys.includes(tool.key));
      const b = el('button', 'debug-tool');
      b.type = 'button';
      b.append(el('span', 'debug-tool-name', c?.action ?? tool.key), ...keycaps(tool.key));
      if (c?.when) b.append(el('span', 'debug-tool-when', c.when));
      b.addEventListener('click', () => {
        tool.run();
        this.refreshDebug();
      });
      grid.append(b);
      this.debugButtons.push({ tool, button: b });
    }
    const zoom = DEBUG_KEYS.find((d) => d.action === 'Map zoom');
    page.append(grid);
    if (zoom) {
      const list = el('div', 'control-list');
      list.append(controlRow(zoom));
      page.append(list);
    }
    this.addPage('debug', page);
  }

  /** A line at the foot of the menu (the build). */
  setFootnote(text: string): void {
    let note = this.footer.querySelector<HTMLElement>('.gm-note');
    if (!note) {
      note = el('p', 'gm-note');
      this.footer.append(note);
    }
    note.textContent = text;
  }

  private refreshDebug(): void {
    for (const { tool, button } of this.debugButtons) {
      if (tool.on) button.setAttribute('aria-pressed', String(tool.on()));
    }
  }

  private addPage(name: Page, page: HTMLElement): void {
    const order: Page[] = ['settings', 'controls', 'debug', 'host'];
    const tab = el('button', 'gm-tab', PAGE_TITLES[name]);
    tab.type = 'button';
    tab.setAttribute('role', 'tab');
    tab.dataset.page = name;
    tab.addEventListener('click', () => {
      this.show(name);
    });
    page.classList.add('gm-page');
    page.setAttribute('role', 'tabpanel');
    page.dataset.page = name;
    // Pages keep their order whatever order they are added in.
    const next = order.slice(order.indexOf(name) + 1).find((p) => this.pages.has(p));
    const after = next ? this.pages.get(next) : undefined;
    if (after) {
      after.tab.before(tab);
      after.page.before(page);
    } else {
      this.tabs.append(tab);
      this.pagesBox.append(page);
    }
    this.pages.set(name, { tab, page });
    const shown = [...this.pages.values()].some((p) => !p.page.hidden && p.page !== page);
    page.hidden = shown;
    tab.setAttribute('aria-selected', String(!shown));
  }

  private show(name: Page): void {
    for (const [n, { tab, page }] of this.pages) {
      page.hidden = n !== name;
      tab.setAttribute('aria-selected', String(n === name));
    }
    this.pagesBox.scrollTop = 0;
  }

  private settingsPage(): HTMLElement {
    const page = el('section');
    for (const section of sections(this.mobile)) {
      const group = el('div', 'gm-group');
      group.append(el('h2', undefined, section.title));
      const grid = el('div', 'gm-grid');
      for (const spec of section.sliders) grid.append(this.slider(spec));
      if (section.title === 'Look') for (const spec of CHOICES) grid.append(this.choice(spec));
      group.append(grid);
      page.append(group);
    }
    const reset = el('button', 'quiet-button settings-reset', 'Reset');
    reset.type = 'button';
    reset.title = 'Back to the defaults';
    reset.addEventListener('click', () => {
      this.set(defaultSettings(this.mobile));
    });
    const copy = el('button', 'quiet-button settings-copy', 'Copy JSON');
    copy.type = 'button';
    copy.title = 'Copy these settings, to share them';
    copy.addEventListener('click', () => {
      void this.copy(copy);
    });
    const actions = el('div', 'settings-actions');
    actions.append(reset, copy);
    this.fallback.className = 'settings-json';
    this.fallback.readOnly = true;
    this.fallback.rows = 10;
    this.fallback.hidden = true;
    page.append(actions, this.fallback);
    return page;
  }

  private controlsPage(): HTMLElement {
    const page = el('section');
    const groups: { title: string; controls: Control[] }[] = this.mobile
      ? [{ title: 'Touch', controls: TOUCH }, ...KEYBOARD]
      : [...KEYBOARD, { title: 'Touch screens', controls: TOUCH }];
    groups.push({ title: 'Debug', controls: DEBUG_KEYS });
    for (const g of groups) {
      const group = el('div', 'gm-group');
      group.append(el('h2', undefined, g.title));
      const list = el('div', 'control-list');
      for (const c of g.controls) list.append(controlRow(c));
      group.append(list);
      page.append(group);
    }
    return page;
  }

  private slider(spec: SliderSpec): HTMLLabelElement {
    const row = document.createElement('label');
    row.className = 'settings-row';
    const name = el('span', 'settings-name', spec.label);
    const value = el('span', 'settings-value');
    const input = document.createElement('input');
    input.type = 'range';
    input.min = '0';
    input.max = String(SLIDER_STEPS);
    input.step = '1';
    input.title = spec.hint;
    input.addEventListener('input', () => {
      const v = sliderToValue(Number(input.value), spec.min, spec.max, spec.log);
      this.settings = spec.with(this.settings, v);
      value.textContent = spec.format(v);
      this.paint(input);
      this.apply();
    });
    const hint = el('span', 'settings-hint', spec.hint);
    row.append(name, value, input, hint);
    this.rows.push({ spec, input, value });
    return row;
  }

  private choice(spec: ChoiceSpec<CloudQuality>): HTMLDivElement {
    const row = el('div', 'settings-row settings-choice');
    const name = el('span', 'settings-name', spec.label);
    const group = el('div', 'settings-options');
    group.setAttribute('role', 'radiogroup');
    group.setAttribute('aria-label', spec.label);
    const buttons = spec.options.map((o) => {
      const b = el('button', 'settings-option', o.label);
      b.type = 'button';
      b.dataset.value = o.value;
      b.setAttribute('role', 'radio');
      b.addEventListener('click', () => {
        this.settings = spec.with(this.settings, o.value);
        this.mark(buttons, o.value);
        this.apply();
      });
      return b;
    });
    group.append(...buttons);
    row.append(name, group, el('span', 'settings-hint', spec.hint));
    this.choices.push({ spec, buttons });
    return row;
  }

  private mark(buttons: HTMLButtonElement[], value: string): void {
    for (const b of buttons) b.setAttribute('aria-checked', String(b.dataset.value === value));
  }

  /** The slider's filled part, as a CSS variable the track's gradient reads. */
  private paint(input: HTMLInputElement): void {
    input.style.setProperty('--fill', `${String((100 * Number(input.value)) / SLIDER_STEPS)}%`);
  }

  private set(settings: Settings): void {
    this.settings = settings;
    for (const { spec, input, value } of this.rows) {
      const v = spec.get(settings);
      input.value = String(valueToSlider(v, spec.min, spec.max, spec.log));
      value.textContent = spec.format(v);
      this.paint(input);
    }
    for (const { spec, buttons } of this.choices) this.mark(buttons, spec.get(settings));
    this.apply();
  }

  private async copy(button: HTMLButtonElement): Promise<void> {
    const json = settingsJson(this.settings);
    try {
      await navigator.clipboard.writeText(json);
      this.fallback.hidden = true;
      button.textContent = 'Copied';
    } catch {
      // No clipboard (an insecure page, or permission denied): select the JSON to copy by hand.
      this.fallback.value = json;
      this.fallback.hidden = false;
      this.fallback.select();
      button.textContent = 'Select and copy';
    }
    setTimeout(() => {
      button.textContent = 'Copy JSON';
    }, 1500);
  }

  private apply(): void {
    if (!this.fallback.hidden) this.fallback.value = settingsJson(this.settings);
    this.onChange(this.settings);
    save(this.settings);
  }
}
