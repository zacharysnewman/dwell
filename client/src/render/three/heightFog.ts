// Height fog in three.js materials (render/fog.ts has the model and its reference, `hazeAmount`):
// three.js's fog chunks are replaced with the exponential atmosphere's optical depth along the view
// ray. The ray comes from the view-space position, which three.js computes camera-relative, so it
// stays precise from orbit, and it covers batched and instanced meshes alike. The haze's colour is
// the sky's in the ray's direction (render/look.ts `dwellSky`, the same function that draws the
// sky), so distant terrain dissolves into exactly the sky behind it.
import { ShaderChunk, type Material, type WebGLProgramParametersWithUniforms } from 'three';
import { World } from '../../protocol/constants.gen';
import { fogSigma, type FogSettings } from '../fog';
import { SKY_GLSL } from '../look';

/** Shared by every fogged material: set once per change, uploaded with each draw. */
export const fogUniforms = {
  dwellFogSigma: { value: 0 },
  dwellFogHeight: { value: 1 },
  dwellFogDensity: { value: 0 },
  /** The viewer's face: +1 A, −1 B (sky, haze and light are evaluated in its frame). */
  dwellFace: { value: 1 },
};

/** Sets the face the camera's player is on (heightFog and the sky read it). */
export function setViewFace(face: 1 | -1): void {
  fogUniforms.dwellFace.value = face;
}

export function setFogUniforms(fog: FogSettings): void {
  fogUniforms.dwellFogSigma.value = fogSigma(fog.distanceM);
  fogUniforms.dwellFogHeight.value = fog.heightM;
  fogUniforms.dwellFogDensity.value = fog.density;
}

const PARS_VERTEX = 'varying vec3 vFogView;';
const VERTEX = 'vFogView = mvPosition.xyz;';
const PARS_FRAGMENT = [
  'varying vec3 vFogView;',
  // Whether a light belongs to a fragment's face: the sun and the day's hemisphere light point up
  // (toward the sun, the sky) in the world, the moon's and the night's point down; a fragment takes
  // the lights of its own side of the midplane only (BIFACIAL_WORLD.md §6).
  'float dwellOwn(float lightUp, float fragOnA) {',
  '\treturn (lightUp > 0.0) == (fragOnA > 0.5) ? 1.0 : 0.0;',
  '}',
  'uniform float dwellFogSigma;',
  'uniform float dwellFogHeight;',
  'uniform float dwellFogDensity;',
  SKY_GLSL,
].join('\n');
// fog.ts `hazeAmount`, per fragment (distances vary too much across a coarse triangle to
// interpolate). viewMatrix[1] is world up in view space, so the ray's rise is a dot product.
const FRAGMENT = [
  'if (dwellFogDensity > 0.0) {',
  '\tfloat fogDist = length(vFogView);',
  // Heights are the viewer's own: face B's are measured in its face-local frame (mirrored).
  '\tfloat fogRise = dwellFace * dot(vFogView, viewMatrix[1].xyz);',
  `\tfloat fogCam = dwellFace > 0.0 ? cameraPosition.y : ${(World.midplaneY * 2).toFixed(1)} - cameraPosition.y;`,
  `\tfloat fogLow = min(fogCam, fogCam + fogRise) - ${World.seaLevel.toFixed(1)};`,
  '\tfloat fogK = abs(fogRise) / dwellFogHeight;',
  '\tfloat fogBase = exp(min(80.0, -fogLow / dwellFogHeight));',
  '\tfloat fogAlong = fogK < 1e-4 ? 1.0 - fogK * 0.5 : (1.0 - exp(-fogK)) / fogK;',
  '\tfloat fogDepth = dwellFogSigma * fogDist * fogBase * fogAlong;',
  // The haze is mixed in after tone mapping and the colour-space conversion, as three's fog is:
  // the sky is drawn in output colour too. viewMatrix's rotation, transposed, turns the view-space
  // ray into a world direction.
  '\tvec3 fogDir = normalize(vFogView * mat3(viewMatrix));',
  '\tgl_FragColor.rgb = mix(gl_FragColor.rgb, dwellSky(fogDir), dwellFogDensity * (1.0 - exp(-fogDepth)));',
  '}',
].join('\n');

/**
 * three.js's lights, taking from each fragment the lights of the other face away: no shadows, so
 * without this a ceiling on one face would be lit by the other face's sun or moon. The fragment's
 * height comes from the camera-relative view-space position (as the haze's), so it stays precise.
 */
const LIGHTS = [
  'float dwellFragY = cameraPosition.y + dot(vFogView, viewMatrix[1].xyz);',
  `float dwellFragA = dwellFragY >= ${World.midplaneY.toFixed(1)} ? 1.0 : 0.0;`,
  ShaderChunk.lights_fragment_begin
    .replace(
      'getDirectionalLightInfo( directionalLight, directLight );',
      'getDirectionalLightInfo( directionalLight, directLight );\n\t\tdirectLight.color *= dwellOwn( dot( directionalLight.direction, viewMatrix[1].xyz ), dwellFragA );',
    )
    .replace(
      'irradiance += getHemisphereLightIrradiance( hemisphereLights[ i ], geometryNormal );',
      'irradiance += getHemisphereLightIrradiance( hemisphereLights[ i ], geometryNormal ) * dwellOwn( dot( hemisphereLights[ i ].direction, viewMatrix[1].xyz ), dwellFragA );',
    ),
].join('\n');

/** Applies the height fog in the shader (after any existing patch). */
export function patchFogShader(shader: WebGLProgramParametersWithUniforms): void {
  Object.assign(shader.uniforms, fogUniforms);
  shader.vertexShader = shader.vertexShader
    .replace('#include <fog_pars_vertex>', PARS_VERTEX)
    .replace('#include <fog_vertex>', VERTEX);
  shader.fragmentShader = shader.fragmentShader
    .replace('#include <fog_pars_fragment>', PARS_FRAGMENT)
    .replace('#include <fog_fragment>', FRAGMENT)
    .replace('#include <lights_fragment_begin>', LIGHTS);
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
