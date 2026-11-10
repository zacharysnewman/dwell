// In-game HUD: health, crosshair, death/respawn message, and the F3 debug overlay
// (PLAYER_CONTROLLER.md §9). The texts come from the Rust client (`Client.hud()`).
import { Players } from '../protocol/constants.gen';
import { type SpyglassHud, spyglassStyle } from './spyglass';

/** Stamina and air as shares of full (ADR 0069), from `Client.hud().vitals`; null parts: the
 * world's mode has no such rule. */
export interface VitalsHud {
  stamina: number | null;
  oxygen: number | null;
  exhausted: boolean;
}

/** What one bar shows: its width (%) and whether it is shown (faded out when full). */
export function vitalsBar(share: number | null): { width: string; shown: boolean } {
  if (share === null) return { width: '100%', shown: false };
  const s = Math.min(1, Math.max(0, share));
  return { width: `${String(Math.round(s * 1000) / 10)}%`, shown: s < 0.999 };
}

export class Hud {
  private readonly root: HTMLDivElement;
  private readonly health: HTMLDivElement;
  private readonly healthFill: HTMLDivElement;
  /** Stamina and air (ADR 0069), above the health bar; each faded out when full. */
  private readonly vitals: HTMLDivElement;
  private readonly staminaBar: HTMLDivElement;
  private readonly staminaFill: HTMLDivElement;
  private readonly airBar: HTMLDivElement;
  private readonly airFill: HTMLDivElement;
  private readonly message: HTMLDivElement;
  private readonly status: HTMLDivElement;
  private readonly debug: HTMLDivElement;
  private readonly debugText: HTMLPreElement;
  private readonly dump: HTMLDivElement;
  private readonly note: HTMLDivElement;
  private readonly dumpText: HTMLTextAreaElement;
  private damageUntil = 0;
  /** The debug overlay's "Copy block info" button (or F6): dump the blocks around the target. */
  onCopyBlocks: (() => void) | null = null;
  /** Its Map (F4) and Close (F3) buttons. */
  onToggleMap: (() => void) | null = null;
  onCloseDebug: (() => void) | null = null;
  private readonly hint: HTMLDivElement;
  private hintTimer = 0;
  /** The spyglass's vignette (docs/SPYGLASS.md), hidden while unzoomed. */
  private readonly spyglass: HTMLDivElement;
  /** The held tool's swing line (`setSwing`). */
  private readonly swingLine: HTMLDivElement;
  /** The pick-up prompt under the crosshair (ADR 0071). */
  private readonly pickUp: HTMLDivElement;

  constructor(parent: HTMLElement) {
    this.root = document.createElement('div');
    this.root.id = 'hud';
    const crosshair = document.createElement('div');
    crosshair.id = 'crosshair';
    // A held tool's swing line through the crosshair (ADR 0067): where its path will pass.
    this.swingLine = document.createElement('div');
    this.swingLine.className = 'swing-line';
    this.swingLine.hidden = true;
    crosshair.append(this.swingLine);
    this.pickUp = document.createElement('div');
    this.pickUp.className = 'pick-up';
    this.pickUp.hidden = true;
    crosshair.append(this.pickUp);
    this.health = document.createElement('div');
    this.health.id = 'health';
    this.healthFill = document.createElement('div');
    this.health.append(this.healthFill);
    this.vitals = document.createElement('div');
    this.vitals.id = 'vitals';
    const bar = (kind: string, label: string): [HTMLDivElement, HTMLDivElement] => {
      const b = document.createElement('div');
      b.className = `vital ${kind}`;
      b.title = label;
      b.setAttribute('aria-label', label);
      const f = document.createElement('div');
      b.append(f);
      return [b, f];
    };
    [this.airBar, this.airFill] = bar('air', 'Air');
    [this.staminaBar, this.staminaFill] = bar('stamina', 'Stamina');
    this.vitals.append(this.airBar, this.staminaBar);
    this.message = document.createElement('div');
    this.message.id = 'center-message';
    this.status = document.createElement('div');
    this.status.id = 'status-chip';
    this.debug = document.createElement('div');
    this.debug.id = 'debug-overlay';
    this.debug.hidden = true;
    this.debugText = document.createElement('pre');
    // The header: what the overlay is, and its tools with their keys (first: a phone's overlay
    // scrolls its long text under it).
    const head = document.createElement('div');
    head.className = 'debug-head';
    const title = document.createElement('span');
    title.className = 'debug-title';
    title.textContent = 'Debug';
    const tool = (id: string, label: string, key: string, title: string, run: () => void) => {
      const b = document.createElement('button');
      b.type = 'button';
      if (id) b.id = id;
      b.className = 'debug-button';
      b.title = title;
      // Each tool sounds as its key does (main.ts), not a click.
      b.dataset.sound = 'none';
      const k = document.createElement('kbd');
      k.textContent = key;
      b.append(label, k);
      b.addEventListener('click', run);
      return b;
    };
    head.append(
      title,
      tool(
        'debug-copy',
        'Copy block info',
        'F6',
        'The targeted block and its neighbours as JSON',
        () => this.onCopyBlocks?.(),
      ),
      tool('', 'Map', 'F4', 'The terrain map around you (− and = zoom)', () =>
        this.onToggleMap?.(),
      ),
      tool('', 'Close', 'F3', 'Hide the debug overlay', () => this.onCloseDebug?.()),
    );
    this.debug.append(head, this.debugText);
    // Where the clipboard is refused: the dump in a box to select and copy by hand.
    this.dump = document.createElement('div');
    this.dump.id = 'debug-dump';
    this.dump.hidden = true;
    this.dumpText = document.createElement('textarea');
    this.dumpText.readOnly = true;
    const close = document.createElement('button');
    close.type = 'button';
    close.textContent = 'Close';
    close.addEventListener('click', () => (this.dump.hidden = true));
    this.dump.append(this.dumpText, close);
    this.note = document.createElement('div');
    this.note.id = 'hud-note';
    this.hint = document.createElement('div');
    this.hint.id = 'start-hint';
    this.hint.hidden = true;
    this.spyglass = document.createElement('div');
    this.spyglass.id = 'spyglass';
    this.spyglass.hidden = true;
    this.root.append(
      this.spyglass,
      crosshair,
      this.vitals,
      this.health,
      this.message,
      this.status,
      this.debug,
      this.dump,
      this.note,
      this.hint,
    );
    parent.append(this.root);
  }

