//! The LOD colour map (a preview behind `?lodmap=on`; ARCHITECTURE.md §6.6 "Colour map"): nested
//! rings of a minimap-style colour texture centred on the eye, which the LOD sections' tops take
//! their colour from, so a river narrower than an LOD cell still shows, continuous, at every
//! distance (`shared/look/lodmap.json`).
//!
//! - **Placing** ([`window`]): each ring covers `size`² texels of `texel_m` around the eye on the
//!   eye's face, in world texel coordinates (face-local x and z are the world's on both faces),
//!   recentred in steps of `snapTexels`. Texel (gx, gz) is stored at (gx mod size, gz mod size)
//!   ([`toroidal`]): a move rasterises only the strips it uncovers ([`uncovered`]).
//! - **Rasterising** ([`raster`], a job in the generation workers, which hold the world map):
//!   each texel is its column's top block's LOD colour by the terrain's own rules
//!   (`TerrainGenerator::column_lod`), water darkened by depth; then the rivers from their
//!   polylines (`river_hit`, through the terrain's meander), at least `riverMinTexels` wide and
//!   anti-aliased, so they never break into blobs. Alpha 0: beyond the disc.
//! - **Scheduling** ([`LodMap`], the main thread): a ring starts a move only once its last is
//!   whole; meanwhile it claims only what it still holds ([`Ring::valid`]) and the shader takes the
//!   next coarser ring there. At most `jobsInFlight` jobs run at once, so a frame never waits.

use dwell_look::lod_map::{LOD_MAP, LodMapRing, LodMapTunables};
use dwell_world::FaceFrame;
use dwell_worldgen::terrain::{TerrainGenerator, Water};

/// A rectangle of world texels, half-open: x in `x0..x1`, z in `z0..z1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i64,
    pub z0: i64,
    pub x1: i64,
    pub z1: i64,
}

impl Rect {
    pub fn width(&self) -> i64 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> i64 {
        self.z1 - self.z0
    }
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.z1 <= self.z0
    }
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let r = Rect {
            x0: self.x0.max(o.x0),
            z0: self.z0.max(o.z0),
            x1: self.x1.min(o.x1),
            z1: self.z1.min(o.z1),
        };
        (!r.is_empty()).then_some(r)
    }
    pub fn contains(&self, x: i64, z: i64) -> bool {
        (self.x0..self.x1).contains(&x) && (self.z0..self.z1).contains(&z)
    }
}

/// The window a ring covers around the eye (world x, z in metres): `size`² texels, its centre
/// snapped to `snap` texels.
pub fn window(ring: LodMapRing, snap: i64, eye: [f64; 2]) -> Rect {
    let t = ring.texel_m as f64;
    let centre = |v: f64| ((v / t).floor() as i64).div_euclid(snap) * snap;
    let (cx, cz) = (centre(eye[0]), centre(eye[1]));
    let h = ring.size / 2;
    Rect {
        x0: cx - h,
        z0: cz - h,
        x1: cx + h,
        z1: cz + h,
    }
}

/// Where world texel `g` lies in a ring's texture of `size`.
pub fn toroidal(g: i64, size: i64) -> i64 {
    g.rem_euclid(size)
}

