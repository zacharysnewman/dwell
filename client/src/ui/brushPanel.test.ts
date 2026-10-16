import { describe, expect, it } from 'vitest';
import { BRUSH_HINT, brushRows, type BrushPanelState } from './brushPanel';

const state: BrushPanelState = {
  preset: 'Ball',
  brush: 'Ball',
  performer: 'Set',
  size: 3,
  material: 'sandbox_floor',
  replace: null,
  note: '',
};

describe('brush panel', () => {
  it('shows the brush, performer, size and block, and the replace target only when used', () => {
    expect(brushRows(state)).toEqual([
      ['Brush', 'Ball'],
      ['Performer', 'Set'],
      ['Size', '3'],
      ['Block', 'sandbox floor'],
    ]);
    const replace = brushRows({ ...state, performer: 'Replace', replace: 'sand' });
    expect(replace.at(-1)).toEqual(['Replace', 'sand']);
  });

  it('hints the keys', () => {
    for (const k of ['Q E', 'Z', 'P', '[ ]', 'V']) expect(BRUSH_HINT).toContain(k);
  });
});
