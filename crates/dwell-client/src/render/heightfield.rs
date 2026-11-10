//! The water's GPU heightfield, shader side (proposals/WATER_GPU_SURFACE.md §3, step C6a): the
//! WGSL that places the shared grid mesh's corners from the height atlas. The tiles, the grid and
//! the atlas's slots are `dwell_mesh::heightfield` (pure data, tested there); a tier draws this
//! when its data says `heightfield: true` (`shared/look/watertiers.json`), which no tier does yet:
//! the water pass's instanced draw over it is the rest of C6a (the proposal's build plan).
//!
//! The atlas is an `r32float` texture of tiles [`TILE`] texels square, [`TILES_PER_ROW`] to a row,
//! read with `textureLoad` (no filtering, so no float-filterable feature). A tile holds a chunk's
//! face-local column levels in chunk-local metres, `DRY` where a column has no surface; corners
//! take the mean of the wet columns around them within `joinM` of the highest
//! (`dwell_mesh::heightfield::corner_height`, the reference this shader is tested against in
//! spirit: the same rule, the same constants). Each instance names its tile and its chunk's
//! face-local floor; the shader crosses into the world through `FaceFrame::wgsl()`'s `face_y`, so
//! one grid serves both faces (BIFACIAL_WORLD.md §11).

use dwell_mesh::heightfield::{CORNERS, DRY, TILE};

/// Tiles per row of the atlas texture (its width is this × [`TILE`] texels).
pub const TILES_PER_ROW: usize = 64;

/// The atlas texel at which slot `slot`'s tile starts.
pub fn tile_origin(slot: usize) -> [u32; 2] {
    [
        ((slot % TILES_PER_ROW) * TILE) as u32,
        ((slot / TILES_PER_ROW) * TILE) as u32,
    ]
}

/// The heightfield's WGSL: `hf_corner` (a grid corner's face-local height), `hf_world` (a grid
/// vertex's world position) and `hf_column_wet` (a fragment's column test, to discard the dry), with
/// the face frame's helpers they use. `hf_vs`/`hf_fs` are a minimal pair that exercises them.
pub fn wgsl() -> String {
    format!(
        "{face}\n\
const HF_TILE: i32 = {tile};\n\
const HF_CORNERS: u32 = {corners}u;\n\
const HF_DRY: f32 = {dry:?};\n\
\n\
@group(0) @binding(0) var hf_heights: texture_2d<f32>;\n\
\n\
struct HfParams {{ join_m: f32, _pad0: f32, _pad1: f32, _pad2: f32 }};\n\
@group(0) @binding(1) var<uniform> hf_params: HfParams;\n\
\n\
fn hf_level(tile: vec2i, c: vec2i) -> f32 {{\n\
    return textureLoad(hf_heights, tile + clamp(c, vec2i(0), vec2i(HF_TILE - 1)), 0).r;\n\
}}\n\
\n\
fn hf_wet(h: f32) -> bool {{ return h > HF_DRY * 0.5; }}\n\
\n\
// A corner's height: the mean of the wet columns around it within join_m of the highest; HF_DRY\n\
// if none is wet. Corner (i, j) touches tile texels (i, j) to (i + 1, j + 1).\n\
fn hf_corner(tile: vec2i, corner: vec2i) -> f32 {{\n\
    let h = array<f32, 4>(\n\
        hf_level(tile, corner), hf_level(tile, corner + vec2i(1, 0)),\n\
        hf_level(tile, corner + vec2i(0, 1)), hf_level(tile, corner + vec2i(1, 1)));\n\
    var top = HF_DRY;\n\
    for (var k = 0; k < 4; k++) {{ if hf_wet(h[k]) {{ top = max(top, h[k]); }} }}\n\
    if !hf_wet(top) {{ return HF_DRY; }}\n\
    var sum = 0.0;\n\
    var n = 0.0;\n\
    for (var k = 0; k < 4; k++) {{\n\
        if hf_wet(h[k]) && top - h[k] <= hf_params.join_m {{ sum += h[k]; n += 1.0; }}\n\
    }}\n\
    return sum / n;\n\
}}\n\
\n\
// Grid vertex `index` of the instance whose tile starts at `tile` and whose chunk's face-local\n\
// floor is `floor` (x, face-local y, z of its minimum corner; w the face, +1 A, -1 B): its world\n\
// position, the chunk floor's for a corner with no wet column (its column's fragments discard).\n\
fn hf_world(index: u32, tile: vec2i, floor: vec4f) -> vec3f {{\n\
    let corner = vec2i(i32(index % HF_CORNERS), i32(index / HF_CORNERS));\n\
    var h = hf_corner(tile, corner);\n\
    if !hf_wet(h) {{ h = 0.0; }}\n\
    let local_y = floor.y + h;\n\
    return vec3f(floor.x + f32(corner.x), face_y(floor.w, local_y), floor.z + f32(corner.y));\n\
}}\n\
\n\
// Whether the column under chunk-local point `xz` (0..16) is wet.\n\
fn hf_column_wet(tile: vec2i, xz: vec2f) -> bool {{\n\
    let c = clamp(vec2i(floor(xz)), vec2i(0), vec2i(HF_TILE - 3));\n\
    return hf_wet(hf_level(tile, c + vec2i(1)));\n\
}}\n\
\n\
struct HfOut {{\n\
    @builtin(position) clip: vec4f,\n\
    @location(0) local_xz: vec2f,\n\
    @location(1) @interpolate(flat) tile: vec2i,\n\
}};\n\
\n\
@vertex\n\
fn hf_vs(@builtin(vertex_index) index: u32, @location(0) tile: vec2u, @location(1) floor: vec4f) -> HfOut {{\n\
    let t = vec2i(tile);\n\
    let p = hf_world(index, t, floor);\n\
    var out: HfOut;\n\
    out.clip = vec4f(p, 1.0);\n\
    out.local_xz = vec2f(p.x - floor.x, p.z - floor.z);\n\
    out.tile = t;\n\
    return out;\n\
}}\n\
\n\
@fragment\n\
fn hf_fs(v: HfOut) -> @location(0) vec4f {{\n\
    if !hf_column_wet(v.tile, v.local_xz) {{ discard; }}\n\
    return vec4f(0.2, 0.4, 0.8, 1.0);\n\
}}\n",
        face = dwell_world::FaceFrame::wgsl(),
        tile = TILE,
        corners = CORNERS,
        dry = DRY,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heightfield_shader_parses_and_validates() {
        let src = wgsl();
        let module = naga::front::wgsl::parse_str(&src).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
        for e in ["hf_vs", "hf_fs"] {
            assert!(module.entry_points.iter().any(|p| p.name == e), "{e}");
        }
        for f in ["hf_corner", "hf_world", "hf_column_wet"] {
            assert!(src.contains(&format!("fn {f}(")), "{f}");
        }
    }

    #[test]
    fn tiles_pack_in_rows() {
        assert_eq!(tile_origin(0), [0, 0]);
        assert_eq!(tile_origin(1), [TILE as u32, 0]);
        assert_eq!(tile_origin(TILES_PER_ROW), [0, TILE as u32]);
    }

    /// The shader's join is the reference's: the same constant from the same data.
    #[test]
    fn the_join_is_the_datas() {
        let j = dwell_look::water_tiers::HEIGHTFIELD.join_m;
        assert!(j > 0.0 && j < 1.0, "{j}");
        assert!(
            dwell_look::water_tiers::WATER_TIERS
                .iter()
                .all(|t| !t.heightfield),
            "no tier draws the heightfield until the water pass does (C6a's rest)"
        );
    }
}
