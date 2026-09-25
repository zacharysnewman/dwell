// The in-game loop (PLAYER_CONTROLLER.md §8, §9): fixed 60 Hz ticks of local prediction with the
// client's sim core, input datagrams, snapshot reconciliation, interpolated remote players, and
// the first-person camera.
import type { GameMessage } from '../net/session';
import {
  ControllerFlags,
  MAX_INPUTS_PER_DATAGRAM,
  MessageType,
  PlayerEventKind,
  PlayerFlags,
  SIM_HZ,
} from '../protocol/constants.gen';
import type { InputFrame, Message, Vec3 } from '../protocol/messages';
import type { PlayerInputState } from '../predict/input';
import { quantizeInput } from '../predict/input';
import type { PlayerView, Renderer } from '../render';
import type { ClientCore, ClientState } from '../sim/clientCore';
import { formatDebug, type Hud } from '../ui/hud';
import { isCrouched, isDead, RemotePlayers } from './remotes';

const TICK_MS = 1000 / SIM_HZ;
const MAX_TICKS_PER_FRAME = 5;
/** Terrain drawn within this many chunks of the player (horizontally), one chunk up and down. */
const VIEW_CHUNKS = 2;
const CHUNK = 32;
/** Camera eye smoothing (crouch) rate, 1/s; step smoothing speed, m/s (PPC SmoothSteps). */
const EYE_RATE = 12;
const STEP_SMOOTH_SPEED = 4;
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
}

export class Game {
  private readonly remotes = new RemotePlayers();
  private readonly proxies = new Set<number>();
  private readonly recent: InputFrame[] = [];
  private readonly chunks = new Set<string>();
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
  private eyeHeight = 1.62;
  private stepFeet: number | null = null;

  constructor(
    readonly playerId: number,
    private readonly core: ClientCore,
    private readonly renderer: Renderer,
    private readonly input: InputSource,
    private readonly hud: Hud,
    private readonly host: GameHost,
  ) {
    this.current = core.state();
  }

  /** Handles a snapshot or player event from the session. */
  onGameMessage(m: GameMessage, bytes: Uint8Array, nowMs: number): void {
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
    this.draw(nowMs, elapsed / 1000);
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
    };
  }

  private tick(): void {
    if (!this.current.active) return;
    const input = this.input.sample();
    const frame = quantizeInput(input, this.core.nextSeq());
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
    // The camera turns with rotating ground (PPC yawDelta).
    this.input.yaw += this.current.platformYawDelta;
  }

  private draw(nowMs: number, dt: number): void {
    const c = this.current;
    const p = this.previous ?? c;
    const alpha = this.accumulator / TICK_MS;
    const lerp = (a: number, b: number) => a + (b - a) * alpha;
    const center: Vec3 = [
      lerp(p.position[0] + p.renderOffset[0], c.position[0] + c.renderOffset[0]),
      lerp(p.position[1] + p.renderOffset[1], c.position[1] + c.renderOffset[1]),
      lerp(p.position[2] + p.renderOffset[2], c.position[2] + c.renderOffset[2]),
    ];
    const feet = center[1] - c.halfHeight;
    this.streamTerrain(c.active ? center : [0.5, 1, 0.5]);

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
      this.hud.setMessage('You died — respawning…');
    } else {
      this.renderer.setPlayer(this.playerId, null);
      this.hud.setMessage(c.active ? '' : 'Joining…');
      const crouched = (c.controllerFlags & ControllerFlags.crouching) !== 0;
      const targetEye = crouched ? c.crouchEyeHeight : c.eyeHeight;
      this.eyeHeight += (targetEye - this.eyeHeight) * Math.min(1, dt * EYE_RATE);
      // Step smoothing: hide the one-tick lift onto slabs and small steps.
      const grounded = (c.controllerFlags & ControllerFlags.grounded) !== 0;
      if (
        this.stepFeet === null ||
        !grounded ||
        feet < this.stepFeet ||
        feet - this.stepFeet > c.maxStepHeight + 0.05
      ) {
        this.stepFeet = feet;
      } else {
        this.stepFeet = Math.min(feet, this.stepFeet + STEP_SMOOTH_SPEED * dt);
      }
      this.renderer.setCamera(
        [center[0], this.stepFeet + this.eyeHeight, center[2]],
        this.input.yaw,
        this.input.pitch,
      );
    }

    this.hud.setHealth(this.health, nowMs);
    if (this.hud.debugVisible) {
      this.hud.setDebug(
        formatDebug({
          core: c,
          health: this.health,
          inputBuffer: this.inputBuffer,
          tickRate: this.tickRate,
          remotes: views.length,
          rttMs: this.host.rttMs(),
        }),
      );
      this.renderer.setDebugLines(this.probeLines(center, c));
    } else {
      this.renderer.setDebugLines(null);
    }
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

  /** Builds render meshes for chunks near `center` (a couple per frame) and drops far ones. */
  private streamTerrain(center: Vec3): void {
    const [cx, cy, cz] = center.map((v) => Math.floor(v / CHUNK)) as [number, number, number];
    let budget = 2;
    for (let r = 0; r <= VIEW_CHUNKS && budget > 0; r++) {
      for (let x = cx - r; x <= cx + r && budget > 0; x++) {
        for (let z = cz - r; z <= cz + r && budget > 0; z++) {
          if (Math.max(Math.abs(x - cx), Math.abs(z - cz)) !== r) continue;
          for (let y = cy - 1; y <= cy + 1 && budget > 0; y++) {
            const key = `${String(x)},${String(y)},${String(z)}`;
            if (this.chunks.has(key)) continue;
            this.chunks.add(key);
            this.renderer.setTerrainChunk(
              key,
              [x * CHUNK, y * CHUNK, z * CHUNK],
              this.core.chunkFaces(x, y, z),
            );
            budget--;
          }
        }
      }
    }
    for (const key of this.chunks) {
      const [x = 0, y = 0, z = 0] = key.split(',').map(Number);
      if (Math.max(Math.abs(x - cx), Math.abs(z - cz)) > VIEW_CHUNKS + 1 || Math.abs(y - cy) > 2) {
        this.renderer.setTerrainChunk(key, [0, 0, 0], null);
        this.chunks.delete(key);
      }
    }
  }
}
