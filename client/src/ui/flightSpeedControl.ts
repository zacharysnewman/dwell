// The flight speed slider (PLAYER_CONTROLLER.md §6.7): shown while flying, top right. It sets the
// level every input frame carries; the − and = keys step it too, since the slider can't be dragged
// while the pointer is locked.
import { flySpeedLabel, MAX_FLY_SPEED_LEVEL } from '../predict/flightSpeed';

export class FlightSpeedControl {
  private readonly root = document.createElement('div');
  private readonly slider = document.createElement('input');
  private readonly value = document.createElement('span');

  constructor(
    parent: HTMLElement,
    private readonly onChange: (level: number) => void,
  ) {
    this.root.id = 'flight-speed';
    this.root.hidden = true;
    const label = document.createElement('label');
    const name = document.createElement('span');
    name.className = 'flight-speed-name';
    name.textContent = 'Flight speed';
    this.value.className = 'flight-speed-value';
    this.slider.type = 'range';
    this.slider.min = '0';
    this.slider.max = String(MAX_FLY_SPEED_LEVEL);
    this.slider.step = '1';
    this.slider.title = 'Flight speed (− and = keys)';
    this.slider.addEventListener('input', () => {
      this.onChange(Number(this.slider.value));
      // Unfocused afterwards on release, so Space and the arrow keys go to the game.
    });
    this.slider.addEventListener('change', () => {
      this.slider.blur();
    });
    const hint = document.createElement('span');
    hint.className = 'flight-speed-hint';
    hint.textContent = '− / = keys';
    label.append(name, this.value, this.slider, hint);
    this.root.append(label);
    parent.append(this.root);
  }

  set visible(v: boolean) {
    this.root.hidden = !v;
  }

  /** Shows a level (set from here, the keys, or a saved setting). */
  show(level: number): void {
    this.slider.value = String(level);
    this.value.textContent = flySpeedLabel(level);
  }
}
