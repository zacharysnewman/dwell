//! A player inside solid ground (BUGS.md #82: `?at=` to a y under the ground, a teleport into a
//! mountain) is lifted to the first free space above it: inside a solid cell the terrain's
//! collision has no face to stop the capsule, and it fell through the ground toward the midplane
//! and face B. Face-local throughout ([`FaceFrame`]), so the same on both faces.

use super::Server;
use dwell_physics::{DVec3, Vec3};
use dwell_world::{FaceFrame, cell_of_pos, chunk_of, m_of_row, rows_for_m};

/// The most rows (½ m) a buried player is searched up for a free space (a mountain's 2 km).
const MAX_LIFT_ROWS: i32 = 4096;

/// What a cell holds for the lift (whether it collides, and whether it is a full cell that does):
/// None when its chunk isn't loaded. "Collides" is the terrain collision's own test
/// (`dwell_blocks::collision_class`), so a passable block such as leaves buries no one (BUGS.md
/// #94: the lift took a crown for ground, and the client's prediction fell through it).
fn cell_solid(world: &dwell_world::VoxelWorld, c: [i32; 3]) -> Option<(bool, bool)> {
    world.find(chunk_of(c[0], c[1], c[2]))?;
    let m = world.get_voxel_if_loaded(c);
    let collides = !matches!(dwell_blocks::collision_class(m), 0 | u16::MAX);
    Some(dwell_blocks::try_info(m).map_or((false, false), |i| {
        (
            collides,
            collides && i.shape == dwell_blocks::VoxelShape::Full,
        )
    }))
}

/// Where a body `rows` tall whose centre is in a solid full cell at face-local `local` should
/// stand: the first row above its feet with solid ground under it and `rows` free cells, as
/// face-local feet. `Err(())`: buried, but a cell on the way up isn't loaded yet (hold it).
/// `Ok(None)`: not buried.
fn lift_to(
    world: &dwell_world::VoxelWorld,
    f: FaceFrame,
    local: [f64; 3],
    half: f64,
    rows: i32,
) -> Result<Option<[f64; 3]>, ()> {
    let at = |c: [i32; 3]| cell_solid(world, f.cell(c));
    let centre = cell_of_pos(local);
    if !matches!(at(centre), Some((_, true))) {
        return Ok(None);
    }
    let [x, feet, z] = cell_of_pos([local[0], local[1] - half, local[2]]);
    for y in feet + 1..=feet + MAX_LIFT_ROWS {
        let ground = at([x, y - 1, z]).ok_or(())?.0;
        if !ground {
            continue;
        }
        let mut free = true;
        for k in 0..rows {
            if at([x, y + k, z]).ok_or(())?.0 {
                free = false;
                break;
            }
        }
        if free {
            return Ok(Some([local[0], m_of_row(y), local[2]]));
        }
    }
    Err(())
}

impl Server {
    /// Lifts every player whose capsule's centre is inside a colliding full cell to the first free
    /// space above it, at rest; one whose way up isn't loaded yet is held where it is.
    pub(super) fn lift_buried_players(&mut self) {
        let handles: Vec<_> = self.sessions.values().filter_map(|s| s.handle).collect();
        for h in handles {
            let p = self.players.position(&self.physics, h);
            let f = FaceFrame::at_height(p.y);
            let local = f.point([p.x, p.y, p.z]);
            let half = f64::from(self.players.half_height(h));
            let rows = rows_for_m(2.0 * half);
            let centre = match lift_to(&self.world, f, local, half, rows) {
                Ok(None) => continue,
                Ok(Some(feet)) => f.point([feet[0], feet[1] + half, feet[2]]),
                Err(()) => [p.x, p.y, p.z],
            };
            self.players
                .teleport(&mut self.physics, h, DVec3::from(centre), Vec3::ZERO);
        }
    }
}
