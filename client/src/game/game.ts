// The in-game loop (PLAYER_CONTROLLER.md §8, §9): fixed 60 Hz ticks of local prediction with the
// client's sim core, input datagrams, snapshot reconciliation, interpolated remote players, and
// the first-person camera.
import type { BlockInteraction, EditAction } from '../interact/blockInteraction';
import type { GameMessage } from '../net/session';
import {
  ControllerFlags,
  MAX_INPUTS_PER_DATAGRAM,
  MessageType,
  PlayerEventKind,
  PlayerFlags,
  PlayerState,
  SIM_HZ,
} from '../protocol/constants.gen';
import type { InputFrame, Message, Vec3 } from '../protocol/messages';
import type { PlayerInputState } from '../predict/input';
import { quantizeInput } from '../predict/input';
import { predictionMayRun } from './gate';
import { hudText } from './hudText';
import type { PlayerView, Renderer } from '../render';
import { formatRenderStats } from '../render/display';
import type { ClientCore, ClientState } from '../sim/clientCore';
import { formatDebug, type Hud } from '../ui/hud';
import type { ChunkStreamer, StreamStats } from '../world/chunkStream';
import type { LodCamera } from '../lod/frustum';
import { formatLodStats, type LodStats, type LodSystem } from '../lod/lodSystem';
import { SHAPES } from '../world/blocks';
import { materialStyle } from '../world/materials';
import { EyeCamera } from './eye';
import { isCrouched, isDead, RemotePlayers } from './remotes';

const TICK_MS = 1000 / SIM_HZ;
const MAX_TICKS_PER_FRAME = 5;
/**
 * Main-thread time per frame for starting mesh jobs (copying each chunk's voxels out of the sim;
 * at least one per frame). Generation and meshing run in worker pools.
 */
const MESH_BUDGET_MS = 4;
/** Server input buffer outside [LOW, HIGH] nudges the local tick rate by ±RATE_NUDGE. */
const BUFFER_LOW = 1;
const BUFFER_HIGH = 4;
const RATE_NUDGE = 0.02;

const PLAYER_COLORS = [0xe0564a, 0x4a8fe0, 0xe0c24a, 0x5fc26b, 0xb15fe0, 0x4ad0c8, 0xe08a4a];

export interface GameHost {
  /** Sends a gameplay datagram (PlayerInput). */
  send(m: Message): void;
  rttMs(): number | null;
}

export interface InputSource {
  sample(): PlayerInputState;
  yaw: number;
  pitch: number;
}

/** What automated tests read through `window.__dwell` (see main.ts). */
export interface GameDebugState {
  playerId: number;
  active: boolean;
  feet: Vec3;
  health: number;
  dead: boolean;
  remotes: { playerId: number; feet: Vec3; dead: boolean }[];
  stats: ClientState['stats'];
  /** Streamed terrain (§6.3). */
  terrain: StreamStats;
  /** The chunks around the player are loaded, so prediction runs. */
  terrainReady: boolean;
  /** The targeted block (§6.5), if any. */
  target: { cell: Vec3; face: number } | null;
  /** The whole-world view (§6.6), when running. */
  lod: LodStats | null;
  /** The player is flying (creative flight, PLAYER_CONTROLLER.md §6.7). */
  flying: boolean;
}

/** The drawing's vertical field of view (degrees), aspect and height (px), for LOD selection. */
export type ViewportInfo = () => { fovYDeg: number; aspect: number; heightPx: number };

export class Game {
  private readonly remotes = new RemotePlayers();
  private readonly proxies = new Set<number>();
  private readonly recent: InputFrame[] = [];
  private accumulator = 0;
  private lastFrameMs: number | null = null;
  private lastSnapshotTick = 0;
  private inputBuffer = 0;
  private tickRate = 1;
  private health = 0;
  private dead = false;
  private deathFeet: Vec3 = [0, 0, 0];
  private previous: ClientState | null = null;
  private current: ClientState;
  private readonly eye = new EyeCamera();
  /** Extra line for the debug overlay (the regenerate-and-diff check, main.ts). */
  debugNote = '';
  /** The debug overlay's memory line (ui/memory.ts, refreshed each second by main.ts). */
  memoryNote = '';
  /** The whole-world view, once the session has verified its generator (main.ts). */
  lod: LodSystem | null = null;
  viewport: ViewportInfo = () => ({ fovYDeg: 75, aspect: 16 / 9, heightPx: 1080 });

