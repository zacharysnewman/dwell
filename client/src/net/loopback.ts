import type { FromWorker, ToWorker } from '../local/messages';
import type { LocalWorld } from '../local/world';
import { Channel, TransportKind } from '../protocol/constants.gen';
import type { Transport, TransportHandlers } from './Transport';

const SESSION = 1;

/**
 * Transport to the integrated server running in a local-mode worker (ARCHITECTURE.md §8.1).
 * Carries the same protocol bytes as the network transports.
 */
export class LoopbackTransport implements Transport {
  readonly kind = TransportKind.Loopback;
  readonly binding = new Uint8Array(32);
  private handlers: TransportHandlers | null = null;
  private closed = false;

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
          transport.post({ t: 'connect', session: SESSION, binding: transport.binding });
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

  close(): void {
    if (this.closed) return;
    this.post({ t: 'disconnect', session: SESSION });
    this.finish('closed');
  }

  private post(msg: ToWorker): void {
    this.worker.postMessage(msg);
  }

  private finish(message: string): void {
    this.closed = true;
    this.handlers?.onClose({ message });
    this.handlers = null;
  }

  private onWorkerMessage(msg: FromWorker): void {
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
      case 'ready':
        break;
    }
  }
}
