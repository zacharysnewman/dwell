import { describe, expect, it } from 'vitest';
import { checkStructure } from '../local/structures';
import { blueprintCellsOf, blueprintPanelLines, parseBlueprintPanel, rowShort } from './blueprints';

const well = checkStructure(
  {
    format: 1,
    name: 'dwell:my_well',
    kind: 'voxels',
    size: [3, 1, 2],
    anchor: [1, 0, 0],
    palette: { '.': null, s: 'dwell:stone', l: 'dwell:log', _: 'dwell:air' },
    layers: [['s.l', 's_s']],
  },
  'test',
);

describe('blueprints', () => {
  it('turns a library structure into a blueprint record’s cells: air and kept cells keep', () => {
    const { palette, cells } = blueprintCellsOf(well);
    const block = (n: string | undefined) => n?.replace(/\[.*$/, '') ?? null;
    expect(palette.split('\n').map(block).sort()).toEqual(['dwell:log', 'dwell:stone']);
    const at = (i: number) =>
      cells[i] === 0 ? null : block(palette.split('\n')[(cells[i] ?? 1) - 1]);
    expect([0, 1, 2, 3, 4, 5].map(at)).toEqual([
      'dwell:stone',
      null,
      'dwell:log',
      'dwell:stone',
      null,
      'dwell:stone',
    ]);
  });

  it('refuses a structure over a blueprint’s side', () => {
    const big = { ...well, size: [33, 1, 2] as [number, number, number] };
    expect(() => blueprintCellsOf(big)).toThrow(/at most 32/);
  });

  it('reads the client’s panel: name, cells left, have/need per item, folded rows', () => {
    const v = parseBlueprintPanel('dwell:bp_hut\n111\ndwell:log 12 61\ndwell:stone ∞ 50\n+2');
    expect(v).toEqual({
      name: 'dwell:bp_hut',
      left: 111,
      rows: [
        { item: 'dwell:log', have: '12', need: 61 },
        { item: 'dwell:stone', have: '∞', need: 50 },
      ],
      more: 2,
    });
    if (!v) throw new Error('a panel');
    expect(blueprintPanelLines(v)).toEqual([
      'bp_hut · 111 left',
      'log 12/61',
      'stone ∞/50',
      '+2 more',
    ]);
    expect(v.rows.map(rowShort)).toEqual([true, false]);
    expect(parseBlueprintPanel('')).toBeNull();
  });
});