  constructor(
    readonly playerId: number,
    private readonly core: ClientCore,
    private readonly renderer: Renderer,
    private readonly input: InputSource,
    private readonly hud: Hud,
    private readonly host: GameHost,
    private readonly terrain: ChunkStreamer,
    private readonly interaction: BlockInteraction | null = null,
    /** Material at a voxel (the outline is half height on slabs). */
    private readonly voxel: (x: number, y: number, z: number) => number = () => 0,
  ) {
    this.current = core.state();
    this.eye.tick(this.current, 1 / SIM_HZ);
  }

  /** Handles a snapshot, player event, or chunk message from the session. */
  onGameMessage(m: GameMessage, bytes: Uint8Array, nowMs: number): void {
    if (m.type === MessageType.ChunkData) {
      this.terrain.onChunkData(m);
      return;
    }
    if (m.type === MessageType.ChunkUnload) {
      this.terrain.onChunkUnload(m.coords);
      return;
    }
    if (m.type === MessageType.VoxelModification) {
      this.terrain.onVoxelModification(m.chunks);
      return;
    }
    if (
      m.type === MessageType.LodIndex ||
      m.type === MessageType.LodIndexUpdate ||
      m.type === MessageType.LodData
    ) {
      this.lod?.onMessage(m, bytes.length, nowMs);
      return;
    }
    if (m.type === MessageType.PhysicsSnapshot) {
      if (m.serverTick <= this.lastSnapshotTick) return; // reordered datagram
      this.lastSnapshotTick = m.serverTick;
      this.core.snapshot(bytes);
      this.health = m.local.health;
      this.dead = (m.local.flags & PlayerFlags.dead) !== 0;
      if (this.dead) this.deathFeet = [...m.local.position];
      this.inputBuffer = m.local.inputBuffer;
      this.tickRate =
        this.inputBuffer <= BUFFER_LOW
          ? 1 + RATE_NUDGE
          : this.inputBuffer >= BUFFER_HIGH
            ? 1 - RATE_NUDGE
            : 1;
      this.remotes.push(m.serverTick, m.remotes, nowMs);
      const seen = new Set<number>();
      for (const r of this.remotes.latest()) {
        if (isDead(r)) continue;
        seen.add(r.playerId);
        this.core.setRemote(r.playerId, r.feet, r.velocity, isCrouched(r));
        this.proxies.add(r.playerId);
      }
      for (const id of this.proxies) {
        if (!seen.has(id)) {
          this.core.removeRemote(id);
          this.proxies.delete(id);
        }
      }
      if (!this.current.active) this.current = this.core.state();
      return;
    }
    // PlayerEvent.
    if (m.kind === PlayerEventKind.Knockback && m.playerId === this.playerId) {
      this.core.knockback(m.inputSeq, m.vector);
    } else if (m.kind === PlayerEventKind.Damage && m.playerId === this.playerId) {
      this.hud.flashDamage(nowMs);
    }
  }

  /** One animation frame: run due ticks, then draw. */
  frame(nowMs: number): void {
    const elapsed = this.lastFrameMs === null ? 0 : nowMs - this.lastFrameMs;
    this.lastFrameMs = nowMs;
    this.accumulator = Math.min(
      this.accumulator + elapsed * this.tickRate,
      MAX_TICKS_PER_FRAME * TICK_MS,
    );
    while (this.accumulator >= TICK_MS) {
      this.accumulator -= TICK_MS;
      this.tick();
    }
    this.draw(nowMs);
  }

