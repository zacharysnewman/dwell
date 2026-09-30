// Bindings and variables of the master Worker (wrangler.jsonc).
import type { Directory } from './directory';
import type { Room } from './room';

export interface Env {
  DIRECTORY: DurableObjectNamespace<Directory>;
  ROOM: DurableObjectNamespace<Room>;
  /** Comma-separated origins allowed to call the master from a browser. */
  ALLOWED_ORIGINS: string;
  /** Cloudflare TURN key (Worker secrets, Phase 5c manual setup); absent: STUN only. */
  TURN_KEY_ID?: string;
  TURN_KEY_API_TOKEN?: string;
}
