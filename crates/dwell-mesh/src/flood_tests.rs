//! The flood light in the meshes (docs/DAY_NIGHT.md §11): a carved tunnel, a tall cave and a long
//! tunnel under a hill go dark inside, ambient and direct alike; an emitter glows; both faces.
//! The owner's bug (2026-10-21): "I carved a tunnel in 0.22.5 and it was lit up like day".

use crate::{MeshArrays, mesh_chunk_lit};
use dwell_look::ambient::{AMBIENT, Shading, shading};
use dwell_world::{
    CELL_H, CHUNK_SIZE, ChunkCoord, FaceFrame, MIDPLANE_Y, SKY_APRON, VoxelWorld, empty_generator,
};

const N: i32 = CHUNK_SIZE;
const ROW: i32 = MIDPLANE_Y.div_euclid(N) + 2;
const BASE: i32 = ROW * N;
const GROUND: i32 = BASE + 8;
const UP: [f64; 3] = [0.0, 1.0, 0.0];

fn rock() -> u16 {
    dwell_blocks::require("dwell:sandbox_floor")
}

/// Face-local cells on face `f`, held by a lit world (as the client's streamer holds them).
struct Fixture {
    f: FaceFrame,
    src: VoxelWorld,
    lit: VoxelWorld,
}

impl Fixture {
    fn new(f: FaceFrame) -> Self {
        let mut lit = VoxelWorld::streamed();
        lit.enable_light();
        Self {
            f,
            src: VoxelWorld::new(empty_generator(), None),
            lit,
        }
    }

    fn fill(&mut self, lo: [i32; 3], hi: [i32; 3], id: u16) {
        for y in lo[1]..hi[1] {
            for z in lo[2]..hi[2] {
                for x in lo[0]..hi[0] {
                    let [wx, wy, wz] = self.f.cell([x, y, z]);
                    self.src.set_voxel(wx, wy, wz, id);
                }
            }
        }
    }

    /// Holds face-local chunks x in `xs`, rows ROW + `dys`, z in `zs`.
    fn hold(&mut self, xs: std::ops::RangeInclusive<i32>, dys: std::ops::RangeInclusive<i32>) {
        for dy in dys {
            for z in -1..=1 {
                for x in xs.clone() {
                    let c = self.f.chunk(ChunkCoord::new(x, ROW + dy, z));
                    if let Some(chunk) = self.src.find(c) {
                        self.lit.put(c, chunk.clone());
                    }
                    self.lit.light_load(c);
                }
            }
        }
    }

    /// The opaque mesh of face-local chunk (cx, ROW, 0) with its flood light, and its origin (m).
    fn mesh(&mut self, cx: i32) -> (MeshArrays, [f64; 3]) {
        let (m, origin) = self.meshes(cx);
        (m.opaque, origin)
    }

    /// The transparent (water) mesh of face-local chunk (cx, ROW, 0), as the water shader lights
    /// it: no baked light (1), its flood light.
    fn water(&mut self, cx: i32) -> (MeshArrays, [f64; 3]) {
        let (m, origin) = self.meshes(cx);
        let mut w = m.transparent;
        let count = w.positions.len() / 3;
        assert!(count > 0, "no water drawn");
        assert_eq!(w.flood.len(), 2 * count, "the water's flood light stream");
        w.light = vec![1.0; count];
        (w, origin)
    }

    fn meshes(&mut self, cx: i32) -> (crate::ChunkMeshes, [f64; 3]) {
        let c = self.f.chunk(ChunkCoord::new(cx, ROW, 0));
        let block = self.src.padded_box(c, SKY_APRON);
        let light = self.lit.padded_light(c);
        let m = mesh_chunk_lit(&block, &light, self.f.is_b(), [c.x * N, c.y * N, c.z * N])
            .expect("block");
        let origin = [
            f64::from(c.x * N),
            f64::from(c.y * N) * CELL_H,
            f64::from(c.z * N),
        ];
        (m, origin)
    }