  debugState(): GameDebugState {
    const c = this.current;
    return {
      playerId: this.playerId,
      active: c.active,
      feet: [c.position[0], c.position[1] - c.halfHeight, c.position[2]],
      health: this.health,
      dead: this.dead,
      remotes: this.remotes
        .latest()
        .map((r) => ({ playerId: r.playerId, feet: r.feet, dead: isDead(r) })),
      stats: c.stats,
      terrain: this.terrain.stats(),
      terrainReady: this.terrainReady(),
      target: this.interaction?.target ?? null,
      lod: this.lod?.debugStats() ?? null,
      flying: c.state === PlayerState.Flying,
    };
  }

  /** Breaks or places at the crosshair (§6.5): only while alive and playing. */
  edit(action: EditAction, nowMs: number): boolean {
    if (this.dead || !this.terrainReady()) return false;
    return this.interaction?.act(action, nowMs) ?? false;
  }

  /** Prediction waits until the terrain around the player has arrived (collision needs it). */
  private terrainReady(): boolean {
    return this.current.active && this.terrain.readyAround(this.current.position);
  }

  private tick(): void {
    const c = this.current;
    if (!predictionMayRun(c.active, c.state, this.terrainReady())) return;
    const frame = quantizeInput(this.input.sample(), this.core.nextSeq());
    this.core.tick(frame);
    this.recent.push(frame);
    while (this.recent.length > MAX_INPUTS_PER_DATAGRAM) this.recent.shift();
    this.host.send({
      type: MessageType.PlayerInput,
      lastSnapshotTick: this.lastSnapshotTick,
      inputs: [...this.recent],
    });
    this.previous = this.current;
    this.current = this.core.state();
    this.eye.tick(this.current, 1 / SIM_HZ);
    // The camera turns with rotating ground (PPC yawDelta).
    this.input.yaw += this.current.platformYawDelta;
  }

  private draw(nowMs: number): void {
    const c = this.current;
    const p = this.previous ?? c;
    const alpha = this.accumulator / TICK_MS;
    const lerp = (a: number, b: number) => a + (b - a) * alpha;
    const center: Vec3 = [
      lerp(p.position[0] + p.renderOffset[0], c.position[0] + c.renderOffset[0]),
      lerp(p.position[1] + p.renderOffset[1], c.position[1] + c.renderOffset[1]),
      lerp(p.position[2] + p.renderOffset[2], c.position[2] + c.renderOffset[2]),
    ];
    this.terrain.meshDirty(c.active ? center : [0.5, 64, 0.5], MESH_BUDGET_MS);

    // Remote players, interpolated.
    const views = this.remotes.sample(nowMs);
    const drawn = new Set<number>();
    for (const v of views) {
      drawn.add(v.playerId);
      this.renderer.setPlayer(
        v.playerId,
        this.playerView(v.playerId, v.feet, v.yaw, isCrouched(v), isDead(v), c),
      );
    }
    for (const id of this.remotes.ids()) if (!drawn.has(id)) this.renderer.setPlayer(id, null);

    // Local player: first person, or third person over the body while dead.
    if (this.dead) {
      this.renderer.setPlayer(
        this.playerId,
        this.playerView(this.playerId, this.deathFeet, this.input.yaw, false, true, c),
      );
      const yaw = (this.input.yaw * Math.PI) / 180;
      const [x, y, z] = this.deathFeet;
      this.renderer.setCamera(
        [x - Math.sin(yaw) * 4, y + 2.5, z - Math.cos(yaw) * 4],
        this.input.yaw,
        -25,
      );
      this.showHudText();
    } else {
      this.renderer.setPlayer(this.playerId, null);
      this.showHudText();
      // Eye height is smoothed per tick (steps, crouching; see eye.ts), then interpolated.
      const eye: Vec3 = [center[0], this.eye.draw(alpha), center[2]];
      this.renderer.setCamera(eye, this.input.yaw, this.input.pitch);
      this.target(this.terrainReady() ? eye : null);
    }
    if (this.dead) this.target(null);
    this.updateLod(c, nowMs);

    this.hud.setHealth(this.health, nowMs);
    if (this.hud.debugVisible) {
      const text = formatDebug({
        core: c,
        health: this.health,
        inputBuffer: this.inputBuffer,
        tickRate: this.tickRate,
        remotes: views.length,
        rttMs: this.host.rttMs(),
      });
      const lines = [text];
      if (this.lod) lines.push(formatLodStats(this.lod.debugStats()));
      lines.push(formatRenderStats(this.renderer.stats()));
      if (this.memoryNote) lines.push(this.memoryNote);
      if (this.debugNote) lines.push(this.debugNote);
      this.hud.setDebug(lines.join('\n'));
      this.renderer.setDebugLines(this.probeLines(center, c));
    } else {
      this.renderer.setDebugLines(null);
    }
  }

