// WebAssembly SIMD (owner, 2026-11-06): the game's module is built with `+simd128`
// (.cargo/config.toml), so a browser without SIMD cannot compile it. The shell asks first, before
// downloading it, and says the browser is too old rather than failing blankly (ui/startupError.ts).

/**
 * The smallest module that needs SIMD: one function returning a `v128.const` of zeros. A browser
 * that validates it has fixed-width SIMD (the v128 type and its opcodes).
 */
export const SIMD_PROBE = new Uint8Array([
  ...[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00], // the magic and version
  ...[0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7b], // types: () -> v128
  ...[0x03, 0x02, 0x01, 0x00], // functions: one, of type 0
  ...[0x0a, 0x16, 0x01, 0x14, 0x00], // code: one body of 20 bytes, no locals
  ...[0xfd, 0x0c, ...new Array<number>(16).fill(0)], // v128.const 0
  0x0b, // end
]);

/** The part of `WebAssembly` the check reads (a stand-in in tests). */
export interface WasmValidator {
  validate(bytes: Uint8Array): boolean;
}

/** Whether this browser's WebAssembly has SIMD (false with no WebAssembly at all: null). */
export function hasWasmSimd(
  wasm: WasmValidator | null = (globalThis as { WebAssembly?: WasmValidator }).WebAssembly ?? null,
): boolean {
  try {
    return typeof wasm?.validate === 'function' && wasm.validate(SIMD_PROBE);
  } catch {
    return false;
  }
}
