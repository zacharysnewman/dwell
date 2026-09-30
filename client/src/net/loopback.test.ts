import { describe, expect, it } from 'vitest';
import type { FromWorker, ToWorker } from '../local/messages';
import { LoopbackTransport } from './loopback';

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
});
