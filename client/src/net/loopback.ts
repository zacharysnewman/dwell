import type { FromWorker, ToWorker } from '../local/messages';
import type { LocalWorld } from '../local/world';
import { Channel, TransportKind } from '../protocol/constants.gen';
import type { Transport, TransportHandlers } from './Transport';

/** The local player's session; friend-world guests get others (net/hosting.ts). */
export const LOCAL_SESSION = 1;
const SESSION = LOCAL_SESSION;

/** Worker output for a session other than the local player's (a friend-world guest). */
export type GuestOutput = Extract<FromWorker, { session: number }>;

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
  /** Callers waiting for a save to finish, oldest first (the worker answers in order). */
  private readonly saving: ((ok: boolean) => void)[] = [];

  private constructor(private readonly worker: Worker) {
    worker.onmessage = (e: MessageEvent<FromWorker>) => {
      this.onWorkerMessage(e.data);
    };
  }

  /** Starts the worker and resolves once the WASM core is loaded. */
  static start(worker: Worker, world: LocalWorld): Promise<LoopbackTransport> {
    return new Promise((resolve, reject) => {
      worker.onmessage = (e: MessageEvent<FromWorker>) => {
        if (e.data.t === 'ready') {
          const transport = new LoopbackTransport(worker);
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
        reject(new Error(e.message || 'local world worker failed'));
      };
      worker.postMessage({ t: 'start', ...world } satisfies ToWorker);
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
      case 'memory':
        this.heapBytes = msg.bytes;
        break;
      case 'ready':
        break;
    }
  }
}
