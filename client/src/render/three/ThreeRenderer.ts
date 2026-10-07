import {
  BoxGeometry,
  BufferAttribute,
  BufferGeometry,
  CapsuleGeometry,
  Color,
  DataTexture,
  DirectionalLight,
  DoubleSide,
  EdgesGeometry,
  Group,
  ACESFilmicToneMapping,
  HemisphereLight,
  LinearMipmapLinearFilter,
  LineBasicMaterial,
  LineSegments,
  Mesh,
  MeshLambertMaterial,
  NearestFilter,
  Object3D,
  PerspectiveCamera,
  RGBAFormat,
  Scene,
  SRGBColorSpace,
  Vector2,
  Vector3,
  WebGLRenderer,
  type MeshLambertMaterialParameters,
} from 'three';
import type { ChunkCoord, Vec3 } from '../../protocol/messages';
import { viewForward } from '../../world/face';
import type { FlatMesh, SectionMeshes } from '../../mesh/lodMesher';
import type { ChunkMeshes, MeshArrays } from '../../mesh/mesher';
import { CHUNK_SIZE, Lod, World } from '../../protocol/constants.gen';
import { debugLineArrays, type DebugSegment } from '../debugLines';
import { DEFAULT_FOG, type FogSettings } from '../fog';
import { VERTICAL_FOV, verticalFov } from '../fov';
import {
  DEFAULT_EXPOSURE,
  HORIZON_COLOR,
  LIGHT,
  MOON_DIRECTION_WORLD,
  SUN_DIRECTION,
  sanitizeExposure,
} from '../look';
import { sharedAtlas } from '../textures';
import {
  RendererUnavailableError,
  type PlayerView,
  type Renderer,
  type RendererOptions,
  type RenderStats,
} from '../Renderer';
import { setFogUniforms, setViewFace, withHeightFog } from './heightFog';
import { boxClipPlanes } from './clipBox';
import { Sky } from './sky';
import { LodSectionGeometry, releaseOnUpload } from './lodSection';
import { BatchedTerrain } from './batchedTerrain';
import { type BatchHandle, geometryBytes, MeshBatch } from './meshBatch';

/** Near/far depth split (§6.6): LOD beyond it in a far pass, then a depth clear and a near pass. */
const NEAR_SPLIT = Lod.nearSplitM;
const FAR_PLANE = 5e7;
/** Most LOD stand-ins drawn per frame (clipped ancestor meshes, §6.6). */
const MAX_STAND_INS = 64;

function flatGeometry(m: FlatMesh): BufferGeometry | null {
  if (m.indices.length === 0) return null;
  const g = new BufferGeometry();
  g.setAttribute('position', new BufferAttribute(m.positions, 3));
  g.setAttribute('normal', new BufferAttribute(m.normals, 3));
  g.setAttribute('color', new BufferAttribute(m.colors, 3));
  g.setIndex(new BufferAttribute(m.indices, 1));
  g.computeBoundingSphere();
  return g;
}

interface LodEntry {
  /** The section's surface and skirts, one draw call (lodSection.ts); null if it has none. */
  mesh: Mesh | null;
  section: LodSectionGeometry | null;
  level: number;
  /** The section's water in the shared batch (lodWater), if it has any. */
  water: BatchHandle | null;
  shown: boolean;
  /** Bytes of the section's own mesh (not its water, which the batch counts). */
  bytes: number;
}

/** Level tints for the debug per-level colouring (?lodcolors=1). */
const LEVEL_TINTS = [
  0xffffff, 0xff6060, 0xffb060, 0xffff60, 0x80ff60, 0x60ffd0, 0x60a0ff, 0x9060ff, 0xff60e0,
  0xff8080, 0xffd080, 0xffff90, 0xa0ff90, 0x90ffe0, 0x90c0ff, 0xb090ff, 0xff90f0, 0xc0c0c0,
  0x808080, 0x404040,
];

