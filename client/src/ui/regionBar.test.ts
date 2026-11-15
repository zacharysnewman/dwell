// The touch region bar's data and rules (its DOM is e2e/touch.spec.ts's: this suite has no DOM).
import { describe, expect, it } from 'vitest';
import data from '../../../shared/input/touchRegion.json';
import {
  REGION_BAR,
  barStateOf,
  buttonsIn,
  regionBarShown,
  touchStatus,
  validateRegionBar,
} from './regionBar';

const creative = (a: string) => a === 'structures' || a === 'flight';
const adventure = (a: string) => a === 'damage';

describe('the touch region bar', () => {
  it('shows only while flying in creative (the data says so)', () => {
    const rule = REGION_BAR.showWhen;
    expect(rule).toEqual({ flying: true, abilities: ['structures'] });
    const at = (flying: boolean, allows: (a: string) => boolean, enabled = true) =>
      regionBarShown(rule, { flying, allows, enabled });
    expect(at(true, creative)).toBe(true);
    expect(at(false, creative)).toBe(false);
    expect(at(true, adventure)).toBe(false);
    expect(at(false, adventure)).toBe(false);
    // A server that takes no region edits.
    expect(at(true, creative, false)).toBe(false);
    // The rule is data: without `flying` it shows on the ground too.
    expect(
      regionBarShown(
        { flying: false, abilities: ['structures'] },
        { flying: false, allows: creative, enabled: true },
      ),
    ).toBe(true);
  });

  it("shows the buttons for the tools' state: choosing a box, or paste mode", () => {
    expect(barStateOf([1, 0, 1])).toBe('select');
    expect(barStateOf([1, 1, 0])).toBe('paste');
    expect(barStateOf([])).toBe('select');
    const labels = (st: 'select' | 'paste') => buttonsIn(REGION_BAR, st).map((b) => b.label);
    expect(labels('select')).toEqual(['Set A', 'Set B', 'Copy', 'Paste', 'Undo', 'Save', 'Clear']);
    expect(labels('paste')).toEqual(['Place', 'Rotate', 'Mirror', 'Cancel', 'Undo']);
    expect(buttonsIn(REGION_BAR, 'select').map((b) => b.run)).toEqual([
      'tool:cornerA',
      'tool:cornerB',
      'tool:copy',
      'tool:pasteMode',
      'tool:undo',
      'save',
      'tool:deselect',
    ]);
  });

  it("shows the tools' line without the keyboard's hints", () => {
    expect(
      touchStatus('Paste 3×1×1 (90°, mirrored) · R rotate · M mirror · right click paste'),
    ).toBe('Paste 3×1×1 (90°, mirrored)');
    expect(touchStatus('Selection 2×1×1 (2 blocks) · Copied 2×1×1')).toBe(
      'Selection 2×1×1 (2 blocks) · Copied 2×1×1',
    );
    expect(touchStatus('Select a box first (B, N)')).toBe('Select a box first');
    expect(touchStatus('Copy a box first (Ctrl+C)')).toBe('Copy a box first');
  });

  it('refuses bad data, naming the entry, and takes a new button by data alone', () => {
    const base = structuredClone(data) as unknown as { buttons: Record<string, unknown>[] };
    const withButton = (b: Record<string, unknown>) => ({
      ...base,
      buttons: [...base.buttons, b],
    });
    expect(() =>
      validateRegionBar(withButton({ id: 'x', label: 'X', run: 'tool:explode', in: ['select'] })),
    ).toThrow(/buttons\[11\]\.run: unknown action "tool:explode"/);
    expect(() =>
      validateRegionBar(withButton({ id: 'region-copy', label: 'X', run: 'save', in: ['select'] })),
    ).toThrow(/region-copy is used twice/);
    expect(() =>
      validateRegionBar(withButton({ id: 'y', label: 'Y', run: 'save', in: ['flying'] })),
    ).toThrow(/unknown state "flying"/);
    expect(() =>
      validateRegionBar({ ...base, showWhen: { flying: true, abilities: ['wings'] } }),
    ).toThrow(/unknown ability "wings"/);
    // A fixture entry by data alone: a Fill button in the bar.
    const fill = validateRegionBar(
      withButton({ id: 'region-fill', label: 'Fill', run: 'tool:fill', in: ['select'] }),
    );
    expect(buttonsIn(fill, 'select').at(-1)).toEqual({
      id: 'region-fill',
      label: 'Fill',
      run: 'tool:fill',
      in: ['select'],
    });
    expect(buttonsIn(fill, 'paste').map((b) => b.id)).not.toContain('region-fill');
  });
});
