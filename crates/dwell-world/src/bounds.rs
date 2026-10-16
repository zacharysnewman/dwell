//! The build volume (ARCHITECTURE.md §6.3, ADR 0025): the one predicate that bounds building and
//! storage — edits (the server's check and the client's pre-check), generation and storage (chunks
//! wholly outside are air, never generated or stored), and streaming and LOD requests. It does not
//! bound movement (D3): players may fly anywhere.
//!
//! The shape is a small closed set in code ([`BoundsShape`]: a sphere for the world, a slab kept
//! for tests); its parameters are data (`bounds` in `shared/protocol/constants.json`). A voxel is
//! inside when its centre is; a box of voxels is classified conservatively, so `Outside` means no
//! voxel of it is inside.

pub use dwell_protocol::BoundsShape;

use crate::{CHUNK_SIZE, ChunkCoord};

/// How a box of voxels lies relative to the volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlap {
    /// Every voxel inside.
    Inside,
    /// Some voxels may be inside.
    Partial,
    /// No voxel inside.
    Outside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildVolume(pub BoundsShape);

/// The world's build volume, from data.
pub const BUILD_VOLUME: BuildVolume = BuildVolume(dwell_protocol::BOUNDS);

/// Twice a coordinate's distance from the nearest point of [lo2, hi2] (doubled coordinates).
fn nearest2(c2: i128, lo2: i128, hi2: i128) -> i128 {
    if c2 < lo2 {
        lo2 - c2
    } else if c2 > hi2 {
        c2 - hi2
    } else {
        0
    }
}

impl BuildVolume {
    /// The voxel (x, y, z): its centre (x + ½, y + ½, z + ½) is inside.
    pub fn contains_voxel(&self, x: i64, y: i64, z: i64) -> bool {
        match self.0 {
            BoundsShape::Sphere { center, radius } => {
                // Doubled coordinates keep the half-voxel centre exact in integers.
                let d = |v: i64, c: i64| i128::from(2 * v + 1) - 2 * i128::from(c);
                let (dx, dy, dz) = (d(x, center[0]), d(y, center[1]), d(z, center[2]));
                let r2 = 2 * i128::from(radius);
                dx * dx + dy * dy + dz * dz < r2 * r2
            }
            BoundsShape::Slab { min_y, max_y } => (min_y..max_y).contains(&y),
        }
    }

    /// The box of voxels `min..max` (exclusive), by its voxels' centres.
    pub fn box_overlap(&self, min: [i64; 3], max: [i64; 3]) -> Overlap {
        if (0..3).any(|a| min[a] >= max[a]) {
            return Overlap::Outside;
        }
        match self.0 {
            BoundsShape::Sphere { center, radius } => {
                // The voxels' centres span [2·min + 1, 2·max − 1] in doubled coordinates.
                let lo = |a: usize| 2 * i128::from(min[a]) + 1;
                let hi = |a: usize| 2 * i128::from(max[a]) - 1;
                let c = |a: usize| 2 * i128::from(center[a]);
                let r2 = 2 * i128::from(radius);
                let r2 = r2 * r2;
                let near: i128 = (0..3)
                    .map(|a| nearest2(c(a), lo(a), hi(a)).pow(2))
                    .sum();
                if near >= r2 {
                    return Overlap::Outside;
                }
                let far: i128 = (0..3)
                    .map(|a| (c(a) - lo(a)).abs().max((hi(a) - c(a)).abs()).pow(2))
                    .sum();
                if far < r2 {
                    Overlap::Inside
                } else {
                    Overlap::Partial
                }
            }
            BoundsShape::Slab { min_y, max_y } => {
                if max[1] <= min_y || min[1] >= max_y {
                    Overlap::Outside
                } else if min[1] >= min_y && max[1] <= max_y {
                    Overlap::Inside
                } else {
                    Overlap::Partial
                }
            }
        }
    }

    /// A chunk's voxels.
    pub fn chunk_overlap(&self, c: ChunkCoord) -> Overlap {
        let s = i64::from(CHUNK_SIZE);
        let min = [i64::from(c.x) * s, i64::from(c.y) * s, i64::from(c.z) * s];
        self.box_overlap(min, [min[0] + s, min[1] + s, min[2] + s])
    }

    /// Whether any voxel of the chunk may be inside: chunks for which this is false are air, never
    /// generated, stored, streamed or requested.
    pub fn chunk_may_hold(&self, c: ChunkCoord) -> bool {
        self.chunk_overlap(c) != Overlap::Outside
    }

    /// The voxel rows anything can be built in: `lo..hi`.
    pub const fn y_range(&self) -> (i64, i64) {
        match self.0 {
            BoundsShape::Sphere { center, radius } => (center[1] - radius, center[1] + radius),
            BoundsShape::Slab { min_y, max_y } => (min_y, max_y),
        }
    }
}

