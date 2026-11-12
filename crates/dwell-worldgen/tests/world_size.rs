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