/// The parts of `window` outside `kept` (inside it, or none), as rectangles: the rows above and
/// below `kept` across the whole window, then the columns either side of it.
pub fn uncovered(window: &Rect, kept: Option<&Rect>) -> Vec<Rect> {
    let Some(k) = kept else {
        return vec![*window];
    };
    let w = window;
    [
        Rect { z1: k.z0, ..*w },
        Rect { z0: k.z1, ..*w },
        Rect {
            x1: k.x0,
            z0: k.z0,
            z1: k.z1,
            ..*w
        },
        Rect {
            x0: k.x1,
            z0: k.z0,
            z1: k.z1,
            ..*w
        },
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

/// A rectangle split into jobs of at most `texels` (whole rows each).
pub fn tiles(r: &Rect, texels: i64) -> Vec<Rect> {
    let rows = (texels / r.width().max(1)).max(1);
    (r.z0..r.z1)
        .step_by(rows as usize)
        .map(|z| Rect {
            z0: z,
            z1: (z + rows).min(r.z1),
            ..*r
        })
        .collect()
}

/// The colours the map paints with: each block state's LOD top colour (sRGB, the LOD mesher's
/// `lod_color`) and the water's.
fn top_rgb(state: dwell_blocks::StateId) -> [f64; 3] {
    let c = dwell_mesh::lod_color(state, 0);
    [16, 8, 0].map(|s| dwell_look::srgb_to_linear(f64::from((c >> s) & 0xff) / 255.0))
}

fn srgb_byte(v: f64) -> u8 {
    dwell_mesh::linear_to_srgb_byte(v)
}

/// Rasterises `rect` (world texels of `texel_m` on the face `terrain` generates) into rgba8 sRGB,
/// rows of x from `z0`: the column's top colour, water darkened by its depth, rivers drawn over
/// it from their polylines at least `river_min_texels` wide; alpha 0 beyond the disc.
pub fn raster(terrain: &TerrainGenerator, t: &LodMapTunables, texel_m: i64, rect: &Rect) -> Vec<u8> {
    let water_state = dwell_blocks::state(t.water_block).unwrap_or(0);
    let water = top_rgb(water_state);
    let shade = |depth: f64| {
        let k = 1.0 - (-depth.max(0.0) / t.water_depth_m).exp();
        1.0 + (t.water_deep_shade - 1.0) * k
    };
    let s = texel_m as f64;
    let min_half = 0.5 * t.river_min_texels * s;
    let mut out = Vec::with_capacity((rect.width() * rect.height() * 4) as usize);
    for gz in rect.z0..rect.z1 {
        for gx in rect.x0..rect.x1 {
            let (x, z) = (gx * texel_m + texel_m / 2, gz * texel_m + texel_m / 2);
            let col = terrain.column_lod(x, z, texel_m);
            if col.outside {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let mut c = if col.water != Water::None {
                let k = shade(f64::from(col.level - col.height));
                water.map(|v| v * k)
            } else {
                top_rgb(TerrainGenerator::top_state(&col))
            };
            if let Some(r) = terrain.river_hit(x, z, min_half) {
                // Coverage of the texel by the channel (its edge anti-aliased over one texel).
                let a = ((r.half - r.d) / s + 0.5).clamp(0.0, 1.0);
                if a > 0.0 {
                    let k = shade(r.depth.min(t.water_depth_m * 0.5));
                    for (v, w) in c.iter_mut().zip(water) {
                        *v += (w * k - *v) * a;
                    }
                }
            }
            out.extend(c.map(srgb_byte));
            out.push(255);
        }
    }
    out
}

/// One ring's state.
#[derive(Clone, Debug, Default)]
pub struct Ring {
    /// The window it is filling (or holds).
    pub window: Option<Rect>,
    /// What it holds now: the shader reads it only here.
    pub valid: Option<Rect>,
    pending: Vec<Rect>,
    in_flight: usize,
}

/// A raster job to run: ring `ring`'s `rect` on `face`, for generation `generation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileJob {
    pub ring: usize,
    pub face: FaceFrame,
    pub texel_m: i64,
    pub rect: Rect,
    pub generation: u32,
}

/// The rings' scheduler (main thread): which strips to rasterise as the eye moves, and what each
/// ring holds.
#[derive(Clone, Debug)]
pub struct LodMap {
    pub tunables: LodMapTunables,
    pub rings: Vec<Ring>,
    pub face: Option<FaceFrame>,
    /// Bumped on a face change: results of older jobs are dropped.
    pub generation: u32,
    in_flight: usize,
}

impl Default for LodMap {
    fn default() -> Self {
        Self::new(LOD_MAP)
    }
}

impl LodMap {
    pub fn new(tunables: LodMapTunables) -> Self {
        Self {
            rings: vec![Ring::default(); tunables.rings.len()],
            tunables,
            face: None,
            generation: 0,
            in_flight: 0,
        }
    }

    /// The jobs to start this frame for the eye at world `eye` (x, z) on `face`.
    pub fn update(&mut self, eye: [f64; 2], face: FaceFrame) -> Vec<TileJob> {
        if self.face != Some(face) {
            self.face = Some(face);
            self.generation = self.generation.wrapping_add(1);
            self.in_flight = 0;
            for r in &mut self.rings {
                *r = Ring::default();
            }
        }
        let t = self.tunables;
        for (ring, spec) in self.rings.iter_mut().zip(t.rings) {
            if ring.in_flight > 0 || !ring.pending.is_empty() {
                continue;
            }
            let want = window(*spec, t.snap_texels, eye);
            if ring.window == Some(want) {
                continue;
            }
            let kept = ring.valid.and_then(|v| v.intersect(&want));
            ring.pending = uncovered(&want, kept.as_ref())
                .iter()
                .flat_map(|r| tiles(r, t.tile_texels))
                .collect();
            // Rows nearest the eye first.
            ring.pending.reverse();
            ring.window = Some(want);
            ring.valid = kept;
        }
        let mut jobs = Vec::new();
        // Finest ring first.
        for (i, (ring, spec)) in self.rings.iter_mut().zip(t.rings).enumerate() {
            while self.in_flight < t.jobs_in_flight {
                let Some(rect) = ring.pending.pop() else {
                    break;
                };
                ring.in_flight += 1;
                self.in_flight += 1;
                jobs.push(TileJob {
                    ring: i,
                    face,
                    texel_m: spec.texel_m,
                    rect,
                    generation: self.generation,
                });
            }
        }
        jobs
    }

    /// A job's result is in (uploaded by the caller when this returns true: it is still wanted).
    pub fn on_done(&mut self, job: &TileJob) -> bool {
        if job.generation != self.generation {
            return false;
        }
        self.in_flight = self.in_flight.saturating_sub(1);
        let Some(ring) = self.rings.get_mut(job.ring) else {
            return false;
        };
        ring.in_flight = ring.in_flight.saturating_sub(1);
        if ring.in_flight == 0 && ring.pending.is_empty() {
            ring.valid = ring.window;
        }
        true
    }
}

/// The shader's block for the eye at world `eye` (`render/lod_map.rs`): per ring, the rectangle
/// it holds relative to the eye (m; empty when none) and the eye's texel coordinates mod size,
/// 1 / texel and 1 / size.
pub fn ring_uniforms(map: &LodMap, eye: [f64; 3]) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    for (ring, spec) in map.rings.iter().zip(map.tunables.rings) {
        let t = spec.texel_m as f64;
        let n = spec.size as f64;
        match ring.valid {
            Some(r) => out.push([
                (r.x0 as f64 * t - eye[0]) as f32,
                (r.z0 as f64 * t - eye[2]) as f32,
                (r.x1 as f64 * t - eye[0]) as f32,
                (r.z1 as f64 * t - eye[2]) as f32,
            ]),
            None => out.push([1.0, 1.0, -1.0, -1.0]),
        }
        out.push([
            (eye[0] / t).rem_euclid(n) as f32,
            (eye[2] / t).rem_euclid(n) as f32,
            (1.0 / t) as f32,
            (1.0 / n) as f32,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::both_faces;

    #[test]
    fn a_move_uncovers_only_the_strips_it_enters_and_they_tile_the_window() {
        let spec = LodMapRing {
            texel_m: 4,
            size: 64,
        };
        let a = window(spec, 8, [10.0, -10.0]);
        assert_eq!(a.width(), 64);
        let b = window(spec, 8, [10.0 + 4.0 * 20.0, -10.0 - 4.0 * 9.0]);
        let kept = a.intersect(&b).unwrap();
        let parts = uncovered(&b, Some(&kept));
        let area: i64 = parts.iter().map(|r| r.width() * r.height()).sum();
        assert_eq!(area + kept.width() * kept.height(), 64 * 64);
        for p in &parts {
            assert!(p.intersect(&kept).is_none(), "{p:?} overlaps what is kept");
        }
        let tiled: i64 = parts
            .iter()
            .flat_map(|r| tiles(r, 100))
            .inspect(|t| assert!(t.width() * t.height() <= 100.max(t.width())))
            .map(|r| r.width() * r.height())
            .sum();
        assert_eq!(tiled, area);
    }

    #[test]
    fn toroidal_storage_wraps_negative_texels_and_every_window_texel_has_its_own_slot() {
        let spec = LodMapRing {
            texel_m: 16,
            size: 32,
        };
        let w = window(spec, 4, [-1000.0, 333.0]);
        let mut seen = vec![false; 32 * 32];
        for z in w.z0..w.z1 {
            for x in w.x0..w.x1 {
                let at = (toroidal(z, 32) * 32 + toroidal(x, 32)) as usize;
                assert!(!seen[at]);
                seen[at] = true;
            }
        }
        assert_eq!(toroidal(-1, 32), 31);
    }

    both_faces!(
        /// The rings follow the eye on its face (face-local x and z are the world's), restart on a
        /// face change, and hold their window once every job is in; the uniforms place the eye
        /// inside each ring's rectangle at its texel coordinates.
        fn the_rings_follow_the_eye_on_either_face_and_hold_their_window_once_filled(f: FaceFrame) {
            let mut map = LodMap::default();
            let eye = [1234.5, -987.25];
            let mut jobs = map.update(eye, f.other());
            assert!(!jobs.is_empty());
            // A face change drops what was in flight.
            let stale = jobs[0];
            jobs = map.update(eye, f);
            assert!(!map.on_done(&stale));
            let mut guard = 0;
            while !jobs.is_empty() {
                for j in &jobs {
                    assert_eq!(j.face, f);
                    assert!(map.on_done(j));
                }
                jobs = map.update(eye, f);
                guard += 1;
                assert!(guard < 10_000);
            }
            for (ring, spec) in map.rings.iter().zip(map.tunables.rings) {
                let w = window(*spec, map.tunables.snap_texels, eye);
                assert_eq!(ring.valid, Some(w));
                let (gx, gz) = (
                    (eye[0] / spec.texel_m as f64).floor() as i64,
                    (eye[1] / spec.texel_m as f64).floor() as i64,
                );
                assert!(w.contains(gx, gz));
            }
            let u = ring_uniforms(&map, [eye[0], f.y(40.0), eye[1]]);
            for (k, spec) in map.tunables.rings.iter().enumerate() {
                let (r, e) = (u[2 * k], u[2 * k + 1]);
                assert!(r[0] < 0.0 && r[1] < 0.0 && r[2] > 0.0 && r[3] > 0.0);
                let gx = (eye[0] / spec.texel_m as f64).rem_euclid(spec.size as f64);
                assert!((f64::from(e[0]) - gx).abs() < 1e-3);
            }
            // A small move refills only strips.
            let moved = [eye[0] + 300.0, eye[1]];
            let jobs = map.update(moved, f);
            let texels: i64 = jobs.iter().map(|j| j.rect.width() * j.rect.height()).sum();
            assert!(texels < map.tunables.rings[0].size.pow(2));
        }
    );
}