    /// [`Self::shade`] of the surface facing `n` within a cell below `p` (a fluid's top sits under
    /// its cell's top).
    fn shade_near(&self, m: &(MeshArrays, [f64; 3]), p: [f64; 3], n: [f64; 3]) -> Shading {
        let (mesh, origin) = m;
        let w = self.f.point([p[0], p[1] * CELL_H, p[2]]);
        let wn = self.f.vector(n);
        let axis = (0..3)
            .max_by(|&a, &b| wn[a].abs().total_cmp(&wn[b].abs()))
            .expect("axis");
        let plane = (0..mesh.positions.len() / 3)
            .filter(|&i| (0..3).all(|a| (f64::from(mesh.normals[i * 3 + a]) - wn[a]).abs() < 1e-4))
            .map(|i| f64::from(mesh.positions[i * 3 + axis]) + origin[axis])
            .filter(|&c| (c - w[axis]).abs() <= CELL_H)
            .min_by(|a, b| (a - w[axis]).abs().total_cmp(&(b - w[axis]).abs()))
            .expect("a surface near the point");
        let mut q = w;
        q[axis] = plane;
        let local = self.f.point(q);
        self.shade(m, [local[0], local[1] / CELL_H, local[2]], n)
    }

    /// The shading of the surface at face-local point `p` (x, z in metres, y in rows) facing
    /// face-local `n`, interpolated over its triangle as the GPU does.
    fn shade(&self, (m, origin): &(MeshArrays, [f64; 3]), p: [f64; 3], n: [f64; 3]) -> Shading {
        let w = self.f.point([p[0], p[1] * CELL_H, p[2]]);
        let wn = self.f.vector(n);
        let pos = |i: u32| -> [f64; 3] {
            std::array::from_fn(|a| f64::from(m.positions[i as usize * 3 + a]) + origin[a])
        };
        let facing = |i: u32| {
            (0..3).all(|a| (f64::from(m.normals[i as usize * 3 + a]) - wn[a]).abs() < 1e-4)
        };
        let sub = |p: [f64; 3], q: [f64; 3]| -> [f64; 3] { std::array::from_fn(|k| p[k] - q[k]) };
        let dot = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
        for t in m.indices.chunks_exact(3) {
            if !facing(t[0]) {
                continue;
            }
            let [a, b, c] = [pos(t[0]), pos(t[1]), pos(t[2])];
            let (e1, e2, d) = (sub(b, a), sub(c, a), sub(w, a));
            if dot(d, wn).abs() > 1e-4 {
                continue;
            }
            let (d11, d12, d22) = (dot(e1, e1), dot(e1, e2), dot(e2, e2));
            let (d1, d2) = (dot(d, e1), dot(d, e2));
            let den = d11 * d22 - d12 * d12;
            let v = (d22 * d1 - d12 * d2) / den;
            let u = (d11 * d2 - d12 * d1) / den;
            if v < -1e-6 || u < -1e-6 || u + v > 1.0 + 1e-6 {
                continue;
            }
            let bary = |f: &dyn Fn(usize) -> f64| {
                (1.0 - u - v) * f(t[0] as usize) + v * f(t[1] as usize) + u * f(t[2] as usize)
            };
            let light = bary(&|i| f64::from(m.light[i]));
            let sky = bary(&|i| f64::from(m.flood[i * 2]));
            let block = bary(&|i| f64::from(m.flood[i * 2 + 1]));
            return shading(light, sky, block);
        }
        panic!("no surface at {p:?} facing {n:?}")
    }
}

/// Ground, a hill from x = 4 on 10 m over it, and a 2 m × 2 m tunnel carved 20 m into it.
fn tunnel(f: FaceFrame) -> Fixture {
    let mut fx = Fixture::new(f);
    fx.fill([-40, BASE, -40], [72, GROUND, 72], rock());
    fx.fill([4, GROUND, -40], [72, GROUND + 20, 72], rock());
    fx.fill([4, GROUND, 15], [24, GROUND + 4, 17], 0);
    fx.hold(-1..=1, -1..=1);
    fx
}