  private showHudText(): void {
    const text = hudText({
      dead: this.dead,
      active: this.current.active,
      terrainReady: this.terrainReady(),
      terrainError: this.terrain.error?.message ?? null,
    });
    this.hud.setMessage(text.center);
    this.hud.setStatus(text.status);
  }

  /** The whole-world view around the camera. */
  private updateLod(c: ClientState, nowMs: number): void {
    if (!this.lod || !c.active) return;
    const position: Vec3 = this.dead
      ? this.deathFeet
      : [c.position[0], c.position[1], c.position[2]];
    const camera: LodCamera = {
      position,
      yawDeg: this.input.yaw,
      pitchDeg: this.input.pitch,
      ...this.viewport(),
    };
    // Detail loads ahead of where the player is heading (§6.6).
    if (!this.dead) camera.velocity = [c.velocity[0], c.velocity[1], c.velocity[2]];
    this.lod.update(camera, nowMs);
  }

  /** Targets the block under the crosshair from `eye` and outlines it (none while not playing). */
  private target(eye: Vec3 | null): void {
    const t = this.interaction?.update(eye, this.input.yaw, this.input.pitch) ?? null;
    if (!t) {
      this.renderer.setBlockOutline(null);
      return;
    }
    // The outline hugs the shape's top: slabs and low slopes are not a full cell tall.
    const style = materialStyle(this.voxel(...t.cell));
    const shape = style.look === 'shaped' ? SHAPES[style.shape] : undefined;
    this.renderer.setBlockOutline(t.cell, shape && !shape.inverted ? shape.maxY : 1);
  }

  private playerView(
    id: number,
    feet: Vec3,
    yaw: number,
    crouched: boolean,
    dead: boolean,
    c: ClientState,
  ): PlayerView {
    return {
      feet,
      yaw,
      crouched,
      dead,
      color: PLAYER_COLORS[id % PLAYER_COLORS.length] ?? 0xffffff,
      radius: c.radius,
      height: c.standingHeight,
    };
  }

  /** Probe rays (ground ring, walls) and velocity, for the debug overlay. */
  private probeLines(center: Vec3, c: ClientState): { from: Vec3; to: Vec3; color: number }[] {
    const lines: { from: Vec3; to: Vec3; color: number }[] = [];
    const grounded = (c.controllerFlags & ControllerFlags.grounded) !== 0;
    const ring = c.radius * 0.9;
    const down = c.halfHeight + (grounded ? c.maxStepHeight : 0.15);
    for (let i = -1; i < 16; i++) {
      const a = (i * Math.PI * 2) / 16;
      const o: Vec3 =
        i < 0
          ? center
          : [center[0] + Math.cos(a) * ring, center[1], center[2] + Math.sin(a) * ring];
      lines.push({ from: o, to: [o[0], o[1] - down, o[2]], color: grounded ? 0x3ce05a : 0xe0463c });
    }
    for (const [dx, dz] of [
      [0, 1],
      [0, -1],
      [-1, 0],
      [1, 0],
    ] as const) {
      const o: Vec3 = [center[0] + dx * ring, center[1], center[2] + dz * ring];
      lines.push({
        from: o,
        to: [o[0] + dx * 0.16, o[1], o[2] + dz * 0.16],
        color: c.touchingWall ? 0xffd23c : 0x9aa3b5,
      });
    }
    lines.push({
      from: center,
      to: [
        center[0] + c.velocity[0] * 0.25,
        center[1] + c.velocity[1] * 0.25,
        center[2] + c.velocity[2] * 0.25,
      ],
      color: 0x3cc8ff,
    });
    return lines;
  }
}
