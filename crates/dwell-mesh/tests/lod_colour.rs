//! How a distant hillside's colour holds across the LOD levels (ARCHITECTURE.md §6.6, "LOD colour"):
//! the same generated region drawn at full detail (the chunk mesher, textured) and as each LOD
//! level's sections (flat colours), rasterised orthographically from the same views with a depth
//! buffer, and the mean colour of the visible terrain compared in CIELAB (ΔE76). Albedo × face
//! tint only, as both meshes carry them; the lighting the renderer adds is the same for both.

use std::collections::HashMap;

use dwell_mesh::{
    ATLAS_SIZE, FlatMesh, MeshArrays, MeshSectionOptions, PADDED_VOLUME, SURFACE_STRIDE,
    build_atlas, mesh_chunk_at, mesh_section, padded_index, srgb_to_linear,
};
use dwell_world::lod::{self, LodCoord, LodKind, LodSurfaces};
use dwell_world::{CELL_H, CHUNK_SIZE, Chunk, ChunkCoord, Face, local_index};
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
        let p = [0, 1, 2].map(|k| [0, 1, 2].map(|a| f64::from(m.positions[at(k, 3) + a]) + origin[a]));
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

/// The region measured: x and z in metres, [x0, x0 + size) × [z0, z0 + size).
#[derive(Clone, Copy, Debug)]
struct Region {
    x0: f64,
    z0: f64,
    size: f64,
}

/// The rows (face A) the ground spans over the region with a margin.
fn ground_rows(t: &BifacialTerrain, r: Region, margin: f64) -> (i32, i32) {
    let g = t.local(Face::A);
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    let (x0, x1) = ((r.x0 - margin) as i32, (r.x0 + r.size + margin) as i32);
    let (z0, z1) = ((r.z0 - margin) as i32, (r.z0 + r.size + margin) as i32);
    for z in (z0..z1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            if let Some(y) = g.ground_y(x, z) {
                lo = lo.min(y);
                hi = hi.max(y);
            }
        }
    }
    (lo, hi)
}

/// Face A's chunks over the region and a one-chunk ring, meshed at full detail (metres).
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
                let m = mesh_chunk_at(&padded, false, [cx * s, cy * s, cz * s]).unwrap();
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

/// Face A's sections of `level` over the region (and a ring of one section), meshed (metres).
fn lod_level(t: &BifacialTerrain, r: Region, level: i32) -> Vec<Tri> {
    let size = lod::section_size(level);
    let cell = lod::cell_size(level) as f64;
    let sec = |v: f64, origin: i64| lod::floor_div64(v.floor() as i64 - origin, size) as i32;
    let (i0, i1) = (sec(r.x0, lod::ORIGIN_X) - 1, sec(r.x0 + r.size - 1.0, lod::ORIGIN_X) + 1);
    let (k0, k1) = (sec(r.z0, lod::ORIGIN_Z) - 1, sec(r.z0 + r.size - 1.0, lod::ORIGIN_Z) + 1);
    let (lo, hi) = ground_rows(t, r, size as f64);
    let (j0, j1) = (
        sec(f64::from(lo), lod::ORIGIN_Y) - 1,
        sec(f64::from(hi), lod::ORIGIN_Y) + 1,
    );
    let mut out = Vec::new();
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
                let surface = surface_floats(&a);
                assert_eq!(surface.len() % SURFACE_STRIDE, 0);
                let opts = MeshSectionOptions {
                    surface: Some(&surface),
                    water_drop: 0.125 / cell,
                    mirror: false,
                };
                let m = mesh_section(&cells, &opts).unwrap();
                let o = lod::section_origin(c);
                let origin = [o.x as f64, o.y as f64 * CELL_H, o.z as f64];
                flat_tris(&m.opaque, origin, cell, &mut out);
            }
        }
    }
    out
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

/// The view: looking along `dir` (from the camera into the scene), orthographic.
#[derive(Clone, Copy, Debug)]
struct View {
    elevation_deg: f64,
    azimuth_deg: f64,
}

impl View {
    fn basis(self) -> (V3, V3, V3) {
        let (e, a) = (self.elevation_deg.to_radians(), self.azimuth_deg.to_radians());
        let f = [e.cos() * a.sin(), -e.sin(), e.cos() * a.cos()];
        let right = norm(cross(f, [0.0, 1.0, 0.0]));
        let up = cross(right, f);
        (f, right, up)
    }
}

/// The visible terrain's pixels in the region, linear RGB.
struct Picture {
    w: usize,
    h: usize,
    px: Vec<Option<V3>>,
}

