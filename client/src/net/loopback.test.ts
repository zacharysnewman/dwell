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
