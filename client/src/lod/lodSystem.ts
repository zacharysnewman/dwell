// The client's whole-world view (ARCHITECTURE.md §6.6, ADR 0012): each frame the LOD octree is
// walked from the root around the camera, refining a node while its cells project larger than
// LOD_PIXEL_ERROR pixels. A parent stays drawn until all its children are ready (meshed, or known
// empty or buried); level-0 children are the streamed chunks. Unmodified sections are generated in
// the worldgen pool; sections the LOD index says may be modified are asked of the server
// (LodRequest), and a `Generated` answer covers the section's whole subtree. Jobs go coarsest first,
// then nearest; content and meshes live in a cache bounded by LOD_CACHE_MB. Beyond the streamed
// view, the chunks a refined level-1 section needs are asked of the server (ChunkRequest, within
// RENDER_RADIUS_CHUNKS), nearest first. Distances look ahead along the camera's velocity, so
// detail loads before the camera arrives.
import type { SectionMeshes } from '../mesh/lodMesher';
import { sectionBytes, SURFACE_STRIDE } from '../mesh/lodMesher';
import type { SectionMesher } from '../mesh/pool';
import type { LodMessage } from '../net/session';
import { CHUNK_SIZE, Limits, Lod, LodForm, MessageType, World } from '../protocol/constants.gen';
import type { ChunkCoord, LodIndexEntry, LodSectionRequest, Vec3 } from '../protocol/messages';
import type { SectionSource } from '../worldgen/pool';
import { FACE_A, FACE_B, type FaceSign } from '../world/face';
import { stateId } from '../world/blocks';
import { Frustum, type LodCamera } from './frustum';
import {
  cellSize,
  chunkOfLod,
  INDEX_LEVEL,
  kindFromBounds,
  lodCell,
  lodChild,
  lodId,
  isFaceBSection,
  lodRowFace,
  MAX_ALIGNED_LEVEL,
  lodInWorld,
  mirrorSectionOrigin,
  LodKind,
  lodOfChunk,
  lodParent,
  sectionAt,
  MAX_LEVEL,
  SECTION_CELLS,
  sectionOrigin,
  sectionSize,
  type LodBounds,
  type LodCoord,
} from './grid';

/** What the LOD system draws with (the renderer). Sections are keyed by `lodId`. */
export interface LodView {
  /** Adds, replaces or (null) removes a section's meshes; positions are in cells from `origin`. */
  setLodSection(id: number, origin: Vec3, cellSize: number, meshes: SectionMeshes | null): void;
  /** The sections to draw this frame, with the sides (bit per face index) whose skirt shows. */
  showLodSections(visible: ReadonlyMap<number, number>): void;
  /** Ancestor meshes drawn clipped to boxes (world metres) this frame; [] for none. */
  showLodStandIns(standIns: readonly { id: number; lo: Vec3; hi: Vec3 }[]): void;
}

/** Which streamed chunks can stand in for level 0 (loaded, and meshed or all air). */
export interface ChunkReadiness {
  drawable(coord: ChunkCoord): boolean;
  /** Received (and not unloaded), drawable or not yet (default: drawable). */
  isLoaded?(coord: ChunkCoord): boolean;
}

export interface LodOptions {
  pixelError: number;
  cacheBytes: number;
  /** Generation and meshing jobs in flight at once (the pools' capacities). */
  maxGenerationJobs: number;
  maxMeshJobs: number;
  /** LodRequest pacing (sections per second, under the server's limit) and unanswered retry. */
  requestsPerSecond?: number;
  requestTimeoutMs?: number;
  /**
   * Asks the server for chunks beyond the view (ChunkRequest, at most Limits.maxResyncChunks per
   * call); without it, level 0 is only the streamed view.
   */
  requestChunks?: (coords: ChunkCoord[]) => void;
  /** Chunk request pacing (per second, under the server's limit). */
  chunkRequestsPerSecond?: number;
}

export interface LodStats {
  /** Sections drawn per level (index = level). */
  drawn: number[];
  /** Level-1 sections shown as their streamed chunks. */
  chunkSections: number;
  nodes: number;
  generating: number;
  meshing: number;
  asking: number;
  cacheBytes: number;
  /** Bytes of the sections the view uses (this frame's traversal): what must fit the cache. */
  inUseBytes: number;
  /** The pixel error's multiplier, raised while the view does not fit the cache (see adapt). */
  errorScale: number;
  /** Bytes per second received on the lod stream (over the last second). */
  bytesPerSecond: number;
}

/** The leaves of one frame's selection: every region of the view is in exactly one (tests). */
export interface Selection {
  drawn: LodCoord[];
  /** Known empty or buried: nothing to draw. */
  empty: LodCoord[];
  /** Level-1 sections whose 8 chunks are drawn instead. */
  chunks: LodCoord[];
  /** Holes filled by an ancestor's mesh clipped to them (see visit). */
  standIns: StandIn[];
}

export interface StandIn {
  /** The region with nothing ready to draw (its box is what gets drawn). */
  hole: LodCoord;
  /** The nearest ready, meshed, non-buried ancestor whose mesh is drawn clipped to `hole`. */
  from: LodCoord;
}

interface Node {
  coord: LodCoord;
  id: number;
  /** The section's box in world metres. */
  lo: Vec3;
  hi: Vec3;
  parent: Node | null;
  /** In-world children, once the node has been refined. */
  kids: Node[] | null;
  kind: LodKind | null;
  /** The server sent this section's content (it differs from generation somewhere below). */
  modified: boolean;
  revision: number;
  /** Must hear from the server before it is known (possibly modified, or full-chunk mode). */
  needsAnswer: boolean;
  /** Held content may be out of date: ask again with its revision (drawn meanwhile). */
  stale: boolean;
  asked: number | null;
  generating: boolean;
  meshing: boolean;
  /** Content to mesh (dropped once meshed unless modified: neighbours' aprons read it). */
  cells: Uint16Array<ArrayBuffer> | null;
  /** Generated sections: each column's exact surface (worldgen GeneratedSection.surface). */
  surface: Float32Array<ArrayBuffer> | null;
  /** Level ≥ 7 only: face B's column surfaces (tints), which the section's rows may hold too. */
  surfaceB: Float32Array<ArrayBuffer> | null;
  meshed: boolean;
  /** Needs a (new) mesh: its content or a neighbour's border changed. */
  remesh: boolean;
  bytes: number;
  lastUsed: number;
  /** Frame it was last drawn in (skirts: is the neighbour drawn at the same level?). */
  drawnFrame: number;
  token: number;
}

