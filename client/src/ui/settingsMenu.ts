// The game menu: a button in the top-left corner (or Esc) opens a full-screen menu over the game
// with pages, listed down the side (a row of tabs on phones) — the settings in four panes (owner,
// 2026-11-07: "arranged concisely and mostly vertically", sound, graphics, etc. in their own panes):
// Graphics (the Preset slider, ui/graphicsPresets.ts and shared/settings/graphics-presets.json; the
// render scale, water, shadows, shafts, clouds and detail it sets; the exposure and the water's
// flow), Sound (the volumes, audio/settings.ts), Gameplay (the view, motion and explosions) and On
// screen (shared/ui/hud.json), one compact row a setting with Reset and Copy JSON under them
// (ARCHITECTURE.md §5, §6.6); Controller (ui/controllerPanel.ts), Controls (every key
// and touch control, ui/controls.ts), Debug (the debug tools as buttons, with their keys) and Host
// (a local world's hosting, ui/hostPanel.ts) — and Resume and Quit to main menu (Phase 5a). The
// settings are kept in this browser.
import { defaultHud, hideableParts, hudRecord, sanitizeHud, type HudSettings } from './hudSettings';
import { defaultDetail, detailLimits, sanitizeDetail, type DetailSettings } from './detailSettings';
import {
  RENDER_SCALE_KEY,
  UPSCALE,
  defaultRenderScale,
  renderScaleRecord,
  sanitizeRenderScale,
  type RenderScaleSettings,
} from './renderScaleSettings';
import { DEFAULT_EXPOSURE, EXPOSURE_LIMITS, sanitizeExposure } from './viewSettings';
import {
  CLOUD_QUALITIES,
  DEFAULT_SHAFTS,
  SHAFT_QUALITIES,
  defaultClouds,
  sanitizeShafts,
  type CloudQuality,
  type ShaftQuality,
} from './viewSettings';
import {
  DEFAULT_FOV,
  FOV_LIMITS,
  VIEW_KEY,
  sanitizeFovSetting,
  sanitizeShowArms,
  viewRecord,
} from './viewSettings';
import {
  GRAPHICS_PRESETS,
  applyPreset,
  defaultPreset,
  findPreset,
  type GraphicsPreset,
  type PresetData,
} from './graphicsPresets';
import {
  CLOUDS_KEY,
  DETAIL_KEY,
  EXPOSURE_KEY,
  HUD_KEY,
  PRESET_KEY,
  SHADOWS_KEY,
  SHAFTS_KEY,
  WATER_KEY,
  pinnedRecord,
  pinsIn,
  presetRecord,
  resolveSettings,
  settingFields,
  storedPresetId,
  type SettingField,
} from './settingsPins';
import { DEBUG_KEYS, KEYBOARD, TOUCH, keysLabel, type Control } from './controls';
import { audio } from '../audio/engine';
import { DEFAULT_VOLUMES, type Volumes } from '../audio/mixer';
import { AUDIO_KEY, loadVolumes } from '../audio/settings';
import {
  defaultWater,
  sanitizeWater,
  WATER_QUALITIES,
  WATER_QUALITY_LABELS,
  waterRecord,
  type WaterQuality,
  type WaterSettings,
} from './waterSettings';
import {
  defaultShadows,
  sanitizeShadows,
  SHADOW_QUALITIES,
  SHADOW_QUALITY_LABELS,
  shadowRecord,
  type ShadowQuality,
  type ShadowSettings,
} from './shadowSettings';
import {
  MOTION_KEY,
  defaultMotion,
  motionRecord,
  prefersReducedMotion,
  sanitizeMotion,
  type MotionSettings,
} from './motionSettings';
import {
  EXPLOSIONS_KEY,
  defaultExplosions,
  explosionRecord,
  sanitizeExplosions,
  type ExplosionSettings,
} from './explosionSettings';

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
  detail: DetailSettings;
  /** Tone-mapping exposure (dwell-look; the time of day's exposure multiplies it). */
  exposure: number;
  /** Sky rotation in degrees, 0–360 (dwell-look's sky_frame.rs; this browser only). */
  /** Master, effects, interface and music volumes, 0–1 (audio/settings.ts; this browser only). */
  audio: Volumes;
  /** The sky's clouds: off, low or high (dwell-look's clouds.rs). */
  clouds: CloudQuality;
  /** The light shafts: off, low or high (dwell-look's shafts.rs, ADR 0064). */
  shafts: ShaftQuality;
  /** The water quality (ui/waterSettings.ts). */
  water: WaterSettings;
  /** The sun's and moon's shadows (ui/shadowSettings.ts). */
  shadows: ShadowSettings;
  /** Speed effects: the FOV boost and the wind (docs/MOTION_EFFECTS.md). */
  motion: MotionSettings;
  /** A blast's flash, smoke and dust, and screen shake (ui/explosionSettings.ts). */
  explosions: ExplosionSettings;
  /** The vertical field of view in degrees (60–100; narrower on wide screens, dwell-lod's base_fov). */
  fovDeg: number;
  /** Your first-person arms and what they hold (docs/CHARACTERS.md §5). */
  showArms: boolean;
  /** What the on-screen interface shows (ui/hudSettings.ts, shared/ui/hud.json). */
  hud: HudSettings;
  /** Render scale and its upscaler (ui/renderScaleSettings.ts, shared/render/upscale.json). */
  renderScale: RenderScaleSettings;
}

/**
 * The defaults: the graphics are the device's default preset's (ui/graphicsPresets.ts: Mobile on
 * phones, High on desktops), over each setting's own default.
 */
