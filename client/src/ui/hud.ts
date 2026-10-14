// In-game HUD: health, crosshair, death/respawn message, and the F3 debug overlay
// (PLAYER_CONTROLLER.md §9). The texts come from the Rust client (`Client.hud()`).
import { Players } from '../protocol/constants.gen';

export class Hud {
  private readonly root: HTMLDivElement;
  private readonly health: HTMLDivElement;
  private readonly healthFill: HTMLDivElement;
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

  constructor(parent: HTMLElement) {
    this.root = document.createElement('div');
    this.root.id = 'hud';
    const crosshair = document.createElement('div');
    crosshair.id = 'crosshair';
    this.health = document.createElement('div');
    this.health.id = 'health';
    this.healthFill = document.createElement('div');
    this.health.append(this.healthFill);
    this.message = document.createElement('div');
    this.message.id = 'center-message';
    this.status = document.createElement('div');
    this.status.id = 'status-chip';
    this.debug = document.createElement('div');
    this.debug.id = 'debug-overlay';
    this.debug.hidden = true;
    this.debugText = document.createElement('pre');
    const copy = document.createElement('button');
    copy.id = 'debug-copy';
    copy.type = 'button';
    copy.textContent = 'Copy block info';
    copy.title = 'The targeted block and its neighbours as JSON (F6)';
    copy.addEventListener('click', () => this.onCopyBlocks?.());
    this.debug.append(copy, this.debugText); // first: a phone's overlay clips its long text
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
    this.root.append(
      crosshair,
      this.health,
      this.message,
      this.status,
      this.debug,
      this.dump,
      this.note,
    );
    parent.append(this.root);
  }

  get debugVisible(): boolean {
    return !this.debug.hidden;
  }

  toggleDebug(): void {
    this.debug.hidden = !this.debug.hidden;
  }

  setHealth(health: number, nowMs: number): void {
    this.healthFill.style.width = `${String((100 * health) / Players.maxHealth)}%`;
    this.health.classList.toggle('hurt', nowMs < this.damageUntil);
  }

  flashDamage(nowMs: number): void {
    this.damageUntil = nowMs + 400;
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

  /** A short note under the middle of the view that clears itself (the centre message is the game's). */
  flash(text: string, ms = 2000): void {
    this.note.textContent = text;
    setTimeout(() => {
      if (this.note.textContent === text) this.note.textContent = '';
    }, ms);
  }
}
