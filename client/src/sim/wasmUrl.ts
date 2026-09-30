// URLs of the WASM modules in public/wasm (the core and worldgen). Their names are fixed, unlike
// the hashed bundle files, so each carries the build's version: after a deploy a browser that
// cached the old loader or .wasm fetches the matching new pair instead of mixing builds (a stale
// pair hung local worlds on "Joining…" on iOS Safari after protocol v10).
import { buildInfo } from '../buildInfo';

/** The URL of a file in public/wasm, for this build. */
export function wasmUrl(file: string, version: string = buildInfo.sha): string {
  return `${import.meta.env.BASE_URL}wasm/${file}?v=${encodeURIComponent(version)}`;
}

/** Emscripten's `locateFile` option: the loader's .wasm with the same version as the loader. */
export function versionedLocateFile(
  version: string = buildInfo.sha,
): (path: string, prefix: string) => string {
  return (path, prefix) => `${prefix}${path}?v=${encodeURIComponent(version)}`;
}
