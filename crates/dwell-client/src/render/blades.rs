//! A plant's distance fade blade by blade, not pixel by pixel (WORK_QUEUE, the owner's "Yes
//! performance pass to get up to 60fps", 2026-11-15). A cross plant (tall grass, flowers) fades
//! out over its block's `fade` (shared/blocks/dwell.json, e.g. tall grass 48–88 m): before, every
//! blade out to the fade's end was drawn and a growing share of its pixels discarded (dithered),
//! so the fade's band, most of the grass on screen past a few metres, cost its full pixels. Now
//! each column of plants takes a fixed random rank, and over the fade a growing share of the
//! columns is dropped whole in the vertex stage (no pixels), each dissolving (dithered) over the
//! last [`DISSOLVE`] of its rank as the share passes it. The same in the shadow passes, so a
//! dropped blade casts nothing.
//!
//! A column is the cell its cross stands in: its centre, the point a quad's four corners share
//! (each corner less its half-width along the quad, from the quad's normal and its `u`), rounded
//! to the cell's middle, so the corners agree exactly. Its distance from the eye is across the
//! ground (x, z: a mirror in y leaves it alone, so both faces agree); its rank a hash of its world
//! x, z (modulo the wind's period, as the gust field's place). This module is the reference the
//! WGSL ([`wgsl`]) follows, and its tests.

/// The share of the rank (0–1) over which a column dissolves as the fade passes it.
pub const DISSOLVE: f32 = 0.2;

/// Half a cross quad's width (dwell-mesh `CROSS_HALF_WIDTH`: corner to corner of the cell).
pub const CROSS_HALF_WIDTH: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// The column a cross quad's corner stands in, chunk-local (x, z of the cell's middle): the
/// corner `position` (x, z) less its half-width along the quad, the quad's direction from its
/// `normal`'s horizontal part (the mesher's normal is (−dz, ·, dx), leaned up), the corner's side
/// from its `u` (left under ½).
pub fn column(position: [f32; 2], normal: [f32; 2], u: f32) -> [f32; 2] {
    let (ax, az) = (normal[1] + 1e-6, -normal[0]);
    let len = (ax * ax + az * az).sqrt();
    let half = if u < 0.5 {
        -CROSS_HALF_WIDTH
    } else {
        CROSS_HALF_WIDTH
    };
    [
        (position[0] - ax / len * half).floor() + 0.5,
        (position[1] - az / len * half).floor() + 0.5,
    ]
}

/// A column's rank (0–1) from its world cell (x, z).
pub fn rank(cell: [i32; 2]) -> f32 {
    let qx = (cell[0] + 4096) as u32;
    let qz = (cell[1] + 4096) as u32;
    let mut h = qx.wrapping_mul(0x8da6_b343) ^ qz.wrapping_mul(0xd816_3841);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    ((h ^ (h >> 16)) >> 8) as f32 / 16_777_216.0
}

/// GLSL's smoothstep.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a column of `rank` is drawn at `distance` m (across the ground) through its
/// fade from `start` to `end`: 1 in full, 0 dropped whole.
pub fn blade(rank: f32, start: f32, end: f32, distance: f32) -> f32 {
    let share = smoothstep(start, end, distance);
    ((rank - share * (1.0 + DISSOLVE)) / DISSOLVE + 1.0).clamp(0.0, 1.0)
}

