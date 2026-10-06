// The release channel the launcher opens (RELEASES.md §3, §5): stable by default; dev builds are
// opt-in, from the About section of the main menu. Read by the launcher from the same key.
export type Channel = 'stable' | 'dev';

export const CHANNEL_KEY = 'dwell.channel';

/** The part of `Storage` the setting uses. */
export interface ChannelStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function loadChannel(store: ChannelStore | null): Channel {
  try {
    return store?.getItem(CHANNEL_KEY) === 'dev' ? 'dev' : 'stable';
  } catch {
    return 'stable';
  }
}

export function saveChannel(store: ChannelStore | null, channel: Channel): void {
  try {
    store?.setItem(CHANNEL_KEY, channel);
  } catch {
    // Storage blocked: the choice isn't kept.
  }
}
