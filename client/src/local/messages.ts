// Messages between the main thread and the local-mode worker (LoopbackTransport ↔ worker.ts).
import type { Channel, HostState, TransportKind } from '../protocol/constants.gen';
import type { HostPolicy } from './wasmCore';

export type ToWorker =
  | { t: 'start'; worldSeed: number; generatorVersion: number; file?: string }
  | { t: 'connect'; session: number; kind: TransportKind; binding: Uint8Array }
  | { t: 'reliable'; session: number; channel: Channel; bytes: Uint8Array }
  | { t: 'datagram'; session: number; bytes: Uint8Array }
  | { t: 'disconnect'; session: number }
  /** Save the world now (the page is being hidden or closed); answered with `saved`. */
  | { t: 'save' }
  /** Friend-world hosting (§10.2): start hosting with these limits and policies. */
  | {
      t: 'host';
      maxPlayers: number;
      edits: HostPolicy;
      flight: HostPolicy;
      hostKey: Uint8Array;
    }
  /** The host's page was hidden or shown: pause or resume the world and tell the guests. */
  | { t: 'hostStatus'; state: HostState }
  /** Stop hosting: end the guests' sessions (Reject(ServerClosing)). */
  | { t: 'closeGuests' };

export type FromWorker =
  /** `persisted`: the world is saved in the browser (OPFS, §6.4). */
  | { t: 'ready'; persisted: boolean }
  | { t: 'error'; message: string }
  /** The world was saved (in reply to `save`; `ok` is false if it could not be). */
  | { t: 'saved'; ok: boolean }
  | { t: 'reliable'; session: number; channel: Channel; bytes: Uint8Array }
  | { t: 'datagram'; session: number; bytes: Uint8Array }
  | { t: 'close'; session: number };
