//! A world's own disc and build volume (docs/proposals/BIG_MAP.md M2): the world's size
//! (`dwell_protocol::world_size::WorldSize`, from its settings) carried to everything that bounds
//! generation, storage, streaming, editing and the LOD. A 1× world's are the build's
//! (`inside_world_disc`, `chunk_disc_overlap`, `BUILD_VOLUME`, `lod::in_world`) exactly; a bigger
//! world's disc is `radius_scale` × as wide, and its build volume as wide as its disc and as tall as
//! today's (`BuildVolume::sized`), so the rows every size builds in are the same.

use dwell_protocol::world_size::WorldSize;

use crate::{
    BUILD_VOLUME, BuildVolume, ChunkCoord, DiscOverlap, chunk_disc_overlap_of, inside_disc_of,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldBounds {
    pub size: WorldSize,
    radius: i64,
    volume: BuildVolume,
}

impl Default for WorldBounds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl WorldBounds {
    /// A 1× world's: the build's disc and volume.
    pub const DEFAULT: Self = Self::of(WorldSize::DEFAULT);

    pub const fn of(size: WorldSize) -> Self {
        Self {
            size,
            radius: size.world_radius_m(),
            volume: BUILD_VOLUME.sized(size),
        }
    }

    /// A grid's disc (docs/proposals/MOON_WORLD.md): radius `radius_m` round its own origin, the
    /// build's volume (a grid's chunks use the same rows as home's). The moon's and the sun's LOD
    /// select their sections in it (`dwell_worldgen::grid_lod`).
    pub const fn grid(radius_m: i64) -> Self {
        Self {
            size: WorldSize::DEFAULT,
            radius: radius_m,
            volume: BUILD_VOLUME,
        }
    }

    /// The disc's radius (m).
    pub const fn radius(&self) -> i64 {
        self.radius
    }
    /// The build volume (`BuildVolume`: `contains_voxel`, `chunk_may_hold`, …).
    pub const fn volume(&self) -> &BuildVolume {
        &self.volume
    }
    /// The column (x, z) is inside the world's disc.
    pub fn inside_disc(&self, x: i64, z: i64) -> bool {
        inside_disc_of(self.radius, x, z)
    }
    /// How a chunk column lies relative to the world's disc.
    pub fn chunk_disc_overlap(&self, cx: i32, cz: i32) -> DiscOverlap {
        chunk_disc_overlap_of(self.radius, cx, cz)
    }
    /// The cell is one an edit may write: inside the world's build volume and its disc.
    pub fn contains_cell(&self, c: [i32; 3]) -> bool {
        let [x, y, z] = c.map(i64::from);
        self.volume.contains_voxel(x, y, z) && self.inside_disc(x, z)
    }
    /// Whether any voxel of the chunk may be inside the world: its disc and its build volume.
    pub fn chunk_may_hold(&self, c: ChunkCoord) -> bool {
        self.chunk_disc_overlap(c.x, c.z) != DiscOverlap::Outside && self.volume.chunk_may_hold(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_1x_world_s_bounds_are_the_build_s() {
        let b = WorldBounds::DEFAULT;
        for (x, z) in [
            (0, 0),
            (49_999, 0),
            (50_000, 0),
            (35_354, 35_354),
            (35_356, 35_356),
        ] {
            assert_eq!(b.inside_disc(x, z), crate::inside_world_disc(x, z));
        }
        for (cx, cz) in [(0, 0), (1_562, 0), (1_563, 0), (1_105, 1_105), (-1_600, 3)] {
            assert_eq!(
                b.chunk_disc_overlap(cx, cz),
                crate::chunk_disc_overlap(cx, cz)
            );
        }
        assert_eq!(*b.volume(), BUILD_VOLUME);
    }

    #[test]
    fn a_2x_world_s_disc_is_twice_as_wide() {
        let b = WorldBounds::of(WorldSize::named("2x").unwrap());
        assert_eq!(b.radius(), 100_000);
        assert!(b.inside_disc(99_999, 0) && !b.inside_disc(100_000, 0));
        assert!(b.inside_disc(0, -70_000) && !crate::inside_world_disc(0, -70_000));
        assert_eq!(b.chunk_disc_overlap(2_000, 0), DiscOverlap::Inside);
        assert_eq!(b.chunk_disc_overlap(3_125, 0), DiscOverlap::Outside);
        assert!(b.chunk_may_hold(ChunkCoord::new(2_500, 0, 0)));
        assert!(!WorldBounds::DEFAULT.chunk_may_hold(ChunkCoord::new(2_500, 0, 0)));
    }
}
