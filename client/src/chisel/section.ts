// Chisel's 3D section view (ADR 0074, the 3D section, 0.52.19): a square of the world, sampled by the worker's 'section'
// lane (`ChiselPreview.section`, the detail preview's generator path) as a blocky stepped mesh, drawn
// here with WebGPU: sky light plus the sun (the mesh's tops carry a baked sun shadow), water as a
// translucent layer at each wet column's level, an orbit / pan / zoom camera.

/** The section's state: on or off, its centre (face-local m) and its side (m). */
export interface SectionView {
  on: boolean;
  x: number;
  z: number;
  size: number;
}

/** The sizes the size control offers (m a side). */
export const SECTION_SIZES = [256, 512, 1024, 2048, 4096, 8192] as const;

/** A section build's passes: a coarse pass first (4 m, or 256 columns a side for a bigger
 * square), then the fine one (2 m, 1 m for the smallest square, or 512 columns a side): at most
 * 512² columns, so a volcano's whole cone fits at 8–16 m. */
export function sectionPasses(size: number): { n: number; texel: number }[] {
  const coarse = Math.max(4, size / 256);
  const fine = size <= 256 ? 1 : Math.max(2, size / 512);
  return [coarse, fine].map((texel) => ({ n: Math.round(size / texel), texel }));
}

/** The 3D view's vertical exaggeration for a square `size` m a side: true scale up to 2 km, then
 * more, so a 700 m cone 7 km wide still reads as a cone in an 8 km square. */
export function exaggeration(size: number): number {
  return size <= 2048 ? 1 : size <= 4096 ? 2 : 3;
}

/** The section's corner (face-local m) for its centre and side. */
export function sectionCorner(v: SectionView): { x0: number; z0: number } {
  return { x0: Math.round(v.x - v.size / 2), z0: Math.round(v.z - v.size / 2) };
}

/** The section's state in the page's URL parameters (`s3d`, `sx`, `sz`, `ssize`). */
export function sectionToParams(v: SectionView, p: URLSearchParams) {
  if (v.on) p.set('s3d', '1');
  else p.delete('s3d');
  p.set('sx', String(Math.round(v.x)));
  p.set('sz', String(Math.round(v.z)));
  p.set('ssize', String(v.size));
}

/** The section's state read back from the URL (off without `s3d`); a missing or bad number
 * keeps `base`'s. */
export function sectionFromParams(p: URLSearchParams, base: SectionView): SectionView {
  const n = (k: string, d: number) => {
    const v = p.get(k);
    return v !== null && v !== '' && Number.isFinite(Number(v)) ? Number(v) : d;
  };
  const size = n('ssize', base.size);
  return {
    on: p.get('s3d') === '1',
    x: n('sx', base.x),
    z: n('sz', base.z),
    size: (SECTION_SIZES as readonly number[]).includes(size) ? size : base.size,
  };
}

// --- matrices (column-major, WebGPU clip z in [0, 1]) ---------------------------------------------

export type Mat4 = Float32Array;

export function perspective(fovY: number, aspect: number, near: number, far: number): Mat4 {
  const f = 1 / Math.tan(fovY / 2);
  const m = new Float32Array(16);
  m[0] = f / aspect;
  m[5] = f;
  m[10] = far / (near - far);
  m[11] = -1;
  m[14] = (near * far) / (near - far);
  return m;
}

export function lookAt(eye: number[], at: number[], up: number[]): Mat4 {
  const sub = (a: number[], b: number[]) => a.map((v, i) => v - (b[i] ?? 0));
  const norm = (a: number[]) => {
    const l = Math.hypot(...a) || 1;
    return a.map((v) => v / l);
  };
  const cross = (a: number[], b: number[]) => [
    (a[1] ?? 0) * (b[2] ?? 0) - (a[2] ?? 0) * (b[1] ?? 0),
    (a[2] ?? 0) * (b[0] ?? 0) - (a[0] ?? 0) * (b[2] ?? 0),
    (a[0] ?? 0) * (b[1] ?? 0) - (a[1] ?? 0) * (b[0] ?? 0),
  ];
  const dot = (a: number[], b: number[]) => a.reduce((s, v, i) => s + v * (b[i] ?? 0), 0);
  const z = norm(sub(eye, at));
  const x = norm(cross(up, z));
  const y = cross(z, x);
  const [x0 = 0, x1 = 0, x2 = 0] = x;
  const [y0 = 0, y1 = 0, y2 = 0] = y;
  const [z0 = 0, z1 = 0, z2 = 0] = z;
  return new Float32Array([
    x0,
    y0,
    z0,
    0,
    x1,
    y1,
    z1,
    0,
    x2,
    y2,
    z2,
    0,
    -dot(x, eye),
    -dot(y, eye),
    -dot(z, eye),
    1,
  ]);
}