function geometryOf(arrays: MeshArrays): BufferGeometry | null {
  if (arrays.indices.length === 0) return null;
  const g = new BufferGeometry();
  g.setAttribute('position', new BufferAttribute(arrays.positions, 3));
  g.setAttribute('normal', new BufferAttribute(arrays.normals, 3));
  g.setAttribute('color', new BufferAttribute(arrays.colors, 3));
  g.setAttribute('uv', new BufferAttribute(arrays.uvs, 2));
  g.setAttribute('tint', new BufferAttribute(arrays.tints, 3));
  g.setAttribute('tile', new BufferAttribute(arrays.tiles, 4));
  g.setIndex(new BufferAttribute(arrays.indices, 1));
  g.computeBoundingSphere();
  // Static: the CPU copies go once uploaded (they were most of the page's memory as chunks loaded).
  for (const a of Object.values(g.attributes)) releaseOnUpload(a as BufferAttribute);
  releaseOnUpload(g.index as BufferAttribute);
  return g;
}

/**
 * A Lambert material for chunk meshes whose texture repeats once per block across greedy-merged
 * quads (mesh/mesher.ts): `uv` is in blocks and the `tile` attribute is the atlas rectangle, so the
 * shader samples tile.xy + fract(uv) × tile.zw. Gradients come from the unwrapped uv, so mip level
 * selection is continuous across block edges (no seams where fract wraps).
 */
function chunkMaterial(params: MeshLambertMaterialParameters): MeshLambertMaterial {
  const material = new MeshLambertMaterial(params);
  material.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader
      .replace(
        '#include <common>',
        '#include <common>\nattribute vec4 tile;\nattribute vec3 tint;\nvarying vec4 vTile;\nvarying vec3 vTint;',
      )
      .replace('#include <uv_vertex>', '#include <uv_vertex>\n\tvTile = tile;\n\tvTint = tint;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying vec4 vTile;\nvarying vec3 vTint;')
      .replace(
        '#include <map_fragment>',
        [
          '#ifdef USE_MAP',
          '\tvec2 atlasUv = vTile.xy + fract(vMapUv) * vTile.zw;',
          '\tvec4 texel = textureGrad(map, atlasUv, dFdx(vMapUv) * vTile.zw, dFdy(vMapUv) * vTile.zw);',
          '\tdiffuseColor *= texel;',
          // The atlas alpha is 1 − the share of a texel the biome tint colours (TINT_MASKS); the
          // opaque passes ignore alpha, water has none.
          '\tdiffuseColor.rgb *= mix(vTint, vec3(1.0), texel.a);',
          '#endif',
        ].join('\n'),
      );
  };
  material.customProgramCacheKey = () => 'dwell-chunk';
  return material;
}

interface PlayerMesh {
  group: Group;
  body: Mesh<CapsuleGeometry, MeshLambertMaterial>;
  key: string;
}

