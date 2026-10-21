import { describe, expect, it } from 'vitest';
import { mediaBase } from './media.ts';

describe('media base', () => {
  it('is the site root for a version directory, else the build its own', () => {
    expect(mediaBase('/dwell/v/0.25.0/')).toBe('/dwell/media/');
    expect(mediaBase('/dwell/v/0.25.0-dev.3/')).toBe('/dwell/media/');
    expect(mediaBase('/dwell/')).toBe('/dwell/media/');
    expect(mediaBase('/')).toBe('/media/');
  });
});