export function defaultSettings(mobile: boolean, reducedMotion = false): Settings {
  return applyPreset(baseSettings(mobile, reducedMotion), defaultPreset(mobile), mobile);
}

/** Each setting's own default, before the device's preset is written over the graphics. */
export function baseSettings(mobile: boolean, reducedMotion = false): Settings {
  return {
    detail: defaultDetail(mobile),
    exposure: DEFAULT_EXPOSURE,
    audio: { ...DEFAULT_VOLUMES },
    clouds: defaultClouds(mobile),
    shafts: DEFAULT_SHAFTS,
    water: defaultWater(mobile),
    shadows: defaultShadows(mobile),
    motion: defaultMotion(reducedMotion),
    explosions: defaultExplosions(reducedMotion),
    fovDeg: DEFAULT_FOV,
    showArms: true,
    hud: defaultHud(),
    renderScale: defaultRenderScale(),
  };
}

/** The settings as JSON to copy and share (rounded: whole metres, fractions to 0.01). */
export function settingsJson(s: Settings): string {
  const rounded: Settings = {
    detail: {
      distanceM: Math.round(s.detail.distanceM),
      pixelError: Math.round(s.detail.pixelError * 10) / 10,
      memoryMb: Math.round(s.detail.memoryMb),
    },
    exposure: Math.round(s.exposure * 100) / 100,
    audio: {
      master: Math.round(s.audio.master * 100) / 100,
      effects: Math.round(s.audio.effects * 100) / 100,
      ui: Math.round(s.audio.ui * 100) / 100,
      music: Math.round(s.audio.music * 100) / 100,
    },
    clouds: s.clouds,
    shafts: s.shafts,
    water: {
      quality: s.water.quality,
      flowRipples: s.water.flowRipples,
      flowFoam: s.water.flowFoam,
      flowParticles: s.water.flowParticles,
      waterfalls: s.water.waterfalls,
    },
    shadows: { quality: s.shadows.quality, torch: s.shadows.torch },
    motion: {
      speedEffects: s.motion.speedEffects,
      fovBoost: Math.round(s.motion.fovBoost * 100) / 100,
      plantSway: s.motion.plantSway,
    },
    explosions: { ...s.explosions },
    fovDeg: Math.round(s.fovDeg),
    showArms: s.showArms,
    hud: { shown: { ...s.hud.shown } },
    renderScale: { ...s.renderScale },
  };
  return JSON.stringify(rounded, null, 2);
}

// `dwell.fog` (the fog sliders, before the fog was fixed look data: shared/look/fog.json) is no
// longer read or written. Builds before it still read it; it is left as it is.
// `dwell.skyRotation` (the sky rotation slider before the day/night cycle) is no longer read or
// written: the time of day is the world's. Builds before it still read it; it is left as it is.
// The keys themselves: ui/settingsPins.ts, with the settings each holds.

/** Every stored setting (ui/settingsPins.ts): the pins' vocabulary. */
export const SETTING_FIELDS: readonly SettingField[] = settingFields({
  motion: MOTION_KEY,
  explosions: EXPLOSIONS_KEY,
  view: VIEW_KEY,
  renderScale: RENDER_SCALE_KEY,
  audio: AUDIO_KEY,
});

/** The graphics settings a preset sets: changing one by hand pins it and reads Custom. */
export const PRESET_PINS: ReadonlySet<string> = new Set(
  SETTING_FIELDS.filter((f) => f.preset).map((f) => f.id),
);

/** Every setting pinned: what builds before 0.50.31 stored (`saveSettings`'s default). */
export const ALL_PINS: ReadonlySet<string> = new Set(SETTING_FIELDS.map((f) => f.id));

/**
 * The settings with which of them the player pinned (ui/settingsPins.ts): a pinned one is stored
 * and kept; any other follows this build's default for the device, the graphics ones the preset
 * chosen (`preset`, its id) or, with none, the device's default preset.
 */
export interface SettingsState {
  settings: Settings;
  pins: ReadonlySet<string>;
  preset: string | null;
}

/** Where the defaults come from: the presets data and each setting's own default (tests swap them). */
export interface DefaultsSource {
  presets?: PresetData;
  /** Each setting's own default, before a preset is applied (`baseSettings`). */
  base?: (mobile: boolean, reducedMotion: boolean) => Settings;
}

/**
 * The defaults the unpinned settings follow: the preset chosen (if the data still lists it) or the
 * device's default preset, over each setting's own default.
 */
export function settingsDefaults(
  mobile: boolean,
  preset: string | null,
  reducedMotion = false,
  src: DefaultsSource = {},
): Settings {
  const d = src.presets ?? GRAPHICS_PRESETS;
  const p = (preset === null ? undefined : findPreset(preset, d)) ?? defaultPreset(mobile, d);
  return applyPreset((src.base ?? baseSettings)(mobile, reducedMotion), p, mobile);
}

function load(key: string): unknown {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as unknown) : null;
  } catch {
    return null; // storage missing or blocked
  }
}

/**
 * The stored settings over `defaults`, each record read by its setting's own sanitizer: anything
 * missing or unknown takes the default. The read path builds before 0.50.31 used as their whole
 * `loadSettings`, so it is also how an older build reads this one's pinned-only records.
 */