export function mul(a: Mat4, b: Mat4): Mat4 {
  const m = new Float32Array(16);
  for (let c = 0; c < 4; c++)
    for (let r = 0; r < 4; r++) {
      let s = 0;
      for (let k = 0; k < 4; k++) s += (a[k * 4 + r] ?? 0) * (b[c * 4 + k] ?? 0);
      m[c * 4 + r] = s;
    }
  return m;
}

/** The orbit camera: around `target`, `yaw` / `pitch` (rad), `dist` m away. */
export interface Orbit {
  target: [number, number, number];
  yaw: number;
  pitch: number;
  dist: number;
}

export function eyeOf(o: Orbit): [number, number, number] {
  const c = Math.cos(o.pitch);
  return [
    o.target[0] + o.dist * c * Math.sin(o.yaw),
    o.target[1] + o.dist * Math.sin(o.pitch),
    o.target[2] + o.dist * c * Math.cos(o.yaw),
  ];
}

// --- the renderer ---------------------------------------------------------------------------------

/** The sun (face-local, unit), as `crate::chisel_section::SUN` (the baked shadow's). */
const SUN = [-0.579228, 0.573576, -0.579228];

const SHADER = /* wgsl */ `
struct U { vp: mat4x4<f32>, sun: vec4<f32>, look: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
struct O { @builtin(position) p: vec4<f32>, @location(0) c: vec3<f32>, @location(1) d: f32 };
@vertex fn vs(@location(0) pos: vec3<f32>, @location(1) packed: u32) -> O {
  let face = packed >> 24u;
  var n = vec3<f32>(0.0, 1.0, 0.0);
  if (face == 1u) { n = vec3<f32>(1.0, 0.0, 0.0); }
  else if (face == 2u) { n = vec3<f32>(-1.0, 0.0, 0.0); }
  else if (face == 3u) { n = vec3<f32>(0.0, 0.0, 1.0); }
  else if (face == 4u) { n = vec3<f32>(0.0, 0.0, -1.0); }
  let srgb = vec3<f32>(f32((packed >> 16u) & 255u), f32((packed >> 8u) & 255u), f32(packed & 255u)) / 255.0;
  let lin = pow(srgb, vec3<f32>(2.2));
  // Sky light (more from above) and the sun.
  let light = 0.42 + 0.16 * n.y + 0.85 * max(dot(n, u.sun.xyz), 0.0);
  var o: O;
  o.p = u.vp * vec4<f32>(pos.x, pos.y * u.look.z, pos.z, 1.0);
  o.c = lin * light;
  o.d = o.p.w;
  return o;
}
@fragment fn fs(i: O) -> @location(0) vec4<f32> {
  // Aerial haze with distance, toward the sky's colour.
  let haze = 1.0 - exp(-i.d / u.look.x);
  let sky = vec3<f32>(0.62, 0.74, 0.86);
  let c = mix(pow(i.c, vec3<f32>(1.0 / 2.2)), sky, haze * 0.6);
  return vec4<f32>(c, u.look.y);
}`;

// WebGPU's flag values (the spec's; the DOM typings here carry the types but not the namespaces).
const STAGE_VERTEX = 1;
const STAGE_FRAGMENT = 2;
const BUF_COPY_DST = 8;
const BUF_INDEX = 16;
const BUF_VERTEX = 32;
const BUF_UNIFORM = 64;
const TEX_RENDER_ATTACHMENT = 16;

interface Buffers {
  verts: GPUBuffer;
  index: GPUBuffer;
  count: number;
}

/** The section view's WebGPU drawing; `null` from `create` without WebGPU. */
export class SectionRenderer {
  orbit: Orbit = { target: [0, 0, 0], yaw: 0.6, pitch: 0.6, dist: 900 };
  private ground: Buffers | null = null;
  private water: Buffers | null = null;
  private depth: GPUTexture | null = null;
  private size = 0;
  /** The vertical exaggeration (`exaggeration`). */
  exag = 1;

