import { describe, expect, it } from 'vitest';
import { parseRoomEvent } from './roomSocket';

describe('room events', () => {
  it('parse the room’s messages', () => {
    expect(parseRoomEvent('{"t":"guest","peer":2}')).toEqual({ t: 'guest', peer: 2 });
    expect(parseRoomEvent('{"t":"guest-left","peer":2}')).toEqual({ t: 'guest-left', peer: 2 });
    expect(parseRoomEvent('{"t":"host-left"}')).toEqual({ t: 'host-left' });
    expect(parseRoomEvent('{"t":"signal","from":2,"data":{"type":"offer","sdp":"v=0"}}')).toEqual({
      t: 'signal',
      from: 2,
      data: { type: 'offer', sdp: 'v=0' },
    });
    expect(parseRoomEvent('{"t":"signal","data":{"type":"candidate","candidate":null}}')).toEqual({
      t: 'signal',
      from: null,
      data: { type: 'candidate', candidate: null },
    });
  });

  it('ignore anything else', () => {
    expect(parseRoomEvent('not json')).toBeNull();
    expect(parseRoomEvent('{"t":"guest"}')).toBeNull();
    expect(parseRoomEvent('{"t":"signal","data":{"type":"offer"}}')).toBeNull();
    expect(parseRoomEvent('{"t":"signal","data":{"type":"bogus","sdp":"x"}}')).toBeNull();
    expect(parseRoomEvent('{"t":"other"}')).toBeNull();
  });
});