export function storedSettings(mobile: boolean, defaults: Settings): Settings {
  return {
    detail: sanitizeDetail(load(DETAIL_KEY), defaults.detail, detailLimits(mobile)),
    exposure: sanitizeExposure(load(EXPOSURE_KEY)),
    audio: loadVolumes(),
    clouds: CLOUD_QUALITIES.find((q) => q === load(CLOUDS_KEY)) ?? defaults.clouds,
    shafts: sanitizeShafts(load(SHAFTS_KEY), defaults.shafts),
    water: sanitizeWater(load(WATER_KEY), defaults.water),
    shadows: sanitizeShadows(load(SHADOWS_KEY), defaults.shadows),
    motion: sanitizeMotion(load(MOTION_KEY), prefersReducedMotion()),
    explosions: sanitizeExplosions(load(EXPLOSIONS_KEY), defaultExplosions(prefersReducedMotion())),
    fovDeg: sanitizeFovSetting(load(VIEW_KEY)),
    showArms: sanitizeShowArms(load(VIEW_KEY)),
    hud: sanitizeHud(load(HUD_KEY)),
    renderScale: sanitizeRenderScale(load(RENDER_SCALE_KEY), UPSCALE, defaults.renderScale),
  };
}

/** The settings kept in this browser, which of them are pinned, and the preset chosen. */
export function loadSettingsState(mobile: boolean, src: DefaultsSource = {}): SettingsState {
  const id = storedPresetId(load(PRESET_KEY));
  // A preset the data no longer lists reads as none: the device's default.
  const preset = id !== null && findPreset(id, src.presets ?? GRAPHICS_PRESETS) ? id : null;
  const defaults = settingsDefaults(mobile, preset, prefersReducedMotion(), src);
  const pins = pinsIn(SETTING_FIELDS, load);
  const settings = resolveSettings(
    SETTING_FIELDS,
    defaults,
    storedSettings(mobile, defaults),
    pins,
  );
  return { settings, pins, preset };
}

/** The settings kept in this browser, or the defaults. */
export function loadSettings(mobile: boolean): Settings {
  return loadSettingsState(mobile).settings;
}

/**
 * Stores the pinned settings and the preset chosen: each key's record keeps the fields it doesn't
 * know (append-only) and holds the pinned fields; known fields that follow a default are taken
 * out, and a key left with nothing is removed.
 */
export function saveSettingsState(state: SettingsState): void {
  writeSettings(state.settings, state.pins);
  try {
    const rec = presetRecord(load(PRESET_KEY), state.preset);
    if (rec) localStorage.setItem(PRESET_KEY, JSON.stringify(rec));
    else localStorage.removeItem(PRESET_KEY);
  } catch {
    // Not kept: the settings still apply for this visit.
  }
}

/** Stores the settings, by default every one pinned (as builds before 0.50.31 did); no preset. */
export function saveSettings(s: Settings, pins: ReadonlySet<string> = ALL_PINS): void {
  writeSettings(s, pins);
}

function writeSettings(s: Settings, pins: ReadonlySet<string>): void {
  // The whole record each key would hold, the stored one's unknown fields kept (append-only).
  const full: [string, unknown][] = [
    [DETAIL_KEY, { ...asRecord(load(DETAIL_KEY)), ...s.detail }],
    [EXPOSURE_KEY, s.exposure],
    [AUDIO_KEY, { ...asRecord(load(AUDIO_KEY)), ...s.audio }],
    [CLOUDS_KEY, s.clouds],
    [SHAFTS_KEY, s.shafts],
    [WATER_KEY, waterRecord(load(WATER_KEY), s.water)],
    [SHADOWS_KEY, shadowRecord(load(SHADOWS_KEY), s.shadows)],
    [MOTION_KEY, motionRecord(load(MOTION_KEY), s.motion)],
    [VIEW_KEY, viewRecord(load(VIEW_KEY), s.fovDeg, s.showArms)],
    [HUD_KEY, hudRecord(load(HUD_KEY), s.hud)],
    [RENDER_SCALE_KEY, renderScaleRecord(load(RENDER_SCALE_KEY), s.renderScale)],
    [EXPLOSIONS_KEY, explosionRecord(load(EXPLOSIONS_KEY), s.explosions)],
  ];
  for (const [key, value] of full) {
    try {
      const kept = pinnedRecord(SETTING_FIELDS, key, value, pins);
      if (kept === undefined) localStorage.removeItem(key);
      else localStorage.setItem(key, JSON.stringify(kept));
    } catch {
      // Not kept: the settings still apply for this visit.
    }
  }
}

// What the menu does with the state, as plain functions (tested; the DOM part is thin).

/** A control changed: its setting takes the new value and is pinned (even if that is the default). */
export function changeSetting(state: SettingsState, pin: string, next: Settings): SettingsState {
  return { ...state, settings: next, pins: new Set([...state.pins, pin]) };
}

/** A pin's marker pressed: the setting follows its default again (the preset's, if it sets it). */
export function clearPin(
  state: SettingsState,
  pin: string,
  mobile: boolean,
  src: DefaultsSource = {},
): SettingsState {
  const pins = new Set(state.pins);
  pins.delete(pin);
  return withDefaults({ ...state, pins }, mobile, src);
}

/**
 * A preset chosen: its id is stored, and every setting it sets follows it (their pins cleared, so a
 * later retune of the preset reaches them). Settings outside the presets keep their pins.
 */
export function choosePreset(
  state: SettingsState,
  id: string,
  mobile: boolean,
  src: DefaultsSource = {},
): SettingsState {
  const pins = new Set([...state.pins].filter((p) => !PRESET_PINS.has(p)));
  return withDefaults({ ...state, pins, preset: id }, mobile, src);
}