  get debugVisible(): boolean {
    return !this.debug.hidden;
  }

  toggleDebug(): void {
    this.debug.hidden = !this.debug.hidden;
  }

  /** The ground item under the crosshair (its label: "Stick"), or none: the pick-up prompt. */
  setPickUp(label: string | null): void {
    this.pickUp.hidden = label === null;
    const text = label === null ? '' : `E  Pick up ${label}`;
    if (this.pickUp.textContent !== text) this.pickUp.textContent = text;
  }

  /** The held tool's swing line through the crosshair: level, vertical, or none. */
  setSwing(mode: 'level' | 'vertical' | null): void {
    this.swingLine.hidden = mode === null;
    this.swingLine.classList.toggle('level', mode === 'level');
    this.swingLine.classList.toggle('vertical', mode === 'vertical');
  }

  setHealth(health: number, nowMs: number): void {
    this.healthFill.style.width = `${String((100 * health) / Players.maxHealth)}%`;
    this.health.classList.toggle('hurt', nowMs < this.damageUntil);
  }

  /** Stamina and air (null: a mode without them). */
  setVitals(v: VitalsHud | null): void {
    const stamina = vitalsBar(v?.stamina ?? null);
    const air = vitalsBar(v?.oxygen ?? null);
    this.staminaFill.style.width = stamina.width;
    this.airFill.style.width = air.width;
    this.staminaBar.classList.toggle('shown', stamina.shown);
    this.airBar.classList.toggle('shown', air.shown);
    this.staminaBar.classList.toggle('exhausted', v?.exhausted ?? false);
  }

  flashDamage(nowMs: number): void {
    this.damageUntil = nowMs + 400;
  }

  /** The spyglass's vignette from `Client.hud().spyglass` (null: unzoomed). */
  setSpyglass(s: SpyglassHud | null): void {
    this.spyglass.hidden = s === null;
    if (s === null) return;
    const style = spyglassStyle(s);
    if (this.spyglass.style.background !== style.background)
      this.spyglass.style.background = style.background;
    this.spyglass.style.opacity = style.opacity;
  }

  setMessage(text: string): void {
    this.message.textContent = text;
  }

  /** Background work (terrain loading), in a corner so it never covers the view. */
  setStatus(text: string): void {
    if (this.status.textContent !== text) this.status.textContent = text;
  }

  setDebug(text: string): void {
    if (!this.debug.hidden) this.debugText.textContent = text;
  }

  /** Shows text (a block dump the clipboard refused) selected in a box, to copy by hand. */
  showDump(text: string): void {
    this.dumpText.value = text;
    this.dump.hidden = false;
    this.dumpText.focus();
    this.dumpText.select();
  }

  /** A hint at the foot of the view (the keys, when a game starts), fading after `ms`. */
  showHint(text: string, ms = 9000): void {
    this.hint.textContent = text;
    this.hint.hidden = false;
    this.hint.classList.remove('fading');
    window.clearTimeout(this.hintTimer);
    this.hintTimer = window.setTimeout(() => {
      this.hint.classList.add('fading');
      this.hintTimer = window.setTimeout(() => (this.hint.hidden = true), 800);
    }, ms);
  }

  /** A short note under the middle of the view that clears itself (the centre message is the game's). */
  flash(text: string, ms = 2000): void {
    this.note.textContent = text;
    setTimeout(() => {
      if (this.note.textContent === text) this.note.textContent = '';
    }, ms);
  }
}