const ROOT: LodCoord = [MAX_LEVEL, 0, 0, 0];
/** Load order: work out of view ranks as if its cells were this much smaller (see schedule). */
const OFF_VIEW_PRIORITY = 1 / 8;
/** Drawable chunks wait at most this long (ms) for the LOD levels above them (see findCovered). */
export const FORCE_CHUNKS_AFTER_MS = 1000;
/** Other-face sections closer than this to the camera are kept (a shaft through the core). */
export const OTHER_FACE_NEAR_M = 512;
/** Within this of the rim the other face can be seen past the edge: nothing is culled. */
export const RIM_VIEW_M = 16_384;
/** Distances look this far ahead along the camera's velocity (s), and at most this far (m). */
export const LOOKAHEAD_S = 1.5;
export const MAX_LOOKAHEAD_M = 1024;
/** A requested chunk that has not arrived is asked for again after this long (ms). */
export const CHUNK_REQUEST_RETRY_MS = 10_000;
/** Chunks are asked for within this many chunks of the camera (the server's radius, less a margin). */
export const CHUNK_REQUEST_RADIUS = World.renderRadiusChunks - 1;
/**
 * The view must fit the cache (LOD_CACHE_MB): while the sections it uses are over the budget, the
 * pixel error is raised by ERROR_SCALE_STEP (at most once per COARSER_AFTER_MS), and lowered again
 * when they use under ROOM of it (at most once per FINER_AFTER_MS). What the view needs changes in
 * jumps (a whole band of distance refines at once), so a finer step can overflow: that scale is not
 * tried again until the camera has moved RETRY_FINER_AFTER_M, or it would swing back and forth,
 * reloading sections each time. Without it the cache grew without bound on large or 2× screens:
 * over 1 GB and thousands of draw calls.
 */
export const ERROR_SCALE_STEP = 1.15;
export const MAX_ERROR_SCALE = 16;
const ROOM = 0.6;
export const COARSER_AFTER_MS = 1000;
export const FINER_AFTER_MS = 10_000;
export const RETRY_FINER_AFTER_M = 512;
/** The chunks draw water's surface 1/8 m below a full block (mesher.ts WATER_SURFACE). */
const CHUNK_WATER_DROP_M = 0.125;
const NEIGHBOURS: readonly (readonly [number, number, number])[] = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
];

const neighbourId = (c: LodCoord, d: readonly [number, number, number]): number =>
  lodId(c[0], c[1] + d[0], c[2] + d[1], c[3] + d[2]);
const boundsKey = (level: number, i: number, k: number): number => lodId(level, i, 0, k);

export class LodSystem {
  private readonly nodes = new Map<number, Node>();
  private readonly bounds = new Map<number, LodBounds | 'pending'>();
  /** Index entries (level-8 sections with modifications), and the sections they make modified. */
  private readonly index = new Map<number, number>();
  private readonly indexed = new Set<number>();
  private fullMode = false;
  private frame = 0;
  private token = 0;
  private generating = 0;
  private meshing = 0;
  private cacheBytes = 0;
  private readonly asking = new Map<number, Node>();
  private requestCredit: number;
  private lastRequestMs: number | null = null;
  private readonly received: { at: number; bytes: number }[] = [];
  private selection: Selection = { drawn: [], empty: [], chunks: [], standIns: [] };
  private readonly refined = new Set<number>();
  /**
   * Level-1 sections around the camera whose 8 chunks are drawable, and all their ancestors: the
   * traversal always reaches them, whatever is still generating above (the player's surroundings
   * never wait for coarse levels).
   */
  private readonly covered = new Set<number>();
  private readonly coveredAncestors = new Set<number>();
  /** When each level-1 section around the camera first had all its chunks drawable (ms). */
  private readonly drawableSince = new Map<number, number>();
  /** Full-detail distance (m, the settings menu): chunks within it, never beyond; null: by pixel error. */
  private detailM: number | null = null;
  /** Where the camera will be (LOOKAHEAD_S along its velocity), if it moves. */
  private ahead: Vec3 | null = null;
  /** The face the camera is on, and whether it is near the rim (other-face culling, see culled). */
  private viewFace: FaceSign = 1;
  private nearRim = false;
  private position: Vec3 = [0, 0, 0];
  private cameraChunk: ChunkCoord = [0, 0, 0];
  /** Chunks asked of the server (by key: the chunk and when), and the pacing of those requests. */
  private readonly chunkAsked = new Map<string, { coord: ChunkCoord; at: number }>();
  private chunkCredit = 0;
  private lastChunkRequestMs: number | null = null;
  // Collected by the traversal each frame.
  private wantedChunks: { coord: ChunkCoord; d: number }[] = [];
  private wanted: Node[] = [];
  private due: Node[] = [];
  private remeshed: Node[] = [];
  private stats: LodStats | null = null;
  /** Bytes of the sections this frame's traversal reached (see adapt). */
  private inUse = 0;
  private errorScale = 1;
  private lastScaleMs: number | null = null;
  private lastStepFiner = false;
  /** A scale that overflowed right after a finer step, and where: not tried again nearby. */
  private tooFine: { scale: number; at: Vec3 } | null = null;
  /** The root is ready: LOD draws, and chunks show only where their level-1 section is refined. */
  active = false;

