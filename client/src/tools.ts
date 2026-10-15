// What other pages load from this build, at a fixed name: `<build>/tools.js` (RELEASES.md §5). The
// launcher's menu has no WASM of its own; its server browser pings dedicated servers with the
// latest build's networking.
import { loadWasm, pingServer } from './engine/wasm';
import type { PingServer } from './ui/menuPage';

/** A dedicated server's round trip, ms, through this build's client. */
export const ping: PingServer = async (url, certHash, rtc, timeoutMs) => {
  await loadWasm();
  return pingServer(url, certHash, rtc, timeoutMs);
};
