import { describe, expect, it } from 'vitest';
import { stateId } from '../world/blocks';
import { StructureStore, type StructureDir } from './structureStore';
import { structureFromVoxels, structureText, type StructureJson } from './structures';

function memoryDir(initial: Record<string, string> = {}): StructureDir & {
  files: Map<string, string>;
} {
  const files = new Map(Object.entries(initial));
  return {
    files,
    names: () => Promise.resolve([...files.keys()]),
    read: (f) => Promise.resolve(files.get(f) ?? null),
    write: (f, text) => {
      files.set(f, text);
      return Promise.resolve();
    },
    remove: (f) => {
      files.delete(f);
      return Promise.resolve();
    },
  };
}

const stone = stateId('dwell:stone');

function post(name = 'dwell:post'): StructureJson {
  return structureFromVoxels({ name, size: [1, 2, 1], cells: [stone, stone] });
}

describe('structures store', () => {
  it('saves, lists with sizes, renames and deletes', async () => {
    const dir = memoryDir();
    const store = new StructureStore(dir);
    expect(await store.save(post())).toBe('post.json');
    expect(dir.files.get('post.json')).toBe(structureText(post()));
    const [entry] = await store.list();
    expect(entry?.status).toBe('ok');
    if (entry?.status === 'ok') expect(entry.structure.size).toEqual([1, 2, 1]);
    await expect(store.save(post())).rejects.toThrow('exists');
    expect(await store.rename('post.json', 'dwell:pillar')).toBe('pillar.json');
    expect([...dir.files.keys()]).toEqual(['pillar.json']);
    expect(JSON.parse(dir.files.get('pillar.json') ?? '')).toMatchObject({ name: 'dwell:pillar' });
    await store.remove('pillar.json');
    expect(dir.files.size).toBe(0);
  });

  it('refuses a structure the generator would refuse', async () => {
    const store = new StructureStore(memoryDir());
    await expect(store.save({ ...post(), palette: { s: 'dwell:nope' } })).rejects.toThrow(
      'unknown block "dwell:nope"',
    );
    await expect(store.save({ ...post(), size: [40, 2, 1] })).rejects.toThrow('size');
  });

  // The store is shared by every app version (RELEASES.md §6): append-only.
  it('keeps fields it does not know and files it cannot read', async () => {
    const future = { ...post('dwell:tree'), tags: ['tree'], grows: { stages: 3 } };
    const broken = '{"format": 1, "name": "dwell:broken", ';
    const newer = JSON.stringify({ ...post('dwell:next'), format: 2, curve: [0, 1] });
    const invalid = JSON.stringify({ ...post('dwell:odd'), palette: { s: 'dwell:not_in_this_build' } });
    const dir = memoryDir({
      'tree.json': JSON.stringify(future),
      'broken.json': broken,
      'next.json': newer,
      'odd.json': invalid,
      'notes.txt': 'not a structure',
    });
    const store = new StructureStore(dir);
    const listed = await store.list();
    expect(listed.map((e) => [e.file, e.status])).toEqual([
      ['broken.json', 'unreadable'],
      ['next.json', 'unreadable'],
      ['odd.json', 'unreadable'],
      ['tree.json', 'ok'],
    ]);

    // A rewrite keeps the unknown fields: a rename, then saving over it with new blocks.
    await store.rename('tree.json', 'dwell:oak');
    expect(JSON.parse(dir.files.get('oak.json') ?? '')).toMatchObject({
      tags: ['tree'],
      grows: { stages: 3 },
    });
    const rebuilt = structureFromVoxels({
      name: 'dwell:oak',
      size: [1, 3, 1],
      cells: [stone, stone, stone],
    });
    await store.save(rebuilt, { overwrite: true });
    const saved = JSON.parse(dir.files.get('oak.json') ?? '') as StructureJson;
    expect(saved).toMatchObject({ size: [1, 3, 1], tags: ['tree'], grows: { stages: 3 } });

    // Unreadable files are never rewritten or deleted.
    for (const file of ['broken.json', 'next.json', 'odd.json']) {
      await expect(store.remove(file)).rejects.toThrow('kept');
      await expect(store.rename(file, 'dwell:other')).rejects.toThrow();
    }
    await expect(store.save(post('dwell:broken'), { overwrite: true })).rejects.toThrow('kept');
    await store.remove('oak.json');
    expect(dir.files.get('broken.json')).toBe(broken);
    expect(dir.files.get('next.json')).toBe(newer);
    expect(dir.files.get('odd.json')).toBe(invalid);
    expect(dir.files.get('notes.txt')).toBe('not a structure');
    // They still export as they are.
    expect(await store.exportText('next.json')).toBe(newer);
  });

  it('imports with the fields of the imported file', async () => {
    const dir = memoryDir();
    const store = new StructureStore(dir);
    await store.save({ ...post(), tags: ['rock'] });
    expect(JSON.parse(dir.files.get('post.json') ?? '')).toMatchObject({ tags: ['rock'] });
  });
});
