// Play advanced (docs/DEV_TOOLS.md, Play advanced): the main menu's alternate Play. It opens a
// one-off local world by link (`?local=1`) with overrides the game already reads from the address —
// the world type, seed, spawn, time of day, … — one field per entry of shared/play/overrides.json
// (generated into playOverrides.gen.ts). Only the fields the player set are passed; nothing is
// saved, and the overrides leave the address on the next route (ui/launch.ts ROUTE_PARAMS).
import { OVERRIDE_FIELDS } from './playOverrides.gen';

export interface OverrideOption {
  value: string;
  label: string;
}

/** One override: a URL parameter and how the menu asks for it. */
export type OverrideField = { param: string; label: string; hint?: string } & (
  | { kind: 'choice'; options: OverrideOption[] }
  | { kind: 'integer'; min: number; max: number }
  | { kind: 'number'; min: number; max: number; step: number }
  | { kind: 'clock' }
);

/** Every parameter Play advanced may set (ui/launch.ts drops them on the next route). */
export const OVERRIDE_PARAMS: readonly string[] = OVERRIDE_FIELDS.map((f) => f.param);

/** A 24-hour time ("06:30") as the share of the day the game's `?time=` takes, or null. */
export function clockShare(text: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return Math.round(((h * 60 + min) / 1440) * 1e4) / 1e4;
}

/**
 * The route Play advanced opens for the values the player entered (by parameter; empty means not
 * set), or the first problem with them, naming its field.
 */
export function advancedRoute(
  values: Record<string, string>,
  fields: readonly OverrideField[] = OVERRIDE_FIELDS,
): { route: Record<string, string> } | { problem: string } {
  const route: Record<string, string> = { local: '1' };
  for (const f of fields) {
    const raw = (values[f.param] ?? '').trim();
    if (raw === '') continue;
    switch (f.kind) {
      case 'choice': {
        if (!f.options.some((o) => o.value === raw))
          return { problem: `${f.label}: not a choice.` };
        route[f.param] = raw;
        break;
      }
      case 'integer': {
        const n = Number(raw);
        if (!Number.isSafeInteger(n) || n < f.min || n > f.max) {
          return {
            problem: `${f.label}: a whole number from ${String(f.min)} to ${String(f.max)}.`,
          };
        }
        route[f.param] = String(n);
        break;
      }
      case 'number': {
        const n = Number(raw);
        if (!Number.isFinite(n) || n < f.min || n > f.max) {
          return { problem: `${f.label}: a number from ${String(f.min)} to ${String(f.max)}.` };
        }
        route[f.param] = String(n);
        break;
      }
      case 'clock': {
        const share = clockShare(raw);
        if (share === null) return { problem: `${f.label}: a time such as 06:30.` };
        route[f.param] = String(share);
        break;
      }
    }
  }
  return { route };
}

/** The panel: a field per override and its Play button, which calls `go` with the route. */
export function playAdvancedPanel(
  go: (route: Record<string, string>) => void,
  fields: readonly OverrideField[] = OVERRIDE_FIELDS,
): HTMLFormElement {
  const form = document.createElement('form');
  form.className = 'play-advanced';
  form.id = 'play-advanced';
  form.hidden = true;
  const intro = document.createElement('p');
  intro.className = 'menu-hint';
  intro.textContent =
    'Opens a world for this seed and type with the settings below, just for this visit. Leave a field empty for its default.';
  form.append(intro);
  const inputs = new Map<string, HTMLInputElement | HTMLSelectElement>();
  for (const f of fields) {
    const row = document.createElement('label');
    row.className = 'menu-field play-advanced-field';
    const name = document.createElement('span');
    name.textContent = f.label;
    let input: HTMLInputElement | HTMLSelectElement;
    if (f.kind === 'choice') {
      const select = document.createElement('select');
      for (const o of f.options) {
        const option = document.createElement('option');
        option.value = o.value;
        option.textContent = o.label;
        select.append(option);
      }
      input = select;
    } else {
      const text = document.createElement('input');
      if (f.kind === 'clock') {
        text.type = 'time';
      } else {
        text.type = 'number';
        text.min = String(f.min);
        text.max = String(f.max);
        text.step = f.kind === 'number' ? String(f.step) : '1';
        text.inputMode = f.kind === 'number' ? 'decimal' : 'numeric';
      }
      input = text;
    }
    input.name = f.param;
    inputs.set(f.param, input);
    row.append(name, input);
    if (f.hint) {
      const hint = document.createElement('small');
      hint.textContent = f.hint;
      row.append(hint);
    }
    form.append(row);
  }
  const problem = document.createElement('p');
  problem.className = 'menu-hint play-advanced-problem';
  problem.setAttribute('aria-live', 'polite');
  const play = document.createElement('button');
  play.type = 'submit';
  play.className = 'block-button';
  play.textContent = 'Play';
  form.append(problem, play);
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const values: Record<string, string> = {};
    for (const [param, input] of inputs) values[param] = input.value;
    const result = advancedRoute(values, fields);
    if ('problem' in result) {
      problem.textContent = result.problem;
      return;
    }
    problem.textContent = '';
    go(result.route);
  });
  return form;
}