/** Three.js / WebGL2 implementation of the render interface (ADR 0002). */
export class ThreeRenderer implements Renderer {
  private readonly renderer: WebGLRenderer;
  private readonly scene = new Scene();
  /** The sky, drawn first in the far pass (look.ts, sky.ts). */
  private readonly skyScene = new Scene();
  private readonly sky = new Sky();
  private readonly camera = new PerspectiveCamera(VERTICAL_FOV, 1, 0.05, NEAR_SPLIT);
  /** Block textures: tiled-noise atlas (render/textures.ts), crisp up close, mipmapped far away. */
  private readonly atlas = ThreeRenderer.createAtlasTexture();
  private readonly opaqueMaterial = withHeightFog(
    chunkMaterial({ vertexColors: true, map: this.atlas }),
    'chunk',
  );
  private readonly waterMaterial = withHeightFog(
    chunkMaterial({
      vertexColors: true,
      map: this.atlas,
      transparent: true,
      opacity: 0.55,
      depthWrite: false,
      side: DoubleSide,
    }),
    'chunk',
  );
  private readonly chunks = new Map<string, Group>();
  /** Chunk groups' coordinates, for the LOD system's visibility (§6.6). */
  private readonly chunkCoords = new Map<string, ChunkCoord>();
  private chunkVisible: ((coord: ChunkCoord) => boolean) | null = null;
  private readonly lod = new Map<number, LodEntry>();
  private lodShown: ReadonlyMap<number, number> = new Map();
  private lodStandIns: readonly { id: number; lo: Vec3; hi: Vec3 }[] = [];
  private readonly standInSlots: Mesh[] = [];
  private warnedStandIns = false;
  private readonly lodMaterial = withHeightFog(new MeshLambertMaterial({ vertexColors: true }));
  /** Like the chunks' water (see-through from both sides, at their opacity), untextured. */
  private readonly lodWaterMaterial = withHeightFog(
    new MeshLambertMaterial({
      vertexColors: true,
      transparent: true,
      opacity: 0.55,
      depthWrite: false,
      side: DoubleSide,
    }),
  );
  private readonly lodWater = new MeshBatch(this.lodWaterMaterial);
  /** The chunks and LOD sections in batches (?batch=1), or null: a mesh each. */
  private readonly batch: BatchedTerrain | null;
  private readonly frameStats: RenderStats = {
    calls: 0,
    triangles: 0,
    batched: false,
    pixelRatio: 1,
    meshBytes: 0,
    screenBytes: 0,
  };
  /** Bytes of the separate chunk and LOD meshes (the batches count their own). */
  private separateBytes = 0;
  private readonly chunkBytes = new Map<string, number>();
  private readonly drawingBuffer = new Vector2();
  private lodLevelMaterials: MeshLambertMaterial[] | null = null;
  /** The far pass's camera (the main camera is the near pass's). */
  private readonly farCamera = new PerspectiveCamera(VERTICAL_FOV, 1, NEAR_SPLIT, FAR_PLANE);
  private readonly players = new Map<number, PlayerMesh>();
  /** The face of the player the camera is on (setCamera). */
  private face: 1 | -1 = 1;
  private debug: LineSegments<BufferGeometry, LineBasicMaterial> | null = null;
  /** Outline of the targeted block (§6.5): a unit box's edges, scaled for slabs. */
  private readonly outline = new LineSegments(
    new EdgesGeometry(new BoxGeometry(1.004, 1.004, 1.004).translate(0.5, 0.5, 0.5)),
    new LineBasicMaterial({ color: 0x101418, transparent: true, opacity: 0.8 }),
  );

  /** The shape about to be placed (SLOPE_BLOCKS.md §6): its edges, lighter than the block outline. */
  private readonly preview = new LineSegments(
    new BufferGeometry(),
    new LineBasicMaterial({ color: 0xfff6c8, transparent: true, opacity: 0.95 }),
  );

