// API constants. Kept out of index.ts: a Worker's main module may export only handlers and
// Durable Object classes.

export const API_VERSION = 1;
/** Largest request body the master reads. */
export const MAX_BODY_BYTES = 16 * 1024;