/** The Preset row's marker: back to the device's default preset, no graphics setting pinned. */
export function clearPreset(
  state: SettingsState,
  mobile: boolean,
  src: DefaultsSource = {},
): SettingsState {
  const pins = new Set([...state.pins].filter((p) => !PRESET_PINS.has(p)));
  return withDefaults({ ...state, pins, preset: null }, mobile, src);
}

/** Reset: nothing pinned and no preset chosen, so everything follows this build's defaults. */
export function resetSettings(mobile: boolean, src: DefaultsSource = {}): SettingsState {
  return withDefaults(
    { settings: baseSettings(mobile), pins: new Set(), preset: null },
    mobile,
    src,
  );
}

/**
 * The preset the Preset row shows: the one chosen (or the device's default) while no setting it
 * sets is pinned; null (Custom) once one is. With one pinned the preset stays the base the other
 * graphics settings follow, so the slider stays on it.
 */
export function shownPreset(
  state: SettingsState,
  mobile: boolean,
  d: PresetData = GRAPHICS_PRESETS,
): { base: GraphicsPreset; custom: boolean } {
  const base =
    (state.preset === null ? undefined : findPreset(state.preset, d)) ?? defaultPreset(mobile, d);
  return { base, custom: [...state.pins].some((p) => PRESET_PINS.has(p)) };
}

/** The unpinned settings set again from the defaults (the preset's, then each setting's own). */
function withDefaults(state: SettingsState, mobile: boolean, src: DefaultsSource): SettingsState {
  const defaults = settingsDefaults(mobile, state.preset, prefersReducedMotion(), src);
  return {
    ...state,
    settings: resolveSettings(SETTING_FIELDS, defaults, state.settings, state.pins),
  };
}

function asRecord(v: unknown): Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : {};
}

interface SliderSpec {
  label: string;
  /** The setting it changes (a SETTING_FIELDS id): changing it pins it. */
  pin: string;
  hint: string;
  min: number;
  max: number;
  log: boolean;
  format: (v: number) => string;
  get: (s: Settings) => number;
  with: (s: Settings, v: number) => Settings;
}

/** The Look section's choices (a row of buttons each, as the water quality's). */
const CHOICES: ChoiceSpec<CloudQuality>[] = [
  {
    label: 'Clouds',
    pin: 'clouds',
    hint: 'Clouds in the sky; Low is lighter on the GPU',
    choices: CLOUD_QUALITIES,
    labels: { off: 'Off', low: 'Low', high: 'High' },
    get: (s) => s.clouds,
    with: (s, v) => ({ ...s, clouds: v }),
  },
  {
    label: 'Light shafts',
    pin: 'shafts',
    hint: 'Sun rays through trees, clouds and water; Low is lighter on the GPU',
    choices: SHAFT_QUALITIES,
    labels: { off: 'Off', low: 'Low', high: 'High' },
    get: (s) => s.shafts,
    with: (s, v) => ({ ...s, shafts: v }),
  },
];

/** The Graphics section's choices, from the data (shared/render/upscale.json). */
const RENDER_SCALE_CHOICES: ChoiceSpec<string>[] = [
  {
    label: 'Render scale',
    pin: 'renderScale.scale',
    hint: 'The share of the screen drawn, then upscaled; lower is lighter on the GPU',
    choices: UPSCALE.scales.map((c) => String(c.value)),
    labels: Object.fromEntries(UPSCALE.scales.map((c) => [String(c.value), c.label])),
    get: (s) => String(s.renderScale.scale),
    with: (s, v) => ({ ...s, renderScale: { ...s.renderScale, scale: Number(v) } }),
  },
  {
    label: 'Upscaler',
    pin: 'renderScale.upscaler',
    hint: UPSCALE.upscalers.map((u) => `${u.label}: ${u.hint}`).join('. '),
    choices: UPSCALE.upscalers.map((u) => u.id),
    labels: Object.fromEntries(UPSCALE.upscalers.map((u) => [u.id, u.label])),
    get: (s) => s.renderScale.upscaler,
    with: (s, v) => ({ ...s, renderScale: { ...s.renderScale, upscaler: v } }),
  },
];

interface ToggleSpec {
  label: string;
  pin: string;
  hint: string;
  get: (s: Settings) => boolean;
  with: (s: Settings, v: boolean) => Settings;
}

/** One row of a pane: a slider, a row of choices, or a checkbox. */
type Row = { slider: SliderSpec } | { choice: ChoiceSpec<string> } | { toggle: ToggleSpec };

/** A titled group of rows in a pane (no title: the pane's own rows). */
interface Group {
  title?: string;
  rows: Row[];
}

/** The settings panes, each a page of the menu. */
export type SettingsPane = 'graphics' | 'sound' | 'gameplay' | 'screen';
export const SETTINGS_PANES: readonly SettingsPane[] = ['graphics', 'sound', 'gameplay', 'screen'];

