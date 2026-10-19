//! Ports `client/src/lod/lodSystem.test.ts`. The TS fakes resolve worker promises when the test
//! says; here the fake host queues each job and `finish` reports results back with their tokens.

use super::*;
use crate::grid::{MAX_ALIGNED_LEVEL, is_face_b_section, origin_m};
use dwell_world::lod::{PAD, VOLUME, ancestor, mirror_row_bottom, section_origin};
use dwell_world::{MIDPLANE_Y, inside_build_volume};

/// A deterministic PRNG (mulberry32), as the TS tests use.
fn rng(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed as i32;
    move || {
        a = a.wrapping_add(0x6d2b_79f5_u32 as i32);
        let mut t = (a ^ ((a as u32) >> 15) as i32).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ ((t as u32) >> 7) as i32).wrapping_mul(61 | t)) ^ t;
        f64::from((t ^ ((t as u32) >> 14) as i32) as u32) / 4_294_967_296.0
    }
}

fn zero() -> f64 {
    0.0
}

/// The flat test world with its surface at `top` instead of 0.
fn flat_bounds_at(top: f64) -> LodBounds {
    LodBounds {
        lo: top,
        hi: top - 1.0,
        lo_b: f64::NEG_INFINITY,
        hi_b: f64::NEG_INFINITY,
        any_inside: true,
        bifacial: false,
    }
}

/// The bifacial test world's crust per face: the 10-layer core (Phase 16), so 20 layers in all.
const CRUST: i64 = dwell_world::CORE_LAYERS as i64;

/// Both faces, each flat: ground where a face-local cell's bottom is below `CRUST`.
fn bifacial_bounds() -> LodBounds {
    LodBounds {
        lo: f64::NEG_INFINITY,
        hi: CRUST as f64,
        lo_b: f64::NEG_INFINITY,
        hi_b: CRUST as f64,
        any_inside: true,
        bifacial: true,
    }
}

/// Face A's ground in the bifacial test world is this material, face B's is stone.
const FACE_A_MATERIAL: u16 = 3;
fn stone() -> u16 {
    dwell_blocks::require("dwell:stone")
}
const SURFACE_LEN: usize = (PAD * PAD) as usize * SURFACE_STRIDE;

/// The bifacial test world at LOD: both faces flat, as `bifacial_bounds` says.
fn bifacial_section(c: LodCoord) -> GeneratedSection {
    let mut cells = vec![0u16; VOLUME];
    let kind = kind_from_bounds(c, &bifacial_bounds());
    let y0 = section_origin(c).y;
    let size = cell_size(c.level);
    if kind != LodKind::Empty {
        for y in -1..=32 {
            let bottom = y0 + i64::from(y) * size;
            let on_a = row_face(c, y) == Face::A;
            // The face-local bottom of the cell, within the crust: on face A its own, on face B
            // that of its mirror image; the other side of the midplane is that face's crust.
            let solid = if on_a {
                bottom < CRUST && bottom + size > -CRUST
            } else {
                mirror_row_bottom(bottom, size) < CRUST
                    && mirror_row_bottom(bottom, size) + size > -CRUST
            };
            if !solid {
                continue;
            }
            let material = if on_a { FACE_A_MATERIAL } else { stone() };
            for z in -1..=32 {
                for x in -1..=32 {
                    cells[cell_index(x, y, z)] = material;
                }
            }
        }
    }
    GeneratedSection {
        cells,
        surface: Some(vec![0.0; SURFACE_LEN]),
        surface_b: Some(vec![0.0; SURFACE_LEN]),
    }
}

/// The flat world at LOD: stone where a cell's bottom is below 0.
fn flat_section(c: LodCoord) -> GeneratedSection {
    flat_section_at(c, 0)
}

fn flat_section_at(c: LodCoord, top: i64) -> GeneratedSection {
    let mut cells = vec![0u16; VOLUME];
    let kind = kind_from_bounds(c, &flat_bounds_at(top as f64));
    let y0 = section_origin(c).y;
    let size = cell_size(c.level);
    if kind != LodKind::Empty {
        for y in -1..=32 {
            if y0 + i64::from(y) * size >= top {
                continue;
            }
            for z in -1..=32 {
                for x in -1..=32 {
                    cells[cell_index(x, y, z)] = 2;
                }
            }
        }
    }
    GeneratedSection {
        cells,
        surface: None,
        surface_b: None,
    }
}

enum Job {
    Bounds(i32, i32, i32),
    Generate(LodCoord, u64),
    Mesh(u64, usize),
}

/// What a meshing job was given (the TS kept every job's options and cells).
#[derive(Clone, Debug)]
struct MeshRecord {
    water_drop: f64,
    /// The first column of `surface` (None: no surface).
    surface_head: Option<[f32; 4]>,
    /// With `record_cells`: whether the cells hold face A's material, and stone.
    has_face_a: bool,
    has_stone: bool,
}

/// Worker pools whose jobs finish when the test says, in any order; the renderer; the streamed
/// chunks; the network.
#[derive(Default)]
struct Host {
    pending: Vec<Job>,
    /// Generation (and bounds) below this level never finishes: a stalled or very slow device.
    stuck_below: i32,
    /// Serve the bifacial test world instead of the flat one.
    bifacial: bool,
    /// The flat test world's surface (m; 0 unless a test needs ground deeper than the midplane).
    flat_top: i64,
    /// Every generated section's surface is a river: floor 118 m, water 121 m.
    river: bool,
    /// Every section asked of the generator.
    generated: Vec<LodCoord>,
    meshes: Vec<MeshRecord>,
    record_cells: bool,
    /// Bytes of each mesh (for the cache budget); 0: empty meshes.
    mesh_bytes: usize,
    // The view.
    meshed: HashSet<LodId>,
    /// Every section given a payload, in order.
    set: Vec<LodId>,
    shown: Vec<(LodId, u8)>,
    stand_ins: Vec<StandInBox>,
    // The chunks: drawable (and loaded) where `drawable` says, or held.
    drawable: Option<Box<dyn Fn(ChunkCoord) -> bool>>,
    held: HashSet<ChunkCoord>,
    // The network.
    requests: Vec<Vec<LodSectionRequest>>,
    chunk_requests: Vec<Vec<ChunkCoordNet>>,
    arriving: Vec<ChunkCoord>,
}

impl Host {
    fn new() -> Self {
        Self::default()
    }
    fn with_chunks(drawable: impl Fn(ChunkCoord) -> bool + 'static) -> Self {
        Self {
            drawable: Some(Box::new(drawable)),
            ..Self::default()
        }
    }
}