  private constructor(
    private canvas: HTMLCanvasElement,
    private device: GPUDevice,
    private ctx: GPUCanvasContext,
    private solid: GPURenderPipeline,
    private glass: GPURenderPipeline,
    private uniform: GPUBuffer,
    private bind: GPUBindGroup,
  ) {}

  static async create(canvas: HTMLCanvasElement): Promise<SectionRenderer | null> {
    const adapter = 'gpu' in navigator ? await navigator.gpu.requestAdapter() : null;
    if (!adapter) return null;
    const device = await adapter.requestDevice();
    const ctx = canvas.getContext('webgpu') as GPUCanvasContext | null;
    if (!ctx) return null;
    const format = navigator.gpu.getPreferredCanvasFormat();
    ctx.configure({ device, format, alphaMode: 'opaque' });
    const module = device.createShaderModule({ code: SHADER });
    const layout = device.createBindGroupLayout({
      entries: [
        {
          binding: 0,
          visibility: STAGE_VERTEX | STAGE_FRAGMENT,
          buffer: {},
        },
      ],
    });
    const pipeline = (blend: boolean) =>
      device.createRenderPipeline({
        layout: device.createPipelineLayout({ bindGroupLayouts: [layout] }),
        vertex: {
          module,
          entryPoint: 'vs',
          buffers: [
            {
              arrayStride: 16,
              attributes: [
                { shaderLocation: 0, offset: 0, format: 'float32x3' },
                { shaderLocation: 1, offset: 12, format: 'uint32' },
              ],
            },
          ],
        },
        fragment: {
          module,
          entryPoint: 'fs',
          targets: [
            blend
              ? {
                  format,
                  blend: {
                    color: { srcFactor: 'src-alpha', dstFactor: 'one-minus-src-alpha' },
                    alpha: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha' },
                  },
                }
              : { format },
          ],
        },
        primitive: { topology: 'triangle-list', cullMode: blend ? 'none' : 'back' },
        depthStencil: {
          format: 'depth24plus',
          depthWriteEnabled: !blend,
          depthCompare: 'less',
        },
      });
    const uniform = device.createBuffer({
      size: 96,
      usage: BUF_UNIFORM | BUF_COPY_DST,
    });
    const bind = device.createBindGroup({
      layout,
      entries: [{ binding: 0, resource: { buffer: uniform } }],
    });
    return new SectionRenderer(canvas, device, ctx, pipeline(false), pipeline(true), uniform, bind);
  }

  private upload(verts: Uint32Array, index: Uint32Array): Buffers | null {
    if (index.length === 0) return null;
    const v = this.device.createBuffer({
      size: verts.byteLength,
      usage: BUF_VERTEX | BUF_COPY_DST,
    });
    this.device.queue.writeBuffer(v, 0, verts);
    const i = this.device.createBuffer({
      size: index.byteLength,
      usage: BUF_INDEX | BUF_COPY_DST,
    });
    this.device.queue.writeBuffer(i, 0, index);
    return { verts: v, index: i, count: index.length };
  }

  /** Replaces the mesh; `reframe` points the camera at the new section (a new square). */
  setMesh(
    m: {
      ground: Uint32Array;
      groundIndex: Uint32Array;
      water: Uint32Array;
      waterIndex: Uint32Array;
      n: number;
      texel: number;
    },
    reframe: boolean,
  ) {
    for (const b of [this.ground, this.water]) {
      b?.verts.destroy();
      b?.index.destroy();
    }
    this.ground = this.upload(m.ground, m.groundIndex);
    this.water = this.upload(m.water, m.waterIndex);
    const side = m.n * m.texel;
    this.size = side;
    this.exag = exaggeration(side);
    if (reframe) {
      // The mean height of the tops (every vertex's y; walls sit between tops).
      const f = new Float32Array(m.ground.buffer, m.ground.byteOffset, m.ground.length);
      let sum = 0;
      let count = 0;
      for (let k = 1; k < f.length; k += 4 * 16) {
        const y = f[k] ?? 0;
        sum += y;
        count++;
      }
      const y = (count ? sum / count : 0) * this.exag;
      this.orbit = { target: [side / 2, y, side / 2], yaw: 0.7, pitch: 0.55, dist: side * 1.45 };
    }
    this.draw();
  }