  constructor(
    private readonly sections: SectionSource,
    private readonly mesher: SectionMesher,
    private readonly view: LodView,
    private readonly chunks: ChunkReadiness,
    private readonly request: (sections: LodSectionRequest[]) => void,
    private options: LodOptions,
  ) {
    this.requestCredit = this.rate();
  }

  /**
   * Draws chunks (every block) out to `m` from the camera, and level 1 at most beyond it (lod/detail.ts);
   * null goes back to the pixel error alone.
   */
  setDetailDistance(m: number | null): void {
    this.detailM = m;
  }

  /**
   * The pixel error (CSS pixels) and cache budget (bytes), from the settings menu. The error scale
   * starts over from 1, so the view re-fits the new budget at once rather than a step per 10 s.
   */
  setQuality(pixelError: number, cacheBytes: number): void {
    if (pixelError === this.options.pixelError && cacheBytes === this.options.cacheBytes) return;
    this.options = { ...this.options, pixelError, cacheBytes };
    this.errorScale = 1;
    this.tooFine = null;
    this.lastScaleMs = null;
    this.lastStepFiner = false;
  }

  /** Full-chunk mode (§6.3): nothing is generated here; every section comes from the server. */
  setFullMode(full: boolean): void {
    this.fullMode = full;
  }

  /** A level-0 chunk is drawn when its level-1 section is refined (or before LOD is ready). */
  chunkVisible(coord: ChunkCoord): boolean {
    if (!this.active) return true;
    const [, i, j, k] = lodParent(lodOfChunk(coord));
    return this.refined.has(lodId(1, i, j, k));
  }

  lastSelection(): Selection {
    return this.selection;
  }

  onMessage(m: LodMessage, byteLength: number, nowMs: number): void {
    this.received.push({ at: nowMs, bytes: byteLength });
    if (m.type === MessageType.LodIndex || m.type === MessageType.LodIndexUpdate) {
      this.onIndex(m.entries);
      return;
    }
    const node = this.nodes.get(lodId(m.level, ...m.section));
    if (!node) return; // evicted meanwhile
    this.asking.delete(node.id);
    node.asked = null;
    node.needsAnswer = false;
    node.stale = false;
    if (m.form === LodForm.Unchanged) return;
    if (m.form === LodForm.Generated) {
      // Nothing modified here or below: generate it (an old mesh stays drawn until replaced).
      if (node.modified) {
        node.modified = false;
        this.setCells(node, null);
        node.remesh = true;
      }
      node.revision = 0;
      return;
    }
    // Explicit: new content, which may also change what lies under it.
    const changed = node.revision !== m.revision || !node.modified;
    node.modified = true;
    node.revision = m.revision;
    node.kind = LodKind.Content;
    this.setCells(node, m.cells ? new Uint16Array(m.cells) : null);
    node.remesh = true;
    if (changed) this.childrenStale(node);
    for (const d of NEIGHBOURS) {
      const n = this.nodes.get(neighbourId(node.coord, d));
      if (n && (n.meshed || n.cells)) n.remesh = true;
    }
  }

  /**
   * One frame: selects what to draw for `camera`, hands the renderer its sections, and starts the
   * jobs and requests the view is waiting for.
   */
  update(camera: LodCamera, nowMs: number): void {
    this.frame++;
    const p = camera.position;
    const viewFace: FaceSign = p[1] >= World.midplaneY ? 1 : -1;
    if (viewFace !== this.viewFace) this.faceChanged();
    this.viewFace = viewFace;
    this.nearRim = Math.hypot(p[0], p[2]) > World.worldRadius - RIM_VIEW_M;
    this.position = p;
    const frustum = new Frustum(camera);
    this.ahead = lookahead(camera);
    this.cameraChunk = camera.position.map((v) => Math.floor(v / CHUNK_SIZE)) as ChunkCoord;
    this.wantedChunks = [];
    this.wanted = [];
    this.due = [];
    this.remeshed = [];
    this.inUse = 0;
    const selection: Selection = { drawn: [], empty: [], chunks: [], standIns: [] };
    this.refined.clear();
    this.findCovered(camera.position, nowMs);
    const root = this.node(ROOT, null);
    this.touch(root);
    this.active = this.ready(root);
    if (this.active) this.visit(root, frustum, selection, null);
    else this.wanted.push(root);
    this.selection = selection;

    // Skirts close cracks where the neighbour is not drawn at the same level.
    const visible = new Map<number, number>();
    for (const c of selection.drawn) {
      let mask = 0;
      for (let face = 0; face < 6; face++) {
        const o = this.nodes.get(neighbourId(c, NEIGHBOURS[face] ?? [0, 0, 0]));
        if (o && (o.drawnFrame === this.frame || o.kind === LodKind.Buried)) continue;
        mask |= 1 << face;
      }
      visible.set(lodId(...c), mask);
    }
    this.view.showLodSections(visible);
    this.view.showLodStandIns(
      selection.standIns.map((s) => ({ id: lodId(...s.from), ...this.box(s.hole) })),
    );

    this.schedule(frustum);
    this.sendRequests(nowMs);
    this.sendChunkRequests(nowMs);
    this.evict();
    this.adapt(camera.position, nowMs);
    this.stats = this.computeStats(nowMs);
  }

  debugStats(): LodStats {
    return (
      this.stats ?? {
        drawn: [],
        chunkSections: 0,
        nodes: 0,
        generating: 0,
        meshing: 0,
        asking: 0,
        cacheBytes: 0,
        inUseBytes: 0,
        errorScale: 1,
        bytesPerSecond: 0,
      }
    );
  }

  // --- selection -------------------------------------------------------------------------------

  /** A section wholly on the face the camera is not on, far from it: not loaded or drawn. */
  private culled(n: Node): boolean {
    if (this.nearRim || n.coord[0] > MAX_ALIGNED_LEVEL) return false;
    const onB = isFaceBSection(n.coord);
    if (onB === (this.viewFace === -1)) return false;
    return boxDistance(this.position, n.lo, n.hi) > OTHER_FACE_NEAR_M;
  }

