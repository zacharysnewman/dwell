//! How a hillside's colour holds across the LOD levels (ARCHITECTURE.md §6.6 "Colour"): the same
//! generated region drawn at full detail (the chunk mesher, textured) and as each LOD level's
//! sections (flat colours), rasterised orthographically from the same views with a depth buffer,
//! and the mean colour of the visible terrain compared in CIELAB (ΔE76). Albedo × face tint only,
//! as both meshes carry them; the lighting the renderer adds is the same for both. Either face:
//! face B's terrain is turned face-local (upright) before it is drawn, so the views are the same.
//!
//! The measurements behind the LOD colour (ignored, slow; run with
//! `cargo test --release -p dwell-mesh --test lod_colour -- --ignored --nocapture`):
//! `measure_lod_colour_by_level` prints each level's colour against full detail on seed 7
//! hillsides (and writes the pictures as PPMs to `$LOD_COLOUR_OUT`), `measure_fine_rise_makeup`
//! the makeup of the full-detail rise by gradient that `shared/look/lodcolour.json` holds.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use dwell_mesh::{
    ATLAS_SIZE, FlatMesh, MeshArrays, MeshSectionOptions, PADDED_VOLUME, SURFACE_STRIDE,
    build_atlas, linear_to_srgb_byte, mesh_chunk_at, mesh_section, padded_index, srgb_to_linear,
    tile_rect,
};
use dwell_world::lod::{self, LodCoord, LodKind, LodSurfaces};
use dwell_world::{
    CELL_H, CHUNK_SIZE, Chunk, ChunkCoord, Face, FaceFrame, face_of_chunk_y, local_index,
};
use dwell_worldgen::BifacialTerrain;

type V3 = [f64; 3];

/// A triangle: flat (LOD) or textured (full detail).
struct Tri {
    p: [V3; 3],
    /// Flat: the linear colour per vertex. Textured: the vertex colour (the face tint).
    c: [V3; 3],
    /// Textured: uv per vertex and the tile rectangle (u0, v0, w, h).
    tex: Option<([[f64; 2]; 3], [f64; 4])>,
}

fn chunk_tris(m: &MeshArrays, origin: V3, out: &mut Vec<Tri>) {
    for t in m.indices.chunks_exact(3) {
        let at = |k: usize, s: usize| t[k] as usize * s;
        let p =
            [0, 1, 2].map(|k| [0, 1, 2].map(|a| f64::from(m.positions[at(k, 3) + a]) + origin[a]));
        let c = [0, 1, 2].map(|k| [0, 1, 2].map(|a| f64::from(m.colors[at(k, 3) + a])));
        let uv = [0, 1, 2].map(|k| [f64::from(m.uvs[at(k, 2)]), f64::from(m.uvs[at(k, 2) + 1])]);
        let tile = [0, 1, 2, 3].map(|a| f64::from(m.tiles[at(0, 4) + a]));
        out.push(Tri {
            p,
            c,
            tex: Some((uv, tile)),
        });
    }
}

fn flat_tris(m: &FlatMesh, origin: V3, scale: f64, out: &mut Vec<Tri>) {
    for t in m.indices.chunks_exact(3) {
        let at = |k: usize| t[k] as usize * 3;
        let p = [0, 1, 2]
            .map(|k| [0, 1, 2].map(|a| f64::from(m.positions[at(k) + a]) * scale + origin[a]));
        let c = [0, 1, 2].map(|k| [0, 1, 2].map(|a| f64::from(m.colors[at(k) + a])));
        out.push(Tri { p, c, tex: None });
    }
}

/// The region measured: x and z in metres, [x0, x0 + size) × [z0, z0 + size), on one face.
#[derive(Clone, Copy, Debug)]
struct Region {
    x0: f64,
    z0: f64,
    size: f64,
    face: Face,
}

/// The world rows the region's ground spans (with a margin), lowest first.
fn ground_rows(t: &BifacialTerrain, r: Region, margin: f64) -> (i32, i32) {
    let g = t.local(r.face);
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    let (x0, x1) = ((r.x0 - margin) as i32, (r.x0 + r.size + margin) as i32);
    let (z0, z1) = ((r.z0 - margin) as i32, (r.z0 + r.size + margin) as i32);
    let frame = if r.face == Face::B {
        FaceFrame::B
    } else {
        FaceFrame::A
    };
    for z in (z0..z1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            if let Some(y) = g.ground_y(x, z) {
                let y = frame.row(y);
                lo = lo.min(y);
                hi = hi.max(y);
            }
        }
    }
    (lo, hi)
}

