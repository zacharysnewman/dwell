import { describe, expect, it } from 'vitest';
import { BlockEditAction, MessageType, Players } from '../protocol/constants.gen';
import type { Message, Vec3 } from '../protocol/messages';
import { MATERIALS } from '../world/materials';
import { BlockInteraction, ladderFor, PALETTE, viewDirection } from './blockInteraction';

const name = (id: number) => MATERIALS[id]?.name;
const round = (v: Vec3) => v.map((x) => Math.round(x * 1000) / 1000 + 0);

describe('block palette', () => {
  it('offers every placeable material once, ladders as one slot (C++ Placeable)', () => {
    expect(PALETTE.map((s) => s.name)).toEqual([
      'stone',
      'dirt',
      'grass',
      'stone_slab',
      'ladder',
      'sand',
      'sandstone',
      'gravel',
      'snow',
      'log',
      'leaves',
      'coal_ore',
      'iron_ore',
      'gold_ore',
    ]);
  });

  it('mounts ladders on the clicked face, facing out of it, or facing the player on tops', () => {
    expect(name(ladderFor(0, 0))).toBe('dwell:ladder[facing=east,flooded=false]');
    expect(name(ladderFor(1, 0))).toBe('dwell:ladder[facing=west,flooded=false]');
    expect(name(ladderFor(4, 0))).toBe('dwell:ladder[facing=south,flooded=false]');
    expect(name(ladderFor(5, 0))).toBe('dwell:ladder[facing=north,flooded=false]');
    expect(name(ladderFor(2, 0))).toBe('dwell:ladder[facing=north,flooded=false]'); // looking +Z: the ladder faces −Z
    expect(name(ladderFor(2, 90))).toBe('dwell:ladder[facing=west,flooded=false]'); // looking +X
    expect(name(ladderFor(3, 180))).toBe('dwell:ladder[facing=south,flooded=false]');
  });
});

describe('BlockInteraction', () => {
  function setup() {
    const sent: Message[] = [];
    const rays: { dir: Vec3; max: number }[] = [];
    const interaction = new BlockInteraction(
      {
        target: (_origin, dir, max) => {
          rays.push({ dir, max });
          return { cell: [3, 0, 4], face: 5 };
        },
      },
      (m) => sent.push(m),
    );
    return { sent, rays, interaction };
  }

  it('targets along the view within reach', () => {
    const { rays, interaction } = setup();
    interaction.update([0, 1.6, 0], 90, 0);
    expect(round(rays[0]?.dir ?? [0, 0, 0])).toEqual([1, 0, 0]);
    expect(rays[0]?.max).toBe(Players.reachDistance);
    expect(round(viewDirection(0, 90))).toEqual([0, 1, 0]);
    expect(interaction.update(null, 0, 0)).toBeNull();
  });

  it('sends break and place requests for the target, at most one per interval', () => {
    const { sent, interaction } = setup();
    expect(interaction.act('break', 0)).toBe(false); // nothing targeted yet
    interaction.update([0, 1.6, 0], 0, 0);
    expect(interaction.act('break', 1000)).toBe(true);
    expect(interaction.act('place', 1000 + Players.blockEditIntervalMs - 1)).toBe(false);
    interaction.select(1);
    expect(interaction.act('place', 1000 + Players.blockEditIntervalMs)).toBe(true);
    expect(sent).toEqual([
      {
        type: MessageType.BlockEditRequest,
        action: BlockEditAction.Break,
        cell: [3, 0, 4],
        face: 5,
        material: 0,
      },
      {
        type: MessageType.BlockEditRequest,
        action: BlockEditAction.Place,
        cell: [3, 0, 4],
        face: 5,
        material: 3, // dirt
      },
    ]);
  });

  it('places the ladder that fits the targeted face', () => {
    const { sent, interaction } = setup();
    interaction.update([0, 1.6, 0], 0, 0);
    interaction.select(PALETTE.findIndex((s) => s.ladder));
    interaction.act('place', 0);
    const m = sent[0];
    expect(m?.type === MessageType.BlockEditRequest && name(m.material)).toBe(
      'dwell:ladder[facing=north,flooded=false]',
    );
  });

  it('selects slots by number and scroll, wrapping around', () => {
    const { interaction } = setup();
    const seen: number[] = [];
    interaction.onSelect = (s) => seen.push(s);
    interaction.scroll(-1);
    expect(interaction.selected).toBe(PALETTE.length - 1);
    interaction.scroll(1);
    interaction.select(4);
    expect(seen).toEqual([PALETTE.length - 1, 0, 4]);
  });
});
