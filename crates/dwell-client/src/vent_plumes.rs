//! The run-time plumes' first source (docs/CLOUDS.md §8.7): a faint steam plume over each of the
//! world's lava vents (its worldgen config's `tectonics.lavaVents`, docs/proposals/VOLCANOES.md §6.2)
//! while its lava is there, its look `clouds.json` `sources.lavaVent`. The client looks down the
//! vent's column in the cells it holds now and then: a plume over the top molten cell, none when
//! there is none (the lava cooled, or the column isn't loaded), so a remote server needs no new
//! message.

use dwell_blocks::StateId;
use dwell_look::clouds::CLOUDS;
use dwell_look::plumes::PlumeSpec;
use dwell_world::FaceFrame;

/// How often the vents' columns are looked at (s).
pub const CHECK_EVERY_S: f64 = 2.0;
/// The plumes' names start with this (the source's own, replaced together).
pub const PREFIX: &str = "lava-vent:";
/// How far up a vent's column is looked at, face-locally from sea level (m): over the tallest cone.
const SCAN_TOP_M: f64 = 7000.0;
const SCAN_BOTTOM_M: f64 = -200.0;

/// The world's vents and when they were last looked at.
#[derive(Clone, Debug, Default)]
pub struct VentPlumes {
    /// Each vent's face and world column.
    pub sites: Vec<(FaceFrame, [i32; 2])>,
    next_s: f64,
}

impl VentPlumes {
    /// The vents of worldgen config `text` (none for the build's ground or a config without
    /// `lavaVents`), each on the faces it lists.
    pub fn from_worldgen(text: &str) -> Self {
        let sites = dwell_worldgen::config::lava_vent_sites(text).unwrap_or_default();
        let mut out = Vec::new();
        for (v, [x, z]) in sites {
            for f in [FaceFrame::A, FaceFrame::B] {
                if v.faces[usize::from(f.is_b())] {
                    out.push((f, [x.floor() as i32, z.floor() as i32]));
                }
            }
        }
        Self {
            sites: out,
            next_s: 0.0,
        }
    }

    /// Whether the columns are due a look at clock `now_s` (s); when they are, the next look is set.
    pub fn due(&mut self, now_s: f64) -> bool {
        if self.sites.is_empty() || now_s < self.next_s {
            return false;
        }
        self.next_s = now_s + CHECK_EVERY_S;
        true
    }

    /// The plumes over the vents: `top_lava(face, column)` the face-local height (m) of the top of
    /// the column's top molten cell, if it has one.
    pub fn plumes(
        &self,
        mut top_lava: impl FnMut(FaceFrame, [i32; 2]) -> Option<f64>,
    ) -> Vec<(String, PlumeSpec)> {
        let sea = dwell_protocol::world::SEA_LEVEL_M;
        self.sites
            .iter()
            .enumerate()
            .filter_map(|(i, &(f, c))| {
                let y = top_lava(f, c)?;
                let at = [f64::from(c[0]) + 0.5, y - sea, f64::from(c[1]) + 0.5];
                Some((
                    format!("{PREFIX}{i}"),
                    PlumeSpec::from_source(&CLOUDS.sources.lava_vent, f, at),
                ))
            })
            .collect()
    }
}

/// The face-local height (m) of the top of the top cell of `column` on face `f` that `is_lava`
/// holds, looking at what `voxel` (a world cell's state, air where not loaded) gives.
pub fn top_lava_in(
    f: FaceFrame,
    [x, z]: [i32; 2],
    mut voxel: impl FnMut([i32; 3]) -> StateId,
    is_lava: impl Fn(StateId) -> bool,
) -> Option<f64> {
    let sea = dwell_protocol::world::SEA_LEVEL_M;
    let top = dwell_world::row_of_m(sea + SCAN_TOP_M);
    let bottom = dwell_world::row_of_m(sea + SCAN_BOTTOM_M);
    (bottom..=top)
        .rev()
        .find(|&row| is_lava(voxel(f.cell([x, row, z]))))
        .map(|row| dwell_world::m_of_row(row + 1))
}

/// Whether state `s` is a molten fluid (`dwell:lava`, by name).
pub fn is_lava(s: StateId) -> bool {
    s != 0 && dwell_blocks::try_info(s).is_some() && dwell_blocks::block_of(s).name == "dwell:lava"
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_world::both_faces;

    const CONFIG: &str = "test:starter";

    #[test]
    // both-faces: checks the vent is listed on each face.
    fn the_starter_worlds_vents_are_its_sources_and_the_default_world_has_none() {
        assert!(VentPlumes::from_worldgen("").sites.is_empty());
        let v = VentPlumes::from_worldgen(CONFIG);
        assert!(!v.sites.is_empty(), "the starter world lists lava vents");
        // The lake vent is on both faces.
        assert!(v.sites.iter().any(|(f, _)| *f == FaceFrame::A));
        assert!(v.sites.iter().any(|(f, _)| *f == FaceFrame::B));
    }

    #[test]
    fn the_columns_are_looked_at_every_few_seconds() {
        let mut v = VentPlumes::from_worldgen(CONFIG);
        assert!(v.due(10.0));
        assert!(!v.due(10.5));
        assert!(v.due(10.0 + CHECK_EVERY_S));
        assert!(!VentPlumes::default().due(10.0), "no vents, no looks");
    }

    both_faces! {
        fn a_plume_stands_over_active_lava_and_goes_with_it(f: FaceFrame) {
            let sea = dwell_protocol::world::SEA_LEVEL_M;
            let lava = 7 as StateId;
            let is = |s: StateId| s == lava;
            // A molten column on face `f`, its top cell at face-local row `row`.
            let row = dwell_world::row_of_m(sea + 230.0);
            let voxel = |c: [i32; 3]| {
                let local = f.cell(c);
                if local[0] == 5 && local[2] == -3 && (row - 3..=row).contains(&local[1]) { lava } else { 0 }
            };
            let top = top_lava_in(f, [5, -3], voxel, is).expect("the lava is found");
            assert_eq!(top, dwell_world::m_of_row(row + 1));
            assert_eq!(top_lava_in(f, [6, -3], voxel, is), None, "no lava beside it");
            // Its plume: over the lava's top, face-local, the source's look.
            let v = VentPlumes { sites: vec![(f, [5, -3])], next_s: 0.0 };
            let plumes = v.plumes(|ff, c| top_lava_in(ff, c, voxel, is));
            assert_eq!(plumes.len(), 1);
            let (name, p) = &plumes[0];
            assert!(name.starts_with(PREFIX));
            assert_eq!(p.face, f);
            assert_eq!((p.x_m, p.z_m), (5.5, -2.5));
            assert!((p.base_m - (top - sea)).abs() < 1e-9);
            assert_eq!(p.top_m - p.base_m, CLOUDS.sources.lava_vent.height_m);
            // The lava gone: no plume.
            assert!(v.plumes(|ff, c| top_lava_in(ff, c, |_| 0, is)).is_empty());
        }
    }
}
