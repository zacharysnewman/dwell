// Block interaction (ARCHITECTURE.md §6.5): what the crosshair targets, the infinite creative
// palette and its selection, and turning break/place actions into BlockEditRequests. The client
// does not predict edits: the change shows when the server's VoxelModification arrives.
import { BlockEditAction, MessageType, Players } from '../protocol/constants.gen';
import type { Message, Vec3 } from '../protocol/messages';
import type { BlockTarget } from '../sim/clientCore';
import { MATERIALS, PLACEABLE } from '../world/materials';

export type EditAction = 'break' | 'place';

/** A hotbar slot: one placeable material, or every ladder (the facing follows the placement). */
export interface PaletteSlot {
  name: string;
  /** Material shown and placed (for ladders: the one facing north). */
  material: number;
  ladder: boolean;
}

/** The palette (§6.5): every placeable material, ladders as one slot. */
export const PALETTE: readonly PaletteSlot[] = PLACEABLE.flatMap((id): PaletteSlot[] => {
  const style = MATERIALS[id];
  if (!style) return [];
  if (style.look !== 'ladder') return [{ name: style.name, material: id, ladder: false }];
  return style.name === 'ladder_n' ? [{ name: 'ladder', material: id, ladder: true }] : [];
});

const LADDER_BY_FACING = new Map(
  MATERIALS.flatMap((m, id) => (m.ladderFace !== undefined ? [[m.ladderFace, id] as const] : [])),
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

/** Unit view direction for yaw/pitch in degrees (yaw 0 = +Z, pitch up > 0), as the camera looks. */
export function viewDirection(yawDeg: number, pitchDeg: number): Vec3 {
  const yaw = (yawDeg * Math.PI) / 180;
  const pitch = (pitchDeg * Math.PI) / 180;
  return [Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch)];
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
  update(eye: Vec3 | null, yawDeg: number, pitchDeg: number): BlockTarget | null {
    this.yaw = yawDeg;
    this.current = eye
      ? this.world.target(eye, viewDirection(yawDeg, pitchDeg), Players.reachDistance)
      : null;
    return this.current;
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
    const slot = PALETTE[this.selected];
    const material =
      action === 'break' || !slot ? 0 : slot.ladder ? ladderFor(t.face, this.yaw) : slot.material;
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
