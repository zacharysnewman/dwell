// Messages between the main thread and the local-mode worker (LoopbackTransport ↔ worker.ts).
import type { Channel } from '../protocol/constants.gen';

export type ToWorker =
  | { t: 'start'; worldSeed: number }
  | { t: 'connect'; session: number; binding: Uint8Array }
  | { t: 'reliable'; session: number; channel: Channel; bytes: Uint8Array }
  | { t: 'datagram'; session: number; bytes: Uint8Array }
  | { t: 'disconnect'; session: number };

export type FromWorker =
  | { t: 'ready' }
  | { t: 'error'; message: string }
  | { t: 'reliable'; session: number; channel: Channel; bytes: Uint8Array }
  | { t: 'datagram'; session: number; bytes: Uint8Array }
  | { t: 'close'; session: number };