/// The region's chunks and a one-chunk ring, meshed at full detail (metres, world frame).
fn full_detail(t: &BifacialTerrain, r: Region) -> Vec<Tri> {
    let s = CHUNK_SIZE;
    let cx0 = (r.x0 as i32).div_euclid(s) - 1;
    let cz0 = (r.z0 as i32).div_euclid(s) - 1;
    let n = (r.size as i32) / s + 2;
    let (lo, hi) = ground_rows(t, r, f64::from(s));
    let (cy0, cy1) = (lo.div_euclid(s) - 1, hi.div_euclid(s) + 1);
    let mut chunks: HashMap<(i32, i32, i32), Chunk> = HashMap::new();
    for cz in cz0 - 1..=cz0 + n + 1 {
        for cx in cx0 - 1..=cx0 + n + 1 {
            for cy in cy0 - 1..=cy1 + 1 {
                let mut ch = Chunk::default();
                t.generate(ChunkCoord::new(cx, cy, cz), &mut ch);
                chunks.insert((cx, cy, cz), ch);
            }
        }
    }
    let voxel = |wx: i32, wy: i32, wz: i32| -> u16 {
        let key = (wx.div_euclid(s), wy.div_euclid(s), wz.div_euclid(s));
        chunks.get(&key).map_or(0, |c| {
            c.voxels()[local_index(wx.rem_euclid(s), wy.rem_euclid(s), wz.rem_euclid(s))]
        })
    };
    let mut out = Vec::new();
    for cz in cz0..=cz0 + n {
        for cx in cx0..=cx0 + n {
            for cy in cy0..=cy1 {
                let mut padded = vec![0u16; PADDED_VOLUME];
                for y in -1..=s {
                    for z in -1..=s {
                        for x in -1..=s {
                            padded[padded_index(x, y, z)] =
                                voxel(cx * s + x, cy * s + y, cz * s + z);
                        }
                    }
                }
                let mirror = face_of_chunk_y(cy) == Face::B;
                let m = mesh_chunk_at(&padded, mirror, [cx * s, cy * s, cz * s]).unwrap();
                let origin = [
                    f64::from(cx * s),
                    f64::from(cy * s) * CELL_H,
                    f64::from(cz * s),
                ];
                chunk_tris(&m.opaque, origin, &mut out);
            }
        }
    }
    out
}

fn surface_floats(s: &LodSurfaces) -> Vec<f32> {
    s.iter()
        .flat_map(|c| {
            [
                c.height,
                f32::from(c.material),
                f32::from(u8::from(c.valid) | (u8::from(c.wet) << 1)),
                c.water,
            ]
        })
        .collect()
}

