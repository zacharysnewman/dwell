// Settings menu: a button in the top-left corner opens a panel of sliders — the height fog
// (render/fog.ts), the detail settings (lod/detail.ts) and the tone-mapping exposure
// (render/look.ts), ARCHITECTURE.md §5, §6.6. The settings
// are kept in this browser. In a game the panel is also the game menu (Phase 5a): Resume and Quit
// to main menu above the sliders; it opens when the pointer is released (Esc).
import { defaultDetail, detailLimits, sanitizeDetail, type DetailSettings } from '../lod/detail';
import { DEFAULT_FOG, FOG_LIMITS, sanitizeFog, type FogSettings } from '../render/fog';
import { DEFAULT_EXPOSURE, EXPOSURE_LIMITS, sanitizeExposure } from '../render/look';

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
}

export function defaultSettings(mobile: boolean): Settings {
  return { fog: { ...DEFAULT_FOG }, detail: defaultDetail(mobile), exposure: DEFAULT_EXPOSURE };
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
  };
  return JSON.stringify(rounded, null, 2);
}

const FOG_KEY = 'dwell.fog';
const DETAIL_KEY = 'dwell.detail';
const EXPOSURE_KEY = 'dwell.exposure';

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
  };
}

function save(s: Settings): void {
  try {
    localStorage.setItem(FOG_KEY, JSON.stringify(s.fog));
    localStorage.setItem(DETAIL_KEY, JSON.stringify(s.detail));
    localStorage.setItem(EXPOSURE_KEY, JSON.stringify(s.exposure));
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
      ],
    },
  ];
}

export class SettingsMenu {
  private readonly panel: HTMLDivElement;
  private readonly button: HTMLButtonElement;
  private readonly rows: { spec: SliderSpec; input: HTMLInputElement; value: HTMLSpanElement }[] =
    [];
  /** Shows the JSON to copy by hand where the clipboard is unavailable. */
  private readonly fallback = document.createElement('textarea');
  private settings: Settings;

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
    button.textContent = '☰';

    this.panel = document.createElement('div');
    this.panel.id = 'settings-menu';
    this.panel.hidden = true;
    for (const section of sections(mobile)) {
      const title = document.createElement('h2');
      title.textContent = section.title;
      this.panel.append(title);
      for (const spec of section.sliders) this.panel.append(this.slider(spec));
    }
    const reset = document.createElement('button');
    reset.type = 'button';
    reset.className = 'settings-reset';
    reset.textContent = 'Reset';
    reset.addEventListener('click', () => {
      this.set(defaultSettings(this.mobile));
    });
    const copy = document.createElement('button');
    copy.type = 'button';
    copy.className = 'settings-copy';
    copy.textContent = 'Copy JSON';
    copy.addEventListener('click', () => {
      void this.copy(copy);
    });
    const actions = document.createElement('div');
    actions.className = 'settings-actions';
    actions.append(reset, copy);
    this.fallback.className = 'settings-json';
    this.fallback.readOnly = true;
    this.fallback.rows = 10;
    this.fallback.hidden = true;
    this.panel.append(actions, this.fallback);

    button.addEventListener('click', () => {
      this.setOpen(!this.isOpen);
      // Unfocused, so Space (jump) doesn't press it again.
      button.blur();
    });
    parent.append(button, this.panel);
    this.set(this.settings);
  }

  get current(): Settings {
    return this.settings;
  }

  get isOpen(): boolean {
    return !this.panel.hidden;
  }

  setOpen(open: boolean): void {
    this.panel.hidden = !open;
    this.button.setAttribute('aria-expanded', String(open));
  }

  /**
   * Makes the panel the game menu: Resume (closes it, then `onResume`, e.g. to lock the pointer
   * again) and Quit to main menu (`onQuit`, which saves and leaves).
   */
  addGameActions(onResume: () => void, onQuit: () => void): void {
    const resume = document.createElement('button');
    resume.type = 'button';
    resume.id = 'menu-resume';
    resume.textContent = 'Resume';
    resume.addEventListener('click', () => {
      this.setOpen(false);
      onResume();
    });
    const quit = document.createElement('button');
    quit.type = 'button';
    quit.id = 'menu-quit';
    quit.textContent = 'Quit to main menu';
    quit.addEventListener('click', () => {
      quit.disabled = true;
      quit.textContent = 'Saving…';
      onQuit();
    });
    const actions = document.createElement('div');
    actions.className = 'menu-actions';
    actions.append(resume, quit);
    this.panel.prepend(actions);
  }

  /**
   * Adds Host… to the game menu (local worlds, Phase 5c): it shows `panel` (ui/hostPanel.ts) below
   * the actions. Call after addGameActions.
   */
  addHostPanel(panel: HTMLElement): void {
    const actions = this.panel.querySelector('.menu-actions');
    if (!actions) return;
    panel.hidden = true;
    const host = document.createElement('button');
    host.type = 'button';
    host.id = 'menu-host';
    host.textContent = 'Host…';
    host.setAttribute('aria-expanded', 'false');
    host.addEventListener('click', () => {
      panel.hidden = !panel.hidden;
      host.setAttribute('aria-expanded', String(!panel.hidden));
    });
    actions.querySelector('#menu-quit')?.before(host);
    actions.after(panel);
  }

  private slider(spec: SliderSpec): HTMLLabelElement {
    const row = document.createElement('label');
    row.className = 'settings-row';
    const name = document.createElement('span');
    name.className = 'settings-name';
    name.textContent = spec.label;
    const value = document.createElement('span');
    value.className = 'settings-value';
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
      this.apply();
    });
    const hint = document.createElement('span');
    hint.className = 'settings-hint';
    hint.textContent = spec.hint;
    row.append(name, value, input, hint);
    this.rows.push({ spec, input, value });
    return row;
  }

  private set(settings: Settings): void {
    this.settings = settings;
    for (const { spec, input, value } of this.rows) {
      const v = spec.get(settings);
      input.value = String(valueToSlider(v, spec.min, spec.max, spec.log));
      value.textContent = spec.format(v);
    }
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
