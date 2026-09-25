import {
  BufferAttribute,
  BufferGeometry,
  CapsuleGeometry,
  Color,
  DataTexture,
  DirectionalLight,
  DoubleSide,
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
} from 'three';
import type { Vec3 } from '../../protocol/messages';
import { buildChunkMeshes, type MeshArrays } from '../chunkMesh';
import { buildAtlas } from '../textures';
import { RendererUnavailableError, type PlayerView, type Renderer } from '../Renderer';

const SKY = 0x87b5e0;

function geometryOf(arrays: MeshArrays): BufferGeometry | null {
  if (arrays.indices.length === 0) return null;
  const g = new BufferGeometry();
  g.setAttribute('position', new BufferAttribute(arrays.positions, 3));
  g.setAttribute('normal', new BufferAttribute(arrays.normals, 3));
  g.setAttribute('color', new BufferAttribute(arrays.colors, 3));
  g.setAttribute('uv', new BufferAttribute(arrays.uvs, 2));
  g.setIndex(new BufferAttribute(arrays.indices, 1));
  g.computeBoundingSphere();
  return g;
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
  private readonly camera = new PerspectiveCamera(75, 1, 0.05, 400);
  /** Block textures: tiled-noise atlas (render/textures.ts), crisp up close, mipmapped far away. */
  private readonly atlas = ThreeRenderer.createAtlasTexture();
  private readonly opaqueMaterial = new MeshLambertMaterial({
    vertexColors: true,
    map: this.atlas,
  });
  private readonly waterMaterial = new MeshLambertMaterial({
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
    this.camera.position.set(0, 6, 14);
    this.camera.lookAt(0, 0, 0);
  }

  private static createAtlasTexture(): DataTexture {
    const atlas = buildAtlas();
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
    this.camera.updateProjectionMatrix();
  }

  renderFrame(): void {
    this.renderer.render(this.scene, this.camera);
  }

  setTerrainChunk(key: string, origin: Vec3, faces: Uint8Array | null): void {
    const old = this.chunks.get(key);
    if (old) {
      for (const child of old.children) {
        if (child instanceof Mesh) (child.geometry as BufferGeometry).dispose();
      }
      this.scene.remove(old);
      this.chunks.delete(key);
    }
    if (!faces || faces.length === 0) return;
    const meshes = buildChunkMeshes(faces);
    const group = new Group();
    group.position.set(...origin);
    const opaque = geometryOf(meshes.opaque);
    if (opaque) group.add(new Mesh(opaque, this.opaqueMaterial));
    const water = geometryOf(meshes.transparent);
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

  setDebugLines(segments: readonly { from: Vec3; to: Vec3; color: number }[] | null): void {
    if (this.debug) {
      this.debug.geometry.dispose();
      this.scene.remove(this.debug);
      this.debug = null;
    }
    if (!segments || segments.length === 0) return;
    const positions = new Float32Array(segments.length * 6);
    const colors = new Float32Array(segments.length * 6);
    const c = new Color();
    segments.forEach((s, i) => {
      positions.set([...s.from, ...s.to], i * 6);
      c.setHex(s.color);
      colors.set([c.r, c.g, c.b, c.r, c.g, c.b], i * 6);
    });
    const g = new BufferGeometry();
    g.setAttribute('position', new BufferAttribute(positions, 3));
    g.setAttribute('color', new BufferAttribute(colors, 3));
    this.debug = new LineSegments(
      g,
      new LineBasicMaterial({ vertexColors: true, depthTest: false }),
    );
    this.debug.renderOrder = 2;
    this.scene.add(this.debug);
  }

  dispose(): void {
    this.atlas.dispose();
    this.renderer.dispose();
  }
}