fn rasterise(tris: &[Tri], r: Region, view: View, pixel_m: f64, atlas: &[u8]) -> Picture {
    let (f, right, up) = view.basis();
    // Screen bounds of the region's box (heights from the triangles).
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
    for t in tris {
        // Back faces: culled, as the renderer does.
        let n = cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0]));
        if dot(n, f) >= 0.0 {
            continue;
        }
        let s = t.p.map(|p| [(dot(p, right) - sx0) / pixel_m, (sy1 - dot(p, up)) / pixel_m, dot(p, f)]);
        let area = (s[1][0] - s[0][0]) * (s[2][1] - s[0][1]) - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
        if area.abs() < 1e-12 {
            continue;
        }
        let minx = s.iter().map(|v| v[0]).fold(f64::MAX, f64::min).floor().max(0.0) as usize;
        let maxx = (s.iter().map(|v| v[0]).fold(f64::MIN, f64::max).ceil() as usize).min(w - 1);
        let miny = s.iter().map(|v| v[1]).fold(f64::MAX, f64::min).floor().max(0.0) as usize;
        let maxy = (s.iter().map(|v| v[1]).fold(f64::MIN, f64::max).ceil() as usize).min(h - 1);
        for y in miny..=maxy {
            for x in minx..=maxx {
                let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                let e = |a: [f64; 3], b: [f64; 3]| (b[0] - a[0]) * (cy - a[1]) - (b[1] - a[1]) * (cx - a[0]);
                let (w0, w1, w2) = (e(s[1], s[2]) / area, e(s[2], s[0]) / area, e(s[0], s[1]) / area);
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let d = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let i = y * w + x;
                if d >= depth[i] {
                    continue;
                }
                depth[i] = d;
                let lerp = |v: [V3; 3]| [0, 1, 2].map(|a| w0 * v[0][a] + w1 * v[1][a] + w2 * v[2][a]);
                let world = lerp(t.p);
                let inside = world[0] >= r.x0
                    && world[0] < r.x0 + r.size
                    && world[2] >= r.z0
                    && world[2] < r.z0 + r.size;
                if !inside {
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

#[derive(Clone, Copy, Debug, Default)]
struct Stats {
    /// Mean linear RGB of the visible pixels.
    mean: V3,
    /// Share of pixels whose CIELAB hue is green (over 100°).
    green: f64,
    pixels: usize,
}

fn stats(p: &Picture) -> Stats {
    let (mut sum, mut n, mut green) = ([0.0; 3], 0usize, 0usize);
    for c in p.px.iter().flatten() {
        for a in 0..3 {
            sum[a] += c[a];
        }
        let l = lab(*c);
        if l[2].atan2(l[1]).to_degrees() > 100.0 {
            green += 1;
        }
        n += 1;
    }
    let n1 = n.max(1) as f64;
    Stats {
        mean: sum.map(|v| v / n1),
        green: green as f64 / n1,
        pixels: n,
    }
}

fn srgb_hex(c: V3) -> String {
    let b = c.map(|v| dwell_mesh::linear_to_srgb_byte(v));
    format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

fn hue(c: V3) -> f64 {
    let l = lab(c);
    l[2].atan2(l[1]).to_degrees()
}

/// Writes a picture as a binary PPM (sRGB), the background black.
fn write_ppm(p: &Picture, path: &std::path::Path) {
    let mut bytes = format!("P6\n{} {}\n255\n", p.w, p.h).into_bytes();
    for c in &p.px {
        let b = c.map_or([0, 0, 0], |c| c.map(dwell_mesh::linear_to_srgb_byte));
        bytes.extend(b);
    }
    std::fs::write(path, bytes).unwrap();
}

const VIEWS: [View; 6] = [
    View { elevation_deg: 8.0, azimuth_deg: 30.0 },
    View { elevation_deg: 8.0, azimuth_deg: 210.0 },
    View { elevation_deg: 20.0, azimuth_deg: 120.0 },
    View { elevation_deg: 20.0, azimuth_deg: 300.0 },
    View { elevation_deg: 45.0, azimuth_deg: 75.0 },
    View { elevation_deg: 80.0, azimuth_deg: 0.0 },
];

/// Inland hillside windows of seed 7: moderate slope, well above the sea.
fn hillsides(seed: u64, count: usize, size: f64) -> Vec<Region> {
    let g = dwell_worldgen::TerrainGenerator::new(dwell_worldgen::bifacial::face_seed(seed, Face::A));
    let mut out = Vec::new();
    for gz in -12..=12 {
        for gx in -12..=12 {
            if out.len() == count {
                return out;
            }
            let (x, z) = (gx * 1500, gz * 1500);
            let c = g.column_at(x, z);
            if c.outside || c.coast < 3000.0 || c.height < dwell_world::SEA_LEVEL_M as f32 + 8.0 {
                continue;
            }
            let d = 16;
            let sx = (g.column_at(x + d, z).height - g.column_at(x - d, z).height) / (2 * d) as f32;
            let sz = (g.column_at(x, z + d).height - g.column_at(x, z - d).height) / (2 * d) as f32;
            let slope = sx.hypot(sz);
            if (0.15..0.6).contains(&slope) {
                // Section-aligned at level 4, so every level's sections tile it.
                let a = lod::section_size(4) as f64;
                let snap = |v: i32| ((f64::from(v) - lod::ORIGIN_X as f64) / a).floor() * a + lod::ORIGIN_X as f64;
                out.push(Region { x0: snap(x), z0: snap(z), size });
            }
        }
    }
    out
}

/// The measurement (ignored: slow; run with `--release -- --ignored --nocapture`). Writes PPMs to
/// `$LOD_COLOUR_OUT` when set.
#[test]
#[ignore]
fn measure_lod_colour_by_level() {
    let seed = 7;
    let t = BifacialTerrain::new(seed);
    let atlas = build_atlas();
    let size: f64 = std::env::var("LOD_COLOUR_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(256.0);
    let regions: usize = std::env::var("LOD_COLOUR_REGIONS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let out_dir = std::env::var("LOD_COLOUR_OUT").ok().map(std::path::PathBuf::from);
    for (ri, r) in hillsides(seed, regions, size).into_iter().enumerate() {
        let started = std::time::Instant::now();
        let full = full_detail(&t, r);
        println!("region {ri} {r:?}: full detail {} triangles ({:?})", full.len(), started.elapsed());
        let mut levels = vec![(0, full)];
        for level in 1..=5 {
            let started = std::time::Instant::now();
            let tris = lod_level(&t, r, level);
            println!("  level {level}: {} triangles ({:?})", tris.len(), started.elapsed());
            levels.push((level, tris));
        }
        for (vi, view) in VIEWS.iter().enumerate() {
            let pixel = size / 512.0;
            let pics: Vec<_> = levels
                .iter()
                .map(|(l, tris)| (*l, rasterise(tris, r, *view, pixel, &atlas)))
                .collect();
            let base = stats(&pics[0].1);
            let mut line = format!(
                "  view {:>2}° {:>3}°: full {} hue {:.0} green {:.0}%",
                view.elevation_deg,
                view.azimuth_deg,
                srgb_hex(base.mean),
                hue(base.mean),
                100.0 * base.green
            );
            for (l, p) in &pics[1..] {
                let s = stats(p);
                line += &format!(
                    " | L{l} {} ΔE {:.1} green {:.0}%",
                    srgb_hex(s.mean),
                    delta_e(s.mean, base.mean),
                    100.0 * s.green
                );
            }
            println!("{line}");
            if let Some(dir) = &out_dir {
                std::fs::create_dir_all(dir).unwrap();
                for (l, p) in &pics {
                    write_ppm(p, &dir.join(format!("r{ri}_v{vi}_L{l}.ppm")));
                }
            }
        }
    }
}

#[test]
#[ignore]
fn print_tile_averages() {
    let atlas = build_atlas();
    for name in ["grass", "grassSide", "dirt", "dirtSkirt", "stone", "sand"] {
        let r = dwell_mesh::tile_rect(name);
        let (x0, y0) = (
            (r.u0 * ATLAS_SIZE as f64).round() as usize,
            (r.v0 * ATLAS_SIZE as f64).round() as usize,
        );
        let mut halves = [[0.0; 3]; 2];
        for y in 0..32 {
            for x in 0..32 {
                let o = ((y0 + y) * ATLAS_SIZE + x0 + x) * 4;
                for a in 0..3 {
                    halves[y / 16][a] += srgb_to_linear(atlas[o + a]) / 512.0;
                }
            }
        }
        let all = [0, 1, 2].map(|a| (halves[0][a] + halves[1][a]) / 2.0);
        println!(
            "TILE {name}: avg {:06x} computed {} rows0-15 {} rows16-31 {}",
            dwell_mesh::average_tile_color(name),
            srgb_hex(all),
            srgb_hex(halves[0]),
            srgb_hex(halves[1])
        );
    }
}