/// The sections of `level` over the region (and a ring of one section), meshed (metres, world
/// frame), and the time the meshing took.
fn lod_level(t: &BifacialTerrain, r: Region, level: i32) -> (Vec<Tri>, Duration, usize) {
    let size = lod::section_size(level);
    let cell = lod::cell_size(level) as f64;
    let sec = |v: f64, origin: i64| lod::floor_div64(v.floor() as i64 - origin, size) as i32;
    let (i0, i1) = (
        sec(r.x0, lod::ORIGIN_X) - 1,
        sec(r.x0 + r.size - 1.0, lod::ORIGIN_X) + 1,
    );
    let (k0, k1) = (
        sec(r.z0, lod::ORIGIN_Z) - 1,
        sec(r.z0 + r.size - 1.0, lod::ORIGIN_Z) + 1,
    );
    let (lo, hi) = ground_rows(t, r, size as f64);
    let (j0, j1) = (
        sec(f64::from(lo), lod::ORIGIN_Y) - 1,
        sec(f64::from(hi), lod::ORIGIN_Y) + 1,
    );
    let (mut out, mut meshing, mut sections) = (Vec::new(), Duration::ZERO, 0);
    for k in k0..=k1 {
        for i in i0..=i1 {
            for j in j0..=j1 {
                let c = LodCoord::new(level, i, j, k);
                let mut cells = Vec::new();
                let (mut a, mut b) = (LodSurfaces::new(), LodSurfaces::new());
                let kind = t.generate_lod(c, &mut cells, Some(&mut a), Some(&mut b));
                if kind != LodKind::Content {
                    continue;
                }
                let o = lod::section_origin(c);
                // Sections up to level 11 lie wholly on one face (ARCHITECTURE.md §6.6).
                let face_b = o.y < i64::from(dwell_world::MIDPLANE_Y);
                if face_b != (r.face == Face::B) {
                    continue;
                }
                // In cells from the section's (or its mirror image's) bottom, as the client hands
                // them to the mesher (dwell-lod `surface_in_cells`).
                let y0 = if face_b {
                    FaceFrame::B.span(o.y, size)
                } else {
                    o.y
                } as f64;
                let mut surface = surface_floats(if face_b { &b } else { &a });
                for col in surface.chunks_mut(SURFACE_STRIDE) {
                    col[0] = ((f64::from(col[0]) - y0) / cell) as f32;
                    col[3] = ((f64::from(col[3]) - y0) / cell) as f32;
                }
                let opts = MeshSectionOptions {
                    surface: Some(&surface),
                    water_drop: 0.125 / cell,
                    mirror: face_b,
                };
                let started = Instant::now();
                let m = mesh_section(&cells, &opts).unwrap();
                meshing += started.elapsed();
                sections += 1;
                let origin = [o.x as f64, o.y as f64 * CELL_H, o.z as f64];
                flat_tris(&m.opaque, origin, cell, &mut out);
            }
        }
    }
    (out, meshing, sections)
}

/// Face B's terrain turned face-local (upright, re-wound), face A's as it is.
fn upright(mut tris: Vec<Tri>, face: Face) -> Vec<Tri> {
    if face == Face::B {
        for t in &mut tris {
            for p in &mut t.p {
                p[1] = FaceFrame::B.y(p[1]);
            }
            t.p.swap(1, 2);
            t.c.swap(1, 2);
            if let Some((uv, _)) = &mut t.tex {
                uv.swap(1, 2);
            }
        }
    }
    tris
}

/// Drops faces wholly below the ground (by more than two of the level's cells): the insides of
/// sections that the region's cut-open sides would show, which the game never does. Upright
/// triangles.
fn surface_only(tris: Vec<Tri>, t: &BifacialTerrain, r: Region, level: i32) -> Vec<Tri> {
    let g = t.local(r.face);
    let margin = 2.0 * lod::cell_size(level) as f64 * CELL_H + 1.0;
    tris.into_iter()
        .filter(|tri| {
            let top = tri.p.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
            let cx = (tri.p[0][0] + tri.p[1][0] + tri.p[2][0]) / 3.0;
            let cz = (tri.p[0][2] + tri.p[1][2] + tri.p[2][2]) / 3.0;
            let ground = f64::from(g.column_at(cx.floor() as i32, cz.floor() as i32).height);
            top >= ground - margin
        })
        .collect()
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    a.map(|v| v / l)
}

/// An orthographic view looking down at `elevation_deg` toward `azimuth_deg`.
#[derive(Clone, Copy, Debug)]
struct View {
    elevation_deg: f64,
    azimuth_deg: f64,
}

impl View {
    /// Forward (into the scene), right and up.
    fn basis(self) -> (V3, V3, V3) {
        let (e, a) = (
            self.elevation_deg.to_radians(),
            self.azimuth_deg.to_radians(),
        );
        let f = [e.cos() * a.sin(), -e.sin(), e.cos() * a.cos()];
        let right = norm(cross(f, [0.0, 1.0, 0.0]));
        let up = cross(right, f);
        (f, right, up)
    }
}

/// Grazing views (a far hillside seen from the ground), oblique ones and one from above.
const VIEWS: [View; 6] = [
    View {
        elevation_deg: 8.0,
        azimuth_deg: 30.0,
    },
    View {
        elevation_deg: 8.0,
        azimuth_deg: 210.0,
    },
    View {
        elevation_deg: 20.0,
        azimuth_deg: 120.0,
    },
    View {
        elevation_deg: 20.0,
        azimuth_deg: 300.0,
    },
    View {
        elevation_deg: 45.0,
        azimuth_deg: 75.0,
    },
    View {
        elevation_deg: 80.0,
        azimuth_deg: 0.0,
    },
];