/** Each pane's rows, in groups; the detail ranges depend on the device (lod/detail.ts). */
function panes(mobile: boolean): Record<SettingsPane, Group[]> {
  const DETAIL_LIMITS = detailLimits(mobile);
  const volume = (label: string, hint: string, key: keyof Volumes): Row => ({
    slider: {
      label,
      pin: `audio.${key}`,
      hint,
      min: 0,
      max: 1,
      log: false,
      format: (v) => `${String(Math.round(v * 100))}%`,
      get: (s) => s.audio[key],
      with: (s, v) => ({ ...s, audio: { ...s.audio, [key]: v } }),
    },
  });
  const choice = <T extends string>(spec: ChoiceSpec<T>): Row => ({
    choice: spec as unknown as ChoiceSpec<string>,
  });
  return {
    graphics: [
      {
        title: 'Quality',
        rows: [
          ...RENDER_SCALE_CHOICES.map(choice),
          choice(WATER_CHOICE),
          choice(SHADOW_CHOICE),
          choice(TORCH_SHADOW_CHOICE),
          ...CHOICES.map(choice),
          {
            slider: {
              label: 'Full detail',
              pin: 'detail.distanceM',
              hint: 'How far every block is drawn; farther uses more memory',
              ...DETAIL_LIMITS.distanceM,
              log: false,
              format: formatMetres,
              get: (s) => s.detail.distanceM,
              with: (s, v) => ({ ...s, detail: { ...s.detail, distanceM: v } }),
            },
          },
          {
            slider: {
              label: 'Distant detail',
              pin: 'detail.pixelError',
              hint: 'Largest step in the distant terrain, in pixels; smaller is sharper and slower',
              ...DETAIL_LIMITS.pixelError,
              log: true,
              format: (v) => `${v.toFixed(1)} px`,
              get: (s) => s.detail.pixelError,
              with: (s, v) => ({ ...s, detail: { ...s.detail, pixelError: v } }),
            },
          },
          {
            slider: {
              label: 'Distant memory',
              pin: 'detail.memoryMb',
              hint: 'Memory for the distant terrain; past it, distant detail coarsens to fit',
              ...DETAIL_LIMITS.memoryMb,
              log: true,
              format: (v) => `${String(Math.round(v))} MB`,
              get: (s) => s.detail.memoryMb,
              with: (s, v) => ({ ...s, detail: { ...s.detail, memoryMb: v } }),
            },
          },
        ],
      },
      {
        title: 'Look',
        rows: [
          {
            slider: {
              label: 'Exposure',
              pin: 'exposure',
              hint: 'How bright the picture is after tone mapping',
              ...EXPOSURE_LIMITS,
              log: true,
              format: (v) => `${v.toFixed(2)}×`,
              get: (s) => s.exposure,
              with: (s, v) => ({ ...s, exposure: v }),
            },
          },
          ...WATER_FLOW_TOGGLES.map((toggle) => ({ toggle })),
        ],
      },
    ],
    sound: [
      {
        rows: [
          volume('Master', 'Everything Dwell plays', 'master'),
          volume('Effects', 'Footsteps, blocks, water, other players', 'effects'),
          volume('Interface', 'Menus, buttons and the hotbar', 'ui'),
          volume('Music', 'The music; 0% turns it off', 'music'),
        ],
      },
    ],
    gameplay: [
      {
        title: 'View',
        rows: [
          {
            slider: {
              label: 'Field of view',
              pin: 'fovDeg',
              hint: 'How much the view takes in, top to bottom (narrower on wide screens)',
              ...FOV_LIMITS,
              log: false,
              format: (v) => `${String(Math.round(v))}°`,
              get: (s) => s.fovDeg,
              with: (s, v) => ({ ...s, fovDeg: Math.round(v) }),
            },
          },
          {
            toggle: {
              label: 'Show your arms',
              pin: 'showArms',
              hint: 'Your hand and what it holds, in first person',
              get: (s) => s.showArms,
              with: (s, v) => ({ ...s, showArms: v }),
            },
          },
        ],
      },
      {
        title: 'Motion',
        rows: [
          {
            toggle: {
              label: 'Speed effects',
              pin: 'motion.speedEffects',
              hint: 'The view widens and wind streaks past when moving fast',
              get: (s) => s.motion.speedEffects,
              with: (s, v) => ({ ...s, motion: { ...s.motion, speedEffects: v } }),
            },
          },
          {
            slider: {
              label: 'FOV boost',
              pin: 'motion.fovBoost',
              hint: 'How much the view widens at speed (0: not at all)',
              min: 0,
              max: 1,
              log: false,
              format: (v) => `${String(Math.round(v * 100))}%`,
              get: (s) => s.motion.fovBoost,
              with: (s, v) => ({ ...s, motion: { ...s.motion, fovBoost: v } }),
            },
          },
          {
            toggle: {
              label: 'Plant sway',
              pin: 'motion.plantSway',
              hint: 'Tall grass sways in the wind',
              get: (s) => s.motion.plantSway,
              with: (s, v) => ({ ...s, motion: { ...s.motion, plantSway: v } }),
            },
          },
        ],
      },
      { title: 'Explosions', rows: EXPLOSION_TOGGLES.map((toggle) => ({ toggle })) },
    ],
    // What the screen shows, one row per part listed as data (shared/ui/hud.json).
    screen: [
      {
        rows: hideableParts(mobile).map((part): Row => ({
          toggle: {
            label: part.label,
            pin: `hud.shown.${part.id}`,
            hint: part.hint,
            get: (st) => st.hud.shown[part.id] ?? part.shown,
            with: (st, v) => ({ ...st, hud: { shown: { ...st.hud.shown, [part.id]: v } } }),
          },
        })),
      },
    ],
  };
}

/** A setting picked from a few named choices (a row of buttons). */
interface ChoiceSpec<T extends string> {
  label: string;
  pin: string;
  hint: string;
  choices: readonly T[];
  labels: Record<T, string>;
  get: (s: Settings) => T;
  with: (s: Settings, v: T) => Settings;
}

const WATER_CHOICE: ChoiceSpec<WaterQuality> = {
  label: 'Water',
  pin: 'water.quality',
  hint: 'Ripples, reflections and glints on water; lower is faster on phones',
  choices: WATER_QUALITIES,
  labels: WATER_QUALITY_LABELS,
  get: (s) => s.water.quality,
  with: (s, v) => ({ ...s, water: { ...s.water, quality: v } }),
};

