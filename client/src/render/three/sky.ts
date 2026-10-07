// The sky behind the world (render/look.ts has the gradient and its shader twin): a full-screen
// triangle drawn first, each pixel coloured by `dwellSky` of its view direction, so the haze that
// terrain fades into (heightFog.ts) is the very colour behind it. One draw call, no geometry
// beyond three vertices, no depth.
import {
  BufferAttribute,
  BufferGeometry,
  Mesh,
  ShaderMaterial,
  Vector3,
  type PerspectiveCamera,
} from 'three';
import { SKY_GLSL } from '../look';
import { fogUniforms } from './heightFog';

const VERTEX = [
  'varying vec2 vNdc;',
  'void main() {',
  '\tvNdc = position.xy;',
  '\tgl_Position = vec4(position.xy, 1.0, 1.0);',
  '}',
].join('\n');

const FRAGMENT = [
  'varying vec2 vNdc;',
  'uniform vec3 dwellRight;',
  'uniform vec3 dwellUp;',
  'uniform vec3 dwellForward;',
  'uniform vec2 dwellTan;',
  SKY_GLSL,
  'void main() {',
  '\tvec3 dir = normalize(dwellForward + vNdc.x * dwellTan.x * dwellRight + vNdc.y * dwellTan.y * dwellUp);',
  // Written as is: the gradient is already sRGB, and no tone mapping touches the sky.
  '\tgl_FragColor = vec4(dwellSky(dir), 1.0);',
  '}',
].join('\n');

export class Sky {
  readonly mesh: Mesh<BufferGeometry, ShaderMaterial>;
  constructor() {
    const geometry = new BufferGeometry();
    geometry.setAttribute(
      'position',
      new BufferAttribute(new Float32Array([-1, -1, 0, 3, -1, 0, -1, 3, 0]), 3),
    );
    const material = new ShaderMaterial({
      vertexShader: VERTEX,
      fragmentShader: FRAGMENT,
      uniforms: {
        dwellRight: { value: new Vector3() },
        dwellUp: { value: new Vector3() },
        dwellForward: { value: new Vector3() },
        dwellTan: { value: { x: 1, y: 1 } },
        dwellFace: fogUniforms.dwellFace,
        dwellDayPole: fogUniforms.dwellDayPole,
        dwellSun: fogUniforms.dwellSun,
        dwellMoon: fogUniforms.dwellMoon,
      },
      depthTest: false,
      depthWrite: false,
      toneMapped: false,
    });
    this.mesh = new Mesh(geometry, material);
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = -1;
  }

  /** Aims the sky's view rays like the camera's. */
  update(camera: PerspectiveCamera): void {
    const u = this.mesh.material.uniforms;
    const basis = camera.matrixWorld;
    (u.dwellRight?.value as Vector3).setFromMatrixColumn(basis, 0);
    (u.dwellUp?.value as Vector3).setFromMatrixColumn(basis, 1);
    (u.dwellForward?.value as Vector3).setFromMatrixColumn(basis, 2).negate();
    const tanY = Math.tan((camera.fov * Math.PI) / 360);
    const tan = u.dwellTan?.value as { x: number; y: number };
    tan.x = tanY * camera.aspect;
    tan.y = tanY;
  }
}