  /** Level ≥ 7 sections are meshed for the viewer's face only: mesh them again on a change. */
  private faceChanged(): void {
    for (const n of this.nodes.values()) {
      if (n.coord[0] > MAX_ALIGNED_LEVEL && n.meshed) n.remesh = true;
    }
  }

  private touch(n: Node): void {
    if (n.lastUsed !== this.frame) this.inUse += n.bytes + (n.cells?.byteLength ?? 0);
    n.lastUsed = this.frame;
    if ((n.needsAnswer || n.stale) && n.asked === null) this.due.push(n);
    if (n.remesh) this.remeshed.push(n);
  }

  private visit(node: Node, frustum: Frustum, selection: Selection, fallback: Node | null): void {
    if (this.culled(node)) {
      selection.empty.push(node.coord);
      return;
    }
    // A node that is not ready is only reached on the way down to the covered chunks.
    const ready = this.ready(node);
    // Where this branch leaves a hole, the nearest ready mesh above it is drawn clipped to it.
    const next = ready && node.kind === LodKind.Content && node.meshed ? node : fallback;
    if (ready && node.kind === LodKind.Empty) {
      selection.empty.push(node.coord);
      return;
    }
    if (ready && node.kind === LodKind.Buried) {
      this.visitBuried(node, frustum, selection);
      return;
    }
    if (node.coord[0] === 1) {
      // Level 0 is the streamed chunks: all 8 must be drawable. Those the view does not stream are
      // asked for; the section stays drawn until they arrive (no wait once they have).
      const refine = ready && this.refineToChunks(node, frustum);
      if (refine) this.wantChunks(node, frustum);
      if ((this.covered.has(node.id) && (!ready || refine)) || (refine && this.allDrawable(node))) {
        selection.chunks.push(node.coord);
        this.refined.add(node.id);
      } else if (ready) {
        this.draw(node, selection);
      } else if (fallback) {
        this.standIn(node, fallback, selection);
      }
      return;
    }
    if (!ready) {
      // On the way to the covered chunks: descend; everything else here is a stand-in.
      if (this.coveredAncestors.has(node.id)) this.visitKids(node, frustum, selection, next);
      else if (fallback) this.standIn(node, fallback, selection);
      return;
    }
    if (!this.refine(node, frustum)) {
      this.draw(node, selection);
      return;
    }
    if (!node.kids) {
      node.kids = [];
      for (let o = 0; o < 8; o++) {
        const c = lodChild(node.coord, o);
        if (lodInWorld(c)) node.kids.push(this.node(c, node));
      }
    }
    let allReady = true;
    for (const kid of node.kids) {
      if (this.culled(kid)) continue;
      this.touch(kid);
      if (!this.ready(kid)) {
        allReady = false;
        this.wanted.push(kid);
      }
    }
    if (!allReady) {
      // Coarse until the children are ready — unless that would hide the player's surroundings.
      if (this.coveredAncestors.has(node.id)) {
        for (const kid of node.kids) this.visit(kid, frustum, selection, next);
      } else {
        this.draw(node, selection);
      }
      return;
    }
    for (const kid of node.kids) this.visit(kid, frustum, selection, next);
  }

  private standIn(hole: Node, from: Node, selection: Selection): void {
    this.touch(from); // keeps it in the cache while it stands in
    selection.standIns.push({ hole: hole.coord, from: from.coord });
  }

  /**
   * A buried section: its LOD cells are solid rock with nothing to draw (the distant view leaves
   * out deep caves, §6.6). Up close the streamed chunks are the truth, caves and all, so a buried
   * section the camera would refine descends to them; its children are buried too (finer cells
   * only raise a column's lower bound). Where no chunks are drawable it stays undrawn. Buried
   * chunks are never asked for (wantChunks): only those the view streams anyway are drawn, as
   * asking for all the rock within the full-detail distance loaded hundreds of chunks for nothing.
   */
  private visitBuried(node: Node, frustum: Frustum, selection: Selection): void {
    if (this.culled(node)) {
      selection.empty.push(node.coord);
      return;
    }
    if (node.coord[0] === 1) {
      const refine = this.refineToChunks(node, frustum);
      if (refine && (this.covered.has(node.id) || this.allDrawable(node))) {
        selection.chunks.push(node.coord);
        this.refined.add(node.id);
      } else {
        selection.empty.push(node.coord);
      }
      return;
    }
    if (!this.refine(node, frustum)) {
      selection.empty.push(node.coord);
      return;
    }
    if (!node.kids) {
      node.kids = [];
      for (let o = 0; o < 8; o++) {
        const c = lodChild(node.coord, o);
        if (lodInWorld(c)) node.kids.push(this.node(c, node));
      }
    }
    let allReady = true;
    for (const kid of node.kids) {
      if (this.culled(kid)) continue;
      this.touch(kid);
      if (!this.ready(kid)) {
        allReady = false;
        this.wanted.push(kid);
      }
    }
    if (allReady || this.coveredAncestors.has(node.id)) {
      for (const kid of node.kids) this.visit(kid, frustum, selection, null);
    } else {
      selection.empty.push(node.coord); // undrawn as a whole until the children are known
    }
  }

  /** Visits the children of a node that is not ready itself (on the way to covered chunks). */
  private visitKids(
    node: Node,
    frustum: Frustum,
    selection: Selection,
    fallback: Node | null,
  ): void {
    if (!node.kids) {
      node.kids = [];
      for (let o = 0; o < 8; o++) {
        const c = lodChild(node.coord, o);
        if (lodInWorld(c)) node.kids.push(this.node(c, node));
      }
    }
    for (const kid of node.kids) {
      if (this.culled(kid)) continue;
      this.touch(kid);
      if (!this.ready(kid)) this.wanted.push(kid);
      this.visit(kid, frustum, selection, fallback);
    }
  }