/** How moving water shows its flow (docs/WATER_SURFACE_GRID.md §2c; owner: each one a setting). */
const WATER_FLOW_TOGGLES: ToggleSpec[] = [
  {
    label: 'Flowing ripples',
    pin: 'water.flowRipples',
    hint: 'Ripples on rivers and moving water are carried along with the current',
    get: (s) => s.water.flowRipples,
    with: (s, v) => ({ ...s, water: { ...s.water, flowRipples: v } }),
  },
  {
    label: 'Foam streaks',
    pin: 'water.flowFoam',
    hint: 'Pale streaks along fast water, showing which way it runs',
    get: (s) => s.water.flowFoam,
    with: (s, v) => ({ ...s, water: { ...s.water, flowFoam: v } }),
  },
  {
    label: 'Drifting flecks',
    pin: 'water.flowParticles',
    hint: 'Small flecks floating downstream on moving water',
    get: (s) => s.water.flowParticles,
    with: (s, v) => ({ ...s, water: { ...s.water, flowParticles: v } }),
  },
  {
    label: 'Waterfalls',
    pin: 'water.waterfalls',
    hint: 'Water falling over a drop shows as a sheet, with foam and spray where it lands',
    get: (s) => s.water.waterfalls,
    with: (s, v) => ({ ...s, water: { ...s.water, waterfalls: v } }),
  },
];

/** A blast's effects (docs/EXPLOSIVES.md §4; each one a setting). */
const EXPLOSION_TOGGLES: ToggleSpec[] = [
  {
    label: 'Explosion flash',
    pin: 'explosions.flash',
    hint: 'A blast lights up its surroundings for a moment, with a fireball',
    get: (s) => s.explosions.flash,
    with: (s, v) => ({ ...s, explosions: { ...s.explosions, flash: v } }),
  },
  {
    label: 'Smoke and dust',
    pin: 'explosions.smoke',
    hint: 'A blast throws up dust and leaves a cloud of smoke',
    get: (s) => s.explosions.smoke,
    with: (s, v) => ({ ...s, explosions: { ...s.explosions, smoke: v } }),
  },
  {
    label: 'Screen shake',
    pin: 'explosions.shake',
    hint: 'The view shakes a little when a blast goes off nearby',
    get: (s) => s.explosions.shake,
    with: (s, v) => ({ ...s, explosions: { ...s.explosions, shake: v } }),
  },
];

const SHADOW_CHOICE: ChoiceSpec<ShadowQuality> = {
  label: 'Sun & moon shadows',
  pin: 'shadows.quality',
  hint: 'Sun and moon shadows near you; Low is lighter on the GPU',
  choices: SHADOW_QUALITIES,
  labels: SHADOW_QUALITY_LABELS,
  get: (s) => s.shadows.quality,
  with: (s, v) => ({ ...s, shadows: { ...s.shadows, quality: v } }),
};

const TORCH_SHADOW_CHOICE: ChoiceSpec<ShadowQuality> = {
  label: 'Torch shadows',
  pin: 'shadows.torch',
  hint: 'Shadows from the nearest torches, held and placed; High draws more of them, sharper',
  choices: SHADOW_QUALITIES,
  labels: SHADOW_QUALITY_LABELS,
  get: (s) => s.shadows.torch,
  with: (s, v) => ({ ...s, shadows: { ...s.shadows, torch: v } }),
};

/** A debug tool as a button in the Debug page (`key` matches an entry of DEBUG_KEYS). */
export interface DebugTool {
  key: string;
  run: () => void;
  /** Whether it is on now (a toggle), for the button's pressed state. */
  on?: () => boolean;
}

type Page = SettingsPane | 'controller' | 'controls' | 'structures' | 'debug' | 'host';

/** The pages in the order they are listed, whatever order they are added in. */
const PAGE_ORDER: readonly Page[] = [
  ...SETTINGS_PANES,
  'controller',
  'controls',
  'structures',
  'debug',
  'host',
];

