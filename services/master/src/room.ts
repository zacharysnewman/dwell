// The Room Durable Object (ARCHITECTURE.md §10.2–10.3, ADR 0013): one per hosted friend world,
// relaying WebRTC signaling between the host and its guests over hibernating WebSockets. The class
// exists from Phase 5b so its storage migration is in place; the signaling protocol is Phase 5c.
import { DurableObject } from 'cloudflare:workers';
import type { Env } from './env';
import { problem } from './http';

export class Room extends DurableObject<Env> {
  override fetch(): Response {
    return problem(501, 'not_implemented', 'Friend-world signaling is not available yet.');
  }
}