  constructor(canvas: HTMLCanvasElement, options: RendererOptions = {}) {
    if (!canvas.getContext('webgl2')) {
      throw new RendererUnavailableError('WebGL2 is not available on this device.');
    }
    this.renderer = new WebGLRenderer({ canvas, antialias: true });
    this.renderer.autoClear = false;
    this.renderer.localClippingEnabled = true; // LOD stand-ins are clipped to their holes
    this.renderer.setClearColor(new Color(...HORIZON_COLOR));
    // Tone mapping runs in each material's shader (no extra pass); the haze and the sky are mixed
    // in after it, in output colour.
    this.renderer.toneMapping = ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = DEFAULT_EXPOSURE;
    this.skyScene.add(this.sky.mesh);
    this.setFog(DEFAULT_FOG);
    // Face A: the sun and a day sky's ambient; face B: the counter-angled moon and a night's. Each
    // fragment takes only its own face's pair (three/heightFog.ts), chosen by its side of the
    // midplane: the lights have no shadows, so a ceiling would otherwise take the other face's.
    this.scene.add(
      new HemisphereLight(LIGHT.hemisphereSky, LIGHT.hemisphereGround, LIGHT.hemisphereIntensity),
    );
    const sun = new DirectionalLight(LIGHT.sun, LIGHT.sunIntensity);
    sun.position.set(...SUN_DIRECTION);
    this.scene.add(sun);
    const nightAmbient = new HemisphereLight(
      LIGHT.moonHemisphereSky,
      LIGHT.moonHemisphereGround,
      LIGHT.moonHemisphereIntensity,
    );
    nightAmbient.position.set(0, -1, 0); // its "sky" is below the disc
    this.scene.add(nightAmbient);
    const moon = new DirectionalLight(LIGHT.moon, LIGHT.moonIntensity);
    moon.position.set(...MOON_DIRECTION_WORLD);
    this.scene.add(moon);
    this.outline.visible = false;
    this.scene.add(this.outline);
    this.preview.visible = false;
    this.preview.frustumCulled = false;
    this.scene.add(this.preview);
    this.lodWater.mesh.renderOrder = 1;
    this.scene.add(this.lodWater.mesh);
    this.batch = options.batched
      ? new BatchedTerrain({
          chunk: this.opaqueMaterial,
          chunkWater: this.waterMaterial,
          lod: this.lodMaterial,
        })
      : null;
    this.frameStats.batched = this.batch !== null;
    if (this.batch) this.scene.add(...this.batch.meshes);
    this.camera.position.set(0, 6, 14);
    this.camera.lookAt(0, 0, 0);
    // Nearly everything in the scene is static terrain: world matrices are computed when an object
    // is placed (placed()), not for the whole scene in each of the frame's two passes — that was
    // the frame's largest script cost once a few thousand meshes had loaded.
    this.scene.matrixWorldAutoUpdate = false;
    this.scene.updateMatrixWorld(true);
  }

  /** Computes a placed object's world matrices (the scene does not, see the constructor). */
  private static placed<T extends Object3D>(object: T): T {
    object.updateMatrixWorld(true);
    return object;
  }

  private static createAtlasTexture(): DataTexture {
    const atlas = sharedAtlas();
    const texture = new DataTexture(atlas.data, atlas.size, atlas.size, RGBAFormat);
    texture.colorSpace = SRGBColorSpace;
    texture.magFilter = NearestFilter;
    texture.minFilter = LinearMipmapLinearFilter;
    texture.generateMipmaps = true;
    texture.anisotropy = 4;
    texture.needsUpdate = true;
    return texture;
  }

  resize(width: number, height: number, pixelRatio: number): void {
    this.renderer.setPixelRatio(pixelRatio);
    this.frameStats.pixelRatio = pixelRatio;
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / Math.max(1, height);
    this.camera.fov = verticalFov(this.camera.aspect);
    this.camera.updateProjectionMatrix();
  }

  renderFrame(): void {
    // Chunks the LOD draws instead are hidden; LOD sections show as listed, with their skirts.
    for (const [key, group] of this.chunks) {
      const c = this.chunkCoords.get(key);
      group.visible = !this.chunkVisible || !c || this.chunkVisible(c);
    }
    for (const [id, l] of this.lod) {
      const mask = this.lodShown.get(id);
      const shown = mask !== undefined;
      if (l.mesh) l.mesh.visible = shown;
      if (l.water && l.shown !== shown) this.lodWater.setVisible(l.water, shown);
      l.shown = shown;
      if (shown) l.section?.setSkirts(mask);
    }
    this.batch?.update(this.chunkVisible, this.lodShown);
    this.placeStandIns();
    // Two passes (§6.6): the far one for everything beyond the split (its near plane pushed out
    // with altitude, where nothing is closer), then a depth clear and the near one.
    // Heights are the viewer's face-local ones (face B's sky is below the disc).
    const localY =
      this.face > 0 ? this.camera.position.y : 2 * World.midplaneY - this.camera.position.y;
    const altitude = localY - World.worldMaxY;
    const far = this.farCamera;
    far.position.copy(this.camera.position);
    far.quaternion.copy(this.camera.quaternion);
    far.fov = this.camera.fov;
    far.aspect = this.camera.aspect;
    far.near = Math.max(NEAR_SPLIT * 0.95, altitude * 0.8);
    far.updateProjectionMatrix();
    this.renderer.clear();
    far.updateMatrixWorld();
    this.sky.update(far);
    this.renderer.render(this.skyScene, far);
    this.renderer.render(this.scene, far);
    const { calls, triangles } = this.renderer.info.render;
    this.renderer.clearDepth();
    this.renderer.render(this.scene, this.camera);
    this.frameStats.calls = calls + this.renderer.info.render.calls;
    this.frameStats.triangles = triangles + this.renderer.info.render.triangles;
  }