  draw() {
    const { canvas, device } = this;
    const w = Math.max(1, Math.round(canvas.clientWidth * devicePixelRatio));
    const h = Math.max(1, Math.round(canvas.clientHeight * devicePixelRatio));
    if (canvas.width !== w || canvas.height !== h || !this.depth) {
      canvas.width = w;
      canvas.height = h;
      this.depth?.destroy();
      this.depth = device.createTexture({
        size: [w, h],
        format: 'depth24plus',
        usage: TEX_RENDER_ATTACHMENT,
      });
    }
    const o = this.orbit;
    const far = Math.max(4000, o.dist * 4 + this.size * 2);
    const vp = mul(
      perspective(0.8, w / h, Math.max(1, o.dist / 200), far),
      lookAt(eyeOf(o), o.target, [0, 1, 0]),
    );
    const u = new Float32Array(24);
    u.set(vp, 0);
    u.set([SUN[0] ?? 0, SUN[1] ?? 0, SUN[2] ?? 0, 0], 16);
    const enc = device.createCommandEncoder();
    const pass = enc.beginRenderPass({
      colorAttachments: [
        {
          view: this.ctx.getCurrentTexture().createView(),
          clearValue: { r: 0.62, g: 0.74, b: 0.86, a: 1 },
          loadOp: 'clear',
          storeOp: 'store',
        },
      ],
      depthStencilAttachment: {
        view: this.depth.createView(),
        depthClearValue: 1,
        depthLoadOp: 'clear',
        depthStoreOp: 'store',
      },
    });
    const haze = Math.max(1500, this.size * 3);
    // A queue write lands at the next submit: the ground's uniform (alpha 1) here, the water's
    // before its own pass below.
    u.set([haze, 1, this.exag, 0], 20);
    device.queue.writeBuffer(this.uniform, 0, u);
    if (this.ground) {
      pass.setPipeline(this.solid);
      pass.setBindGroup(0, this.bind);
      pass.setVertexBuffer(0, this.ground.verts);
      pass.setIndexBuffer(this.ground.index, 'uint32');
      pass.drawIndexed(this.ground.count);
    }
    pass.end();
    device.queue.submit([enc.finish()]);
    if (this.water) {
      // The water in a pass of its own (its alpha in the uniform), over the ground's depth.
      const enc2 = device.createCommandEncoder();
      const p2 = enc2.beginRenderPass({
        colorAttachments: [
          {
            view: this.ctx.getCurrentTexture().createView(),
            loadOp: 'load',
            storeOp: 'store',
          },
        ],
        depthStencilAttachment: {
          view: this.depth.createView(),
          depthLoadOp: 'load',
          depthStoreOp: 'store',
        },
      });
      u.set([haze, 0.62, this.exag, 0], 20);
      device.queue.writeBuffer(this.uniform, 0, u);
      p2.setPipeline(this.glass);
      p2.setBindGroup(0, this.bind);
      p2.setVertexBuffer(0, this.water.verts);
      p2.setIndexBuffer(this.water.index, 'uint32');
      p2.drawIndexed(this.water.count);
      p2.end();
      device.queue.submit([enc2.finish()]);
    }
  }

  /** Mouse: drag orbits, shift- or right-drag pans, the wheel zooms. */
  attach() {
    const c = this.canvas;
    let last: { x: number; y: number; pan: boolean } | null = null;
    c.oncontextmenu = (e) => {
      e.preventDefault();
    };
    c.onpointerdown = (e) => {
      c.setPointerCapture(e.pointerId);
      last = { x: e.clientX, y: e.clientY, pan: e.shiftKey || e.button === 2 };
    };
    c.onpointermove = (e) => {
      if (!last) return;
      const [dx, dy] = [e.clientX - last.x, e.clientY - last.y];
      last = { ...last, x: e.clientX, y: e.clientY };
      const o = this.orbit;
      if (last.pan) {
        const k = o.dist / c.clientHeight;
        const [s, co] = [Math.sin(o.yaw), Math.cos(o.yaw)];
        o.target[0] -= (dx * co - dy * s) * k;
        o.target[2] -= (-dx * s - dy * co) * k;
      } else {
        o.yaw -= dx * 0.008;
        o.pitch = Math.min(1.5, Math.max(0.05, o.pitch + dy * 0.006));
      }
      this.draw();
    };
    c.onpointerup = () => {
      last = null;
    };
    c.onwheel = (e) => {
      e.preventDefault();
      this.orbit.dist = Math.min(8000, Math.max(20, this.orbit.dist * Math.exp(e.deltaY * 0.001)));
      this.draw();
    };
  }
}