dwell_world::both_faces!(
    /// A carved tunnel's light falls off with depth, ambient and direct: lit at its mouth, about
    /// the floor 10 m in, dark at 15 m; open ground outside keeps the 0.22.5 look (all of both).
    fn a_carved_tunnel_falls_off_to_the_floor(f: FaceFrame) {
        let mut fx = tunnel(f);
        let g = f64::from(GROUND);
        let outside = fx.mesh(-1);
        let open = fx.shade(&outside, [-16.0, g, 8.5], UP);
        assert_eq!((open.ambient, open.direct), (1.0, 1.0), "open ground");
        let m = fx.mesh(0);
        let floor = |x: f64| fx.shade(&m, [x, g, 16.0], UP);
        let (mouth, five, ten, fifteen) = (floor(4.5), floor(9.0), floor(14.0), floor(19.0));
        assert!(mouth.direct > 0.5, "the mouth: {mouth:?}");
        assert!(
            five.direct < mouth.direct && five.ambient < mouth.ambient,
            "5 m: {five:?}"
        );
        assert!(
            ten.direct == 0.0 && ten.ambient < five.ambient,
            "10 m: {ten:?}"
        );
        assert!(
            ten.ambient < AMBIENT.floor + 0.1,
            "about the floor 10 m in: {ten:?}"
        );
        assert!(
            fifteen.direct == 0.0 && fifteen.ambient == AMBIENT.floor,
            "15 m: {fifteen:?}"
        );
        // The walls and ceiling 10 m in, too.
        let wall = fx.shade(&m, [14.0, g + 2.0, 15.0], [0.0, 0.0, 1.0]);
        let roof = fx.shade(&m, [14.0, g + 4.0, 16.0], [0.0, -1.0, 0.0]);
        assert!(
            wall.direct == 0.0 && roof.direct == 0.0,
            "{wall:?} {roof:?}"
        );
    }
);

dwell_world::both_faces!(
    /// A cave roofed 10 m over its floor, past the sky rays' reach (0.22.5 baked its floor as
    /// open sky), is dark.
    fn a_tall_cave_is_dark(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        fx.fill([-40, BASE, -40], [72, GROUND + 24, 72], rock());
        fx.fill([4, GROUND, 4], [28, GROUND + 20, 28], 0);
        fx.hold(-1..=1, -1..=1);
        let m = fx.mesh(0);
        let s = fx.shade(&m, [16.0, f64::from(GROUND), 16.0], UP);
        assert!(s.direct == 0.0 && s.ambient == AMBIENT.floor, "{s:?}");
    }
);

dwell_world::both_faces!(
    /// A tunnel 90 m long under a 30 m hill, far past any mesh job's data, is dark in every chunk
    /// it crosses.
    fn a_long_tunnel_under_a_hill_stays_dark(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        fx.fill([-40, BASE, -40], [136, GROUND, 72], rock());
        fx.fill([4, GROUND, -40], [136, GROUND + 60, 72], rock());
        fx.fill([4, GROUND, 15], [96, GROUND + 4, 17], 0);
        fx.hold(-1..=4, -1..=2);
        for cx in 1..=2 {
            let m = fx.mesh(cx);
            for x in [cx * 32 + 4, cx * 32 + 20] {
                let s = fx.shade(&m, [f64::from(x) + 0.5, f64::from(GROUND), 16.0], UP);
                assert!(
                    s.direct == 0.0 && s.ambient == AMBIENT.floor,
                    "{x} m: {s:?}"
                );
            }
        }
    }
);

dwell_world::both_faces!(
    /// An emitter's block light reaches the tunnel's dark floor as glow.
    fn an_emitter_lights_the_dark(f: FaceFrame) {
        let mut fx = tunnel(f);
        let lamp = crate::flood_tests::lamp();
        let at = f.cell([18, GROUND + 3, 16]);
        fx.src.set_voxel(at[0], at[1], at[2], lamp);
        fx.lit.set_voxel(at[0], at[1], at[2], lamp);
        fx.lit.light_edited(&[at]);
        let m = fx.mesh(0);
        let g = f64::from(GROUND);
        let near = fx.shade(&m, [18.5, g, 16.0], UP);
        let far = fx.shade(&m, [10.5, g, 16.0], UP);
        assert!(near.glow > 0.3 && near.direct == 0.0, "under it: {near:?}");
        assert!(far.glow < near.glow, "farther: {far:?}");
    }
);

fn water_id() -> u16 {
    dwell_blocks::require("dwell:water")
}

