// Local-mode host (ARCHITECTURE.md §2.1): runs the WASM server core in a worker and exchanges
// protocol bytes with the main thread's LoopbackTransport.
import { TransportKind } from '../protocol/constants.gen';
import type { FromWorker, ToWorker } from './messages';
import { importDwellCore } from '../sim/module';
import { LocalCore, OutgoingKind } from './wasmCore';

interface WorkerScope {
  postMessage(message: FromWorker, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent<ToWorker>) => void) | null;
}
const scope = self as unknown as WorkerScope;

let core: LocalCore | null = null;
const queued: ToWorker[] = [];

function flush(): void {
  if (!core) return;
  for (const out of core.takeOutbox()) {
    switch (out.kind) {
      case OutgoingKind.Reliable:
        scope.postMessage(
          { t: 'reliable', session: out.session, channel: out.channel, bytes: out.bytes },
          [out.bytes.buffer],
        );
        break;
      case OutgoingKind.Datagram:
        scope.postMessage({ t: 'datagram', session: out.session, bytes: out.bytes }, [
          out.bytes.buffer,
        ]);
        break;
      case OutgoingKind.Close:
        scope.postMessage({ t: 'close', session: out.session });
        break;
    }
  }
}

function handle(msg: ToWorker): void {
  if (!core) {
    queued.push(msg);
    return;
  }
  switch (msg.t) {
    case 'start':
      break;
    case 'connect':
      core.connected(msg.session, TransportKind.Loopback, msg.binding);
      break;
    case 'reliable':
      core.reliable(msg.session, msg.channel, msg.bytes);
      break;
    case 'datagram':
      core.datagram(msg.session, msg.bytes);
      break;
    case 'disconnect':
      core.disconnected(msg.session);
      break;
  }
  flush();
}

async function start(worldSeed: number, generatorVersion: number): Promise<void> {
  try {
    core = await LocalCore.load(await importDwellCore(), worldSeed, generatorVersion);
  } catch (err) {
    scope.postMessage({
      t: 'error',
      message: `Local world unavailable (${err instanceof Error ? err.message : String(err)}).`,
    });
    return;
  }
  scope.postMessage({ t: 'ready' });
  for (const msg of queued.splice(0)) handle(msg);

  // Fixed-step simulation clock (the core accumulates real time into 60 Hz steps).
  let last = performance.now();
  setInterval(() => {
    const now = performance.now();
    core?.advance((now - last) / 1000);
    last = now;
    flush();
  }, 4);
}

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg.t === 'start') void start(msg.worldSeed, msg.generatorVersion);
  else handle(msg);
};
