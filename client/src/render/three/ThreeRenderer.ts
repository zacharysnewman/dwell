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
  HemisphereLight,
  LinearMipmapLinearFilter,
  LineBasicMaterial,
  LineSegments,
  Mesh,
  MeshLambertMaterial,
  NearestFilter,
  PerspectiveCamera,
  RGBAFormat,
  Scene,
  SRGBColorSpace,
  Vector3,
  WebGLRenderer,
  type MeshLambertMaterialParameters,
} from 'three';
import type { ChunkCoord, Vec3 } from '../../protocol/messages';
import type { FlatMesh, SectionMeshes } from '../../mesh/lodMesher';
import type { ChunkMeshes, MeshArrays } from '../../mesh/mesher';
import { CHUNK_SIZE, Lod, World } from '../../protocol/constants.gen';
import { debugLineArrays, type DebugSegment } from '../debugLines';
import { DEFAULT_FOG, type FogSettings } from '../fog';
import { VERTICAL_FOV, verticalFov } from '../fov';
import { sharedAtlas } from '../textures';
import { RendererUnavailableError, type PlayerView, type Renderer } from '../Renderer';
import { setFogUniforms, withHeightFog } from './heightFog';
import { WaterBatch, type WaterHandle } from './waterBatch';

const SKY = 0x87b5e0;
/** Near/far depth split (§6.6): LOD beyond it in a far pass, then a depth clear and a near pass. */
const NEAR_SPLIT = Lod.nearSplitM;
const FAR_PLANE = 5e7;

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

interface LodGroup {
  group: Group;
  level: number;
  /** The section's water in the shared batch (lodWater), if it has any. */
  water: WaterHandle | null;
  /** Per face index; null where the section has no skirt faces on that side. */
  skirts: (Mesh | null)[];
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
  g.setAttribute('tile', new BufferAttribute(arrays.tiles, 4));
  g.setIndex(new BufferAttribute(arrays.indices, 1));
  g.computeBoundingSphere();
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
      .replace('#include <common>', '#include <common>\nattribute vec4 tile;\nvarying vec4 vTile;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\n\tvTile = tile;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying vec4 vTile;')
      .replace(
        '#include <map_fragment>',
        [
          '#ifdef USE_MAP',
          '\tvec2 atlasUv = vTile.xy + fract(vMapUv) * vTile.zw;',
          '\tdiffuseColor *= textureGrad(map, atlasUv, dFdx(vMapUv) * vTile.zw, dFdy(vMapUv) * vTile.zw);',
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
  private readonly lod = new Map<number, LodGroup>();
  private lodShown: ReadonlyMap<number, number> = new Map();
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
  private readonly lodWater = new WaterBatch(this.lodWaterMaterial);
  private lodLevelMaterials: MeshLambertMaterial[] | null = null;
  /** The far pass's camera (the main camera is the near pass's). */
  private readonly farCamera = new PerspectiveCamera(VERTICAL_FOV, 1, NEAR_SPLIT, FAR_PLANE);
  private readonly players = new Map<number, PlayerMesh>();
  private debug: LineSegments | null = null;
  /** Outline of the targeted block (§6.5): a unit box's edges, scaled for slabs. */
  private readonly outline = new LineSegments(
    new EdgesGeometry(new BoxGeometry(1.004, 1.004, 1.004).translate(0.5, 0.5, 0.5)),
    new LineBasicMaterial({ color: 0x101418, transparent: true, opacity: 0.8 }),
  );

  constructor(canvas: HTMLCanvasElement) {
    if (!canvas.getContext('webgl2')) {
      throw new RendererUnavailableError('WebGL2 is not available on this device.');
    }
    this.renderer = new WebGLRenderer({ canvas, antialias: true });
    this.renderer.autoClear = false;
    this.renderer.setClearColor(SKY);
    this.setFog(DEFAULT_FOG);
    this.scene.add(new HemisphereLight(0xdfefff, 0x4a3b2a, 1.4));
    const sun = new DirectionalLight(0xffffff, 1.6);
    sun.position.set(0.4, 1, 0.25);
    this.scene.add(sun);
    this.outline.visible = false;
    this.scene.add(this.outline);
    this.lodWater.mesh.renderOrder = 1;
    this.scene.add(this.lodWater.mesh);
    this.camera.position.set(0, 6, 14);
    this.camera.lookAt(0, 0, 0);
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
      l.group.visible = mask !== undefined;
      if (l.water) this.lodWater.setVisible(l.water, mask !== undefined);
      if (mask === undefined) continue;
      l.skirts.forEach((s, face) => {
        if (s) s.visible = (mask & (1 << face)) !== 0;
      });
    }
    // Two passes (§6.6): the far one for everything beyond the split (its near plane pushed out
    // with altitude, where nothing is closer), then a depth clear and the near one.
    const altitude = this.camera.position.y - World.worldMaxY;
    const far = this.farCamera;
    far.position.copy(this.camera.position);
    far.quaternion.copy(this.camera.quaternion);
    far.fov = this.camera.fov;
    far.aspect = this.camera.aspect;
    far.near = Math.max(NEAR_SPLIT * 0.95, altitude * 0.8);
    far.updateProjectionMatrix();
    this.renderer.clear();
    this.renderer.render(this.scene, far);
    this.renderer.clearDepth();
    this.renderer.render(this.scene, this.camera);
  }

  setLodSection(id: number, origin: Vec3, cellSize: number, meshes: SectionMeshes | null): void {
    const old = this.lod.get(id);
    if (old) {
      for (const child of old.group.children) {
        if (child instanceof Mesh) (child.geometry as BufferGeometry).dispose();
      }
      this.scene.remove(old.group);
      if (old.water) this.lodWater.remove(old.water);
      this.lod.delete(id);
    }
    if (!meshes) return;
    const level = Math.round(Math.log2(cellSize));
    const group = new Group();
    group.position.set(...origin);
    group.scale.setScalar(cellSize);
    group.visible = false;
    const material = this.lodLevelMaterials?.[level] ?? this.lodMaterial;
    const opaque = flatGeometry(meshes.opaque);
    if (opaque) group.add(new Mesh(opaque, material));
    const waterGeometry = flatGeometry(meshes.water);
    const water = waterGeometry ? this.lodWater.add(waterGeometry, origin, cellSize) : null;
    waterGeometry?.dispose();
    const skirts = meshes.skirts.map((s) => {
      const g = flatGeometry(s);
      if (!g) return null;
      const m = new Mesh(g, material);
      group.add(m);
      return m;
    });
    this.scene.add(group);
    this.lod.set(id, { group, level, water, skirts });
  }

  showLodSections(visible: ReadonlyMap<number, number>): void {
    this.lodShown = visible;
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
      const material = this.lodLevelMaterials?.[l.level] ?? this.lodMaterial;
      for (const child of l.group.children) {
        if (child instanceof Mesh) child.material = material;
      }
    }
  }