  stats(): RenderStats {
    this.renderer.getDrawingBufferSize(this.drawingBuffer);
    const pixels = this.drawingBuffer.x * this.drawingBuffer.y;
    return {
      ...this.frameStats,
      meshBytes: this.separateBytes + this.lodWater.bytes + (this.batch?.bytes ?? 0),
      // 4× multisampled colour and depth (antialias), then the resolved and displayed images.
      screenBytes: pixels * (4 * 8 + 8),
    };
  }

  setLodSection(id: number, origin: Vec3, cellSize: number, meshes: SectionMeshes | null): void {
    const old = this.lod.get(id);
    if (old) {
      if (old.mesh) {
        old.mesh.geometry.dispose();
        this.scene.remove(old.mesh);
      }
      if (old.water) this.lodWater.remove(old.water);
      this.separateBytes -= old.bytes;
      this.lod.delete(id);
    }
    if (!meshes) {
      this.batch?.setLodSection(id, origin, cellSize, null);
      return;
    }
    this.batch?.setLodSection(id, origin, cellSize, meshes);
    const level = Math.round(Math.log2(cellSize));
    const section = this.batch ? null : LodSectionGeometry.from(meshes);
    let mesh: Mesh | null = null;
    const bytes = section ? geometryBytes(section.geometry) : 0;
    this.separateBytes += bytes;
    if (section) {
      mesh = new Mesh(section.geometry, this.lodLevelMaterials?.[level] ?? this.lodMaterial);
      mesh.position.set(...origin);
      mesh.scale.setScalar(cellSize);
      mesh.visible = false;
      this.scene.add(ThreeRenderer.placed(mesh));
    }
    const waterGeometry = flatGeometry(meshes.water);
    const water = waterGeometry ? this.lodWater.add(waterGeometry, origin, cellSize) : null;
    waterGeometry?.dispose();
    this.lod.set(id, { mesh, section, level, water, shown: false, bytes });
  }

  showLodSections(visible: ReadonlyMap<number, number>): void {
    this.lodShown = visible;
  }

  showLodStandIns(standIns: readonly { id: number; lo: Vec3; hi: Vec3 }[]): void {
    this.lodStandIns = standIns;
  }

  /** Draws each stand-in: its source section's geometry, clipped to the hole's box (§6.6). */
  private placeStandIns(): void {
    let used = 0;
    for (const entry of this.lodStandIns) {
      const l = this.lod.get(entry.id);
      if (!l?.section || !l.mesh) continue;
      if (used >= MAX_STAND_INS) {
        if (!this.warnedStandIns) console.warn('LOD: more stand-ins than slots; drawing a subset');
        this.warnedStandIns = true;
        break;
      }
      let slot = this.standInSlots[used];
      if (!slot) {
        slot = new Mesh(
          new BufferGeometry(),
          withHeightFog(new MeshLambertMaterial({ vertexColors: true })),
        );
        this.scene.add(slot);
        this.standInSlots.push(slot);
      }
      used++;
      slot.geometry = l.section.geometry; // owned by the section: never disposed here
      slot.position.copy(l.mesh.position);
      slot.scale.copy(l.mesh.scale);
      ThreeRenderer.placed(slot);
      (slot.material as MeshLambertMaterial).clippingPlanes = boxClipPlanes(entry.lo, entry.hi);
      slot.visible = true;
      l.section.setSkirts(0); // its own mesh is not shown this frame, it was refined
    }
    for (let i = used; i < this.standInSlots.length; i++) {
      const slot = this.standInSlots[i];
      if (slot) slot.visible = false;
    }
  }