  /**
   * Level-1 sections near `p` whose chunks have all been drawable for FORCE_CHUNKS_AFTER_MS, and
   * their ancestors. Normally the traversal reaches them long before; the grace period keeps
   * holes (unready siblings on the forced path) to devices whose LOD is stalled or very slow.
   */
  private findCovered(p: Vec3, nowMs: number): void {
    this.covered.clear();
    this.coveredAncestors.clear();
    const seen = new Set<number>();
    const [, ci, cj, ck] = sectionAt(1, p);
    const r = 2; // the streamed sphere (VIEW_RADIUS_CHUNKS) spans ±2 level-1 sections
    for (let j = cj - r; j <= cj + r; j++) {
      for (let k = ck - r; k <= ck + r; k++) {
        for (let i = ci - r; i <= ci + r; i++) {
          const c: LodCoord = [1, i, j, k];
          if (!lodInWorld(c)) continue;
          let all = true;
          for (let o = 0; o < 8 && all; o++) all = this.chunks.drawable(chunkOfLod(lodChild(c, o)));
          if (!all) continue;
          const id = lodId(...c);
          seen.add(id);
          const since = this.drawableSince.get(id) ?? nowMs;
          this.drawableSince.set(id, since);
          if (nowMs - since < FORCE_CHUNKS_AFTER_MS) continue;
          this.covered.add(id);
          for (let a = lodParent(c); ; a = lodParent(a)) {
            const id = lodId(...a);
            if (this.coveredAncestors.has(id)) break;
            this.coveredAncestors.add(id);
            if (a[0] === MAX_LEVEL) break;
          }
        }
      }
    }
    for (const id of this.drawableSince.keys()) if (!seen.has(id)) this.drawableSince.delete(id);
  }

  private draw(node: Node, selection: Selection): void {
    node.drawnFrame = this.frame;
    selection.drawn.push(node.coord);
  }

  /**
   * Refine while cells project larger than the pixel error — in every direction, not only in
   * view: detail depends on distance alone, so turning shows what is already loaded instead of
   * popping in. Where the camera looks only decides what loads first (schedule).
   */
  private refine(node: Node, frustum: Frustum): boolean {
    const d = Math.max(1, this.distance(node.lo, node.hi, frustum));
    // Down to the full-detail distance whatever the pixel error (so level 1 is reached there).
    if (this.detailM !== null && d < this.detailM) return true;
    return (
      (cellSize(node.coord[0]) / d) * frustum.pixelsPerRadian >
      this.options.pixelError * this.errorScale
    );
  }

  /** A level-1 section shows as its chunks: within the full-detail distance (if set). */
  private refineToChunks(node: Node, frustum: Frustum): boolean {
    if (this.detailM === null) return this.refine(node, frustum);
    return this.distance(node.lo, node.hi, frustum) < this.detailM;
  }

  /** Distance to a box from the camera, or from where it is heading if that is nearer. */
  private distance(lo: Vec3, hi: Vec3, frustum: Frustum): number {
    const d = frustum.distance(lo, hi);
    return this.ahead ? Math.min(d, boxDistance(this.ahead, lo, hi)) : d;
  }

  private loaded(c: ChunkCoord): boolean {
    return this.chunks.isLoaded ? this.chunks.isLoaded(c) : this.chunks.drawable(c);
  }

  private allDrawable(node: Node): boolean {
    for (let o = 0; o < 8; o++) {
      if (!this.chunks.drawable(chunkOfLod(lodChild(node.coord, o)))) return false;
    }
    return true;
  }

  /** The chunks of a refined level-1 section, to ask the server for (see sendChunkRequests). */
  private wantChunks(node: Node, frustum: Frustum): void {
    if (!this.options.requestChunks || this.fullMode) return;
    for (let o = 0; o < 8; o++) {
      const c = chunkOfLod(lodChild(node.coord, o));
      const dx = c[0] - this.cameraChunk[0];
      const dy = c[1] - this.cameraChunk[1];
      const dz = c[2] - this.cameraChunk[2];
      const r = CHUNK_REQUEST_RADIUS;
      if (dx * dx + dy * dy + dz * dz > r * r + r) continue;
      if (this.chunkAsked.has(chunkKey(c))) continue;
      const lo: Vec3 = [c[0] * CHUNK_SIZE, c[1] * CHUNK_SIZE, c[2] * CHUNK_SIZE];
      const hi: Vec3 = [lo[0] + CHUNK_SIZE, lo[1] + CHUNK_SIZE, lo[2] + CHUNK_SIZE];
      this.wantedChunks.push({ coord: c, d: this.distance(lo, hi, frustum) });
    }
  }

  /**
   * Asks for the wanted chunks, nearest first, paced under CHUNK_REQUESTS_PER_SECOND. Chunks the
   * client already holds are asked for once too: the server then keeps them while they are within
   * its render radius, rather than unloading them when they leave the view.
   */
  private sendChunkRequests(nowMs: number): void {
    const send = this.options.requestChunks;
    if (!send) return;
    const rate = this.options.chunkRequestsPerSecond ?? World.chunkRequestsPerSecond - 16;
    this.chunkCredit =
      this.lastChunkRequestMs === null
        ? rate
        : Math.min(rate, this.chunkCredit + ((nowMs - this.lastChunkRequestMs) / 1000) * rate);
    this.lastChunkRequestMs = nowMs;
    // Forget requests whose chunk never came (or has since been unloaded): asked again if wanted.
    for (const [key, { coord, at }] of this.chunkAsked) {
      if (nowMs - at > CHUNK_REQUEST_RETRY_MS && !this.loaded(coord)) this.chunkAsked.delete(key);
    }
    if (this.wantedChunks.length === 0) return;
    this.wantedChunks.sort((a, b) => a.d - b.d);
    let batch: ChunkCoord[] = [];
    for (const { coord } of this.wantedChunks) {
      if (this.chunkCredit < 1) break;
      const key = chunkKey(coord);
      if (this.chunkAsked.has(key)) continue;
      this.chunkAsked.set(key, { coord, at: nowMs });
      this.chunkCredit -= 1;
      batch.push(coord);
      if (batch.length === Limits.maxResyncChunks) {
        send(batch);
        batch = [];
      }
    }
    if (batch.length > 0) send(batch);
  }