/// The voxel (x, y, z) is inside the world's build volume.
pub fn inside_build_volume(x: i64, y: i64, z: i64) -> bool {
    BUILD_VOLUME.contains_voxel(x, y, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPHERE: BuildVolume = BuildVolume(BoundsShape::Sphere {
        center: [0, 0, 0],
        radius: 50_000,
    });

    #[test]
    fn the_world_is_the_data_sphere() {
        assert_eq!(BUILD_VOLUME, SPHERE);
        assert_eq!(BUILD_VOLUME.y_range(), (-50_000, 50_000));
    }

    #[test]
    fn a_sphere_holds_the_voxels_whose_centres_lie_within_its_radius() {
        let s = SPHERE;
        assert!(s.contains_voxel(0, 0, 0));
        assert!(s.contains_voxel(-1, -1, -1));
        assert!(s.contains_voxel(0, 49_999, 0) && !s.contains_voxel(0, 50_000, 0));
        assert!(s.contains_voxel(0, -50_000, 0) && !s.contains_voxel(0, -50_001, 0));
        assert!(s.contains_voxel(49_999, 0, 0) && !s.contains_voxel(50_000, 0, 0));
        // On the diagonal: 35,355.5² · 2 > 50,000², 35,354.5² · 2 < 50,000².
        assert!(s.contains_voxel(35_354, 0, 35_354) && !s.contains_voxel(35_355, 0, 35_355));
        // Symmetric about the midplane under the face mirror y ↔ −1 − y.
        for y in [0, 31, 1000, 49_999] {
            assert_eq!(s.contains_voxel(30_000, y, 0), s.contains_voxel(30_000, -1 - y, 0));
        }
        // Far away (players fly anywhere): no overflow.
        assert!(!s.contains_voxel(33_554_432, -33_554_432, 33_554_432));
    }

    #[test]
    fn box_overlap_agrees_with_the_voxels() {
        let s = SPHERE;
        // Chunk boxes over the sphere's surface and the rim, every voxel checked.
        for c in [
            ChunkCoord::new(0, 1561, 0),
            ChunkCoord::new(0, 1562, 0),
            ChunkCoord::new(0, 1563, 0),
            ChunkCoord::new(1562, 0, 0),
            ChunkCoord::new(1562, -1, 0),
            ChunkCoord::new(1104, 0, 1104),
            ChunkCoord::new(1105, 5, 1104),
            ChunkCoord::new(-1563, 0, 0),
            ChunkCoord::new(3, 4, 5),
        ] {
            let mut any = false;
            let mut all = true;
            for z in 0..32 {
                for y in 0..32 {
                    for x in 0..32 {
                        let v = s.contains_voxel(
                            i64::from(c.x * 32 + x),
                            i64::from(c.y * 32 + y),
                            i64::from(c.z * 32 + z),
                        );
                        any |= v;
                        all &= v;
                    }
                }
            }
            match s.chunk_overlap(c) {
                Overlap::Outside => assert!(!any, "{c:?}"),
                Overlap::Inside => assert!(all, "{c:?}"),
                Overlap::Partial => assert!(any && !all, "{c:?} {any} {all}"),
            }
        }
        assert_eq!(s.chunk_overlap(ChunkCoord::new(0, 1562, 0)), Overlap::Partial);
        assert_eq!(s.chunk_overlap(ChunkCoord::new(0, 1563, 0)), Overlap::Outside);
        assert_eq!(s.chunk_overlap(ChunkCoord::new(0, -1563, 0)), Overlap::Partial);
        assert_eq!(s.chunk_overlap(ChunkCoord::new(0, -1564, 0)), Overlap::Outside);
        assert_eq!(s.chunk_overlap(ChunkCoord::new(3, 4, 5)), Overlap::Inside);
        assert_eq!(s.box_overlap([0, 0, 0], [0, 5, 5]), Overlap::Outside);
    }

    #[test]
    fn a_slab_fixture_bounds_rows_only() {
        // The closed set's second shape, from data alone: as a world's bounds would be.
        let slab = BuildVolume(BoundsShape::Slab {
            min_y: -64,
            max_y: 64,
        });
        assert!(slab.contains_voxel(9_000_000, -64, -9_000_000));
        assert!(slab.contains_voxel(0, 63, 0) && !slab.contains_voxel(0, 64, 0));
        assert!(!slab.contains_voxel(0, -65, 0));
        assert_eq!(slab.chunk_overlap(ChunkCoord::new(99, 1, 0)), Overlap::Inside);
        assert_eq!(slab.chunk_overlap(ChunkCoord::new(99, 2, 0)), Overlap::Outside);
        assert_eq!(slab.chunk_overlap(ChunkCoord::new(99, -3, 0)), Overlap::Outside);
        assert_eq!(
            slab.box_overlap([0, 60, 0], [32, 70, 32]),
            Overlap::Partial
        );
        assert_eq!(slab.y_range(), (-64, 64));
    }
}