impl SectionSource for Host {
    fn lod_bounds(&mut self, level: i32, i: i32, k: i32) {
        if level >= self.stuck_below {
            self.pending.push(Job::Bounds(level, i, k));
        }
    }
    fn lod(&mut self, c: LodCoord, token: u64) {
        self.generated.push(c);
        if c.level >= self.stuck_below {
            self.pending.push(Job::Generate(c, token));
        }
    }
}

impl SectionMesher for Host {
    fn mesh_section(&mut self, job: LodMeshJob, token: u64) {
        // Selection only needs to know a mesh exists (the mesher's own tests test meshing).
        let (has_face_a, has_stone) = if self.record_cells {
            let s = stone();
            (job.cells.contains(&FACE_A_MATERIAL), job.cells.contains(&s))
        } else {
            (false, false)
        };
        self.meshes.push(MeshRecord {
            water_drop: job.water_drop,
            surface_head: job.surface.as_ref().map(|s| [s[0], s[1], s[2], s[3]]),
            has_face_a,
            has_stone,
        });
        self.pending.push(Job::Mesh(token, self.mesh_bytes));
    }
}

impl LodView for Host {
    fn set_lod_section(&mut self, id: LodId, _o: Vec3, _c: f64, meshes: Option<SectionMeshes>) {
        if meshes.is_some() {
            self.set.push(id);
            self.meshed.insert(id);
        } else {
            self.meshed.remove(&id);
        }
    }
    fn show_lod_sections(&mut self, visible: &[(LodId, u8)]) {
        self.shown = visible.to_vec();
    }
    fn show_lod_stand_ins(&mut self, stand_ins: &[StandInBox]) {
        self.stand_ins = stand_ins.to_vec();
    }
}

impl ChunkReadiness for Host {
    fn chunk_drawable(&self, c: ChunkCoord) -> bool {
        self.drawable.as_ref().is_some_and(|f| f(c)) || self.held.contains(&c)
    }
}

impl LodNetwork for Host {
    fn request_sections(&mut self, sections: Vec<LodSectionRequest>) {
        self.requests.push(sections);
    }
    fn request_chunks(&mut self, coords: Vec<ChunkCoordNet>) {
        self.arriving
            .extend(coords.iter().map(|&c| ChunkCoord::from_array(c)));
        self.chunk_requests.push(coords);
    }
}

/// Finishes a random share of the pending jobs, reporting their results back.
fn finish(lod: &mut LodSystem, h: &mut Host, random: &mut dyn FnMut() -> f64, share: f64) {
    let now = std::mem::take(&mut h.pending);
    let mut done = Vec::new();
    for job in now {
        if random() < share {
            done.push(job);
        } else {
            h.pending.push(job);
        }
    }
    for job in done {
        match job {
            Job::Bounds(level, i, k) => {
                let b = if h.bifacial {
                    bifacial_bounds()
                } else {
                    flat_bounds_at(h.flat_top as f64)
                };
                lod.on_lod_bounds(level, i, k, Some(b));
            }
            Job::Generate(c, token) => {
                let mut s = if h.bifacial {
                    bifacial_section(c)
                } else {
                    flat_section_at(c, h.flat_top)
                };
                if h.river {
                    let mut surface = vec![0.0f32; SURFACE_LEN];
                    for col in surface.chunks_mut(SURFACE_STRIDE) {
                        col.copy_from_slice(&[118.0, 12.0, 3.0, 121.0]);
                    }
                    s.surface = Some(surface);
                }
                lod.on_generated(token, Some(s));
            }
            Job::Mesh(token, bytes) => {
                let mut m = SectionMeshes::default();
                if bytes > 0 {
                    m.opaque.positions = vec![0.0; bytes / 4];
                }
                lod.on_meshed(token, m, h);
            }
        }
    }
}

fn camera(position: Vec3, yaw_deg: f64, pitch_deg: f64) -> LodCamera {
    LodCamera {
        position,
        yaw_deg,
        pitch_deg,
        fov_y_deg: 75.0,
        aspect: 16.0 / 9.0,
        height_px: 1080.0,
    }
}

fn contains(c: LodCoord, p: Vec3) -> bool {
    let o = origin_m(c);
    let s = section_size(c.level) as f64;
    (0..3).all(|a| p[a] >= o[a] && p[a] < o[a] + s)
}

fn covering(leaves: &[LodCoord], p: Vec3) -> usize {
    leaves.iter().filter(|&&c| contains(c, p)).count()
}

/// A random point in the camera's view, up to `range` away, inside the world (face A).
fn point_in_view(cam: &LodCamera, random: &mut dyn FnMut() -> f64, range: f64) -> Option<Vec3> {
    let yaw = cam.yaw_deg.to_radians();
    let pitch = cam.pitch_deg.to_radians();
    let f = [
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    ];
    let r = [-yaw.cos(), 0.0, yaw.sin()];
    let u = [
        r[1] * f[2] - r[2] * f[1],
        r[2] * f[0] - r[0] * f[2],
        r[0] * f[1] - r[1] * f[0],
    ];
    let ty = (cam.fov_y_deg * std::f64::consts::PI / 360.0).tan();
    let sx = (random() * 2.0 - 1.0) * ty * cam.aspect;
    let sy = (random() * 2.0 - 1.0) * ty;
    let d = random().powi(2) * range;
    let mut p = [0.0; 3];
    for a in 0..3 {
        p[a] = cam.position[a] + d * (f[a] + sx * r[a] + sy * u[a]);
    }
    let radius = f64::from(dwell_world::WORLD_RADIUS);
    let inside = p[1] >= f64::from(MIDPLANE_Y)
        && inside_build_volume(p[0].floor() as i64, p[1].floor() as i64, p[2].floor() as i64)
        && p[0] * p[0] + p[2] * p[2] < radius * radius;
    inside.then_some(p)
}

fn system(pixel_error: f64, cache_mb: usize, gen_jobs: usize, mesh_jobs: usize) -> LodSystem {
    LodSystem::new(LodOptions::new(
        pixel_error,
        cache_mb * 1_048_576,
        gen_jobs,
        mesh_jobs,
    ))
}

fn leaves(s: &Selection, with_stand_ins: bool) -> Vec<LodCoord> {
    let mut l: Vec<LodCoord> = s
        .drawn
        .iter()
        .chain(&s.empty)
        .chain(&s.chunks)
        .copied()
        .collect();
    if with_stand_ins {
        l.extend(s.stand_ins.iter().map(|x| x.hole));
    }
    l
}

fn cc(x: i32, y: i32, z: i32) -> ChunkCoord {
    ChunkCoord::new(x, y, z)
}

fn chebyshev(c: ChunkCoord, at: ChunkCoord) -> i32 {
    (c.x - at.x)
        .abs()
        .max((c.y - at.y).abs())
        .max((c.z - at.z).abs())
}

// --- LOD selection (§6.6) ------------------------------------------------------------------------

