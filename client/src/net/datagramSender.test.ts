import { describe, expect, it } from 'vitest';
import { DatagramSender } from './datagramSender';

/** A writer whose writes complete only when the test says so, like a busy main thread. */
function slowWriter() {
  const written: number[][] = [];
  const pending: (() => void)[] = [];
  const write = (bytes: Uint8Array) =>
    new Promise<void>((resolve) => {
      written.push([...bytes]);
      pending.push(resolve);
    });
  const completeOne = async () => {
    pending.shift()?.();
    await new Promise((r) => setTimeout(r, 0));
  };
  return { written, pending, write, completeOne };
}

describe('datagram sender', () => {
  it('never queues behind a slow writer: only the newest datagram of each type waits', async () => {
    const w = slowWriter();
    const sender = new DatagramSender(w.write);
    // One slow frame's worth of input datagrams (type 1) and a ping (type 2).
    sender.send(new Uint8Array([1, 10]));
    sender.send(new Uint8Array([1, 11]));
    sender.send(new Uint8Array([2, 50]));
    sender.send(new Uint8Array([1, 12]));
    sender.send(new Uint8Array([1, 13]));
    expect(w.written).toEqual([[1, 10]]); // one write in flight
    await w.completeOne();
    expect(w.written).toEqual([
      [1, 10],
      [1, 13],
    ]);
    await w.completeOne();
    expect(w.written).toEqual([
      [1, 10],
      [1, 13],
      [2, 50],
    ]);
    await w.completeOne();
    expect(w.pending).toHaveLength(0);
    // Idle again: the next datagram goes straight out.
    sender.send(new Uint8Array([1, 14]));
    expect(w.written.at(-1)).toEqual([1, 14]);
  });

  it('keeps sending after a failed write', async () => {
    let calls = 0;
    const sender = new DatagramSender(() => {
      calls++;
      return Promise.reject(new Error('closed'));
    });
    sender.send(new Uint8Array([1]));
    await new Promise((r) => setTimeout(r, 0));
    sender.send(new Uint8Array([1]));
    expect(calls).toBe(2);
  });
});
