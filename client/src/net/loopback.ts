import type { FromWorker, ServerStats, ToWorker } from '../local/messages';
import type { LocalWorld } from '../local/world';
import { Channel, TransportKind } from '../protocol/constants.gen';
import type { Transport, TransportHandlers } from './Transport';

/** The local player's session; friend-world guests get others (net/hosting.ts). */
export const LOCAL_SESSION = 1;
const SESSION = LOCAL_SESSION;

/** Worker output for a session other than the local player's (a friend-world guest). */
export type GuestOutput = Extract<FromWorker, { session: number }>;

/** A loading stage of the local world's start began or ended (`atMs` on the page's clock). */
export type StageReport = (id: string, phase: 'begin' | 'end', atMs: number) => void;

/**
 * What the player reads when the local world's worker failed (BUGS.md #91: a 10× world's server
 * ran out of WebAssembly memory and the page said only "Could not connect"): a WebAssembly trap or
 * allocation failure is named as the server running out of memory, with the browser's words kept.
 */
export function workerFailure(message: string): string {
  const m = message.trim() || 'it stopped without saying why';
  const memory =
    /out of memory|memory access out of bounds|unreachable|could not allocate|capacity overflow|allocation failed|Maximum call stack|WebAssembly\.Memory/i;
  return memory.test(m)
    ? `The world's server ran out of memory and stopped (${m}). A smaller world size may fit.`
    : `The world's server stopped (${m}).`;
}

/**
 * Transport to the integrated server running in a local-mode worker (ARCHITECTURE.md §8.1).
 * Carries the same protocol bytes as the network transports.
 */
export class LoopbackTransport implements Transport {
  readonly kind = TransportKind.Loopback;
  readonly binding = new Uint8Array(32);
  private handlers: TransportHandlers | null = null;
  private closed = false;
  /** Output for other sessions: the hosting relay's (§10.2). */
  onGuestOutput: ((msg: GuestOutput) => void) | null = null;
  /** The local server core's WebAssembly memory (bytes), as the worker last reported it. */
  heapBytes = 0;
  /** The local server's figures, as the worker last reported them (every second). */
  serverStats: ServerStats | null = null;
  /** The local server's last second: its ticks, and their mean time (ms; null before a tick). */
  serverTicks: number | null = null;
  serverTickMs: number | null = null;
  /** The world's game mode (ADR 0065) as the local server last said ('' before it did). */
  mode = '';
  private readonly modeWaits: ((mode: string) => void)[] = [];
  private readonly retuneWaits: ((r: { maps: Uint8Array[]; error?: string }) => void)[] = [];
  /** Callers waiting for a save to finish, oldest first (the worker answers in order). */
  private readonly saving: ((ok: boolean) => void)[] = [];
  private readonly reports: ((report: string | null) => void)[] = [];
  private readonly mapFinds: ((result: string | null) => void)[] = [];
  private readonly lavaAnswers: ((result: string | null) => void)[] = [];

  private constructor(private readonly worker: Worker) {
    worker.onmessage = (e: MessageEvent<FromWorker>) => {
      this.onWorkerMessage(e.data);
    };
    // A worker that dies once playing (out of memory) closes the session with why, not silently.
    worker.onerror = (e) => {
      if (!this.closed) this.finish(workerFailure(e.message));
    };
  }

  /**
   * Starts the worker and resolves once the WASM core is loaded. `onStage`: the start's loading
   * stages as the worker reports them (§6.3), `atMs` on this page's clock. `maps`: the world's
   * maps still on the way; the worker starts at once (its world file, its WASM) and waits for them.
   */
  static start(
    worker: Worker,
    world: LocalWorld & {
      maps?: Uint8Array[];
      waterInline?: boolean;
      jobsInline?: boolean;
      growth?: number;
      chop2?: boolean;
    },
    onStage?: StageReport,
    maps?: Promise<Uint8Array[] | undefined>,
  ): Promise<LoopbackTransport> {
    return new Promise((resolve, reject) => {
      worker.onmessage = (e: MessageEvent<FromWorker>) => {
        if (e.data.t === 'stage') {
          onStage?.(e.data.id, e.data.phase, e.data.atMs - performance.timeOrigin);
        } else if (e.data.t === 'ready') {
          const transport = new LoopbackTransport(worker);
          transport.mode = e.data.mode ?? '';
          transport.post({
            t: 'connect',
            session: SESSION,
            kind: TransportKind.Loopback,
            binding: transport.binding,
          });
          resolve(transport);
        } else if (e.data.t === 'error') {
          reject(new Error(e.data.message));
        }
      };
      worker.onerror = (e) => {
        reject(new Error(workerFailure(e.message)));
      };
      worker.postMessage({
        t: 'start',
        ...world,
        ...(maps ? { mapsLater: true } : {}),
      } satisfies ToWorker);
      void maps?.then((m) => {
        worker.postMessage({ t: 'maps', maps: m ?? [] } satisfies ToWorker);
      });
    });
  }

  setHandlers(handlers: TransportHandlers): void {
    this.handlers = handlers;
  }

  sendReliable(channel: Channel, bytes: Uint8Array): void {
    if (!this.closed) this.post({ t: 'reliable', session: SESSION, channel, bytes });
  }

