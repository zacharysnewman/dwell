// Host… in the game menu (ARCHITECTURE.md §10.2, Phase 5c): opens this local world to friends. The
// form sets the guest limit (by platform), who may edit and who may fly; once hosting, the panel
// shows the join code, an invite link to copy, a QR code for phones, the guests connected, and
// Stop hosting.
import qrcode from 'qrcode-generator';
import { HostPolicy, maxGuestsFor, type HostSettings, type HostVisibility } from '../net/hosting';

/** The visibility choices' values, in the dialog's order. */
const VISIBILITIES: readonly HostVisibility[] = ['code', 'network', 'public'];

export interface Hosted {
  /** The code as shown ("KQ7-XM4"), and the link that joins by it. */
  display: string;
  link: string;
}

export interface HostPanelDeps {
  mobile: boolean;
  /** The world's name, listed to the network with "Code + same network". */
  worldName: string;
  /** Starts hosting; rejects with a message for the player. */
  start: (settings: HostSettings) => Promise<Hosted>;
  stop: () => void;
}

const POLICIES: { value: HostPolicy; label: string }[] = [
  { value: HostPolicy.Everyone, label: 'Everyone' },
  { value: HostPolicy.Host, label: 'Only me' },
  { value: HostPolicy.Nobody, label: 'Nobody' },
];

/** "No guests yet (up to 8)", "3 of 8 guests playing". */
export function guestsText(count: number, max: number): string {
  if (count === 0) return `No guests yet (up to ${String(max)})`;
  return `${String(count)} of ${String(max)} guests playing`;
}

/** The QR code's dark modules for `text`, row by row (for drawing, and tests). */
export function qrModules(text: string): boolean[][] {
  const qr = qrcode(0, 'M');
  qr.addData(text);
  qr.make();
  const n = qr.getModuleCount();
  return Array.from({ length: n }, (_, r) => Array.from({ length: n }, (_, c) => qr.isDark(r, c)));
}

function drawQr(canvas: HTMLCanvasElement, text: string): void {
  const modules = qrModules(text);
  const quiet = 4;
  const size = modules.length + quiet * 2;
  const scale = Math.max(1, Math.floor(200 / size));
  canvas.width = canvas.height = size * scale;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.fillStyle = '#fff';
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = '#000';
  modules.forEach((row, r) => {
    row.forEach((dark, c) => {
      if (dark) ctx.fillRect((c + quiet) * scale, (r + quiet) * scale, scale, scale);
    });
  });
}

function select(id: string, options: { value: number; label: string }[], chosen: number) {
  const el = document.createElement('select');
  el.id = id;
  for (const o of options) {
    const option = document.createElement('option');
    option.value = String(o.value);
    option.textContent = o.label;
    option.selected = o.value === chosen;
    el.append(option);
  }
  return el;
}

function labelled(text: string, control: HTMLElement): HTMLLabelElement {
  const label = document.createElement('label');
  label.className = 'host-row';
  const name = document.createElement('span');
  name.textContent = text;
  label.append(name, control);
  return label;
}

function actionButton(text: string, id: string, onClick: () => void): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.id = id;
  b.textContent = text;
  b.addEventListener('click', onClick);
  return b;
}

export class HostPanel {
  readonly element = document.createElement('section');
  private max: number;
  private guests = 0;
  private readonly status = document.createElement('p');

  constructor(private readonly deps: HostPanelDeps) {
    this.element.id = 'host-panel';
    this.max = maxGuestsFor(deps.mobile);
    this.showForm();
  }

  /** The number of guests playing changed. */
  setGuests(count: number): void {
    this.guests = count;
    const el = this.element.querySelector('#host-guests');
    if (el) el.textContent = guestsText(count, this.max);
  }

  /** A notice for the host (e.g. the code stopped working). */
  say(message: string): void {
    this.status.textContent = message;
  }

  private showForm(message = ''): void {
    const max = maxGuestsFor(this.deps.mobile);
    const guests = select(
      'host-max-guests',
      Array.from({ length: max }, (_, i) => ({ value: i + 1, label: String(i + 1) })),
      max,
    );
    const edits = select('host-edits', POLICIES, HostPolicy.Everyone);
    const flight = select('host-flight', POLICIES, HostPolicy.Everyone);
    const visibility = select(
      'host-visibility',
      [
        { value: 0, label: 'Code only' },
        { value: 1, label: 'Code + same network' },
        { value: 2, label: 'Public (server list)' },
      ],
      1,
    );
    this.status.className = 'menu-hint';
    this.status.id = 'host-status';
    this.status.textContent =
      message || 'Friends join with a code, on any network. Keep this page open while hosting.';
    const start = actionButton('Start hosting', 'host-start', () => {
      start.disabled = true;
      start.textContent = 'Starting…';
      const settings: HostSettings = {
        maxGuests: Number(guests.value),
        edits: Number(edits.value) as HostPolicy,
        flight: Number(flight.value) as HostPolicy,
        visibility: VISIBILITIES[Number(visibility.value)] ?? 'code',
        name: this.deps.worldName,
      };
      this.deps.start(settings).then(
        (hosted) => {
          this.max = settings.maxGuests;
          this.showHosting(hosted);
        },
        (err: unknown) => {
          this.showForm(
            `Could not start hosting: ${err instanceof Error ? err.message : String(err)}`,
          );
        },
      );
    });
    const title = document.createElement('h2');
    title.textContent = 'Host';
    this.element.replaceChildren(
      title,
      labelled('Guests', guests),
      labelled('Who can build', edits),
      labelled('Who can fly', flight),
      labelled('Who can see it', visibility),
      this.status,
      start,
    );
  }

  private showHosting(hosted: Hosted): void {
    const title = document.createElement('h2');
    title.textContent = 'Hosting';
    const code = document.createElement('p');
    code.id = 'host-code';
    code.className = 'host-code';
    code.textContent = hosted.display;
    const link = document.createElement('input');
    link.id = 'host-link';
    link.readOnly = true;
    link.value = hosted.link;
    const copy = actionButton('Copy invite link', 'host-copy', () => {
      navigator.clipboard.writeText(hosted.link).then(
        () => {
          copy.textContent = 'Copied';
        },
        () => {
          link.select();
          copy.textContent = 'Select and copy';
        },
      );
      setTimeout(() => {
        copy.textContent = 'Copy invite link';
      }, 1500);
    });
    const qr = document.createElement('canvas');
    qr.className = 'host-qr';
    qr.setAttribute('aria-label', 'QR code of the invite link');
    drawQr(qr, hosted.link);
    const guests = document.createElement('p');
    guests.id = 'host-guests';
    guests.textContent = guestsText(this.guests, this.max);
    this.status.textContent = '';
    const stop = actionButton('Stop hosting', 'host-stop', () => {
      this.deps.stop();
      this.guests = 0;
      this.showForm('Stopped hosting: your guests were disconnected.');
    });
    this.element.replaceChildren(title, code, link, copy, qr, guests, this.status, stop);
  }
}