const PAGE_TITLES: Record<Page, string> = {
  graphics: 'Graphics',
  sound: 'Sound',
  gameplay: 'Gameplay',
  screen: 'On screen',
  controller: 'Controller',
  controls: 'Controls',
  structures: 'Structures',
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
  /** The choice rows: each setting's buttons, the pressed one its value. */
  private readonly choiceRows: {
    get: (s: Settings) => string;
    buttons: Map<string, HTMLButtonElement>;
  }[] = [];
  private readonly toggles: { spec: ToggleSpec; input: HTMLInputElement }[] = [];
  /** Shows the JSON to copy by hand where the clipboard is unavailable. */
  private readonly fallback = document.createElement('textarea');
  /** Reset and Copy JSON, shown under the settings panes. */
  private readonly tail = el('div', 'settings-tail');
  /** The Graphics pane's preset slider and its label. */
  private preset: {
    input: HTMLInputElement;
    value: HTMLSpanElement;
    marker: HTMLButtonElement;
  } | null = null;
  /** The settings, which of them are pinned, and the preset chosen. */
  private state: SettingsState;
  /** Each row's pin marker. */
  private readonly markers: { pin: string; button: HTMLButtonElement }[] = [];
  private get settings(): Settings {
    return this.state.settings;
  }
  /** Told when the menu opens or closes (the touch controls step aside while it is open). */
  onOpenChange: ((open: boolean) => void) | null = null;

  constructor(
    parent: HTMLElement,
    private readonly mobile: boolean,
    private readonly onChange: (settings: Settings) => void,
  ) {
    this.state = loadSettingsState(mobile);
    const button = document.createElement('button');
    this.button = button;
    button.type = 'button';
    button.id = 'menu-button';
    button.setAttribute('aria-label', 'Menu');
    button.setAttribute('aria-expanded', 'false');
    button.setAttribute('aria-controls', 'settings-menu');
    button.title = 'Menu and settings (Esc)';
    // Opening and closing have their own sounds (main.ts), not a click.
    button.dataset.sound = 'none';
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
    close.dataset.sound = 'none';
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

    this.pagesBox.append(this.settingsTail());
    const groups = panes(mobile);
    for (const pane of SETTINGS_PANES) this.addPage(pane, this.settingsPage(pane, groups[pane]));
    this.addPage('controls', this.controlsPage());

    button.addEventListener('click', () => {
      this.setOpen(!this.isOpen);
      // Unfocused, so Space (jump) doesn't press it again.
      button.blur();
    });
    parent.append(button, this.panel);
    this.set(this.state);
    this.show('graphics');
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
    else this.leaveController?.();
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
    resume.dataset.sound = 'none';
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

  /**
   * Adds the Controller page (ui/controllerPanel.ts): `onShow` runs each time it is opened,
   * `onLeave` when another page is shown or the menu closes.
   */
  addControllerPanel(panel: HTMLElement, onShow: () => void, onLeave: () => void): void {
    this.addPage('controller', panel);
    const entry = this.pages.get('controller');
    entry?.tab.setAttribute('id', 'menu-controller');
    entry?.tab.addEventListener('click', onShow);
    this.leaveController = onLeave;
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

  /**
   * Adds the Structures page (ui/structuresPanel.ts): `onShow` runs each time it is opened, to
   * list the saved structures.
   */
  addStructuresPanel(panel: HTMLElement, onShow: () => void): void {
    const page = el('section');
    page.append(panel);
    this.addPage('structures', page);
    const tab = this.pages.get('structures')?.tab;
    tab?.setAttribute('id', 'menu-structures');
    tab?.addEventListener('click', onShow);
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
      // Each tool sounds as its key does (main.ts).
      b.dataset.sound = 'none';
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
    const order = PAGE_ORDER;
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
      // Before the settings' Reset / Copy JSON, which stay last.
      this.pagesBox.insertBefore(page, this.tail);
    }
    this.pages.set(name, { tab, page });
    const shown = [...this.pages.values()].some((p) => !p.page.hidden && p.page !== page);
    page.hidden = shown;
    tab.setAttribute('aria-selected', String(!shown));
  }

  private leaveController: (() => void) | null = null;

  private show(name: Page): void {
    if (name !== 'controller') this.leaveController?.();
    for (const [n, { tab, page }] of this.pages) {
      page.hidden = n !== name;
      tab.setAttribute('aria-selected', String(n === name));
    }
    // Reset and Copy JSON under every settings pane, not the other pages.
    this.tail.hidden = !(SETTINGS_PANES as readonly Page[]).includes(name);
    this.pagesBox.scrollTop = 0;
  }

  /** A settings pane: the Graphics pane's preset slider first, then its groups of rows. */
  private settingsPage(pane: SettingsPane, groups: Group[]): HTMLElement {
    const page = el('section');
    page.classList.add('settings-pane');
    if (pane === 'graphics') page.append(this.presetRow());
    for (const g of groups) {
      const group = el('div', 'gm-group');
      if (g.title) group.append(el('h2', undefined, g.title));
      const list = el('div', 'gm-grid');
      for (const row of g.rows) {
        if ('slider' in row) list.append(this.slider(row.slider));
        else if ('choice' in row) list.append(this.choice(row.choice));
        else list.append(this.toggle(row.toggle));
      }
      group.append(list);
      page.append(group);
    }
    return page;
  }

  /**
   * The Preset slider (shared/settings/graphics-presets.json, cheapest first): moving it applies
   * a preset (stored by id: its settings then follow the preset's current definition); it reads
   * Custom once a setting it sets is changed by hand, the slider staying on the preset the others
   * follow. Its marker goes back to the device's default preset.
   */
  private presetRow(): HTMLElement {
    const presets = GRAPHICS_PRESETS.presets;
    const row = document.createElement('label');
    row.className = 'settings-row settings-preset';
    row.id = 'graphics-preset';
    const value = el('span', 'settings-value');
    const input = document.createElement('input');
    input.type = 'range';
    input.min = '0';
    input.max = String(presets.length - 1);
    input.step = '1';
    input.title = 'A preset of the graphics settings: lower is lighter on the GPU';
    input.addEventListener('input', () => {
      const p = presets[Number(input.value)];
      if (p) this.set(choosePreset(this.state, p.id, this.mobile));
    });
    const ticks = el('span', 'preset-ticks');
    for (const p of presets) ticks.append(el('span', undefined, p.label));
    const marker = this.marker('Preset', () => {
      this.set(clearPreset(this.state, this.mobile));
    });
    row.append(el('span', 'settings-name', 'Preset'), value, input, marker, ticks);
    this.preset = { input, value, marker };
    return row;
  }

  /** Shows the preset the graphics follow, or Custom once one of them is pinned. */
  private showPreset(): void {
    if (!this.preset) return;
    const { input, value, marker } = this.preset;
    const { base, custom } = shownPreset(this.state, this.mobile);
    value.textContent = custom ? 'Custom' : base.label;
    input.value = String(GRAPHICS_PRESETS.presets.indexOf(base));
    this.paint(input, GRAPHICS_PRESETS.presets.length - 1);
    input.closest('.settings-preset')?.classList.toggle('custom', custom);
    marker.hidden = !custom && this.state.preset === null;
  }

  /**
   * A pinned setting's marker (owner, 2026-11-07): shown while the player has changed the setting,
   * pressing it clears the pin, so the setting follows the build's default again. A button, so the
   * keyboard (Tab) and a controller (ui/gamepadMenu.ts) reach it.
   */
  private marker(label: string, clear: () => void): HTMLButtonElement {
    const b = el('button', 'settings-pin', '↺');
    b.type = 'button';
    b.title = 'Changed: press to go back to the default';
    b.setAttribute('aria-label', `${label}: back to the default`);
    b.hidden = true;
    b.addEventListener('click', (e) => {
      // Not the row's control (a label's click would toggle its checkbox).
      e.preventDefault();
      clear();
      b.blur();
    });
    return b;
  }

  /** A row's marker for its setting, shown while the setting is pinned. */
  private pinMarker(label: string, pin: string): HTMLButtonElement {
    const b = this.marker(label, () => {
      this.set(clearPin(this.state, pin, this.mobile));
    });
    this.markers.push({ pin, button: b });
    return b;
  }

  private showMarkers(): void {
    for (const { pin, button } of this.markers) button.hidden = !this.state.pins.has(pin);
  }

  /** Reset and Copy JSON, under each settings pane (shown with them, ui `show`). */
  private settingsTail(): HTMLElement {
    const reset = el('button', 'quiet-button settings-reset', 'Reset');
    reset.type = 'button';
    reset.title = 'Back to the defaults';
    reset.addEventListener('click', () => {
      this.set(resetSettings(this.mobile));
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
    this.tail.append(actions, this.fallback);
    return this.tail;
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
    row.title = spec.hint;
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
      this.state = changeSetting(this.state, spec.pin, spec.with(this.settings, v));
      value.textContent = spec.format(v);
      this.paint(input);
      this.apply();
    });
    const hint = el('span', 'settings-hint', spec.hint);
    row.append(name, value, input, this.pinMarker(spec.label, spec.pin), hint);
    this.rows.push({ spec, input, value });
    return row;
  }

  private choice<T extends string>(spec: ChoiceSpec<T>): HTMLDivElement {
    const row = el('div', 'settings-row');
    row.title = spec.hint;
    const name = el('span', 'settings-name', spec.label);
    const value = el('span', 'settings-value');
    const box = el('div', 'settings-choices');
    box.setAttribute('role', 'group');
    box.setAttribute('aria-label', spec.label);
    const buttons = new Map<string, HTMLButtonElement>();
    for (const c of spec.choices) {
      const b = el('button', 'settings-choice', spec.labels[c]);
      b.type = 'button';
      b.addEventListener('click', () => {
        this.state = changeSetting(this.state, spec.pin, spec.with(this.settings, c));
        this.showChoices();
        this.apply();
        b.blur();
      });
      buttons.set(c, b);
      box.append(b);
    }
    const hint = el('span', 'settings-hint', spec.hint);
    row.append(name, value, box, this.pinMarker(spec.label, spec.pin), hint);
    this.choiceRows.push({ get: spec.get, buttons });
    return row;
  }

  private showChoices(): void {
    for (const { get, buttons } of this.choiceRows) {
      const v = get(this.settings);
      for (const [c, b] of buttons) b.setAttribute('aria-pressed', String(c === v));
    }
  }

  private toggle(spec: ToggleSpec): HTMLLabelElement {
    const row = document.createElement('label');
    row.className = 'settings-row settings-toggle';
    row.title = spec.hint;
    const input = document.createElement('input');
    input.type = 'checkbox';
    input.title = spec.hint;
    input.addEventListener('change', () => {
      this.state = changeSetting(this.state, spec.pin, spec.with(this.settings, input.checked));
      this.apply();
    });
    row.append(
      el('span', 'settings-name', spec.label),
      input,
      this.pinMarker(spec.label, spec.pin),
      el('span', 'settings-hint', spec.hint),
    );
    this.toggles.push({ spec, input });
    return row;
  }

  /** The slider's filled part, as a CSS variable the track's gradient reads. */
  private paint(input: HTMLInputElement, max = SLIDER_STEPS): void {
    input.style.setProperty('--fill', `${String((100 * Number(input.value)) / max)}%`);
  }

  private set(state: SettingsState): void {
    this.state = state;
    const settings = state.settings;
    for (const { spec, input, value } of this.rows) {
      const v = spec.get(settings);
      input.value = String(valueToSlider(v, spec.min, spec.max, spec.log));
      value.textContent = spec.format(v);
      this.paint(input);
    }
    this.showChoices();
    for (const { spec, input } of this.toggles) input.checked = spec.get(settings);
    this.apply();
  }

  private async copy(button: HTMLButtonElement): Promise<void> {
    const json = settingsJson(this.settings);
    try {
      await navigator.clipboard.writeText(json);
      this.fallback.hidden = true;
      button.textContent = 'Copied';
      audio().ui('success');
    } catch {
      // No clipboard (an insecure page, or permission denied): select the JSON to copy by hand.
      this.fallback.value = json;
      this.fallback.hidden = false;
      this.fallback.select();
      button.textContent = 'Select and copy';
      audio().ui('error');
    }
    setTimeout(() => {
      button.textContent = 'Copy JSON';
    }, 1500);
  }

  private apply(): void {
    this.showPreset();
    this.showMarkers();
    if (!this.fallback.hidden) this.fallback.value = settingsJson(this.settings);
    this.onChange(this.settings);
    saveSettingsState(this.state);
  }
}