  /** Out of view, and not close (a section beside the camera can reach into the view). */
  private offView(node: Node, frustum: Frustum, d: number): boolean {
    return d > sectionSize(node.coord[0]) && !frustum.intersects(node.lo, node.hi);
  }

  private box(c: LodCoord): { lo: Vec3; hi: Vec3 } {
    const o = sectionOrigin(c);
    const s = sectionSize(c[0]);
    return { lo: o, hi: [o[0] + s, o[1] + s, o[2] + s] };
  }

  private ready(node: Node): boolean {
    if (node.needsAnswer || node.kind === null) return false;
    return node.kind !== LodKind.Content || node.meshed;
  }

  private node(c: LodCoord, parent: Node | null): Node {
    const id = lodId(...c);
    let n = this.nodes.get(id);
    if (!n) {
      n = {
        coord: c,
        id,
        ...this.box(c),
        parent,
        kids: null,
        kind: null,
        modified: false,
        revision: 0,
        needsAnswer: this.fullMode || this.possiblyModified(c, parent),
        stale: false,
        asked: null,
        generating: false,
        meshing: false,
        cells: null,
        surface: null,
        surfaceB: null,
        meshed: false,
        remesh: false,
        bytes: 0,
        lastUsed: this.frame,
        drawnFrame: 0,
        token: 0,
      };
      this.nodes.set(id, n);
    }
    return n;
  }

  /** From the index at level ≥ 8; below it, exactly when the parent's content came modified. */
  private possiblyModified(c: LodCoord, parent: Node | null): boolean {
    if (c[0] >= INDEX_LEVEL) return this.indexed.has(lodId(...c));
    return parent?.modified ?? false;
  }

  // --- the index and requests ----------------------------------------------------------------

  private onIndex(entries: LodIndexEntry[]): void {
    for (const e of entries) {
      const key = lodId(INDEX_LEVEL, e.i, e.j, e.k);
      if (key < 0 || this.index.get(key) === e.revision) continue;
      this.index.set(key, e.revision);
      // The entry's section and its ancestors are (now differently) modified: ask again.
      let c: LodCoord = [INDEX_LEVEL, e.i, e.j, e.k];
      for (;;) {
        const id = lodId(...c);
        this.indexed.add(id);
        const n = this.nodes.get(id);
        if (n && !n.needsAnswer) {
          if (n.kind === null) n.needsAnswer = true;
          else n.stale = true;
        }
        if (c[0] === MAX_LEVEL) break;
        c = lodParent(c);
      }
    }
  }

  /** A section's content changed: its held children must ask the server what they hold now. */
  private childrenStale(node: Node): void {
    for (const kid of node.kids ?? []) {
      if (kid.needsAnswer) continue;
      // At or above the index level the index alone says what is modified.
      if (kid.coord[0] >= INDEX_LEVEL && !this.fullMode && !this.indexed.has(kid.id)) continue;
      if (kid.kind === null) kid.needsAnswer = true;
      else kid.stale = true;
    }
  }

  private rate(): number {
    return this.options.requestsPerSecond ?? Lod.requestsPerSecond - 4;
  }

  private sendRequests(nowMs: number): void {
    const rate = this.rate();
    if (this.lastRequestMs !== null) {
      this.requestCredit = Math.min(
        rate,
        this.requestCredit + ((nowMs - this.lastRequestMs) / 1000) * rate,
      );
    }
    this.lastRequestMs = nowMs;
    const timeout = this.options.requestTimeoutMs ?? 5000;
    for (const [id, n] of this.asking) {
      if (n.asked !== null && nowMs - n.asked > timeout) {
        n.asked = null; // unanswered (dropped by the server's rate limit): asked again when due
        this.asking.delete(id);
      }
    }
    if (this.due.length === 0) return;
    this.due.sort((a, b) => b.coord[0] - a.coord[0]);
    const batch: LodSectionRequest[] = [];
    for (const n of this.due) {
      if (this.requestCredit < 1) break;
      if (n.asked !== null) continue;
      this.requestCredit -= 1;
      n.asked = nowMs;
      this.asking.set(n.id, n);
      batch.push({
        level: n.coord[0],
        section: [n.coord[1], n.coord[2], n.coord[3]],
        knownRevision: n.modified ? n.revision : 0,
      });
      if (batch.length === Lod.maxRequestSections) this.request(batch.splice(0));
    }
    if (batch.length > 0) this.request(batch);
  }

  // --- jobs ------------------------------------------------------------------------------------

  /** By projected cell size: bounds, generation and meshing for what the view waits for. */
  private schedule(frustum: Frustum): void {
    const wanted = this.wanted;
    for (const n of this.remeshed) if (this.ready(n)) wanted.push(n);
    if (wanted.length === 0) return;
    // Largest projected cells first: coarse before fine and near before far, so the whole view
    // appears coarse at once and sharpens — nearest first, rather than the whole horizon's
    // coarse levels before the camera's own ground. A parent always outranks its children.
    const priority = new Map<Node, number>();
    for (const n of wanted) {
      const d = Math.max(1, this.distance(n.lo, n.hi, frustum));
      // What the view shows now goes first; the ring around it is loaded behind.
      const scale = this.offView(n, frustum, d) ? OFF_VIEW_PRIORITY : 1;
      priority.set(n, (cellSize(n.coord[0]) / d) * scale);
    }
    wanted.sort((a, b) => (priority.get(b) ?? 0) - (priority.get(a) ?? 0));
    for (const n of wanted) {
      if (
        this.generating >= this.options.maxGenerationJobs &&
        this.meshing >= this.options.maxMeshJobs
      ) {
        break;
      }
      if (n.needsAnswer) continue; // waiting for the server
      if (n.kind === null) {
        this.classify(n);
      } else if (n.kind === LodKind.Content && (!n.meshed || n.remesh)) {
        if (n.cells) this.mesh(n);
        else if (!n.modified) this.generate(n);
      }
    }
  }

