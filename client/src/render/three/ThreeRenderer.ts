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
  Fog,
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
import type { Vec3 } from '../../protocol/messages';
import type { ChunkMeshes, MeshArrays } from '../../mesh/mesher';
import { debugLineArrays, type DebugSegment } from '../debugLines';
import { VERTICAL_FOV, verticalFov } from '../fov';
import { sharedAtlas } from '../textures';
import { RendererUnavailableError, type PlayerView, type Renderer } from '../Renderer';

const SKY = 0x87b5e0;

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
  private readonly camera = new PerspectiveCamera(VERTICAL_FOV, 1, 0.05, 400);
  /** Block textures: tiled-noise atlas (render/textures.ts), crisp up close, mipmapped far away. */
  private readonly atlas = ThreeRenderer.createAtlasTexture();
  private readonly opaqueMaterial = chunkMaterial({
    vertexColors: true,
    map: this.atlas,
  });
  private readonly waterMaterial = chunkMaterial({
    vertexColors: true,
    map: this.atlas,
    transparent: true,
    opacity: 0.55,
    depthWrite: false,
    side: DoubleSide,
  });
  private readonly chunks = new Map<string, Group>();
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
    this.renderer.setClearColor(SKY);
    this.scene.background = new Color(SKY);
    this.scene.fog = new Fog(SKY, 60, 140);
    this.scene.add(new HemisphereLight(0xdfefff, 0x4a3b2a, 1.4));
    const sun = new DirectionalLight(0xffffff, 1.6);
    sun.position.set(0.4, 1, 0.25);
    this.scene.add(sun);
    this.outline.visible = false;
    this.scene.add(this.outline);
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
    this.renderer.render(this.scene, this.camera);
  }

  setTerrainChunk(key: string, origin: Vec3, meshes: ChunkMeshes | null): void {
    const old = this.chunks.get(key);
    if (old) {
      for (const child of old.children) {
        if (child instanceof Mesh) (child.geometry as BufferGeometry).dispose();
      }
      this.scene.remove(old);
      this.chunks.delete(key);
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
        new MeshLambertMaterial({ color: view.color }),
      );
      const group = new Group();
      group.add(body);
      // A small "visor" so facing is visible.
      const visor = new Mesh(
        new CapsuleGeometry(view.radius * 0.35, view.radius * 0.6, 4, 8),
        new MeshLambertMaterial({ color: 0x1b1f2a }),
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
    const forward = new Vector3(
      Math.sin(yaw) * Math.cos(pitch),
      Math.sin(pitch),
      Math.cos(yaw) * Math.cos(pitch),
    );
    this.camera.lookAt(this.camera.position.clone().add(forward));
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
