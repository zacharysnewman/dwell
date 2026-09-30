// Height fog in three.js materials (render/fog.ts has the model and its reference, `hazeAmount`):
// three.js's fog chunks are replaced with the exponential atmosphere's optical depth along the view
// ray. The ray comes from the view-space position, which three.js computes camera-relative, so it
// stays precise from orbit, and it covers batched and instanced meshes alike.
import { Vector3, type Material, type WebGLProgramParametersWithUniforms } from 'three';
import { World } from '../../protocol/constants.gen';
import { fogSigma, type FogSettings } from '../fog';

/** Shared by every fogged material: set once per change, uploaded with each draw. */
export const fogUniforms = {
  dwellFogSigma: { value: 0 },
  dwellFogHeight: { value: 1 },
  dwellFogDensity: { value: 0 },
  /** Output (sRGB) colour: the haze is mixed in after the colour-space conversion, as three's. */
  dwellFogColor: { value: new Vector3() },
};

export function setFogUniforms(fog: FogSettings, skyHex: number): void {
  fogUniforms.dwellFogSigma.value = fogSigma(fog.distanceM);
  fogUniforms.dwellFogHeight.value = fog.heightM;
  fogUniforms.dwellFogDensity.value = fog.density;
  fogUniforms.dwellFogColor.value.set(
    ((skyHex >> 16) & 0xff) / 255,
    ((skyHex >> 8) & 0xff) / 255,
    (skyHex & 0xff) / 255,
  );
}

const PARS_VERTEX = 'varying vec3 vFogView;';
const VERTEX = 'vFogView = mvPosition.xyz;';
const PARS_FRAGMENT = [
  'varying vec3 vFogView;',
  'uniform float dwellFogSigma;',
  'uniform float dwellFogHeight;',
  'uniform float dwellFogDensity;',
  'uniform vec3 dwellFogColor;',
].join('\n');
// fog.ts `hazeAmount`, per fragment (distances vary too much across a coarse triangle to
// interpolate). viewMatrix[1] is world up in view space, so the ray's rise is a dot product.
const FRAGMENT = [
  'if (dwellFogDensity > 0.0) {',
  '\tfloat fogDist = length(vFogView);',
  '\tfloat fogRise = dot(vFogView, viewMatrix[1].xyz);',
  `\tfloat fogLow = min(cameraPosition.y, cameraPosition.y + fogRise) - ${World.seaLevel.toFixed(1)};`,
  '\tfloat fogK = abs(fogRise) / dwellFogHeight;',
  '\tfloat fogBase = exp(min(80.0, -fogLow / dwellFogHeight));',
  '\tfloat fogAlong = fogK < 1e-4 ? 1.0 - fogK * 0.5 : (1.0 - exp(-fogK)) / fogK;',
  '\tfloat fogDepth = dwellFogSigma * fogDist * fogBase * fogAlong;',
  '\tgl_FragColor.rgb = mix(gl_FragColor.rgb, dwellFogColor, dwellFogDensity * (1.0 - exp(-fogDepth)));',
  '}',
].join('\n');

/** Applies the height fog in the shader (after any existing patch). */
export function patchFogShader(shader: WebGLProgramParametersWithUniforms): void {
  Object.assign(shader.uniforms, fogUniforms);
  shader.vertexShader = shader.vertexShader
    .replace('#include <fog_pars_vertex>', PARS_VERTEX)
    .replace('#include <fog_vertex>', VERTEX);
  shader.fragmentShader = shader.fragmentShader
    .replace('#include <fog_pars_fragment>', PARS_FRAGMENT)
    .replace('#include <fog_fragment>', FRAGMENT);
}

/** Gives a material the height fog; `key` names its shader variant for three's program cache. */
export function withHeightFog<M extends Material>(material: M, key = 'plain'): M {
  const previous = material.onBeforeCompile.bind(material);
  material.onBeforeCompile = (shader, renderer) => {
    previous(shader, renderer);
    patchFogShader(shader);
  };
  material.customProgramCacheKey = () => `dwell-fog-${key}`;
  return material;
}
