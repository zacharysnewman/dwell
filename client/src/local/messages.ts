// Messages between the main thread and the local-mode worker (LoopbackTransport ↔ worker.ts).
import type { Channel } from '../protocol/constants.gen';

export type ToWorker =
  | { t: 'start'; worldSeed: number; generatorVersion: number; file?: string }
  | { t: 'connect'; session: number; binding: Uint8Array }
  | { t: 'reliable'; session: number; channel: Channel; bytes: Uint8Array }
  | { t: 'datagram'; session: number; bytes: Uint8Array }
  | { t: 'disconnect'; session: number }
  /** Save the world now (the page is being hidden or closed); answered with `saved`. */
  | { t: 'save' };

export type FromWorker =
  /** `persisted`: the world is saved in the browser (OPFS, §6.4). */
  | { t: 'ready'; persisted: boolean }
  | { t: 'error'; message: string }
  /** The world was saved (in reply to `save`; `ok` is false if it could not be). */
  | { t: 'saved'; ok: boolean }
  | { t: 'reliable'; session: number; channel: Channel; bytes: Uint8Array }
  | { t: 'datagram'; session: number; bytes: Uint8Array }
  | { t: 'close'; session: number };