/// The WGSL of [`column`], [`rank`] and [`blade`] (`cutout_vertex` calls `blade_fade`).
pub fn wgsl() -> String {
    format!(
        "const BLADE_DISSOLVE: f32 = {DISSOLVE:?};
const CROSS_HALF_WIDTH: f32 = {CROSS_HALF_WIDTH:?};
// render/blades.rs: a cross plant's fade, column by column (1 drawn, 0 dropped whole).
fn blade_fade(v: CutoutIn, draw: Draw) -> f32 {{
    let across = normalize(vec2f(v.normal.z + 1e-6, -v.normal.x));
    let half = select(CROSS_HALF_WIDTH, -CROSS_HALF_WIDTH, v.uv.x < 0.5);
    let centre = floor(v.position.xz - across * half) + 0.5;
    let rel = draw.offset.xyz + qrot(draw.quat, vec3f(centre.x, v.position.y, centre.y) * draw.offset.w);
    let world = floor(frame.tan.zw + rel.xz);
    let q = vec2u(vec2i(world) + 4096);
    var h = (q.x * 0x8da6b343u) ^ (q.y * 0xd8163841u);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    let rank = f32((h ^ (h >> 16u)) >> 8u) / 16777216.0;
    let share = smoothstep(v.plant.z, v.plant.w, length(rel.xz));
    return clamp((rank - share * (1.0 + BLADE_DISSOLVE)) / BLADE_DISSOLVE + 1.0, 0.0, 1.0);
}}
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The snorm8 bytes the cutout pipeline reads a normal from (render/mod.rs `snorm8x3`).
    fn snorm(n: [f32; 3]) -> [f32; 3] {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        n.map(|c| ((c / len).clamp(-1.0, 1.0) * 127.0).round() / 127.0)
    }

    dwell_world::both_faces!(
        fn every_corner_of_a_cross_finds_its_own_column(f: FaceFrame) {
            // The mesher's tall grass, jittered and turned per cell, on both faces: each quad's
            // four corners (their packed normal, u and position) agree on the cell they stand in,
            // so a quad is kept or dropped whole (a corner dropped alone would draw a sliver).
            // A chunk-local layer of grass on stone, mirrored for face B as its chunks are.
            let grass = f.state(dwell_blocks::require("dwell:tall_grass"));
            let stone = f.state(dwell_blocks::require("dwell:stone"));
            let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
            let (ground, up) = if f.is_b() { (9, 8) } else { (8, 9) };
            for x in 0..12 {
                for z in 0..12 {
                    padded[dwell_mesh::padded_index(x, ground, z)] = stone;
                    padded[dwell_mesh::padded_index(x, up, z)] = grass;
                }
            }
            let m = dwell_mesh::mesh_chunk(&padded, f.is_b()).expect("meshes");
            let c = &m.cutout;
            assert!(!c.indices.is_empty(), "the grass meshed");
            let n = c.positions.len() / 3;
            assert_eq!(n % 4, 0);
            for q in 0..n / 4 {
                let cols: Vec<[f32; 2]> = (0..4)
                    .map(|k| {
                        let i = q * 4 + k;
                        let p = [c.positions[i * 3], c.positions[i * 3 + 2]];
                        let nn =
                            snorm([c.normals[i * 3], c.normals[i * 3 + 1], c.normals[i * 3 + 2]]);
                        column(p, [nn[0], nn[2]], c.uvs[i * 2])
                    })
                    .collect();
                assert!(cols.iter().all(|k| *k == cols[0]), "quad {q}: {cols:?}");
                let p = [c.positions[q * 12], c.positions[q * 12 + 2]];
                assert!(
                    (cols[0][0] - p[0]).abs() < 1.5 && (cols[0][1] - p[1]).abs() < 1.5,
                    "quad {q}'s column {:?} is its cell's, near {p:?}",
                    cols[0]
                );
            }
        }
    );

    #[test]
    fn the_share_drawn_falls_from_all_to_none_over_the_fade() {
        // Every column drawn in full before the fade, none past it, and the share kept falls
        // through it (about half at its middle): the band is drawn by fewer blades, not by all
        // of them with fewer pixels.
        let ranks: Vec<f32> = (-50..50)
            .flat_map(|x| (-50..50).map(move |z| rank([x, z])))
            .collect();
        let kept = |d: f32| {
            ranks.iter().map(|&r| blade(r, 48.0, 88.0, d)).sum::<f32>() / ranks.len() as f32
        };
        let dropped = |d: f32| {
            ranks
                .iter()
                .filter(|&&r| blade(r, 48.0, 88.0, d) == 0.0)
                .count() as f32
                / ranks.len() as f32
        };
        assert_eq!(kept(10.0), 1.0);
        assert_eq!(kept(48.0), 1.0);
        assert_eq!(kept(88.0), 0.0);
        assert_eq!(kept(120.0), 0.0);
        let mid = kept(68.0);
        assert!((0.4..0.6).contains(&mid), "{mid}");
        assert!((0.35..0.6).contains(&dropped(68.0)), "{}", dropped(68.0));
        let mut last = 1.0;
        for d in 48..=88 {
            let k = kept(d as f32);
            assert!(k <= last + 1e-6, "{d} m: {k} after {last}");
            last = k;
        }
    }

    #[test]
    fn ranks_are_spread_evenly() {
        let mut bins = [0u32; 10];
        for x in -100..100 {
            for z in -100..100 {
                let r = rank([x, z]);
                assert!((0.0..1.0).contains(&r));
                bins[(r * 10.0) as usize] += 1;
            }
        }
        for b in bins {
            assert!((3600..4400).contains(&b), "{bins:?}");
        }
    }

    #[test]
    fn the_shader_follows_this_reference() {
        let src = super::super::shaders::source(4);
        assert!(
            src.contains(&wgsl()),
            "the cutout shader takes blades::wgsl"
        );
        assert!(src.contains("o.blade = blade_fade(v, draw);"));
        // The scene's and the shadows' fragments fade a plant by its blade, not its distance.
        assert!(src.contains("fade = i.blade;"));
        assert!(src.contains("i.blade, i.leaf < 0.5)"));
    }
}