  /** Empty and buried sections from their column's bounds; the rest generated. */
  private classify(n: Node): void {
    const [level, i, , k] = n.coord;
    const key = boundsKey(level, i, k);
    const b = this.bounds.get(key);
    if (b === 'pending') return;
    if (!b) {
      if (this.generating >= this.options.maxGenerationJobs) return;
      this.generating++;
      this.bounds.set(key, 'pending');
      this.sections.lodBounds(level, i, k).then(
        (r) => {
          this.generating--;
          this.bounds.set(key, r);
        },
        () => {
          this.generating--;
          this.bounds.delete(key);
        },
      );
      return;
    }
    const kind = kindFromBounds(n.coord, b);
    if (kind !== LodKind.Content && !this.neighbourModified(n)) {
      n.kind = kind;
      return;
    }
    // Content, or buried next to a modified section (which may expose its border).
    n.kind = LodKind.Content;
    this.generate(n);
  }

  private generate(n: Node): void {
    if (n.generating || this.generating >= this.options.maxGenerationJobs) return;
    n.generating = true;
    this.generating++;
    const token = ++this.token;
    n.token = token;
    this.sections.lod(n.coord).then(
      (s) => {
        this.generating--;
        n.generating = false;
        if (n.token !== token || n.modified || this.nodes.get(n.id) !== n) return;
        this.setCells(n, s.cells);
        // A face-B section's surfaces are face-local, those of its mirror image.
        if (n.coord[0] > MAX_ALIGNED_LEVEL) {
          // May hold rows of both faces: meshed for the viewer's face (see mesh).
          n.surface = s.surface ?? null;
          n.surfaceB = s.surfaceB ?? null;
        } else {
          n.surface = (isFaceBSection(n.coord) ? s.surfaceB : s.surface) ?? null;
        }
        n.remesh = true;
      },
      () => {
        this.generating--;
        n.generating = false;
      },
    );
  }

  private mesh(n: Node): void {
    if (n.meshing || !n.cells || this.meshing >= this.options.maxMeshJobs) return;
    const cells = new Uint16Array(n.cells);
    this.patchApron(n, cells);
    n.meshing = true;
    n.remesh = false;
    this.meshing++;
    const token = ++this.token;
    n.token = token;
    // Level ≥ 7 sections may hold rows of both faces: the other face's rows become stone (nothing
    // is drawn against stone), and the tints are the viewer's face's.
    const both = n.coord[0] > MAX_ALIGNED_LEVEL && !this.nearRim;
    if (both) this.blankOtherFace(n, cells);
    const viewerB = both && this.viewFace === -1;
    const options = {
      surface: viewerB ? null : this.surfaceInCells(n),
      ...(both ? { tint: viewerB ? n.surfaceB : n.surface } : {}),
      waterDrop: CHUNK_WATER_DROP_M / cellSize(n.coord[0]),
      // Distant terrain as facets, not terraces (SLOPE_BLOCKS.md §3.2), at every level.
      slopes: true,
      // Face B's terrain faces down: meshed as its mirror image, turned back (lodMesher.ts).
      mirror: isFaceBSection(n.coord),
    };
    void this.mesher.meshSection(cells, options).then((meshes) => {
      this.meshing--;
      n.meshing = false;
      if (n.token !== token || this.nodes.get(n.id) !== n) return;
      this.cacheBytes -= n.bytes;
      n.bytes = sectionBytes(meshes);
      this.cacheBytes += n.bytes;
      this.view.setLodSection(n.id, sectionOrigin(n.coord), cellSize(n.coord[0]), meshes);
      n.meshed = true;
      if (!n.modified) this.setCells(n, null); // regenerated if a neighbour's border changes
    });
  }

  private blankOtherFace(n: Node, cells: Uint16Array): void {
    const stone = stateId('dwell:stone');
    const viewer = this.viewFace === 1 ? FACE_A : FACE_B;
    for (let r = -1; r <= SECTION_CELLS; r++) {
      if (lodRowFace(n.coord, r) === viewer) continue;
      for (let z = -1; z <= SECTION_CELLS; z++) {
        for (let x = -1; x <= SECTION_CELLS; x++) cells[lodCell(x, r, z)] = stone;
      }
    }
  }

  private setCells(n: Node, cells: Uint16Array<ArrayBuffer> | null): void {
    this.cacheBytes += (cells?.byteLength ?? 0) - (n.cells?.byteLength ?? 0);
    n.cells = cells;
    if (!cells) {
      n.surface = null;
      n.surfaceB = null;
    }
  }

  /** The section's column surfaces with heights in cells from its bottom (the mesher's units). */
  private surfaceInCells(n: Node): Float32Array<ArrayBuffer> | null {
    if (!n.surface || n.modified) return null;
    const out = new Float32Array(n.surface);
    const y0 = isFaceBSection(n.coord) ? mirrorSectionOrigin(n.coord) : sectionOrigin(n.coord)[1];
    const size = cellSize(n.coord[0]);
    for (let i = 0; i < out.length; i += SURFACE_STRIDE) {
      out[i] = ((out[i] ?? 0) - y0) / size;
      out[i + 3] = ((out[i + 3] ?? 0) - y0) / size; // the water's level (where wet)
    }
    return out;
  }

  /** Borders of modified same-level neighbours replace a generated apron (it assumed generation). */
  private patchApron(n: Node, cells: Uint16Array): void {
    const N = SECTION_CELLS;
    NEIGHBOURS.forEach((d, face) => {
      const other = this.nodes.get(neighbourId(n.coord, d));
      if (!other?.modified || !other.cells || other === n) return;
      const theirCells = other.cells;
      const axis = face >> 1;
      const ours = d[axis] === 1 ? N : -1;
      const theirs = d[axis] === 1 ? 0 : N - 1;
      const at = (layer: number, a: number, b: number): number =>
        axis === 0
          ? lodCell(layer, a, b)
          : axis === 1
            ? lodCell(a, layer, b)
            : lodCell(a, b, layer);
      for (let a = 0; a < N; a++) {
        for (let b = 0; b < N; b++) cells[at(ours, a, b)] = theirCells[at(theirs, a, b)] ?? 0;
      }
    });
  }

