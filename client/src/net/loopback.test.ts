import { describe, expect, it } from 'vitest';
import type { FromWorker, ToWorker } from '../local/messages';
import { Channel, TransportKind } from '../protocol/constants.gen';
import { LOCAL_SESSION, LoopbackTransport, type GuestOutput } from './loopback';

/** Stands in for the local-mode worker: records what it's sent, answers on request. */
class FakeWorker {
  readonly sent: ToWorker[] = [];
  onmessage: ((e: MessageEvent<FromWorker>) => void) | null = null;
  onerror: ((e: ErrorEvent) => void) | null = null;
  postMessage(msg: ToWorker): void {
    this.sent.push(msg);
  }
  reply(msg: FromWorker): void {
    this.onmessage?.({ data: msg } as MessageEvent<FromWorker>);
  }
}

async function started(): Promise<{ worker: FakeWorker; transport: LoopbackTransport }> {
  const worker = new FakeWorker();
  const pending = LoopbackTransport.start(worker as unknown as Worker, {
    worldSeed: 7,
    generatorVersion: 0,
    file: 'wabc',
  });
  worker.reply({ t: 'ready', persisted: true });
  return { worker, transport: await pending };
}

describe('loopback transport', () => {
  it('opens the chosen world file', async () => {
    const { worker } = await started();
    expect(worker.sent[0]).toEqual({ t: 'start', worldSeed: 7, generatorVersion: 0, file: 'wabc' });
  });

  it('resolves each save when the worker reports it, in order', async () => {
    const { worker, transport } = await started();
    const first = transport.save();
    const second = transport.save();
    expect(worker.sent.filter((m) => m.t === 'save')).toHaveLength(2);
    worker.reply({ t: 'saved', ok: true });
    worker.reply({ t: 'saved', ok: false });
    expect(await first).toBe(true);
    expect(await second).toBe(false);
  });

  it('switches the game mode through the worker and keeps the mode it reports', async () => {
    const worker = new FakeWorker();
    const pending = LoopbackTransport.start(worker as unknown as Worker, {
      worldSeed: 7,
      generatorVersion: 0,
    });
    worker.reply({ t: 'ready', persisted: true, mode: 'creative' });
    const transport = await pending;
    expect(transport.mode).toBe('creative');
    const switched = transport.setMode('adventure');
    expect(worker.sent).toContainEqual({ t: 'setMode', mode: 'adventure' });
    worker.reply({ t: 'mode', mode: 'adventure' });
    expect(await switched).toBe('adventure');
    expect(transport.mode).toBe('adventure');
  });

  it('answers pending and later saves with false once closed', async () => {
    const { worker, transport } = await started();
    const pending = transport.save();
    worker.reply({ t: 'close', session: 1 });
    expect(await pending).toBe(false);
    expect(await transport.save()).toBe(false);
  });

  it('joins as the local session and hands other sessions’ output to the hosting relay', async () => {
    const { worker, transport } = await started();
    expect(worker.sent[1]).toMatchObject({
      t: 'connect',
      session: LOCAL_SESSION,
      kind: TransportKind.Loopback,
    });
    const mine: number[] = [];
    const guests: GuestOutput[] = [];
    transport.setHandlers({
      onReliable: (_, bytes) => mine.push(bytes[0] ?? -1),
      onDatagram: () => undefined,
      onClose: () => undefined,
    });
    transport.onGuestOutput = (msg) => guests.push(msg);
    worker.reply({
      t: 'reliable',
      session: LOCAL_SESSION,
      channel: Channel.control,
      bytes: Uint8Array.of(1),
    });
    worker.reply({ t: 'reliable', session: 5, channel: Channel.world, bytes: Uint8Array.of(2) });
    // A guest's session closing doesn't close the host's transport.
    worker.reply({ t: 'close', session: 5 });
    expect(mine).toEqual([1]);
    expect(guests.map((m) => m.t)).toEqual(['reliable', 'close']);
    expect(await Promise.race([transport.save(), Promise.resolve('open')])).toBe('open');
  });
});

describe('the local world start (§6.3)', () => {
  it('starts the worker before the maps are shaped, and sends them when they are', async () => {
    // Before, the page waited for the maps and only then started the worker: its world file and
    // WASM waited too. Now it starts at once and the maps follow.
    const worker = new FakeWorker();
    let shaped: (m: Uint8Array[] | undefined) => void = () => undefined;
    const maps = new Promise<Uint8Array[] | undefined>((r) => (shaped = r));
    const stages: string[] = [];
    const pending = LoopbackTransport.start(
      worker as unknown as Worker,
      { worldSeed: 7, generatorVersion: 8 },
      (id, phase) => stages.push(`${phase} ${id}`),
      maps,
    );
    expect(worker.sent).toEqual([
      { t: 'start', worldSeed: 7, generatorVersion: 8, mapsLater: true },
    ]);
    worker.reply({ t: 'stage', id: 'worldFile', phase: 'begin', atMs: performance.timeOrigin + 1 });
    worker.reply({ t: 'stage', id: 'worldFile', phase: 'end', atMs: performance.timeOrigin + 2 });
    const bytes = [new Uint8Array([1]), new Uint8Array([2])];
    shaped(bytes);
    await maps;
    await Promise.resolve();
    expect(worker.sent[1]).toEqual({ t: 'maps', maps: bytes });
    worker.reply({ t: 'ready', persisted: true });
    await pending;
    expect(stages).toEqual(['begin worldFile', 'end worldFile']);
  });

  it('sends no maps (the server computes its own) when there are none', async () => {
    const worker = new FakeWorker();
    const none = Promise.resolve(undefined);
    void LoopbackTransport.start(
      worker as unknown as Worker,
      { worldSeed: 1, generatorVersion: 0 },
      undefined,
      none,
    );
    await none;
    await Promise.resolve();
    expect(worker.sent[1]).toEqual({ t: 'maps', maps: [] });
  });
});
