import { describe, expect, it } from 'vitest';
import { SOUND_DATA } from '../src/audio/sounds.gen.ts';
import { mediaBase, mediaSources } from './media.ts';

describe('media base', () => {
  it('is the site root for a version directory, else the build its own', () => {
    expect(mediaBase('/dwell/v/0.25.0/')).toBe('/dwell/media/');
    expect(mediaBase('/dwell/v/0.25.0-dev.3/')).toBe('/dwell/media/');
    expect(mediaBase('/dwell/')).toBe('/dwell/media/');
    expect(mediaBase('/')).toBe('/media/');
  });
});

describe('media files', () => {
  it('ship every sound sample beside the music', () => {
    const files = mediaSources();
    for (const s of SOUND_DATA.sampleFiles)
      expect(files.get(s.file)).toMatch(/shared\/sounds\/samples\//);
    expect(SOUND_DATA.sampleFiles.length).toBeGreaterThan(0);
  });
});