  setTerrainChunk(key: string, origin: Vec3, meshes: ChunkMeshes | null): void {
    const old = this.chunks.get(key);
    if (old) {
      for (const child of old.children) {
        if (child instanceof Mesh) (child.geometry as BufferGeometry).dispose();
      }
      this.scene.remove(old);
      this.chunks.delete(key);
      this.chunkCoords.delete(key);
    }
    if (!meshes) return;
    const group = new Group();
    group.position.set(...origin);
    const opaque = geometryOf(meshes.opaque);
    const water = geometryOf(meshes.transparent);
    if (!opaque && !water) return;
    if (opaque) group.add(new Mesh(opaque, this.opaqueMaterial));
    if (water) {
      const mesh = new Mesh(water, this.waterMaterial);
      mesh.renderOrder = 1;
      group.add(mesh);
    }
    this.scene.add(group);
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
    if (view.dead) {
      // Cosmetic death pose: lying on the ground.
      p.body.rotation.set(Math.PI / 2, 0, 0);
      p.body.position.set(0, view.radius, 0);
    } else {
      p.body.rotation.set(0, 0, 0);
      p.body.position.set(0, height / 2, 0);
    }
  }

  setCamera(eye: Vec3, yawDeg: number, pitchDeg: number): void {
    const yaw = (yawDeg * Math.PI) / 180;
    const pitch = (pitchDeg * Math.PI) / 180;
    this.camera.position.set(...eye);
    this.camera.updateMatrixWorld();
    const forward = new Vector3(
      Math.sin(yaw) * Math.cos(pitch),
      Math.sin(pitch),
      Math.cos(yaw) * Math.cos(pitch),
    );
    this.camera.lookAt(this.camera.position.clone().add(forward));
  }

  setFog(fog: FogSettings): void {
    setFogUniforms(fog, SKY);
  }

  setBlockOutline(cell: Vec3 | null, height = 1): void {
    this.outline.visible = cell !== null;
    if (!cell) return;
    this.outline.position.set(cell[0] - 0.002, cell[1] - 0.002, cell[2] - 0.002);
    this.outline.scale.set(1, height, 1);
  }

  setDebugLines(segments: readonly DebugSegment[] | null): void {
    if (this.debug) {
      this.debug.geometry.dispose();
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
    this.scene.add(this.debug);
  }

  dispose(): void {
    this.atlas.dispose();
    this.renderer.dispose();
  }
}