/// The visible terrain's pixels in the region, linear RGB (None: nothing, or outside).
struct Picture {
    w: usize,
    h: usize,
    px: Vec<Option<V3>>,
}

/// Draws the triangles of the region (only those: nothing outside it hides it) with a depth
/// buffer and back faces culled, textured ones sampled from the atlas as the shader does.
fn rasterise(tris: &[Tri], r: Region, view: View, pixel_m: f64, atlas: &[u8]) -> Picture {
    let (f, right, up) = view.basis();
    let (mut ylo, mut yhi) = (f64::MAX, f64::MIN);
    for t in tris {
        for p in t.p {
            ylo = ylo.min(p[1]);
            yhi = yhi.max(p[1]);
        }
    }
    let (mut sx0, mut sx1, mut sy0, mut sy1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for x in [r.x0, r.x0 + r.size] {
        for z in [r.z0, r.z0 + r.size] {
            for y in [ylo, yhi] {
                let p = [x, y, z];
                sx0 = sx0.min(dot(p, right));
                sx1 = sx1.max(dot(p, right));
                sy0 = sy0.min(dot(p, up));
                sy1 = sy1.max(dot(p, up));
            }
        }
    }
    let w = ((sx1 - sx0) / pixel_m).ceil() as usize + 1;
    let h = ((sy1 - sy0) / pixel_m).ceil() as usize + 1;
    let mut depth = vec![f64::MAX; w * h];
    let mut px: Vec<Option<V3>> = vec![None; w * h];
    let texel = |tile: [f64; 4], uv: [f64; 2]| -> V3 {
        let fu = uv[0] - uv[0].floor();
        let fv = uv[1] - uv[1].floor();
        let ax = ((tile[0] + fu * tile[2]) * ATLAS_SIZE as f64).floor() as usize;
        let ay = ((tile[1] + fv * tile[3]) * ATLAS_SIZE as f64).floor() as usize;
        let o = (ay.min(ATLAS_SIZE - 1) * ATLAS_SIZE + ax.min(ATLAS_SIZE - 1)) * 4;
        [0, 1, 2].map(|i| srgb_to_linear(atlas[o + i]))
    };
    let inside =
        |p: V3| p[0] >= r.x0 && p[0] < r.x0 + r.size && p[2] >= r.z0 && p[2] < r.z0 + r.size;
    for t in tris {
        let (lo, hi) = (
            [0, 2].map(|a| t.p.iter().map(|p| p[a]).fold(f64::MAX, f64::min)),
            [0, 2].map(|a| t.p.iter().map(|p| p[a]).fold(f64::MIN, f64::max)),
        );
        if hi[0] <= r.x0 || lo[0] >= r.x0 + r.size || hi[1] <= r.z0 || lo[1] >= r.z0 + r.size {
            continue;
        }
        let n = cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0]));
        if dot(n, f) >= 0.0 {
            continue;
        }
        let s = t.p.map(|p| {
            [
                (dot(p, right) - sx0) / pixel_m,
                (sy1 - dot(p, up)) / pixel_m,
                dot(p, f),
            ]
        });
        let area =
            (s[1][0] - s[0][0]) * (s[2][1] - s[0][1]) - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
        if area.abs() < 1e-12 {
            continue;
        }
        let ext =
            |a: usize, f: fn(f64, f64) -> f64, init: f64| s.iter().map(|v| v[a]).fold(init, f);
        let minx = ext(0, f64::min, f64::MAX).floor().max(0.0) as usize;
        let maxx = (ext(0, f64::max, f64::MIN).ceil() as usize).min(w - 1);
        let miny = ext(1, f64::min, f64::MAX).floor().max(0.0) as usize;
        let maxy = (ext(1, f64::max, f64::MIN).ceil() as usize).min(h - 1);
        for y in miny..=maxy {
            for x in minx..=maxx {
                let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                let e = |a: V3, b: V3| (b[0] - a[0]) * (cy - a[1]) - (b[1] - a[1]) * (cx - a[0]);
                let (w0, w1, w2) = (
                    e(s[1], s[2]) / area,
                    e(s[2], s[0]) / area,
                    e(s[0], s[1]) / area,
                );
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let d = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let i = y * w + x;
                if d >= depth[i] {
                    continue;
                }
                depth[i] = d;
                let lerp =
                    |v: [V3; 3]| [0, 1, 2].map(|a| w0 * v[0][a] + w1 * v[1][a] + w2 * v[2][a]);
                if !inside(lerp(t.p)) {
                    px[i] = None;
                    continue;
                }
                let c = lerp(t.c);
                px[i] = Some(match &t.tex {
                    None => c,
                    Some((uv, tile)) => {
                        let u = [0, 1].map(|a| w0 * uv[0][a] + w1 * uv[1][a] + w2 * uv[2][a]);
                        let tx = texel(*tile, u);
                        [0, 1, 2].map(|a| tx[a] * c[a])
                    }
                });
            }
        }
    }
    Picture { w, h, px }
}

