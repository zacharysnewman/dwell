//! The terrain volume prototype (generator 9, world type `cliffs`; `volume.rs`): overhangs on steep
//! ground, the height field untouched elsewhere, nothing left standing on air.

use dwell_world::{CHUNK_SIZE, Chunk, ChunkCoord, FaceFrame, GENERATOR_CLIFFS, M, local_index};
use dwell_worldgen::{BifacialTerrain, TerrainGenerator};

/// The first column on a 16 m grid round the origin whose ground falls more than `slope` per metre
/// over ±3 m (the cliff mask's radius), from face-local generator `g`.
fn steep_column(g: &TerrainGenerator, slope: f32) -> (i32, i32) {
    let h = |x: i32, z: i32| g.column_at(x, z).height;
    (0..400)
        .flat_map(|j| (0..400).map(move |i| ((i - 200) * 16, (j - 200) * 16)))
        .find(|&(x, z)| {
            let sx = (h(x + 3, z) - h(x - 3, z)) / 6.0;
            let sz = (h(x, z + 3) - h(x, z - 3)) / 6.0;
            !g.column_at(x, z).outside && (sx * sx + sz * sz).sqrt() > slope
        })
        .expect("steep ground near the origin")
}

/// The face-local chunks round column (x, z) from its ground 12 m down to 12 m up, from both
/// generators: (coordinate, plain terrain's, the volume's).
fn chunks_round(
    plain: &TerrainGenerator,
    cliffs: &TerrainGenerator,
    (x, z): (i32, i32),
) -> Vec<(ChunkCoord, Chunk, Chunk)> {
    let ground = plain.ground_y(x, z).expect("inside the disc");
    let rows = (ground - 24).div_euclid(CHUNK_SIZE)..=(ground + 24).div_euclid(CHUNK_SIZE);
    let (cx, cz) = (x.div_euclid(CHUNK_SIZE), z.div_euclid(CHUNK_SIZE));
    let mut out = Vec::new();
    for dz in -1..=1 {
        for dx in -1..=1 {
            for cy in rows.clone() {
                let c = ChunkCoord::new(cx + dx, cy, cz + dz);
                let (mut a, mut b) = (Chunk::default(), Chunk::default());
                plain.generate(c, &mut a);
                cliffs.generate(c, &mut b);
                out.push((c, a, b));
            }
        }
    }
    out
}

fn solid(m: dwell_blocks::StateId) -> bool {
    m != M.air && m != M.water && !dwell_blocks::info(m).replaceable
}

dwell_world::both_faces!(
    /// The owner (2026-10-27): cliffs and overhangs. On steep ground the cliffs volume reshapes the
    /// surface in 3D: somewhere a solid cell stands over air (an overhang), and the volume both adds
    /// cells (ledges) and removes them (recesses).
    fn steep_ground_has_overhangs(f: FaceFrame) {
        let plain = BifacialTerrain::new(7);
        let cliffs = BifacialTerrain::for_generator(GENERATOR_CLIFFS, 7).expect("terrain");
        let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
        let at = steep_column(plain, 1.1);
        let (mut overhangs, mut added, mut removed) = (0, 0, 0);
        for (_, a, b) in chunks_round(plain, cliffs, at) {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    for y in 0..CHUNK_SIZE {
                        let (pa, pb) = (a.voxels()[local_index(x, y, z)], b.voxels()[local_index(x, y, z)]);
                        added += usize::from(!solid(pa) && solid(pb));
                        removed += usize::from(solid(pa) && !solid(pb));
                        if y > 0 && solid(pb) && !solid(b.voxels()[local_index(x, y - 1, z)]) {
                            overhangs += 1;
                        }
                    }
                }
            }
        }
        assert!(added > 50 && removed > 50, "added {added}, removed {removed} round {at:?}");
        assert!(overhangs > 10, "{overhangs} solid cells over air round {at:?}");
    }
);

dwell_world::both_faces!(
    /// Away from steep ground the cliffs world is the plain terrain, cell for cell: the volume
    /// only reshapes where its mask is on.
    fn gentle_ground_is_the_plain_terrain(f: FaceFrame) {
        let plain = BifacialTerrain::new(7);
        let cliffs = BifacialTerrain::for_generator(GENERATOR_CLIFFS, 7).expect("terrain");
        let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
        let spawn = plain
            .ground_y(0, 0)
            .map(|_| (0, 0))
            .expect("the origin is inside the disc");
        for (c, a, b) in chunks_round(plain, cliffs, spawn) {
            // Only chunks whose ground is gentle everywhere (the mask's lower edge, 0.6).
            let gentle = (0..CHUNK_SIZE).all(|z| {
                (0..CHUNK_SIZE).all(|x| {
                    let (wx, wz) = (c.x * CHUNK_SIZE + x, c.z * CHUNK_SIZE + z);
                    let h = |dx: i32, dz: i32| plain.column_at(wx + dx, wz + dz).height;
                    let sx = (h(3, 0) - h(-3, 0)) / 6.0;
                    let sz = (h(0, 3) - h(0, -3)) / 6.0;
                    (sx * sx + sz * sz).sqrt() < 0.6
                })
            });
            if gentle {
                assert!(a.voxels() == b.voxels(), "chunk {c:?} differs on gentle ground");
            }
        }
    }
);

dwell_world::both_faces!(
    /// Nothing the generator puts on the ground (grass, flowers) is left over air where the volume
    /// removed the cell under it.
    fn no_decoration_floats_over_a_recess(f: FaceFrame) {
        let plain = BifacialTerrain::new(7);
        let cliffs = BifacialTerrain::for_generator(GENERATOR_CLIFFS, 7).expect("terrain");
        let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
        let at = steep_column(plain, 0.8);
        for (c, _, b) in chunks_round(plain, cliffs, at) {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    for y in 1..CHUNK_SIZE {
                        let m = b.voxels()[local_index(x, y, z)];
                        let decoration = m != M.air && m != M.water && dwell_blocks::info(m).replaceable;
                        let under = b.voxels()[local_index(x, y - 1, z)];
                        assert!(
                            !decoration || under != M.air,
                            "a decoration over air at {:?} + ({x}, {y}, {z})",
                            c
                        );
                    }
                }
            }
        }
    }
);