#[test]
fn covers_the_view_with_no_holes_or_overlaps_every_frame_while_moving_and_swapping() {
    let mut random = rng(7);
    // Chunks within 3 of the body are streamed (and drawable after a while).
    let mut host = Host::with_chunks(|c| chebyshev(c, cc(0, 0, 0)) <= 3);
    let mut lod = system(4.0, 64, 16, 8);
    // Walk, look around, then rise with the dev camera to 200 km (four of the world's radii).
    let mut path = Vec::new();
    for i in 0..60 {
        let i = f64::from(i);
        path.push(camera([0.5 + i * 3.0, 1.6, 0.5], i * 6.0, -10.0));
    }
    for i in 0..90 {
        let i = f64::from(i);
        let alt = 1.6 * (2e5f64 / 1.6).powf(i / 89.0);
        path.push(camera(
            [180.0, alt, 0.5 + i * alt * 0.01],
            30.0,
            -20.0 - i * 0.7,
        ));
    }
    let mut checked = 0;
    let mut frames = 0u32;
    for cam in &path {
        for _ in 0..3 {
            frames += 1;
            // A device that keeps up: well within FORCE_CHUNKS_AFTER_MS, so never a forced path.
            lod.update(cam, f64::from(frames), &mut host);
            finish(&mut lod, &mut host, &mut random, 0.5);
            if !lod.active {
                continue;
            }
            let s = lod.last_selection().clone();
            // Every drawn section has a mesh and is shown; nothing else is shown.
            for &c in &s.drawn {
                assert!(host.meshed.contains(&id_of(c)));
            }
            let shown: HashSet<LodId> = host.shown.iter().map(|x| x.0).collect();
            assert_eq!(shown.len(), s.drawn.len());
            let l = leaves(&s, false);
            for _ in 0..100 {
                let Some(p) = point_in_view(cam, &mut random, 3e5) else {
                    continue;
                };
                let n = covering(&l, p);
                assert_eq!(n, 1, "frame {frames}: {n} leaves hold {p:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 2000, "{checked}");
    // Near the body the streamed chunks stand in for level 0; high above, coarse levels only.
    let start = camera([0.5, 1.6, 0.5], 0.0, -10.0);
    let mut i = 0;
    while i < 2000 && !lod.chunk_visible(cc(0, 0, 0)) {
        frames += 1;
        lod.update(&start, f64::from(frames) * 16.0, &mut host);
        finish(&mut lod, &mut host, &mut random, 1.0);
        i += 1;
    }
    assert!(!lod.last_selection().chunks.is_empty());
    assert!(lod.chunk_visible(cc(0, 0, 0)));
    assert!(!lod.chunk_visible(cc(40, 0, 0))); // not streamed: LOD draws there
}

#[test]
fn shows_the_streamed_chunks_around_the_player_while_the_levels_above_them_are_not_ready() {
    // Regression (phone playtest): chunks were drawn only once every LOD level down to them was
    // generated, so a device whose LOD generation stalled drew nothing at all.
    let mut host = Host::with_chunks(|c| chebyshev(c, cc(0, 0, 0)) <= 3);
    host.stuck_below = 12;
    let mut lod = system(4.0, 64, 16, 8);
    let cam = camera([0.5, 1.6, 0.5], 0.0, -10.0);
    let mut random = rng(3);
    // Looking around, then standing still: 3 s, past FORCE_CHUNKS_AFTER_MS.
    for frame in 1..=60 {
        let at = if frame < 40 {
            camera([0.5, 1.6, 0.5], f64::from(frame) * 9.0, -10.0)
        } else {
            cam
        };
        lod.update(&at, f64::from(frame) * 50.0, &mut host);
        finish(&mut lod, &mut host, &mut zero, 1.0);
        let l = leaves(lod.last_selection(), true);
        for _ in 0..100 {
            if let Some(p) = point_in_view(&at, &mut random, 3e6) {
                assert!(covering(&l, p) <= 1);
            }
        }
    }
    assert!(lod.active);
    for c in [cc(0, 0, 0), cc(1, 0, 2), cc(-2, -1, 1)] {
        assert!(lod.chunk_visible(c), "{c:?}");
    }
    // Coarse sections are not drawn over the chunks (unready ones are stand-ins); drawn ones have meshes.
    for &c in &lod.last_selection().drawn {
        assert!(host.meshed.contains(&id_of(c)));
    }
    assert!(!lod.chunk_visible(cc(40, 0, 0))); // not streamed
}

#[test]
fn fills_the_holes_of_the_forced_path_with_clipped_ancestors_no_mountain_is_cut_off() {
    // Regression (phone playtest, 2026-10-07): with level-0/1 jobs lagging and the chunks streamed
    // as a sphere, the forced path to the chunks left the unready sections around them undrawn:
    // peaks vanished (loaded, collidable, hidden) and the section below showed a flat cut.
    let mut host = Host::with_chunks(|c| c.x * c.x + c.y * c.y + c.z * c.z <= 9 + 3);
    host.stuck_below = 2;
    let mut lod = system(4.0, 64, 16, 8);
    lod.set_detail_distance(Some(128.0));
    let cam = camera([0.5, 1.6, 0.5], 0.0, -10.0);
    for frame in 1..=1500 {
        lod.update(&cam, f64::from(frame) * 50.0, &mut host);
        finish(&mut lod, &mut host, &mut zero, 1.0);
    }
    let s = lod.last_selection();
    assert!(!s.stand_ins.is_empty());
    let l = leaves(s, true);
    for x in (-200..=200).step_by(4) {
        for z in (-200..=200).step_by(4) {
            for y in [-1.0, 1.0] {
                let p = [f64::from(x) + 0.5, y, f64::from(z) + 0.5];
                assert_eq!(covering(&l, p), 1, "{p:?}");
            }
        }
    }
    // Each stand-in's source is a meshed ancestor of its hole.
    for st in &s.stand_ins {
        assert!(host.meshed.contains(&id_of(st.from)));
        assert!(st.from.level > st.hole.level);
        assert!(contains(st.from, origin_m(st.hole)));
    }
    assert_eq!(host.stand_ins.len(), s.stand_ins.len());
}

#[test]
fn draws_the_streamed_chunks_inside_buried_sections_caves_deep_underground() {
    // Regression (phone playtest): the LOD treats rock well below the surface as buried and the
    // walk stopped there, so the streamed chunks of a cave inside it were never drawn. In the flat
    // world everything below y = 0 is buried.
    let at = cc(0, -12, 0); // 384 m down
    let mut host = Host::with_chunks(move |c| chebyshev(c, at) <= 3);
    let mut lod = system(4.0, 64, 16, 8);
    let cam = camera([16.5, f64::from(at.y * 32 + 16), 16.5], 30.0, 0.0);
    for frame in 1..=40 {
        lod.update(&cam, f64::from(frame) * 50.0, &mut host);
        finish(&mut lod, &mut host, &mut zero, 1.0);
    }
    assert!(lod.active);
    for c in [at, cc(1, -12, 2), cc(-2, -13, 1)] {
        assert!(lod.chunk_visible(c), "{c:?}");
    }
    // Far from the camera buried rock stays undrawn (no chunks there, nothing to show).
    assert!(!lod.chunk_visible(cc(0, -60, 0)));
}

#[test]
fn turning_around_shows_the_detail_already_loaded_it_does_not_depend_on_the_view() {
    // Regression (phone playtest): sections out of view were never refined, so turning showed
    // coarse sections popping to fine ones everywhere the view swept.
    let mut host = Host::new();
    let mut lod = system(8.0, 256, 64, 64);
    let mut frame = 0u32;
    let settle = |lod: &mut LodSystem, host: &mut Host, frame: &mut u32, cam: &LodCamera| {
        for _ in 0..600 {
            *frame += 1;
            lod.update(cam, f64::from(*frame) * 16.0, host);
            if host.pending.is_empty() && lod.active {
                return;
            }
            finish(lod, host, &mut zero, 1.0);
        }
    };
    let level_at = |lod: &LodSystem, p: Vec3| -> i32 {
        leaves(lod.last_selection(), false)
            .into_iter()
            .find(|&c| contains(c, p))
            .map_or(i32::MAX, |c| c.level)
    };
    let mut random = rng(5);
    settle(
        &mut lod,
        &mut host,
        &mut frame,
        &camera([0.0, 40.0, 0.0], 0.0, -10.0),
    );
    for yaw in [90.0, 180.0, 270.0] {
        let cam = camera([0.0, 40.0, 0.0], yaw, -10.0);
        frame += 1;
        lod.update(&cam, f64::from(frame) * 16.0, &mut host); // nothing new has loaded yet
        let points: Vec<Vec3> = (0..300)
            .filter_map(|_| point_in_view(&cam, &mut random, 50_000.0))
            .collect();
        let before: Vec<i32> = points.iter().map(|&p| level_at(&lod, p)).collect();
        settle(&mut lod, &mut host, &mut frame, &cam);
        for (n, &p) in points.iter().enumerate() {
            assert!(before[n] <= level_at(&lod, p));
        }
    }
}

#[test]
fn draws_water_at_every_level_as_see_through_at_the_chunks_water_height() {
    // Regression (playtest: near and coarse water did not join): every level draws see-through
    // water with its surface 1/8 m lower in world units, as the chunks do.
    let mut host = Host::new();
    let mut lod = system(4.0, 64, 64, 64);
    for altitude in [100.0, 1e6] {
        for i in 0..200 {
            lod.update(
                &camera([0.0, altitude, 0.0], 0.0, -90.0),
                f64::from(i) * 16.0,
                &mut host,
            );
            if host.pending.is_empty() && lod.active {
                break;
            }
            finish(&mut lod, &mut host, &mut zero, 1.0);
        }
    }
    assert!(!host.meshes.is_empty());
    // water_drop is in cells: 1/8 m over the section's cell size, a power of two.
    let mut sizes = Vec::new();
    for m in &host.meshes {
        let size = 0.125 / m.water_drop;
        assert_eq!(size.log2().fract(), 0.0, "{size}");
        sizes.push(size);
    }
    assert!(sizes.iter().copied().fold(f64::INFINITY, f64::min) <= 2.0); // near levels...
    assert!(sizes.iter().copied().fold(0.0, f64::max) >= 256.0); // ...and coarse ones
}

#[test]
fn hands_the_mesher_each_columns_water_level_in_cells_as_its_surface_height() {
    // Regression (playtest: distant rivers drawn above their banks): the mesher draws water at
    // the surface data's water level, which must reach it in cells from the section's bottom like
    // the ground's height. A river: floor 118 m, water 121 m, in every column of every section.
    let mut host = Host::new();
    host.river = true;
    let mut lod = system(4.0, 64, 64, 64);
    for i in 0..200 {
        lod.update(
            &camera([0.0, 300.0, 0.0], 0.0, -30.0),
            f64::from(i) * 16.0,
            &mut host,
        );
        if host.pending.is_empty() && lod.active {
            break;
        }
        finish(&mut lod, &mut host, &mut zero, 1.0);
    }
    let surfaces: Vec<&MeshRecord> = host
        .meshes
        .iter()
        .filter(|m| m.surface_head.is_some())
        .collect();
    assert!(!surfaces.is_empty());
    for m in surfaces {
        let size = 0.125 / m.water_drop; // the section's cell size (m)
        let s = m.surface_head.expect("surface");
        // 3 m of water over the floor, in cells.
        let water = (f64::from(s[3]) - f64::from(s[0])) * size;
        assert!((water - 3.0).abs() < 5e-4, "{water}");
    }
}

/// Runs `cam` for `ms` of 50 ms frames from `now`.
fn run_for(lod: &mut LodSystem, host: &mut Host, now: &mut f64, cam: &LodCamera, ms: f64) {
    let end = *now + ms;
    while *now < end {
        lod.update(cam, *now, host);
        finish(lod, host, &mut zero, 1.0);
        *now += 50.0;
    }
}

#[test]
fn keeps_the_sections_the_view_uses_within_the_cache_budget_coarser_rather_than_over_it() {
    // Regression (playtest: the frame rate fell for minutes and memory kept growing): on a large or
    // 2× screen the sections the view used filled several times the cache budget.
    let mut host = Host::new();
    host.mesh_bytes = 64 * 1024;
    let budget = 80 * 1_048_576;
    let mut lod = system(4.0, 80, 64, 64);
    let mut now = 0.0;
    let low = camera([0.0, 40.0, 0.0], 0.0, -10.0);
    run_for(&mut lod, &mut host, &mut now, &low, 180_000.0);
    let stats = lod.debug_stats();
    assert!(stats.cache_bytes <= budget, "{}", stats.cache_bytes);
    assert!(stats.error_scale > 1.0);
    // Not needlessly coarse.
    assert!(stats.in_use_bytes as f64 > budget as f64 * 0.4);
    assert!(!lod.last_selection().drawn.is_empty());
    // Settled: neither coarser nor finer from here.
    run_for(&mut lod, &mut host, &mut now, &low, 60_000.0);
    assert_eq!(lod.debug_stats().error_scale, stats.error_scale);

    // Where the view needs less, it grows finer again.
    let high = camera([0.0, 20_000.0, 0.0], 0.0, -90.0);
    run_for(&mut lod, &mut host, &mut now, &high, 120_000.0);
    assert!(lod.debug_stats().error_scale < stats.error_scale);
    assert!(lod.debug_stats().cache_bytes <= budget);
}

#[test]
fn takes_the_pixel_error_and_memory_from_the_settings_re_fitting_the_view_at_once() {
    let mut host = Host::new();
    host.mesh_bytes = 64 * 1024;
    let small = 80 * 1_048_576;
    let mut lod = system(4.0, 80, 64, 64);
    let mut now = 0.0;
    let cam = camera([0.0, 40.0, 0.0], 0.0, -10.0);
    run_for(&mut lod, &mut host, &mut now, &cam, 60_000.0);
    assert!(lod.debug_stats().error_scale > 1.0);
    let sections = lod.last_selection().drawn.len();

    // More memory: the view is no longer coarsened, and holds more than the old budget.
    lod.set_quality(4.0, 1024 * 1_048_576);
    run_for(&mut lod, &mut host, &mut now, &cam, 60_000.0);
    assert_eq!(lod.debug_stats().error_scale, 1.0);
    assert!(lod.debug_stats().cache_bytes > small);
    assert!(lod.last_selection().drawn.len() > sections);

    // A finer pixel error refines further; a coarser one draws fewer sections.
    let at4 = lod.last_selection().drawn.len();
    lod.set_quality(8.0, 1024 * 1_048_576);
    run_for(&mut lod, &mut host, &mut now, &cam, 20_000.0);
    assert!(lod.last_selection().drawn.len() < at4);
}

#[test]
fn refines_by_screen_space_error_coarser_with_distance_and_altitude() {
    let mut host = Host::new();
    let mut lod = system(2.0, 256, 64, 64);
    let mut finest = |cam: LodCamera| -> i32 {
        for i in 0..400 {
            lod.update(&cam, f64::from(i) * 16.0, &mut host);
            if host.pending.is_empty() && lod.active {
                break;
            }
            finish(&mut lod, &mut host, &mut zero, 1.0);
        }
        lod.last_selection()
            .drawn
            .iter()
            .map(|c| c.level)
            .min()
            .unwrap_or(i32::MAX)
    };
    let low = finest(camera([0.0, 10.0, 0.0], 0.0, -30.0));
    let high = finest(camera([0.0, 1e6, 0.0], 0.0, -90.0));
    assert_eq!(low, 1);
    assert!(high >= 10, "{high}");
}

#[test]
fn keeps_each_columns_bounds_apart() {
    // The TS keyed a column's bounds by lodId(level, i, 0, k), which is −1 (row 0 is below the
    // world's first row) at most levels since the octree counts y from its root's corner: all
    // columns of those levels shared the first bounds fetched. Each column's key is its own here.
    for level in [1, 5, 8, 10, 11, MAX_LEVEL] {
        let c = section_at(level, [0.0, 0.0, 0.0]);
        let key = bounds_key(level, c.i, c.k);
        assert!(key >= 0, "level {level}");
        if level < MAX_LEVEL {
            assert_ne!(key, bounds_key(level, c.i + 1, c.k), "level {level}");
            assert_ne!(key, bounds_key(level, c.i, c.k + 1), "level {level}");
        }
    }
}

// --- LOD of the other face (§6.6) ----------------------------------------------------------------

fn settle_bifacial(position: Vec3, record_cells: bool) -> (LodSystem, Host) {
    let mut host = Host::new();
    host.bifacial = true;
    host.record_cells = record_cells;
    let mut lod = system(4.0, 256, 16, 8);
    let cam = camera(position, 0.0, -10.0);
    for frame in 1..=1500 {
        lod.update(&cam, f64::from(frame) * 50.0, &mut host);
        finish(&mut lod, &mut host, &mut zero, 1.0);
    }
    (lod, host)
}

fn other_face(c: LodCoord, viewer_on_a: bool) -> bool {
    c.level <= MAX_ALIGNED_LEVEL && is_face_b_section(c) == viewer_on_a
}

fn distance_to(p: Vec3, c: LodCoord) -> f64 {
    let (lo, hi) = section_box(c);
    box_distance(p, lo, hi)
}

fn far_face_not_generated_or_drawn(viewer_on_a: bool) {
    // Regression (phone playtest, 2026-10-07): the traversal ignored which face the camera was on,
    // so the other face's sections (kilometres away through the core) were generated and drawn.
    let y = if viewer_on_a {
        100.0
    } else {
        2.0 * f64::from(MIDPLANE_Y) - 100.0
    };
    let at = [0.5, y, 0.5];
    let (lod, host) = settle_bifacial(at, false);
    let far =
        |c: &&LodCoord| other_face(**c, viewer_on_a) && distance_to(at, **c) > OTHER_FACE_NEAR_M;
    assert!(!host.generated.is_empty());
    assert_eq!(host.generated.iter().filter(far).count(), 0);
    assert_eq!(lod.last_selection().drawn.iter().filter(far).count(), 0);
}

#[test]
fn does_not_generate_or_draw_the_far_faces_sections_camera_on_face_a() {
    far_face_not_generated_or_drawn(true);
}

#[test]
fn does_not_generate_or_draw_the_far_faces_sections_camera_on_face_b() {
    far_face_not_generated_or_drawn(false);
}

#[test]
fn culls_nothing_near_the_rim_where_the_other_face_shows_past_the_edge() {
    let at = [f64::from(dwell_world::WORLD_RADIUS) - 1000.0, 100.0, 0.5];
    let (_, host) = settle_bifacial(at, false);
    assert!(
        host.generated
            .iter()
            .any(|&c| other_face(c, true) && distance_to(at, c) > OTHER_FACE_NEAR_M)
    );
}

#[test]
fn meshes_level_7_and_up_sections_for_the_viewers_face_the_other_faces_rows_are_stone() {
    // Regression: such sections hold rows of both faces and were meshed with face A's surfaces
    // only. Seen from face B they are meshed without surfaces (face A's would be wrong there).
    let at = [0.5, 2.0 * f64::from(MIDPLANE_Y) - 100.0, 0.5];
    let (_, host) = settle_bifacial(at, true);
    let both: Vec<&MeshRecord> = host
        .meshes
        .iter()
        .filter(|m| m.surface_head.is_none())
        .collect();
    assert!(!both.is_empty());
    for m in both {
        // Face A's ground (material 3) is gone, face B's stone is still there.
        assert!(!m.has_face_a);
        assert!(m.has_stone);
    }
}

// --- the thin crust (Phase 16) -----------------------------------------------------------------

/// Signed area (cells²) of a mesh's faces whose normals point up (+) and down (−).
fn up_down_area(m: &dwell_mesh::FlatMesh) -> (f64, f64) {
    let (mut up, mut down) = (0.0, 0.0);
    let p = |i: u32| {
        let i = i as usize * 3;
        [
            f64::from(m.positions[i]),
            f64::from(m.positions[i + 1]),
            f64::from(m.positions[i + 2]),
        ]
    };
    for t in m.indices.chunks_exact(3) {
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        // The y component of the normal, its length twice the triangle's horizontal area.
        let ny = e1[2] * e2[0] - e1[0] * e2[2];
        if ny > 1e-9 {
            up += ny / 2.0;
        } else if ny < -1e-9 {
            down -= ny / 2.0;
        }
    }
    (up, down)
}

/// Columns of a section with any ground: what the viewer's face should cover, once.
fn ground_columns(c: LodCoord) -> f64 {
    let cells = bifacial_section(c).cells;
    let mut n = 0;
    for z in 0..SECTION_CELLS {
        for x in 0..SECTION_CELLS {
            n += i32::from((0..SECTION_CELLS).any(|y| cells[cell_index(x, y, z)] != 0));
        }
    }
    f64::from(n)
}

/// Meshes a section of the 20-layer crust as the system does for a viewer on `view_face`.
fn crust_mesh(c: LodCoord, view_face: i32) -> dwell_mesh::SectionMeshes {
    let mut cells = bifacial_section(c).cells;
    cells_for_viewer(c, &mut cells, view_face, false);
    dwell_mesh::mesh_section(
        &cells,
        &dwell_mesh::MeshSectionOptions {
            mirror: is_face_b_section(c),
            ..Default::default()
        },
    )
    .expect("section volume")
}

#[test]
fn a_thin_crust_meshes_the_viewers_face_whole_and_none_of_the_other_at_every_level() {
    // ADR 0025's risk: with a crust of 20 layers, a cell from level 4 up is thicker than a face's
    // crust. Every level's sections by the midplane, seen from each face: the viewer's face covers
    // every column with ground exactly once (no holes), and no face of the other side faces the
    // viewer (none of it shows through). Levels up to 11 lie wholly on one face; the root holds
    // both and is meshed for the viewer's face.
    for level in 1..=MAX_LEVEL {
        let above = section_at(level, [0.5, 0.5, 0.5]);
        let below = section_at(level, [0.5, -0.5, 0.5]);
        for view_face in [1, -1] {
            for c in [above, below] {
                let (up, down) = up_down_area(&crust_mesh(c, view_face).opaque);
                let ctx = format!("level {level} {c:?} viewer {view_face}: up {up} down {down}");
                let columns = ground_columns(c);
                assert!(columns > 0.0, "{ctx}");
                if level == MAX_LEVEL {
                    // Both faces in one section: only the viewer's, whole.
                    let (mine, other) = if view_face == 1 { (up, down) } else { (down, up) };
                    assert_eq!((mine, other), (columns, 0.0), "{ctx}");
                } else if is_face_b_section(c) {
                    // Face B's own section: its ground faces −y (down, away from face A).
                    assert_eq!((up, down), (0.0, columns), "{ctx}");
                } else {
                    assert_eq!((up, down), (columns, 0.0), "{ctx}");
                }
            }
        }
    }
}

// --- LOD requests (§6.6) -------------------------------------------------------------------------

struct Requests {
    lod: LodSystem,
    host: Host,
    now: f64,
}

impl Requests {
    fn new() -> Self {
        Self {
            lod: system(4.0, 256, 64, 64),
            host: Host::new(),
            now: 0.0,
        }
    }
    fn run(&mut self, frames: usize) {
        let cam = camera([100.0, 30.0, 100.0], 0.0, -20.0);
        for _ in 0..frames {
            self.now += 16.0;
            self.lod.update(&cam, self.now, &mut self.host);
            finish(&mut self.lod, &mut self.host, &mut zero, 1.0);
        }
    }
}

#[test]
fn asks_nothing_while_the_index_has_no_modification_in_view() {
    let mut r = Requests::new();
    let index = Message::LodIndex(dwell_protocol::LodIndex {
        last: true,
        entries: vec![],
    });
    r.lod.on_message(&index, 10, 0.0);
    r.run(200);
    assert!(r.lod.active);
    assert!(r.host.requests.is_empty());
}

/// A server with one modified path (the sections above a build at `site`, levels 1–19, all at
/// `revision`): Explicit on it (Unchanged for the held revision), Generated elsewhere.
struct Server {
    site: Vec3,
    path: HashSet<LodId>,
    revision: u32,
    asked: Vec<LodSectionRequest>,
}

impl Server {
    fn new(site: Vec3) -> Self {
        Self {
            site,
            path: (1..=MAX_LEVEL)
                .map(|level| id_of(section_at(level, site)))
                .collect(),
            revision: 5,
            asked: Vec::new(),
        }
    }
    fn on_path(&self, r: &LodSectionRequest) -> bool {
        self.path.contains(&request_id(r))
    }
    fn answer(&mut self, r: &mut Requests) {
        let requests: Vec<LodSectionRequest> = r.host.requests.drain(..).flatten().collect();
        for q in requests {
            self.asked.push(q);
            let on_path = self.on_path(&q);
            let form = if !on_path {
                LodForm::Generated
            } else if q.known_revision == self.revision {
                LodForm::Unchanged
            } else {
                LodForm::Explicit
            };
            let c = request_coord(&q);
            let m = Message::LodData(LodData {
                form,
                level: q.level,
                section: q.section,
                revision: if on_path { self.revision } else { 0 },
                cells: if form == LodForm::Explicit {
                    flat_section(c).cells
                } else {
                    Vec::new()
                },
            });
            r.lod.on_message(&m, 100, r.now);
        }
    }
    fn index(&mut self, r: &mut Requests, revision: u32) {
        self.revision = revision;
        let e = section_at(8, self.site);
        let m = Message::LodIndexUpdate(vec![LodIndexEntry {
            i: e.i,
            j: e.j,
            k: e.k,
            revision,
        }]);
        r.lod.on_message(&m, 15, 0.0);
    }
    fn exchange(&mut self, r: &mut Requests, rounds: usize) {
        for _ in 0..rounds {
            r.run(1);
            self.answer(r);
        }
    }
}

fn request_coord(r: &LodSectionRequest) -> LodCoord {
    LodCoord::new(i32::from(r.level), r.section[0], r.section[1], r.section[2])
}

fn request_id(r: &LodSectionRequest) -> LodId {
    id_of(request_coord(r))
}

#[test]
fn asks_for_sections_under_an_index_entry_top_down_and_trusts_generated_for_subtrees() {
    let mut r = Requests::new();
    let site = [100.0, 0.0, 100.0];
    let mut s = Server::new(site);
    s.index(&mut r, 5);
    s.exchange(&mut r, 60);
    assert!(r.lod.active);
    // Every section on the modified path was asked about (down to the finest level in view)...
    let asked: HashSet<LodId> = s.asked.iter().map(request_id).collect();
    for level in 8..=MAX_LEVEL {
        assert!(asked.contains(&id_of(section_at(level, site))), "{level}");
    }
    // ...and nothing else but their children: a Generated answer covered the rest of a subtree.
    for q in &s.asked {
        let on_path = s.on_path(q);
        let parent = ancestor(request_coord(q), i32::from(q.level) + 1);
        assert!(on_path || i32::from(q.level) == MAX_LEVEL || s.path.contains(&id_of(parent)));
        if q.level > 8 {
            assert!(on_path); // above the index level: from the index alone
        }
    }
    // Held sections are asked once each.
    assert_eq!(asked.len(), s.asked.len());
}

#[test]
fn asks_again_with_the_revision_it_holds_when_an_index_entry_changes_unchanged_stays() {
    let mut r = Requests::new();
    let site = [100.0, 0.0, 100.0];
    let mut s = Server::new(site);
    s.index(&mut r, 5);
    s.exchange(&mut r, 60);
    s.asked.clear();
    s.index(&mut r, 9);
    s.exchange(&mut r, 60);
    // The entry and its ancestors, with the revision held; then the path below, top down.
    let above: Vec<&LodSectionRequest> = s.asked.iter().filter(|q| q.level >= 8).collect();
    assert_eq!(above.len(), (MAX_LEVEL + 1 - 8) as usize);
    for q in above {
        assert_eq!(q.known_revision, 5);
    }
    for q in &s.asked {
        let parent = ancestor(request_coord(q), i32::from(q.level) + 1);
        assert!(s.on_path(q) || s.path.contains(&id_of(parent)));
    }
    // A third index update at the same revision changes nothing.
    s.asked.clear();
    s.index(&mut r, 9);
    r.run(10);
    s.answer(&mut r);
    assert!(s.asked.is_empty());
    assert!(in_world(section_at(8, site)));
}

// --- LOD chunk requests (§6.6) -------------------------------------------------------------------

fn chunk_of(p: Vec3) -> ChunkCoord {
    cc(
        (p[0] / 32.0).floor() as i32,
        (p[1] / 32.0).floor() as i32,
        (p[2] / 32.0).floor() as i32,
    )
}

/// A LOD system whose view streams chunks within 3 of `body`; requests arrive a frame later.
struct ChunkSetup {
    lod: LodSystem,
    host: Host,
    now: f64,
}

impl ChunkSetup {
    fn new(body: Vec3, pixel_error: f64) -> Self {
        let at = chunk_of(body);
        let mut options = LodOptions::new(pixel_error, 64 * 1_048_576, 16, 8);
        options.request_chunks = true;
        Self {
            lod: LodSystem::new(options),
            host: Host::with_chunks(move |c| chebyshev(c, at) <= 3),
            now: 0.0,
        }
    }
    fn run(&mut self, cam: &LodCamera, frames: usize) {
        let mut random = rng(3);
        for _ in 0..frames {
            let arrived: Vec<ChunkCoord> = self.host.arriving.drain(..).collect();
            self.host.held.extend(arrived);
            self.now += 16.0;
            self.lod.update(cam, self.now, &mut self.host);
            finish(&mut self.lod, &mut self.host, &mut random, 1.0);
        }
    }
    fn requested(&self) -> Vec<ChunkCoord> {
        self.host
            .chunk_requests
            .iter()
            .flatten()
            .map(|&c| ChunkCoord::from_array(c))
            .collect()
    }
}

#[test]
fn flying_out_through_the_build_sphere_asks_for_nothing_outside_it() {
    // Phase 16's exit criterion (ADR 0025 D3): the controller flies out of the sphere — flight is not
    // bounded by it — while the LOD follows the eye, generating sections and asking the server for
    // chunks: none of either is wholly outside the build volume. Runs natively and under WASM.
    use dwell_physics::config::PlayerControllerConfig;
    use dwell_physics::controller::{Input, Players};
    use dwell_physics::{DVec3, Physics};
    use dwell_protocol::players::FLY_SPEED_MAX_LEVEL;
    use dwell_world::{BUILD_VOLUME, Overlap, VoxelWorld, empty_generator};

    let mut world = VoxelWorld::new(empty_generator(), Some(std::sync::Arc::new(|_| true)));
    let mut physics = Physics::default();
    let mut players = Players::new(&mut physics, true);
    let config = PlayerControllerConfig::default();
    // Over the rim, low: up and out, past the sphere's surface ~9 km above (and the rim beside).
    let start = DVec3::new(49_000.0, 100.0, 0.5);
    let h = players.spawn(&mut physics, config, start, 90.0);
    let mut setup = ChunkSetup::new([start.x, start.y + 1.6, start.z], 4.0);
    setup.host.bifacial = true;
    let (mut outside_ticks, mut highest) = (0, 0.0f64);
    for tick in 0..240 {
        players.set_input(
            h,
            Input {
                fly: true,
                fly_speed: FLY_SPEED_MAX_LEVEL as u8,
                jump: true,
                move_y: 1.0,
                look_yaw: 90.0,
                ..Input::default()
            },
        );
        players.tick(&mut world, &mut physics);
        physics.step(1.0 / 60.0);
        let p = players.position(&physics, h);
        highest = highest.max(p.y);
        let cell = [p.x.floor() as i64, p.y.floor() as i64, p.z.floor() as i64];
        if !BUILD_VOLUME.contains_voxel(cell[0], cell[1], cell[2]) {
            outside_ticks += 1;
        }
        if tick % 4 == 0 {
            let cam = camera([p.x, p.y + 1.6, p.z], 90.0, -30.0);
            setup.run(&cam, 1);
        }
    }
    assert!(outside_ticks > 60, "{outside_ticks} ticks outside, highest {highest}");
    assert!(highest > 10_000.0, "{highest}");
    let generated = &setup.host.generated;
    assert!(!generated.is_empty());
    for c in generated {
        let o = section_origin(*c);
        let s = section_size(c.level);
        assert_ne!(
            BUILD_VOLUME.box_overlap([o.x, o.y, o.z], [o.x + s, o.y + s, o.z + s]),
            Overlap::Outside,
            "{c:?}"
        );
    }
    for c in setup.requested() {
        assert!(BUILD_VOLUME.chunk_may_hold(c), "{c:?}");
    }
}

#[test]
fn asks_for_the_chunks_beyond_the_view_nearest_first_within_the_render_radius_once_each() {
    let body = [0.5, 1.6, 0.5];
    let mut t = ChunkSetup::new(body, 4.0);
    t.run(&camera(body, 0.0, -10.0), 400);
    let all = t.requested();
    let at = chunk_of(body);
    let beyond: Vec<ChunkCoord> = all
        .iter()
        .copied()
        .filter(|&c| chebyshev(c, at) > 3)
        .collect();
    assert!(beyond.len() > 20, "{}", beyond.len());
    // Within the render radius (less a margin), and never asked twice.
    let r = CHUNK_REQUEST_RADIUS;
    let d2 = |c: ChunkCoord| {
        let (x, y, z) = (c.x - at.x, c.y - at.y, c.z - at.z);
        x * x + y * y + z * z
    };
    for &c in &all {
        assert!(d2(c) <= r * r + r);
    }
    assert_eq!(all.iter().collect::<HashSet<_>>().len(), all.len());
    // Each batch nearest first.
    for batch in &t.host.chunk_requests {
        let d: Vec<f64> = batch
            .iter()
            .map(|&c| f64::from(d2(ChunkCoord::from_array(c))).sqrt())
            .collect();
        for i in 1..d.len() {
            assert!(d[i] >= d[i - 1] - 1.8);
        }
    }
    // Once they arrive they are drawn, without the view's grace period.
    assert!(beyond.iter().any(|&c| t.lod.chunk_visible(c)));
    assert!(!t.lod.chunk_visible(cc(30, -1, 0))); // beyond the radius: never full detail
}

#[test]
fn never_asks_for_buried_chunks_underground_only_the_streamed_view_is_drawn() {
    // Regression (playtest: RTT and frame time climbing while standing still): drawing caves in
    // buried sections also asked the server for every solid-rock chunk within the full-detail
    // distance. In the flat world everything below its surface is buried; at the surface, only
    // chunks near it are asked. The surface is 1 km up, so the rock below it is face A's (Phase 16:
    // the midplane is at 0).
    let ground = 1024.0;
    let surface = [0.5, ground + 1.6, 0.5];
    let mut top = ChunkSetup::new(surface, 4.0);
    top.host.flat_top = ground as i64;
    top.run(&camera(surface, 0.0, -10.0), 300);
    assert!(top.requested().len() > 20);
    for c in top.requested() {
        assert!(c.y >= 32 - 2, "{c:?}");
    }

    let deep = [16.5, ground - 12.0 * 32.0 + 16.0, 16.5];
    let mut below = ChunkSetup::new(deep, 4.0);
    below.host.flat_top = ground as i64;
    below.run(&camera(deep, 30.0, 0.0), 300);
    assert!(below.requested().is_empty());
    // The streamed chunks around the camera are still drawn (caves stay visible).
    assert!(below.lod.chunk_visible(chunk_of(deep)));
}

#[test]
fn draws_full_detail_out_to_the_chosen_distance_whatever_the_pixel_error_and_not_beyond() {
    let body = [0.5, 1.6, 0.5];
    // A pixel error so coarse it alone would never ask for chunks.
    let mut t = ChunkSetup::new(body, 10_000.0);
    t.lod.set_detail_distance(Some(200.0));
    t.run(&camera(body, 0.0, -10.0), 400);
    let all = t.requested();
    let far = |c: ChunkCoord| f64::from(c.x * 32 + 16).hypot(f64::from(c.z * 32 + 16));
    assert!(all.iter().any(|&c| far(c) > 150.0));
    // Level-1 sections within 200 m (their nearest point): nothing requested past ~200 m + 64 m.
    for &c in &all {
        assert!(far(c) < 200.0 + 64.0 + 32.0);
    }
    assert!(t.lod.chunk_visible(cc(5, -1, 0))); // ~176 m: full detail
    assert!(!t.lod.chunk_visible(cc(9, -1, 0))); // ~300 m: level 1
}

// --- the overlay ---------------------------------------------------------------------------------

#[test]
fn formats_the_overlay_lines() {
    let mut s = LodStats {
        drawn: vec![0; 20],
        chunk_sections: 3,
        nodes: 40,
        generating: 1,
        meshing: 2,
        asking: 0,
        cache_bytes: 3 * 1_048_576,
        in_use_bytes: 1_048_576,
        error_scale: 1.15,
        bytes_per_second: 2048,
    };
    assert_eq!(
        format_lod_stats(&s),
        "LOD — · chunks×3 · nodes 40\nLOD jobs gen 1 mesh 2 ask 0 · cache 3.0 MB (view 1.0) · error ×1.15 · 2.0 KB/s"
    );
    s.drawn[1] = 4;
    s.drawn[9] = 2;
    assert!(format_lod_stats(&s).starts_with("LOD L1:4 L9:2 · "));
}

#[test]
fn ignores_results_whose_token_is_no_longer_current() {
    let mut host = Host::new();
    let mut lod = system(4.0, 64, 64, 64);
    let cam = camera([0.0, 40.0, 0.0], 0.0, -10.0);
    lod.update(&cam, 0.0, &mut host);
    finish(&mut lod, &mut host, &mut zero, 1.0); // the root's bounds
    lod.update(&cam, 16.0, &mut host);
    // The root's generation job: answering it with an unknown token changes nothing.
    assert!(matches!(host.pending.as_slice(), [Job::Generate(_, _)]));
    lod.on_generated(9999, Some(flat_section(ROOT)));
    lod.on_meshed(9999, SectionMeshes::default(), &mut host);
    assert_eq!(lod.generating, 1);
    finish(&mut lod, &mut host, &mut zero, 1.0);
    assert_eq!(lod.generating, 0);
    // A second answer with the same token is stale too.
    lod.on_generated(1, Some(flat_section(ROOT)));
    assert_eq!(lod.generating, 0);
}

#[test]
fn rebuild_all_meshes_every_drawn_section_again() {
    // A renderer switch rebuilds the GPU data from the CPU data, without a reload: every drawn
    // section gets a new payload (regenerated where its cells were dropped after meshing).
    let (mut lod, mut host) = settle_bifacial([0.5, 100.0, 0.5], false);
    let drawn: Vec<LodId> = host
        .shown
        .iter()
        .map(|&(id, _)| id)
        .filter(|id| host.meshed.contains(id))
        .collect();
    assert!(!drawn.is_empty());
    let generated = host.generated.len();
    host.set.clear();
    lod.rebuild_all();
    let cam = camera([0.5, 100.0, 0.5], 0.0, -10.0);
    for frame in 1501..=2500 {
        lod.update(&cam, f64::from(frame) * 50.0, &mut host);
        finish(&mut lod, &mut host, &mut zero, 1.0);
    }
    let again: HashSet<LodId> = host.set.iter().copied().collect();
    // Sections evicted meanwhile (the cache budget) are gone, not stale.
    let missing = drawn
        .iter()
        .filter(|id| lod.nodes.contains_key(id) && !again.contains(id))
        .count();
    assert_eq!(missing, 0, "of {} drawn sections", drawn.len());
    assert!(
        host.generated.len() > generated,
        "dropped cells are regenerated"
    );
}