  setChunkVisibility(visible: ((coord: ChunkCoord) => boolean) | null): void {
    this.chunkVisible = visible;
  }

  setLodLevelColors(on: boolean): void {
    this.lodLevelMaterials = on
      ? LEVEL_TINTS.map((c) =>
          withHeightFog(new MeshLambertMaterial({ vertexColors: true, color: c })),
        )
      : null;
    for (const l of this.lod.values()) {
      if (l.mesh) l.mesh.material = this.lodLevelMaterials?.[l.level] ?? this.lodMaterial;
    }
    this.batch?.setLevelTints(on ? LEVEL_TINTS : null);
  }

  setTerrainChunk(key: string, origin: Vec3, meshes: ChunkMeshes | null): void {
    if (this.batch) {
      const coord: ChunkCoord = [
        Math.floor(origin[0] / CHUNK_SIZE),
        Math.floor(origin[1] / CHUNK_SIZE),
        Math.floor(origin[2] / CHUNK_SIZE),
      ];
      this.batch.setChunk(key, origin, coord, meshes);
      return;
    }
    const old = this.chunks.get(key);
    if (old) {
      for (const child of old.children) {
        if (child instanceof Mesh) (child.geometry as BufferGeometry).dispose();
      }
      this.scene.remove(old);
      this.chunks.delete(key);
      this.chunkCoords.delete(key);
      this.separateBytes -= this.chunkBytes.get(key) ?? 0;
      this.chunkBytes.delete(key);
    }
    if (!meshes) return;
    const group = new Group();
    group.position.set(...origin);
    const opaque = geometryOf(meshes.opaque);
    const water = geometryOf(meshes.transparent);
    if (!opaque && !water) return;
    const bytes = (opaque ? geometryBytes(opaque) : 0) + (water ? geometryBytes(water) : 0);
    this.chunkBytes.set(key, bytes);
    this.separateBytes += bytes;
    if (opaque) group.add(new Mesh(opaque, this.opaqueMaterial));
    if (water) {
      const mesh = new Mesh(water, this.waterMaterial);
      mesh.renderOrder = 1;
      group.add(mesh);
    }
    this.scene.add(ThreeRenderer.placed(group));
    this.chunks.set(key, group);
    this.chunkCoords.set(key, [
      Math.floor(origin[0] / CHUNK_SIZE),
      Math.floor(origin[1] / CHUNK_SIZE),
      Math.floor(origin[2] / CHUNK_SIZE),
    ]);
  }

