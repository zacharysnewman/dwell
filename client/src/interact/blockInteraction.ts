// Block interaction (ARCHITECTURE.md §6.5): what the crosshair targets, the infinite creative
// palette and its selection, and turning break/place actions into BlockEditRequests. The client
// does not predict edits: the change shows when the server's VoxelModification arrives.
import { BlockEditAction, MessageType, Players } from '../protocol/constants.gen';
import type { Message, Vec3 } from '../protocol/messages';
import type { BlockTarget } from '../sim/clientCore';
import { BLOCK_DEFS, STATE_DEFS } from '../world/blocks';
import { type FaceSign, viewForward } from '../world/face';
import { MATERIALS, PLACEABLE } from '../world/materials';

export type EditAction = 'break' | 'place';

/** Offsets to the neighbour across each face: 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z. */
const FACE_DIRS: readonly Vec3[] = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
];

/** A hotbar slot: one placeable material, or every ladder (the facing follows the placement). */
export interface PaletteSlot {
  name: string;
  /** Namespaced block id, e.g. `dwell:stone`. */
  block: string;
  /** Material shown and placed (for ladders: the one facing north). */
  material: number;
  ladder: boolean;
}

/** The palette (§6.5): one slot per placeable block (its first palette state), ladders as one. */
export const PALETTE: readonly PaletteSlot[] = PLACEABLE.flatMap((id, i): PaletteSlot[] => {
  const style = MATERIALS[id];
  const state = STATE_DEFS[id];
  if (!style || !state || style.look === 'shaped') return [];
  const previous = PLACEABLE[i - 1];
  if (previous !== undefined && STATE_DEFS[previous]?.block === state.block) return [];
  const block = BLOCK_DEFS[state.block]?.id ?? style.name;
  return [
    {
      name: block.slice(block.indexOf(':') + 1),
      block,
      material: id,
      ladder: style.look === 'ladder',
    },
  ];
});

const LADDER_BY_FACING = new Map(
  MATERIALS.flatMap((m, id) =>
    m.ladderFace !== undefined && m.placeable ? [[m.ladderFace, id] as const] : [],
  ),
);

/**
 * The ladder to place against `face` of the targeted block: mounted on that block, facing out of
 * the face. On a top or bottom face it faces the player (`yawDeg`: 0 = +Z, 90 = +X).
 */
export function ladderFor(face: number, yawDeg: number): number {
  let facing = face;
  if (face === 2 || face === 3) {
    const yaw = (yawDeg * Math.PI) / 180;
    const bx = -Math.sin(yaw); // back towards the player
    const bz = -Math.cos(yaw);
    facing = Math.abs(bx) > Math.abs(bz) ? (bx > 0 ? 0 : 1) : bz > 0 ? 4 : 5;
  }
  return LADDER_BY_FACING.get(facing) ?? 0;
}

/**
 * Unit view direction for yaw/pitch in degrees (yaw 0 = +Z, pitch toward the player's up > 0), as
 * the camera looks: on face B up is −y (world/face.ts).
 */
export function viewDirection(yawDeg: number, pitchDeg: number, face: FaceSign = 1): Vec3 {
  return viewForward(yawDeg, pitchDeg, face);
}

/** What the interaction needs from the client sim (ClientCore). */
export interface TargetSource {
  target(origin: Vec3, dir: Vec3, maxDistance: number): BlockTarget | null;
}

export class BlockInteraction {
  /** Index into PALETTE. */
  selected = 0;
  /** Touch: what a tap on the view does. */
  touchAction: EditAction = 'break';
  /** Called when the selection changes (the hotbar redraws). */
  onSelect: ((slot: number) => void) | null = null;
  private current: BlockTarget | null = null;
  private yaw = 0;
  private lastEditMs = -Infinity;

  constructor(
    private readonly world: TargetSource,
    private readonly send: (m: Message) => void,
  ) {}

  get target(): BlockTarget | null {
    return this.current;
  }

  /** Re-targets from the eye along the view; a null eye clears the target (dead, loading). */
  update(
    eye: Vec3 | null,
    yawDeg: number,
    pitchDeg: number,
    face: FaceSign = 1,
  ): BlockTarget | null {
    this.yaw = yawDeg;
    const dir = viewDirection(yawDeg, pitchDeg, face);
    this.current = eye ? this.world.target(eye, dir, Players.reachDistance) : null;
    return this.current;
  }

  /** The state the selected slot places against the current target, and where; null with no target. */
  placement(): { cell: Vec3; material: number } | null {
    const t = this.current;
    const slot = PALETTE[this.selected];
    if (!t || !slot) return null;
    const dir = FACE_DIRS[t.face] ?? [0, 0, 0];
    const cell: Vec3 = [t.cell[0] + dir[0], t.cell[1] + dir[1], t.cell[2] + dir[2]];
    if (slot.ladder) return { cell, material: ladderFor(t.face, this.yaw) };
    return { cell, material: slot.material };
  }

  select(slot: number): void {
    const n = PALETTE.length;
    this.selected = ((Math.trunc(slot) % n) + n) % n;
    this.onSelect?.(this.selected);
  }

  /** Scroll wheel: next (delta > 0) or previous slot. */
  scroll(delta: number): void {
    if (delta !== 0) this.select(this.selected + Math.sign(delta));
  }

  /**
   * Breaks the targeted block or places the selected one against the targeted face. Returns false
   * when there is no target or it is too soon after the last edit (BLOCK_EDIT_INTERVAL_MS).
   */
  act(action: EditAction, nowMs: number): boolean {
    const t = this.current;
    if (!t || nowMs - this.lastEditMs < Players.blockEditIntervalMs) return false;
    this.lastEditMs = nowMs;
    const material = action === 'break' ? 0 : (this.placement()?.material ?? 0);
    this.send({
      type: MessageType.BlockEditRequest,
      action: action === 'break' ? BlockEditAction.Break : BlockEditAction.Place,
      cell: t.cell,
      face: t.face,
      material,
    });
    return true;
  }
}
