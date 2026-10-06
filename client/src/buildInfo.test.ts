import { describe, expect, it } from 'vitest';
import { channelOf, formatBuildInfo, recordedVersion } from './buildInfo';

describe('formatBuildInfo', () => {
  it('shows the version, a shortened commit SHA and the date', () => {
    expect(
      formatBuildInfo({
        version: '0.1.0',
        sha: '1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b',
        time: '2026-09-25T12:00:00.000Z',
      }),
    ).toBe('dwell 0.1.0 · 1a2b3c4 · 2026-09-25');
  });

  it('leaves a non-SHA value unchanged and drops build metadata from the version', () => {
    expect(
      formatBuildInfo({
        version: '0.2.0-dev.42+ab12cd3',
        sha: 'unknown',
        time: '2026-09-25T12:00:00.000Z',
      }),
    ).toBe('dwell 0.2.0-dev.42 · unknown · 2026-09-25');
  });
});

describe('versions of a build', () => {
  it('names the channel', () => {
    expect(channelOf('0.1.0')).toBe('stable');
    expect(channelOf('0.2.0-dev.42+ab12cd3')).toBe('dev');
  });

  it('records a version without its build metadata', () => {
    expect(recordedVersion('0.2.0-dev.42+ab12cd3')).toBe('0.2.0-dev.42');
    expect(recordedVersion('0.1.0')).toBe('0.1.0');
  });
});