  sendDatagram(bytes: Uint8Array): void {
    if (!this.closed) this.post({ t: 'datagram', session: SESSION, bytes });
  }

  /**
   * Asks the local server to save the world now (§6.4); resolves with whether the save committed
   * (false once the transport is closed).
   */
  save(): Promise<boolean> {
    if (this.closed) return Promise.resolve(false);
    return new Promise((resolve) => {
      this.saving.push(resolve);
      this.post({ t: 'save' });
    });
  }

  /**
   * The water near a cell as the local server has it (JSON, a diagnostic for the block dump);
   * null once the transport is closed.
   */
  waterReport(at: [number, number, number], radius: number): Promise<string | null> {
    if (this.closed) return Promise.resolve(null);
    return new Promise((resolve) => {
      this.reports.push(resolve);
      this.post({ t: 'waterReport', at, radius });
    });
  }

  /**
   * The world map's nearest water as the local server finds it (JSON or `null`, local.rs
   * `find_on_map`; the dev tools' `goto`); null once the transport is closed.
   */
  findOnMap(
    kind: string,
    at: [number, number, number],
    radius: number,
    face: number,
    minDepth: number,
  ): Promise<string | null> {
    if (this.closed) return Promise.resolve(null);
    return new Promise((resolve) => {
      this.mapFinds.push(resolve);
      this.post({ t: 'findOnMap', kind, at, radius, face, minDepth });
    });
  }

  /** The lava's dev tools (local.rs `lava_dev`; JSON); null once the transport is closed. */
  lavaDev(op: string, args: number[]): Promise<string | null> {
    if (this.closed) return Promise.resolve(null);
    return new Promise((resolve) => {
      this.lavaAnswers.push(resolve);
      this.post({ t: 'lavaDev', op, args });
    });
  }

  /**
   * The host switches the world's game mode (ADR 0065); resolves with the mode the world is in
   * after it (the old one if the name was refused, or once the transport is closed).
   */
  setMode(name: string): Promise<string> {
    if (this.closed) return Promise.resolve(this.mode);
    return new Promise((resolve) => {
      this.modeWaits.push(resolve);
      this.post({ t: 'setMode', mode: name });
    });
  }

  /**
   * The host retunes the world's worldgen config (Chisel in the game, Phase 56 B7): resolves with
   * faces A's and B's new maps (for the page to install), or the refusal.
   */
  retuneWorldgen(
    worldgen: string,
    radius: number,
  ): Promise<{ maps: Uint8Array[]; error?: string }> {
    if (this.closed) return Promise.resolve({ maps: [], error: 'the world is closed' });
    return new Promise((resolve) => {
      this.retuneWaits.push(resolve);
      this.post({ t: 'retune', worldgen, radius });
    });
  }

  close(): void {
    if (this.closed) return;
    this.post({ t: 'disconnect', session: SESSION });
    this.finish('closed');
  }

  private post(msg: ToWorker): void {
    this.worker.postMessage(msg);
  }

  /** Sends a message to the local server (the hosting relay: guests' sessions and controls). */
  postToWorker(msg: ToWorker): void {
    if (!this.closed) this.post(msg);
  }

  private finish(message: string): void {
    this.closed = true;
    for (const resolve of this.saving.splice(0)) resolve(false);
    for (const resolve of this.reports.splice(0)) resolve(null);
    for (const resolve of this.mapFinds.splice(0)) resolve(null);
    for (const resolve of this.lavaAnswers.splice(0)) resolve(null);
    for (const resolve of this.modeWaits.splice(0)) resolve(this.mode);
    for (const resolve of this.retuneWaits.splice(0)) resolve({ maps: [], error: message });
    this.handlers?.onClose({ message });
    this.handlers = null;
  }

  private onWorkerMessage(msg: FromWorker): void {
    if ('session' in msg && msg.session !== SESSION) {
      this.onGuestOutput?.(msg);
      return;
    }
    switch (msg.t) {
      case 'reliable':
        this.handlers?.onReliable(msg.channel, msg.bytes);
        break;
      case 'datagram':
        this.handlers?.onDatagram(msg.bytes);
        break;
      case 'close':
        this.finish('local world closed the session');
        break;
      case 'error':
        this.finish(msg.message);
        break;
      case 'saved':
        this.saving.shift()?.(msg.ok);
        break;
      case 'mode':
        this.mode = msg.mode;
        this.modeWaits.shift()?.(msg.mode);
        break;
      case 'retuned':
        this.retuneWaits.shift()?.(msg);
        break;
      case 'waterReport':
        this.reports.shift()?.(msg.report);
        break;
      case 'findOnMap':
        this.mapFinds.shift()?.(msg.result);
        break;
      case 'lavaDev':
        this.lavaAnswers.shift()?.(msg.result);
        break;
      case 'memory':
        this.heapBytes = msg.bytes;
        break;
      case 'stats':
        this.serverStats = msg.stats;
        break;
      case 'perf':
        this.serverTicks = msg.ticks;
        this.serverTickMs = msg.ticks > 0 ? msg.busyMs / msg.ticks : null;
        break;
      case 'ready':
      case 'stage':
        break;
    }
  }
}
