//! A world's size reaching its generators (docs/proposals/BIG_MAP.md M2): a 2× world's maps, discs,
//! build volumes and spawn are the 2× values, on both faces; a 1× world's are the build's.

use dwell_protocol::world_size::WorldSize;
use dwell_world::{Chunk, ChunkCoord, WorldBounds, both_faces};
use dwell_worldgen::BifacialTerrain;

fn two() -> WorldSize {
    WorldSize::named("2x").expect("a named size")
}

both_faces!(
    /// The 2× world's map is at the 2× cell over the 2× disc (today's cell count), and its disc and
    /// volume are the size's; the 1× world's are the build's.
    fn a_2x_world_s_generators_use_the_2x_values(f: FaceFrame) {
        let t = BifacialTerrain::sized(11, two());
        let g = t.local(f.face);
        assert_eq!(g.map().size, two());
        assert_eq!(g.map().cell, 200.0, "the hydrology cell × 2");
        let one = BifacialTerrain::new(11);
        assert_eq!(g.map().n, one.local(f.face).map().n, "today's cell count");
        assert_eq!(g.world(), WorldBounds::of(two()));
        assert_eq!(t.world().radius(), 100_000);
        assert_eq!(one.local(f.face).world(), WorldBounds::DEFAULT);
        assert_eq!(one.local(f.face).map().size, WorldSize::DEFAULT);
    }
);

both_faces!(
    /// 70 km out is past a 1× world's rim (all air: nothing generated) and in a 2× world's sea: its
    /// sea-level chunk holds water on that face.
    fn a_2x_world_generates_past_today_s_rim(f: FaceFrame) {
        let one = BifacialTerrain::new(11);
        let big = BifacialTerrain::sized(11, two());
        // The chunk just under the sea's surface, 70 km out on +x, on face `f`.
        let row = dwell_world::SEA_LEVEL - 1;
        let y = f.row(row);
        let c = ChunkCoord::new(70_000 / 32, y.div_euclid(32), 0);
        let filled = |t: &BifacialTerrain| {
            let mut ch = Chunk::default();
            t.generate(c, &mut ch);
            ch.voxels()
                .iter()
                .filter(|&&v| v != dwell_world::M.air)
                .count()
        };
        assert_eq!(filled(&one), 0, "past a 1× world's rim");
        assert!(filled(&big) > 0, "a 2× world's sea");
        assert!(one.is_air_chunk(c) && !big.is_air_chunk(c));
    }
);

#[test]
fn a_2x_world_s_spawn_is_on_its_own_island() {
    // both-faces: the spawn search is face A's (face B's mirrors it, `spawn_point_b`).
    let big = BifacialTerrain::sized(11, two());
    let s = big.spawn_point();
    let r = s[0].hypot(s[2]);
    assert!(r < 100_000.0, "on the 2× disc: {r} m out");
    let g = big.local(dwell_world::Face::A);
    assert!(
        g.world().inside_disc(s[0] as i64, s[2] as i64),
        "inside the disc"
    );
}

both_faces!(
    /// A 10× world (BUGS.md #91: the owner's loaded with sky only) spawns on its own disc, its feet
    /// in air on solid ground, on each face's generator (face-local).
    fn a_10x_world_s_spawn_stands_on_its_ground(f: FaceFrame) {
        let ten = WorldSize::named("10x").expect("a named size");
        let big = BifacialTerrain::sized(11, ten);
        let g = big.local(f.face);
        // The spawn search's ground (since 0.58 the spawn is the shrine nearest it, standing on its
        // floor, which the height field's `solid_at` doesn't hold: tests/shrines.rs checks that).
        let s = g.spawn_search_point();
        assert!(s.iter().all(|v| v.is_finite()), "a finite spawn: {s:?}");
        assert!(
            g.world().inside_disc(s[0] as i64, s[2] as i64),
            "on the 10× disc: {s:?}"
        );
        let (x, z) = (s[0].floor() as i32, s[2].floor() as i32);
        let row = dwell_world::row_of_m(s[1]);
        assert!(!g.solid_at(x, row, z), "the feet in air: {s:?}");
        assert!(g.solid_at(x, row - 1, z), "on solid ground: {s:?}");
    }
);

/// BUGS.md #108 (owner: "Sizes 8x and 10x for the map spawn you in the void where there's no
/// terrain only collision"): the LOD sections holding a big world's spawn ground, from level 1 (its
/// chunks') up, are generated as content, not as empty space outside a 1× world's 50 km disc.
fn a_big_world_s_spawn_sections_are_content(f: dwell_world::FaceFrame, name: &str) {
    use dwell_world::lod::{self, LodKind, LodSurfaces};
    let size = WorldSize::named(name).expect("a named size");
    let big = BifacialTerrain::sized(11, size);
    let s = big.local(f.face).spawn_point();
    let r = s[0].hypot(s[2]);
    assert!(
        r > 50_000.0,
        "{name}: the spawn {r} m out is past a 1× rim (else this proves nothing)"
    );
    // Just under the feet, in the world, on face `f`.
    let ground = [s[0], f.y(s[1] - 0.25), s[2]];
    let world = big.world();
    for level in 1..=8 {
        let c = lod_section_at(level, ground);
        let b = big.lod_bounds_at(level, c.i, c.k);
        assert_eq!(
            lod::kind_from_bounds_of(c, &b, &world),
            LodKind::Content,
            "{name} {:?}: level {level} {c:?} by its bounds",
            f.face
        );
        let mut cells = Vec::new();
        let (mut a, mut sb) = (LodSurfaces::new(), LodSurfaces::new());
        let kind = big.generate_lod(c, &mut cells, Some(&mut a), Some(&mut sb));
        assert_eq!(
            kind,
            LodKind::Content,
            "{name} {:?}: level {level} {c:?}",
            f.face
        );
        assert!(
            cells.iter().any(|&v| v != dwell_world::M.air),
            "{name} {:?}: level {level} {c:?} generated all air",
            f.face
        );
    }
}

/// The section of `level` holding the point `p` (world metres).
fn lod_section_at(level: i32, p: [f64; 3]) -> dwell_world::lod::LodCoord {
    use dwell_world::lod::{LodCoord, ORIGIN_X, ORIGIN_Y, ORIGIN_Z, section_size};
    let s = section_size(level);
    let row = dwell_world::row_of_m(p[1]) as i64;
    let of = |v: i64, o: i64| (v - o).div_euclid(s) as i32;
    LodCoord::new(
        level,
        of(p[0].floor() as i64, ORIGIN_X),
        of(row, ORIGIN_Y),
        of(p[2].floor() as i64, ORIGIN_Z),
    )
}

both_faces!(
    fn an_8x_world_s_spawn_ground_is_drawn_at_every_lod_level(f: FaceFrame) {
        a_big_world_s_spawn_sections_are_content(f, "8x");
    }
);

both_faces!(
    fn a_10x_world_s_spawn_ground_is_drawn_at_every_lod_level(f: FaceFrame) {
        a_big_world_s_spawn_sections_are_content(f, "10x");
    }
);
