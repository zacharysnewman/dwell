import { describe, expect, it } from 'vitest';
import { BlockEditAction, MessageType, Players } from '../protocol/constants.gen';
import type { Message, Vec3 } from '../protocol/messages';
import { MATERIALS } from '../world/materials';
import { BlockInteraction, ladderFor, PALETTE, viewDirection } from './blockInteraction';
import { PIECES } from './shapes';

const name = (id: number) => MATERIALS[id]?.name;
const round = (v: Vec3) => v.map((x) => Math.round(x * 1000) / 1000 + 0);

describe('block palette', () => {
  it('offers every placeable material once, ladders as one slot; shapes are pieces, not slots', () => {
    expect(PALETTE.map((s) => s.name)).toEqual([
      'stone',
      'dirt',
      'grass',
      'ladder',
      'sand',
      'sandstone',
      'gravel',
      'snow',
      'ice',
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

  describe('shaped pieces', () => {
    const slot = (n: string) => PALETTE.findIndex((s) => s.name === n);

    it('cycles the piece for materials that come in shapes, and not for those that do not', () => {
      const { interaction } = setup();
      const seen: string[] = [];
      interaction.onPiece = (p) => seen.push(p);
      interaction.select(slot('stone'));
      interaction.cyclePiece(1);
      interaction.cyclePiece(1);
      interaction.cyclePiece(-1);
      expect(seen).toEqual(['slab', 'wedge', 'slab']);
      interaction.select(slot('leaves'));
      interaction.cyclePiece(1);
      expect(seen).toHaveLength(3); // leaves come only as cubes
      expect(interaction.effectivePiece).toBe('cube');
      interaction.select(slot('stone'));
      expect(interaction.effectivePiece).toBe('slab'); // the choice is kept
      for (let i = 0; i < PIECES.length; i++) interaction.cyclePiece(1);
      expect(interaction.piece).toBe('slab'); // a full lap
    });

    it('places a slope that rises away from the player, upright on a top, hanging from a ceiling', () => {
      const { sent, interaction } = setup();
      interaction.select(slot('stone'));
      interaction.piece = 'wedge';
      interaction.update([3.5, 1.6, 0], 0, 0); // looking +Z at the −Z face (5) of cell (3, 0, 4)
      const placed = interaction.placement();
      expect(placed?.cell).toEqual([3, 0, 3]);
      expect(name(placed?.material ?? 0)).toBe(
        'dwell:stone_slope[facing=north,flooded=false,half=top,shape=wedge]',
      ); // the ray meets the face above its middle: hanging
      interaction.update([3.5, 0.2, 0], 0, 0);
      expect(name(interaction.placement()?.material ?? 0)).toBe(
        'dwell:stone_slope[facing=north,flooded=false,half=bottom,shape=wedge]',
      );
      interaction.update([3.5, 0.2, 0], 90, 0); // looking +X: the slope descends toward −X… west
      expect(name(interaction.placement()?.material ?? 0)).toContain('facing=west');
      interaction.act('place', 0);
      const m = sent[0];
      expect(m?.type === MessageType.BlockEditRequest && m.material).toBe(
        interaction.placement()?.material,
      );
    });

    it('places slabs and cubes, and always the cube for other materials', () => {
      const { interaction } = setup();
      interaction.update([3.5, 0.2, 0], 0, 0);
      interaction.select(slot('stone'));
      interaction.piece = 'slab';
      expect(name(interaction.placement()?.material ?? 0)).toBe(
        'dwell:stone_slab[flooded=false,half=bottom]',
      );
      interaction.piece = 'cube';
      expect(name(interaction.placement()?.material ?? 0)).toBe('dwell:stone');
      interaction.select(slot('coal_ore'));
      interaction.piece = 'wedge';
      expect(name(interaction.placement()?.material ?? 0)).toBe('dwell:coal_ore');
    });
  });
});
