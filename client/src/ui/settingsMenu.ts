// Settings menu: a button in the top-left corner opens a panel of sliders. For now it holds the
// height fog (render/fog.ts, ARCHITECTURE.md §6.6); the settings are kept in this browser.
import { DEFAULT_FOG, FOG_LIMITS, sanitizeFog, type FogSettings } from '../render/fog';

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

const STORAGE_KEY = 'dwell.fog';

/** The fog settings kept in this browser, or the defaults (storage can be missing or blocked). */
export function loadFog(): FogSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? sanitizeFog(JSON.parse(raw)) : { ...DEFAULT_FOG };
  } catch {
    return { ...DEFAULT_FOG };
  }
}

function saveFog(fog: FogSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(fog));
  } catch {
    // Not kept: the settings still apply for this visit.
  }
}

interface SliderSpec {
  key: keyof FogSettings;
  label: string;
  hint: string;
  log: boolean;
  format: (v: number) => string;
}

const FOG_SLIDERS: SliderSpec[] = [
  {
    key: 'distanceM',
    label: 'Distance',
    hint: 'How far the haze reaches half strength at sea level',
    log: true,
    format: formatMetres,
  },
  {
    key: 'density',
    label: 'Density',
    hint: 'The most the haze can hide the far distance (0: no fog)',
    log: false,
    format: (v) => `${String(Math.round(v * 100))}%`,
  },
  {
    key: 'heightM',
    label: 'Height',
    hint: 'How high the haze reaches: the air thins above it',
    log: true,
    format: formatMetres,
  },
];

export class SettingsMenu {
  private readonly panel: HTMLDivElement;
  private readonly inputs = new Map<keyof FogSettings, HTMLInputElement>();
  private readonly values = new Map<keyof FogSettings, HTMLSpanElement>();
  private fog: FogSettings;

  constructor(
    parent: HTMLElement,
    private readonly onFog: (fog: FogSettings) => void,
  ) {
    this.fog = loadFog();
    const button = document.createElement('button');
    button.type = 'button';
    button.id = 'menu-button';
    button.setAttribute('aria-label', 'Settings');
    button.setAttribute('aria-expanded', 'false');
    button.setAttribute('aria-controls', 'settings-menu');
    button.textContent = '☰';

    this.panel = document.createElement('div');
    this.panel.id = 'settings-menu';
    this.panel.hidden = true;
    const title = document.createElement('h2');
    title.textContent = 'Fog';
    this.panel.append(title);
    for (const spec of FOG_SLIDERS) this.panel.append(this.slider(spec));
    const reset = document.createElement('button');
    reset.type = 'button';
    reset.className = 'settings-reset';
    reset.textContent = 'Reset';
    reset.addEventListener('click', () => {
      this.set({ ...DEFAULT_FOG });
    });
    this.panel.append(reset);

    button.addEventListener('click', () => {
      this.panel.hidden = !this.panel.hidden;
      button.setAttribute('aria-expanded', String(!this.panel.hidden));
      // Unfocused, so Space (jump) doesn't press it again.
      button.blur();
    });
    parent.append(button, this.panel);
    this.set(this.fog);
  }

  private slider(spec: SliderSpec): HTMLLabelElement {
    const { min, max } = FOG_LIMITS[spec.key];
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
      const v = sliderToValue(Number(input.value), min, max, spec.log);
      this.fog = { ...this.fog, [spec.key]: v };
      value.textContent = spec.format(v);
      this.apply();
    });
    const hint = document.createElement('span');
    hint.className = 'settings-hint';
    hint.textContent = spec.hint;
    row.append(name, value, input, hint);
    this.inputs.set(spec.key, input);
    this.values.set(spec.key, value);
    return row;
  }

  private set(fog: FogSettings): void {
    this.fog = fog;
    for (const spec of FOG_SLIDERS) {
      const { min, max } = FOG_LIMITS[spec.key];
      const input = this.inputs.get(spec.key);
      if (input) input.value = String(valueToSlider(fog[spec.key], min, max, spec.log));
      const value = this.values.get(spec.key);
      if (value) value.textContent = spec.format(fog[spec.key]);
    }
    this.apply();
  }

  private apply(): void {
    this.onFog(this.fog);
    saveFog(this.fog);
  }
}
