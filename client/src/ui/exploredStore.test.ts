import { describe, expect, it } from 'vitest';
import { ExploredChanges, exploredRecord, mergeIntoStore } from './exploredStore';
import { MAP } from './map.gen';
import { cellKey, NO_HEIGHT } from './minimap';

/** A memory store that counts the cells each write carried across the page's boundary. */
function memoryStore() {
  const records = new Map<string, unknown>();
  return {
    records,
    get: (w: string) => Promise.resolve(structuredClone(records.get(w))),
    put: (w: string, r: unknown) => {
      records.set(w, structuredClone(r));
      return Promise.resolve();
    },
  };
}

describe('explored saves (0.52.3: a stall every ~10 s on a well-explored world)', () => {
  it('hand the store only the cells changed since the last save, not the world explored', async () => {
    const store = memoryStore();
    const changes = new ExploredChanges();
    for (let i = 0; i < 100_000; i++) changes.note(cellKey(0, i % 300, Math.floor(i / 300)), 7, 3);
    const first = changes.take();
    expect(first.cells.size).toBe(100_000);
    await mergeIntoStore(store, 'w', first.cells, first.heights);
    // One more cell explored: the next save is that cell alone.
    changes.note(cellKey(0, -1, -1), 0x123456, NO_HEIGHT);
    const second = changes.take();
    expect(second.cells.size).toBe(1);
    expect(changes.size).toBe(0);
    await mergeIntoStore(store, 'w', second.cells, second.heights);
    const r = store.records.get('w') as { cells: Record<string, number> };
    expect(Object.keys(r.cells).length).toBe(100_001);
    expect(r.cells[cellKey(0, -1, -1)]).toBe(0x123456);
    expect(r.cells[cellKey(0, 5, 0)]).toBe(7);
  });

  it('merged one save at a time give the record a whole save gives', () => {
    const a = new Map([[cellKey(0, 1, 1), 1]]);
    const b = new Map([[cellKey(1, 2, 2), 2]]);
    const ha = new Map([[cellKey(0, 1, 1), 9]]);
    const whole = exploredRecord(undefined, new Map([...a, ...b]), ha);
    const steps = exploredRecord(exploredRecord(undefined, a, ha), b, new Map());
    expect(steps).toEqual(whole);
  });

  it("keep a record of another cell size's earlier saves under cellsByM", () => {
    const other = { format: 1, cellM: MAP.explored.cellM * 2, cells: { '0,1,1': 5 } };
    const once = exploredRecord(other, new Map([[cellKey(0, 2, 2), 7]]));
    const twice = exploredRecord(once, new Map([[cellKey(0, 3, 3), 8]]));
    expect(twice.cells).toEqual({ '0,1,1': 5 });
    expect(twice.cellsByM).toEqual({
      [String(MAP.explored.cellM)]: { [cellKey(0, 2, 2)]: 7, [cellKey(0, 3, 3)]: 8 },
    });
  });

  it('take back a failed save under the changes noted since', () => {
    const changes = new ExploredChanges();
    changes.note('0,1,1', 1, 4);
    changes.note('0,2,2', 2, NO_HEIGHT);
    const taken = changes.take();
    changes.note('0,1,1', 10, 5);
    changes.restore(taken);
    const again = changes.take();
    expect([...again.cells]).toEqual([
      ['0,1,1', 10],
      ['0,2,2', 2],
    ]);
    expect([...again.heights]).toEqual([['0,1,1', 5]]);
  });
});
