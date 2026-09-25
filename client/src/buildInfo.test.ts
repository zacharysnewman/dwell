import { describe, expect, it } from 'vitest';
import { formatBuildInfo } from './buildInfo';

describe('formatBuildInfo', () => {
  it('shortens a commit SHA and keeps the date', () => {
    expect(
      formatBuildInfo({
        sha: '1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b',
        time: '2026-09-25T12:00:00.000Z',
      }),
    ).toBe('dwell 1a2b3c4 · 2026-09-25');
  });

  it('leaves a non-SHA value unchanged', () => {
    expect(formatBuildInfo({ sha: 'unknown', time: '2026-09-25T12:00:00.000Z' })).toBe(
      'dwell unknown · 2026-09-25',
    );
  });
});
