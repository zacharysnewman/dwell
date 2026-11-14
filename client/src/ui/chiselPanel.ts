// Chisel in the game menu (Phase 56 B7, WORK_QUEUE item 34): a local creative world's biome sliders.
// A change, once the slider rests (chisel/inGame.ts `debounce`), retunes the world: the local server
// saves the config with the world and generates the terrain near the player again, the player's
// edits kept. Undo and Reset (to the config the world had when the panel opened). Host only: the
// server refuses while guests are in the world.
import {
  IN_GAME_GROUPS,
  InGameEdit,
  RETUNE_DEBOUNCE_MS,
  debounce,
  editOfWorld,
  sliderSpecs,
} from '../chisel/inGame';

export interface ChiselPanelDeps {
  /** The world's worldgen config as it opened (`""`: the build's ground). */
  worldgen: string;
  /** The world's game mode now, by name: the sliders are offered in creative worlds. */
  mode: () => string;
  /** Sends the new config; resolves with the refusal's message, or null once retuned. */
  retune: (worldgen: string) => Promise<string | null>;
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> = {},
  ...kids: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...kids);
  return e;
}

export class ChiselPanel {
  readonly element = el('div', { id: 'chisel-panel' });
  private readonly edit: InGameEdit | null;
  private biome = 0;
  private readonly status = el('p', { className: 'gm-note', id: 'chisel-status' });
  private readonly body = el('div');
  private readonly send: (text: string) => void;

  constructor(private readonly deps: ChiselPanelDeps) {
    const opened = editOfWorld(deps.worldgen);
    this.edit = opened ? new InGameEdit(opened) : null;
    this.send = debounce((text: string) => {
      this.say('Regenerating the terrain around you…');
      void deps.retune(text).then((err) => {
        this.say(err ? `Not retuned: ${err}` : 'Retuned: saved with the world.');
      });
    }, RETUNE_DEBOUNCE_MS);
    this.element.append(this.body, this.status);
    this.draw();
  }

  /** Redraws the sliders (the menu page shown, or after an undo or reset). */
  draw(): void {
    this.body.replaceChildren();
    if (!this.edit) {
      this.say("This world's ground came from a file Chisel's page tunes, not the game.");
      return;
    }
    if (this.deps.mode() !== 'creative') {
      this.say('Chisel is for creative worlds: switch the mode on the Host page.');
      return;
    }
    const edit = this.edit;
    const pick = el('select', { id: 'chisel-biome' });
    edit.biomes().forEach((name, i) => {
      pick.append(
        el('option', { value: String(i), textContent: name, selected: i === this.biome }),
      );
    });
    pick.onchange = () => {
      this.biome = Number(pick.value);
      this.draw();
    };
    const undo = el('button', { type: 'button', textContent: 'Undo', disabled: !edit.canUndo() });
    undo.onclick = () => {
      if (edit.undo()) this.changed();
    };
    const reset = el('button', { type: 'button', textContent: 'Reset' });
    reset.onclick = () => {
      edit.reset();
      this.changed();
    };
    this.body.append(el('div', { className: 'row' }, pick, undo, reset));
    for (const group of IN_GAME_GROUPS) {
      const values = edit.group(this.biome, group);
      const box = el('fieldset', {}, el('legend', { textContent: group }));
      for (const s of sliderSpecs(group, values)) {
        const v = Number(values[s.key]);
        const out = el('output', { textContent: String(v) });
        const range = el('input', {
          type: 'range',
          min: String(s.min),
          max: String(s.max),
          step: String(s.step),
          value: String(v),
        });
        range.oninput = () => {
          const x = s.whole ? Math.round(Number(range.value)) : Number(range.value);
          const errors = edit.set(this.biome, group, s.key, x);
          if (errors.length) {
            this.say(errors[0] ?? '');
            return;
          }
          out.textContent = String(x);
          undo.disabled = !edit.canUndo();
          this.send(edit.text());
        };
        box.append(
          el('label', { className: 'row' }, el('span', { textContent: s.key }), range, out),
        );
      }
      this.body.append(box);
    }
  }

  private changed(): void {
    this.draw();
    if (this.edit) this.send(this.edit.text());
  }

  say(text: string): void {
    this.status.textContent = text;
  }
}