  private neighbourModified(n: Node): boolean {
    return NEIGHBOURS.some((d) => this.nodes.get(neighbourId(n.coord, d))?.modified ?? false);
  }

  // --- cache ---------------------------------------------------------------------------------

  /** Coarser while the view's sections do not fit the cache, finer again once well within it. */
  private adapt(position: Vec3, nowMs: number): void {
    if (
      this.tooFine &&
      boxDistance(position, this.tooFine.at, this.tooFine.at) > RETRY_FINER_AFTER_M
    ) {
      this.tooFine = null;
    }
    const since = this.lastScaleMs === null ? Infinity : nowMs - this.lastScaleMs;
    const budget = this.options.cacheBytes;
    if (this.inUse > budget && since >= COARSER_AFTER_MS) {
      if (this.errorScale >= MAX_ERROR_SCALE) return;
      if (this.lastStepFiner) this.tooFine = { scale: this.errorScale, at: [...position] };
      this.errorScale = Math.min(MAX_ERROR_SCALE, this.errorScale * ERROR_SCALE_STEP);
      this.lastScaleMs = nowMs;
      this.lastStepFiner = false;
    } else if (this.inUse < budget * ROOM && this.errorScale > 1 && since >= FINER_AFTER_MS) {
      const finer = Math.max(1, this.errorScale / ERROR_SCALE_STEP);
      if (this.tooFine && finer <= this.tooFine.scale * 1.001) return;
      this.errorScale = finer;
      this.lastScaleMs = nowMs;
      this.lastStepFiner = true;
    }
  }

  /**
   * While over the cache budget, drops sets of children the traversal no longer visits, least
   * recently used first (from the leaves up: a node's children go before the node).
   */
  private evict(): void {
    if (this.cacheBytes <= this.options.cacheBytes) return;
    const candidates: Node[] = [];
    for (const n of this.nodes.values()) {
      const kids = n.kids;
      if (!kids) continue;
      if (
        kids.every((k) => k.kids === null && k.lastUsed < this.frame && !k.generating && !k.meshing)
      ) {
        candidates.push(n);
      }
    }
    const last = (n: Node): number => Math.max(...(n.kids ?? []).map((k) => k.lastUsed));
    candidates.sort((a, b) => last(a) - last(b));
    for (const n of candidates) {
      if (this.cacheBytes <= this.options.cacheBytes * 0.9) break;
      for (const kid of n.kids ?? []) this.remove(kid);
      n.kids = null;
    }
  }

  private remove(n: Node): void {
    if (n.meshed) this.view.setLodSection(n.id, [0, 0, 0], 1, null);
    this.cacheBytes -= n.bytes;
    this.setCells(n, null);
    this.nodes.delete(n.id);
    this.asking.delete(n.id);
  }

  private computeStats(nowMs: number): LodStats {
    while (this.received.length > 0 && (this.received[0]?.at ?? nowMs) < nowMs - 1000) {
      this.received.shift();
    }
    const drawn: number[] = new Array<number>(MAX_LEVEL + 1).fill(0);
    for (const c of this.selection.drawn) drawn[c[0]] = (drawn[c[0]] ?? 0) + 1;
    return {
      drawn,
      chunkSections: this.selection.chunks.length,
      nodes: this.nodes.size,
      generating: this.generating,
      meshing: this.meshing,
      asking: this.asking.size,
      cacheBytes: this.cacheBytes,
      inUseBytes: this.inUse,
      errorScale: this.errorScale,
      bytesPerSecond: this.received.reduce((n, r) => n + r.bytes, 0),
    };
  }
}

/** Lines for the F3 overlay. */
export function formatLodStats(s: LodStats): string {
  const levels = s.drawn
    .map((n, level) => (n > 0 ? `L${String(level)}:${String(n)}` : ''))
    .filter(Boolean)
    .join(' ');
  return [
    `LOD ${levels || '—'} · chunks×${String(s.chunkSections)} · nodes ${String(s.nodes)}`,
    `LOD jobs gen ${String(s.generating)} mesh ${String(s.meshing)} ask ${String(s.asking)} · ` +
      `cache ${(s.cacheBytes / 1048576).toFixed(1)} MB (view ${(s.inUseBytes / 1048576).toFixed(1)}) · ` +
      `error ×${s.errorScale.toFixed(2)} · ${(s.bytesPerSecond / 1024).toFixed(1)} KB/s`,
  ].join('\n');
}

const chunkKey = (c: ChunkCoord): string => `${String(c[0])},${String(c[1])},${String(c[2])}`;

/** Where the camera will be LOOKAHEAD_S from now (at most MAX_LOOKAHEAD_M away), if it moves. */
export function lookahead(camera: LodCamera): Vec3 | null {
  const v = camera.velocity;
  if (!v) return null;
  const speed = Math.hypot(v[0], v[1], v[2]);
  if (speed < 0.5) return null;
  const t = Math.min(LOOKAHEAD_S, MAX_LOOKAHEAD_M / speed);
  const p = camera.position;
  return [p[0] + v[0] * t, p[1] + v[1] * t, p[2] + v[2] * t];
}

function boxDistance(p: Vec3, lo: Vec3, hi: Vec3): number {
  let d = 0;
  for (let a = 0; a < 3; a++) {
    const v = p[a] ?? 0;
    const e = v < (lo[a] ?? 0) ? (lo[a] ?? 0) - v : v > (hi[a] ?? 0) ? v - (hi[a] ?? 0) : 0;
    d += e * e;
  }
  return Math.sqrt(d);
}