  setPlayer(id: number, view: PlayerView | null): void {
    let p = this.players.get(id);
    if (!view) {
      if (p) {
        p.body.geometry.dispose();
        p.body.material.dispose();
        this.scene.remove(p.group);
        this.players.delete(id);
      }
      return;
    }
    const height = view.crouched ? view.height / 2 : view.height;
    const key = `${String(view.radius)}:${String(height)}`;
    if (!p) {
      const body = new Mesh(
        new CapsuleGeometry(view.radius, Math.max(0.01, height - 2 * view.radius), 6, 12),
        withHeightFog(new MeshLambertMaterial({ color: view.color })),
      );
      const group = new Group();
      group.add(body);
      // A small "visor" so facing is visible.
      const visor = new Mesh(
        new CapsuleGeometry(view.radius * 0.35, view.radius * 0.6, 4, 8),
        withHeightFog(new MeshLambertMaterial({ color: 0x1b1f2a })),
      );
      visor.rotation.z = Math.PI / 2;
      visor.position.set(0, 0, view.radius * 0.8);
      body.add(visor);
      this.scene.add(group);
      p = { group, body, key };
      this.players.set(id, p);
    } else if (p.key !== key) {
      p.body.geometry.dispose();
      p.body.geometry = new CapsuleGeometry(
        view.radius,
        Math.max(0.01, height - 2 * view.radius),
        6,
        12,
      );
      p.key = key;
    }
    p.body.material.color.setHex(view.dead ? 0x6b6b6b : view.color);
    p.group.position.set(...view.feet);
    p.group.rotation.set(0, (view.yaw * Math.PI) / 180, 0);
    // A face-B player stands upside down: its body hangs from the feet toward −y.
    p.group.scale.set(1, view.face ?? 1, 1);
    if (view.dead) {
      // Cosmetic death pose: lying on the ground.
      p.body.rotation.set(Math.PI / 2, 0, 0);
      p.body.position.set(0, view.radius, 0);
    } else {
      p.body.rotation.set(0, 0, 0);
      p.body.position.set(0, height / 2, 0);
    }
    ThreeRenderer.placed(p.group);
  }

  setCamera(eye: Vec3, yawDeg: number, pitchDeg: number, face: 1 | -1 = 1, roll = 0): void {
    this.face = face;
    setViewFace(face);
    this.camera.position.set(...eye);
    this.camera.up.set(0, face, 0); // a face-B player's head points toward −y
    this.camera.updateMatrixWorld();
    this.camera.lookAt(
      this.camera.position.clone().add(new Vector3(...viewForward(yawDeg, pitchDeg, face))),
    );
    // The camera turning over across the midplane: the view is the new face's at once, rolled back
    // toward the old one and easing out (game/flipRoll.ts).
    if (roll !== 0) this.camera.rotateZ(roll);
  }

  setFog(fog: FogSettings): void {
    setFogUniforms(fog);
  }

  setExposure(exposure: number): void {
    this.renderer.toneMappingExposure = sanitizeExposure(exposure);
  }

  setBlockOutline(cell: Vec3 | null, height = 1): void {
    this.outline.visible = cell !== null;
    if (!cell) return;
    this.outline.position.set(cell[0] - 0.002, cell[1] - 0.002, cell[2] - 0.002);
    this.outline.scale.set(1, height, 1);
    ThreeRenderer.placed(this.outline);
  }

  setPlacementPreview(cell: Vec3 | null, edges?: Float32Array): void {
    this.preview.visible = cell !== null && edges !== undefined;
    if (!cell || !edges) return;
    this.preview.geometry.setAttribute('position', new BufferAttribute(edges, 3));
    this.preview.position.set(cell[0], cell[1], cell[2]);
    ThreeRenderer.placed(this.preview);
  }

  setDebugLines(segments: readonly DebugSegment[] | null): void {
    if (this.debug) {
      this.debug.geometry.dispose();
      this.debug.material.dispose();
      this.scene.remove(this.debug);
      this.debug = null;
    }
    if (!segments || segments.length === 0) return;
    const lines = debugLineArrays(segments);
    const colors = new Float32Array(lines.colors.length * 3);
    const c = new Color();
    lines.colors.forEach((hex, i) => {
      c.setHex(hex);
      colors.set([c.r, c.g, c.b], i * 3);
    });
    const g = new BufferGeometry();
    g.setAttribute('position', new BufferAttribute(lines.positions, 3));
    g.setAttribute('color', new BufferAttribute(colors, 3));
    this.debug = new LineSegments(
      g,
      new LineBasicMaterial({ vertexColors: true, depthTest: false }),
    );
    this.debug.position.set(...lines.origin);
    this.debug.renderOrder = 2;
    this.scene.add(ThreeRenderer.placed(this.debug));
  }

  dispose(): void {
    this.atlas.dispose();
    this.renderer.dispose();
  }
}