dwell_world::both_faces!(
    /// Water takes the flood light as terrain does (owner, 2026-10-21: darkness with Shadows
    /// Off): a pool 14 m into the tunnel is dark, its surface and sides, while a pool in the open
    /// keeps the full sky. Before, the transparent mesh carried no flood light and the water
    /// shader lit every surface as open sky, sun glint included.
    fn a_pool_in_a_cave_is_dark(f: FaceFrame) {
        let mut fx = Fixture::new(f);
        let w = water_id();
        fx.fill([-40, BASE, -40], [72, GROUND, 72], rock());
        fx.fill([4, GROUND, -40], [72, GROUND + 20, 72], rock());
        fx.fill([4, GROUND, 15], [24, GROUND + 4, 17], 0);
        // A pool two rows deep in the tunnel's floor, 14 m in, and one in the open.
        fx.fill([16, GROUND - 2, 15], [20, GROUND, 17], w);
        fx.fill([-20, GROUND - 2, 4], [-12, GROUND, 12], w);
        fx.hold(-1..=1, -1..=1);
        let top = f64::from(GROUND);
        let open = fx.water(-1);
        let s = fx.shade_near(&open, [-16.0, top, 8.0], UP);
        assert_eq!((s.ambient, s.direct), (1.0, 1.0), "the open pool: {s:?}");
        let cave = fx.water(0);
        let s = fx.shade_near(&cave, [18.0, top, 16.0], UP);
        assert!(s.direct == 0.0, "the cave pool takes no sun: {s:?}");
        assert!(
            s.ambient < AMBIENT.floor + 0.05,
            "the cave pool is dark: {s:?}"
        );
    }
);

/// The fixture's emitter (`test:lamp`), registered from its data as a server mod would.
fn lamp() -> u16 {
    static ID: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *ID.get_or_init(|| {
        dwell_blocks::state("test:lamp").unwrap_or_else(|| {
            let mut row = *dwell_blocks::fixture::STATES
                .iter()
                .find(|s| s.name == "test:lamp")
                .expect("the fixture has test:lamp");
            row.shape_index = dwell_blocks::info(rock()).shape_index;
            dwell_blocks::register(
                dwell_blocks::Origin::Server,
                dwell_blocks::NewBlock {
                    name: "test:lamp".into(),
                    properties: Vec::new(),
                    states: vec![row],
                },
            )
            .expect("registers")
        })
    })
}

dwell_world::both_faces!(
    /// Open terrain under the full sky keeps the 0.22.5 look: no up-facing vertex the sky rays see
    /// takes much less than the full sky light (a level off next to an overhang is real). A ramp's corner on its cell's edge once sampled the solid
    /// cell beside it and drew a dark wedge on every terrace (red before `Flood::shaped`).
    fn open_terrain_takes_the_full_sky(f: FaceFrame) {
        use dwell_world::GENERATOR_TERRAIN;
        use dwell_worldgen::{air_test_for, generator_for};
        let seed = 7;
        let air = air_test_for(GENERATOR_TERRAIN, seed);
        let mut src = VoxelWorld::new(generator_for(GENERATOR_TERRAIN, seed), Some(air.clone()));
        let mut lit = VoxelWorld::streamed();
        lit.enable_light();
        let terrain = dwell_worldgen::BifacialTerrain::new(seed);
        let sp = terrain.local(f.face).spawn_point();
        let local = ChunkCoord::new(
            (sp[0] as i32).div_euclid(N),
            ((sp[1] / CELL_H) as i32).div_euclid(N),
            (sp[2] as i32).div_euclid(N),
        );
        let at = |dx: i32, dy: i32, dz: i32| {
            f.chunk(ChunkCoord::new(local.x + dx, local.y + dy, local.z + dz))
        };
        for dy in -2..=2 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let c = at(dx, dy, dz);
                    if !air(c) {
                        let ch = src.read(c).clone();
                        lit.put(c, ch);
                    }
                    lit.light_load(c);
                }
            }
        }
        let (mut checked, mut dark) = (0, 0);
        for dy in -1..=1 {
            let c = at(0, dy, 0);
            let block = src.padded_box(c, SKY_APRON);
            let light = lit.padded_light(c);
            let m = mesh_chunk_lit(&block, &light, f.is_b(), [c.x * N, c.y * N, c.z * N])
                .expect("block");
            for mesh in [&m.opaque, &m.cutout] {
                for i in 0..mesh.positions.len() / 3 {
                    let up = f64::from(mesh.normals[i * 3 + 1]) * f.sign_f64();
                    if up > 0.2 && mesh.light[i] > 0.5 {
                        checked += 1;
                        dark += usize::from(mesh.flood[i * 2] < 0.9);
                    }
                }
            }
        }
        assert!(checked > 1000, "{checked} vertices");
        assert_eq!(dark, 0, "of {checked} open up-facing vertices");
    }
);