/// CIELAB (D65) of a linear sRGB colour.
fn lab(c: V3) -> V3 {
    let [r, g, b] = c;
    let x = 0.4124 * r + 0.3576 * g + 0.1805 * b;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = 0.0193 * r + 0.1192 * g + 0.9505 * b;
    let f = |t: f64| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn delta_e(a: V3, b: V3) -> f64 {
    let (a, b) = (lab(a), lab(b));
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// The mean linear colour of a picture's terrain pixels.
fn mean(p: &Picture) -> V3 {
    let (mut sum, mut n) = ([0.0; 3], 0usize);
    for c in p.px.iter().flatten() {
        for a in 0..3 {
            sum[a] += c[a];
        }
        n += 1;
    }
    assert!(n > 1000, "the region shows terrain ({n} pixels)");
    sum.map(|v| v / n as f64)
}

fn srgb_hex(c: V3) -> String {
    let b = c.map(linear_to_srgb_byte);
    format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

/// Writes a picture as a binary PPM (sRGB), the background black.
fn write_ppm(p: &Picture, path: &std::path::Path) {
    let mut bytes = format!("P6\n{} {}\n255\n", p.w, p.h).into_bytes();
    for c in &p.px {
        bytes.extend(c.map_or([0, 0, 0], |c| c.map(linear_to_srgb_byte)));
    }
    std::fs::write(path, bytes).unwrap();
}

/// Inland hillside windows: moderate slope, well above the sea, aligned to level-4 sections so
/// every level's cells tile them.
fn hillsides(seed: u64, count: usize, size: f64, face: Face) -> Vec<Region> {
    let g = dwell_worldgen::TerrainGenerator::new(dwell_worldgen::bifacial::face_seed(seed, face));
    let mut out = Vec::new();
    for gz in -12..=12 {
        for gx in -12..=12 {
            if out.len() == count {
                return out;
            }
            let (x, z) = (gx * 1500, gz * 1500);
            let c = g.column_at(x, z);
            if c.outside || g.coast_at(x, z) < 3000.0 || c.height < dwell_world::SEA_LEVEL_M as f32 + 8.0 {
                continue;
            }
            let d = 16;
            let sx = (g.column_at(x + d, z).height - g.column_at(x - d, z).height) / (2 * d) as f32;
            let sz = (g.column_at(x, z + d).height - g.column_at(x, z - d).height) / (2 * d) as f32;
            if (0.15..0.6).contains(&sx.hypot(sz)) {
                let a = lod::section_size(4) as f64;
                let snap = |v: i32| {
                    ((f64::from(v) - lod::ORIGIN_X as f64) / a).floor() * a + lod::ORIGIN_X as f64
                };
                out.push(Region {
                    x0: snap(x),
                    z0: snap(z),
                    size,
                    face,
                });
            }
        }
    }
    out
}

/// Each level's ΔE from full detail per view, over `levels`.
fn level_errors(
    t: &BifacialTerrain,
    r: Region,
    levels: std::ops::RangeInclusive<i32>,
    out_dir: Option<&std::path::Path>,
    tag: &str,
) -> Vec<(i32, Vec<f64>, Duration, usize)> {
    let atlas = build_atlas();
    let full = surface_only(upright(full_detail(t, r), r.face), t, r, 0);
    let pixel = r.size / 512.0;
    let base: Vec<_> = VIEWS
        .iter()
        .map(|v| rasterise(&full, r, *v, pixel, &atlas))
        .collect();
    let mut rows = Vec::new();
    let mut line = String::new();
    for (vi, p) in base.iter().enumerate() {
        line += &format!(" v{vi} {}", srgb_hex(mean(p)));
        if let Some(dir) = out_dir {
            write_ppm(p, &dir.join(format!("{tag}_v{vi}_L0.ppm")));
        }
    }
    println!("{tag} full detail:{line}");
    for level in levels {
        let (tris, meshing, sections) = lod_level(t, r, level);
        let tris = surface_only(upright(tris, r.face), t, r, level);
        let mut errors = Vec::new();
        let mut line = String::new();
        for (vi, v) in VIEWS.iter().enumerate() {
            let p = rasterise(&tris, r, *v, pixel, &atlas);
            let e = delta_e(mean(&p), mean(&base[vi]));
            line += &format!(" v{vi} {} ΔE {e:.1}", srgb_hex(mean(&p)));
            errors.push(e);
            if let Some(dir) = out_dir {
                write_ppm(&p, &dir.join(format!("{tag}_v{vi}_L{level}.ppm")));
            }
        }
        println!("{tag} L{level} ({sections} sections meshed in {meshing:?}):{line}");
        rows.push((level, errors, meshing, sections));
    }
    rows
}

/// The owner's bug (2026-10-21): far hillsides turned olive and brown with each coarser level
/// while full detail is green. Each LOD level's visible colour stays within ΔE 4 of full detail
/// on average over the views (each within 10: a grazing view onto a slope facing away shows the
/// fine steps' risers, which a coarse top has none of), on both faces of the mirror world (face B is face
/// A's exact mirror image, so it must agree too). Before the fix: ΔE 8–29, the grazing views
/// worst.
#[test]
fn each_lod_levels_hillside_keeps_the_full_detail_colour_on_both_faces() {
    let seed = 7;
    let t = BifacialTerrain::mirrored(seed);
    let a = hillsides(seed, 1, 128.0, Face::A)[0];
    let mut per_face = Vec::new();
    for face in [Face::A, Face::B] {
        let r = Region { face, ..a };
        let rows = level_errors(&t, r, 1..=4, None, &format!("face {face:?}"));
        for (level, errors, _, _) in &rows {
            let avg = errors.iter().sum::<f64>() / errors.len() as f64;
            let max = errors.iter().copied().fold(0.0, f64::max);
            assert!(
                avg <= 4.0 && max <= 10.0,
                "face {face:?} level {level}: ΔE {errors:.1?} (mean {avg:.1})"
            );
        }
        per_face.push(rows);
    }
    for (a, b) in per_face[0].iter().zip(&per_face[1]) {
        for (ea, eb) in a.1.iter().zip(&b.1) {
            assert!((ea - eb).abs() < 0.5, "face B matches face A: {a:?} {b:?}");
        }
    }
}

/// The measurement (see the crate docs). `LOD_COLOUR_SIZE` (m), `LOD_COLOUR_REGIONS` and
/// `LOD_COLOUR_OUT` (a directory for the pictures) tune it.
#[test]
#[ignore]
fn measure_lod_colour_by_level() {
    let seed = 7;
    let t = BifacialTerrain::new(seed);
    let env = |k: &str| std::env::var(k).ok();
    let size: f64 = env("LOD_COLOUR_SIZE")
        .and_then(|v| v.parse().ok())
        .unwrap_or(256.0);
    let regions: usize = env("LOD_COLOUR_REGIONS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let out_dir = env("LOD_COLOUR_OUT").map(std::path::PathBuf::from);
    if let Some(d) = &out_dir {
        std::fs::create_dir_all(d).unwrap();
    }
    let (mut meshing, mut sections) = (Duration::ZERO, 0);
    let mut all: HashMap<i32, Vec<f64>> = HashMap::new();
    for (ri, r) in hillsides(seed, regions, size, Face::A)
        .into_iter()
        .enumerate()
    {
        println!("region {ri}: {r:?}");
        for (level, errors, m, s) in
            level_errors(&t, r, 1..=5, out_dir.as_deref(), &format!("r{ri}"))
        {
            meshing += m;
            sections += s;
            all.entry(level).or_default().extend(errors);
        }
    }
    let mut levels: Vec<_> = all.into_iter().collect();
    levels.sort_by_key(|l| l.0);
    for (level, e) in levels {
        let avg = e.iter().sum::<f64>() / e.len() as f64;
        let max = e.iter().copied().fold(0.0, f64::max);
        println!("SUMMARY L{level}: ΔE mean {avg:.1} max {max:.1}");
    }
    println!(
        "MESHING {sections} sections in {meshing:?} ({:.2} ms each)",
        meshing.as_secs_f64() * 1e3 / sections.max(1) as f64
    );
}

/// What the full-detail rise is made of, by the ground's gradient: the share of the rise (faces'
/// area projected on the vertical) that shows the top tile (ramps), a banded side tile's top band
/// (the fringe), or the rest (dirt) — `shared/look/lodcolour.json`'s points.
#[test]
#[ignore]
fn measure_fine_rise_makeup() {
    let seed = 7;
    let t = BifacialTerrain::new(seed);
    let g = t.local(Face::A);
    let top_tile = tile_rect("grass");
    let banded = [tile_rect("grassSide"), tile_rect("dirtSkirt")];
    let is = |tile: &[f64; 4], t: &dwell_mesh::TileRect| {
        (tile[0] - t.u0).abs() < 1e-9 && (tile[1] - t.v0).abs() < 1e-9
    };
    // [rise top, rise fringe, rise rest, run] per gradient bin of 0.1.
    let mut bins = vec![[0.0f64; 4]; 16];
    for r in hillsides(seed, 3, 256.0, Face::A) {
        for tri in surface_only(full_detail(&t, r), &t, r, 0) {
            let n = cross(sub(tri.p[1], tri.p[0]), sub(tri.p[2], tri.p[0]));
            let area = dot(n, n).sqrt() / 2.0;
            if area < 1e-9 {
                continue;
            }
            let nn = norm(n);
            let c = [0, 1, 2].map(|a| (tri.p[0][a] + tri.p[1][a] + tri.p[2][a]) / 3.0);
            if c[0] < r.x0 || c[0] >= r.x0 + r.size || c[2] < r.z0 || c[2] >= r.z0 + r.size {
                continue;
            }
            let (x, z) = (c[0].floor() as i32, c[2].floor() as i32);
            let h = |x: i32, z: i32| f64::from(g.column_at(x, z).height);
            let grad = ((h(x + 2, z) - h(x - 2, z)) / 4.0).hypot((h(x, z + 2) - h(x, z - 2)) / 4.0);
            let bin = ((grad / 0.1) as usize).min(15);
            let rise = area * (nn[0] * nn[0] + nn[2] * nn[2]).sqrt();
            bins[bin][3] += area * nn[1].max(0.0);
            let Some((uv, tile)) = &tri.tex else { continue };
            if rise < 1e-9 {
                continue;
            }
            if is(tile, &top_tile) {
                bins[bin][0] += rise;
                continue;
            }
            let band = banded.iter().any(|t| is(tile, t));
            let (k, mut fringe, mut all) = (6, 0, 0);
            for i in 0..k {
                for j in 0..k - i {
                    let (a, b) = ((i as f64 + 0.33) / k as f64, (j as f64 + 0.33) / k as f64);
                    let w = [1.0 - a - b, a, b];
                    let v = w[0] * uv[0][1] + w[1] * uv[1][1] + w[2] * uv[2][1];
                    if band && v - v.floor() >= 1.0 - CELL_H {
                        fringe += 1;
                    }
                    all += 1;
                }
            }
            let f = f64::from(fringe) / f64::from(all);
            bins[bin][1] += rise * f;
            bins[bin][2] += rise * (1.0 - f);
        }
    }
    for (i, b) in bins.iter().enumerate() {
        let rise = b[0] + b[1] + b[2];
        if b[3] < 1.0 || rise <= 0.0 {
            continue;
        }
        println!(
            "RISE gradient {:.1}–{:.1}: rise/run {:.3}; top {:.2}, fringe {:.2}, rest {:.2}; run {:.0} m²",
            i as f64 * 0.1,
            (i + 1) as f64 * 0.1,
            rise / b[3],
            b[0] / rise,
            b[1] / rise,
            b[2] / rise,
            b[3]
        );
    }
}
