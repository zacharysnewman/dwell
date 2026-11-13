import { describe, expect, it, vi } from 'vitest';
import { InGameEdit, debounce, editOfWorld, sliderSpecs } from './inGame';
import { buildEdit, fromJson } from './state';

describe('Chisel in the game', () => {
  it('opens the build world, the starter world and a JSON edit, and refuses other fixtures', () => {
    expect(editOfWorld('')?.biomes.map((b) => b.name)).toEqual(['dwell:default']);
    expect(editOfWorld('test:starter')?.biomes.length).toBeGreaterThan(1);
    const e = buildEdit();
    expect(editOfWorld(JSON.stringify(e))?.biomes[0]?.name).toBe('dwell:default');
    expect(editOfWorld('test:tall_peak')).toBeNull();
    expect(editOfWorld('{"biomes": []}')).toBeNull();
  });

  it('a slider change makes a valid config text, undone and reset', () => {
    const edit = new InGameEdit(buildEdit());
    const before = edit.text();
    expect(edit.set(0, 'detail', 'amplitude', 9)).toEqual([]);
    const after = edit.text();
    expect(after).not.toBe(before);
    const parsed = fromJson(after);
    expect('edit' in parsed && parsed.edit.biomes[0]?.detail).toMatchObject({ amplitude: 9 });
    expect(edit.set(0, 'detail', 'amplitude', 12)).toEqual([]);
    expect(edit.undo()).toBe(true);
    expect(edit.group(0, 'detail').amplitude).toBe(9);
    edit.reset();
    expect(edit.text()).toBe(before);
    expect(edit.undo()).toBe(true); // the reset itself is undoable
    expect(edit.group(0, 'detail').amplitude).toBe(9);
  });

  it('a value the validation refuses is taken back with its message', () => {
    const edit = new InGameEdit(buildEdit());
    const before = edit.text();
    const errors = edit.set(0, 'detail', 'amplitude', -5);
    expect(errors.length).toBeGreaterThan(0);
    expect(edit.text()).toBe(before);
    expect(edit.canUndo()).toBe(false);
  });

  it('sliders come from the validation ranges', () => {
    const specs = sliderSpecs('detail', { amplitude: 6, octaves: 3 });
    const amp = specs.find((s) => s.key === 'amplitude');
    expect(amp).toMatchObject({ min: 0, max: 24, whole: false });
    expect(specs.find((s) => s.key === 'octaves')).toMatchObject({ step: 1, whole: true });
  });

  it('debounces a drag into one retune with the last value', () => {
    vi.useFakeTimers();
    const sent: string[] = [];
    const poke = debounce((v: string) => sent.push(v), 400);
    poke('a');
    vi.advanceTimersByTime(200);
    poke('b');
    vi.advanceTimersByTime(399);
    expect(sent).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(sent).toEqual(['b']);
    vi.useRealTimers();
  });
});
