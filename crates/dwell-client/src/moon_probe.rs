//! The moon as the dev tools read it (`__dwell.moon()`, docs/DEV_TOOLS.md "The moon"): where it is
//! in the world at the clock's time, the way to it and its distance from the player, the grid the
//! player is on now (home or the moon), and the gravity there. The bench's `flyTo` step steers by
//! it (MOON_WORLD.md §7.3: the browser flight).

use dwell_sim::travel::{GRID_CENTRE, TravelMap};
use dwell_world::{FaceFrame, GridFrame};

type V3 = [f64; 3];

/// What `moon_report` reads of the game.
#[derive(Clone, Copy, Debug)]
pub struct Viewer {
    /// The grid the player is on (0 home).
    pub grid: u8,
    /// The feet, in that grid's local coordinates.
    pub feet: V3,
    /// The velocity, in that grid's local coordinates.
    pub velocity: V3,
    /// The world clock (s).
    pub world_s: f64,
    /// Home's surface gravity (m/s², the controller config's).
    pub gravity: f64,
}

/// The moon seen from the player.
#[derive(Clone, Debug, PartialEq)]
pub struct MoonReport {
    /// The moon's grid index (`grids.json`).
    pub index: u8,
    /// Its centre (the midplane's centre), world m.
    pub position: V3,
    /// The player's feet, world m.
    pub feet_world: V3,
    /// The unit world direction from the player's feet to the moon's centre.
    pub direction: V3,
    /// From the player's feet to the moon's centre, m.
    pub distance: f64,
    /// The capture and release spheres' radii round its centre, m.
    pub capture_m: f64,
    pub release_m: f64,
    /// The player's grid now: `home`, `moon` (the name after the namespace).
    pub grid: String,
    /// The gravity on the moon (m/s²): home's × its `gravityScale`.
    pub moon_gravity: f64,
    /// The gravity where the player is now (m/s²).
    pub gravity_now: f64,
    /// The look (degrees, the player's face-local yaw 0 = +Z and pitch) toward the moon's centre.
    pub look: (f64, f64),
    /// The player's velocity along its face's up (m/s; negative falling).
    pub up_mps: f64,
    /// On the moon: the feet's height over the moon's generated ground under them (m).
    pub ground_m: Option<f64>,
    /// The client holds the chunk under the feet (set by the game; `moon_report` leaves it false):
    /// the bench's check that home's chunks came back after a trip.
    pub ground_held: bool,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(v: V3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The moon from `viewer`, or `None` in a world with no moon grid (`map` holds no `dwell:moon`).
pub fn moon_report(map: &TravelMap, world_seed: u64, viewer: Viewer) -> Option<MoonReport> {
    grid_report(map, world_seed, "dwell:moon", viewer)
}

/// [`moon_report`] for any grid of the map by name (`dwell:sun`: `__dwell.sun()`, SUN_WORLD.md):
/// its height over the ground from the grid's own stand-point generator.
pub fn grid_report(
    map: &TravelMap,
    world_seed: u64,
    name: &str,
    viewer: Viewer,
) -> Option<MoonReport> {
    let &(index, def, _) = map.grids.iter().find(|g| g.1.name == name)?;
    let moon = map.frame(index, viewer.world_s);
    let here: GridFrame = map.frame(viewer.grid, viewer.world_s);
    let position = moon.to_world(GRID_CENTRE);
    let feet_world = here.to_world(viewer.feet);
    let to = sub(position, feet_world);
    let distance = len(to);
    let direction = to.map(|c| c / distance.max(1e-9));
    let f = FaceFrame::at_height(viewer.feet[1]);
    let l = here.vector_to_local(f, direction);
    let look = (
        l[0].atan2(l[2]).to_degrees(),
        l[1].clamp(-1.0, 1.0).asin().to_degrees(),
    );
    let name = |i: u8| {
        let all = dwell_worldgen::grids::grids();
        let n = &all[usize::from(i).min(all.len() - 1)].name;
        n.split_once(':').map_or(n.as_str(), |(_, s)| s).to_owned()
    };
    let ground_m = (viewer.grid == index)
        .then(|| ground_height(def, world_seed, f, viewer.feet))
        .flatten();
    let capture_m = def.capture_m.unwrap_or(0.0);
    Some(MoonReport {
        index,
        position,
        feet_world,
        direction,
        distance,
        capture_m,
        release_m: capture_m * map.rules.release_factor,
        grid: name(viewer.grid),
        moon_gravity: viewer.gravity * moon.gravity_scale,
        gravity_now: viewer.gravity * here.gravity_scale,
        look,
        up_mps: f.vector(viewer.velocity)[1],
        ground_m,
        ground_held: false,
    })
}

/// The top of the generated ground the feet stand on at face-local (x, z): its column's top cell's
/// top (`top_row`, the height rounded to the ½ m cells), not the smooth height, which lies up to
/// a quarter cell over or under it (BUGS.md: a landing read 0.11–0.24 m under the ground).
fn ground_top(t: &dwell_worldgen::grids::CrateredTerrain, f: FaceFrame, x: f64, z: f64) -> f64 {
    t.top_row(f.face, x.floor() as i64, z.floor() as i64)
        .map_or_else(
            || t.height(f.face, x, z),
            |top| (top + 1) as f64 * dwell_world::CELL_H,
        )
}

impl MoonReport {
    /// The report as `__dwell.moon()` returns it (JSON).
    pub fn json(&self) -> String {
        serde_json::json!({
            "index": self.index,
            "position": self.position,
            "feet": self.feet_world,
            "direction": self.direction,
            "distance": self.distance,
            "captureM": self.capture_m,
            "releaseM": self.release_m,
            "grid": self.grid,
            "gravity": self.moon_gravity,
            "gravityNow": self.gravity_now,
            "look": { "yaw": self.look.0, "pitch": self.look.1 },
            "upMps": self.up_mps,
            "groundM": self.ground_m,
            "groundHeld": self.ground_held,
        })
        .to_string()
    }
}

/// The ground's face-local height under grid-local `feet` on face `f` of grid `def`.
fn ground_height(
    def: &dwell_worldgen::grids::GridDef,
    world_seed: u64,
    f: FaceFrame,
    feet: V3,
) -> Option<f64> {
    let l = f.point(feet);
    let h = match &def.generator {
        dwell_worldgen::grids::GridGenerator::Cratered(_) => {
            let t = dwell_worldgen::grids::CrateredTerrain::new(def, world_seed)?;
            ground_top(&t, f, l[0], l[2])
        }
        // The ground's top cell, as the generator rounds it (`VolcanicTerrain::rows`).
        dwell_worldgen::grids::GridGenerator::Volcanic(_) => {
            let t = dwell_worldgen::volcanic::VolcanicTerrain::new(def, world_seed)?;
            let h = t.height(f.face, l[0].floor() + 0.5, l[2].floor() + 0.5);
            (h / dwell_world::CELL_H).round() * dwell_world::CELL_H
        }
        _ => return None,
    };
    Some(l[1] - h)
}

/// Where the dev tools put a player before a short flight into the moon's capture (the bench's
/// `flyTo`): `margin_m` outside the capture sphere, over the moon's spawn on its face `f` (the
/// grid-local point), so flying straight at the moon's centre is captured within a few seconds of
/// far flight and descending from there lands by the spawn.
pub fn approach_point(map: &TravelMap, world_seed: u64, f: FaceFrame, margin_m: f64) -> Option<V3> {
    grid_approach_point(map, world_seed, "dwell:moon", f, margin_m)
}

/// [`approach_point`] for any grid by name, over its stand point (`stand_point`).
pub fn grid_approach_point(
    map: &TravelMap,
    world_seed: u64,
    name: &str,
    f: FaceFrame,
    margin_m: f64,
) -> Option<V3> {
    let &(_, def, _) = map.grids.iter().find(|g| g.1.name == name)?;
    let s = dwell_worldgen::grids::stand_point(def, world_seed, f.face)?;
    let r = def.capture_m? + margin_m;
    // Along the face's up over the spawn column, `r` from the centre.
    let at = |up: f64| f.point([s[0], s[1] + up, s[2]]);
    let h = s[0].hypot(s[2]);
    let v0 = (at(0.0)[1] - GRID_CENTRE[1]).abs();
    let up = (r * r - h * h).max(0.0).sqrt() - v0;
    Some(at(up))
}

/// The point `up_m` over the moon's spawn on face `f` (grid-local m): where the dev teleport onto
/// the moon (`LocalCore::moon_land`) puts the capsule's centre, inside the capture sphere.
pub fn landing_point(map: &TravelMap, world_seed: u64, f: FaceFrame, up_m: f64) -> Option<V3> {
    grid_landing_point(map, world_seed, "dwell:moon", f, up_m)
}

/// [`landing_point`] for any grid by name: `up_m` over its stand point (the sun's dry crust away
/// from lava, `sunLand`).
pub fn grid_landing_point(
    map: &TravelMap,
    world_seed: u64,
    name: &str,
    f: FaceFrame,
    up_m: f64,
) -> Option<V3> {
    let &(_, def, _) = map.grids.iter().find(|g| g.1.name == name)?;
    let s = dwell_worldgen::grids::stand_point(def, world_seed, f.face)?;
    Some(f.point([s[0], s[1] + up_m, s[2]]))
}

/// Where the dev tools put a player beside a river head of grid `name` on face `f` (the bench's
/// `sunLand` with `river`, BUGS.md #142): `up_m` over the first dry column (at least a metre over
/// the lava it borders, its neighbours 2 m off not molten) 8–96 m from the river head nearest the
/// grid's stand point, so the head's lava field (grids.json `flow`, placed within `nearM` of a
/// player and run within lava.json `nearRadiusM`) runs beside it. `None` on a grid with no rivers
/// kept flowing, or no such head and bank within 4 km of the stand point.
pub fn grid_river_bank_point(world_seed: u64, name: &str, f: FaceFrame, up_m: f64) -> Option<V3> {
    let t = dwell_worldgen::volcanic::VolcanicTerrain::for_grid(name, world_seed)?;
    let s = t.spawn_point(f.face);
    let h = *t.river_heads(f.face, s[0], s[2], 4_000.0).first()?;
    let dry = |x: f64, z: f64| {
        let c = t.column(f.face, x, z);
        c.ground >= c.lava + 1.0
    };
    for ring in 2..=24 {
        let r = f64::from(ring) * 4.0;
        for k in 0..16 {
            let a = f64::from(k) / 16.0 * std::f64::consts::TAU;
            let (x, z) = (
                (h.x + r * a.cos()).floor() + 0.5,
                (h.z + r * a.sin()).floor() + 0.5,
            );
            let near = [[2.0, 0.0], [-2.0, 0.0], [0.0, 2.0], [0.0, -2.0]];
            if dry(x, z)
                && near
                    .iter()
                    .all(|[dx, dz]| !t.column(f.face, x + dx, z + dz).molten())
            {
                let top =
                    (t.height(f.face, x, z) / dwell_world::CELL_H).round() * dwell_world::CELL_H;
                return Some(f.point([x, top + up_m, z]));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_look::celestial::Celestial;
    use dwell_look::daynight::DAY_NIGHT;

    const SEED: u64 = 7;

    fn map() -> TravelMap {
        TravelMap::new(SEED, 25_000.0, Celestial::new(DAY_NIGHT, None))
    }

    /// A viewer on face `f` falling 2 m/s.
    fn viewer(f: FaceFrame, grid: u8, feet: V3) -> Viewer {
        Viewer {
            grid,
            feet,
            velocity: f.vector([0.0, -2.0, 0.0]),
            world_s: 1234.0,
            gravity: 9.81,
        }
    }

    dwell_world::both_faces!(
        /// Standing on the moon's generated ground on face `f` reads 0 m over it: the ground is
        /// the column's top cell, not the smooth height (which a landing read as up to a quarter
        /// cell over the feet: −0.11 m on face A, −0.24 m on face B).
        fn standing_on_the_moons_ground_reads_zero(f: FaceFrame) {
            let m = map();
            let def = dwell_worldgen::grids::grid("dwell:moon").unwrap();
            let t = dwell_worldgen::grids::CrateredTerrain::new(def, SEED).unwrap();
            let mut off = 0;
            for k in 0..200 {
                let (x, z) = (f64::from(k) * 37.3 - 3700.0, f64::from(k) * -21.7 + 900.0);
                let top = t
                    .top_row(f.face, x.floor() as i64, z.floor() as i64)
                    .unwrap();
                let stand = (top + 1) as f64 * dwell_world::CELL_H;
                off += usize::from((stand - t.height(f.face, x, z)).abs() > 0.05);
                let feet = f.point([x, stand, z]);
                let index = m.grids[0].0;
                let r = moon_report(&m, SEED, viewer(f, index, feet)).unwrap();
                let g = r.ground_m.unwrap();
                assert!(
                    g.abs() < 1e-9,
                    "({x}, {z}): {g} m over the ground it stands on"
                );
            }
            assert!(
                off > 50,
                "the smooth height is not the cells' top ({off} of 200)"
            );
        }
    );

    dwell_world::both_faces!(
        /// From home the moon is where its frame puts it: the direction and distance reach its
        /// centre, the look turned into the player's face points the same way, home's gravity now,
        /// the moon's ⅙.
        fn from_home_the_report_points_at_the_moons_centre(f: FaceFrame) {
            let m = map();
            let feet = f.point([10.0, 40.0, -20.0]);
            let r = moon_report(&m, SEED, viewer(f, 0, feet)).expect("a moon");
            assert_eq!(r.grid, "home");
            let reach: V3 = std::array::from_fn(|k| feet[k] + r.direction[k] * r.distance);
            assert!(
                len(sub(reach, r.position)) < 1e-3,
                "{reach:?} {:?}",
                r.position
            );
            assert!(r.distance > 100_000.0, "the moon is far: {}", r.distance);
            let look = GridFrame::HOME.look(f, r.look.0, r.look.1);
            assert!(
                len(sub(look, r.direction)) < 1e-9,
                "{look:?} {:?}",
                r.direction
            );
            assert!((r.gravity_now - 9.81).abs() < 1e-9);
            assert!((r.moon_gravity / 9.81 - 0.1667).abs() < 1e-6);
            assert!(
                (r.up_mps + 2.0).abs() < 1e-9,
                "falling 2 m/s along the face's up"
            );
            assert_eq!(r.ground_m, None);
            assert!((r.release_m - r.capture_m * 1.1).abs() < 1e-6);
            let j: serde_json::Value = serde_json::from_str(&r.json()).unwrap();
            assert_eq!(j["grid"], "home");
            assert!(j["groundM"].is_null());
        }
    );

    dwell_world::both_faces!(
        /// On the moon, 30 m over its spawn on face `f`: the moon's gravity now, the height over its
        /// ground, its centre straight down the face (the look's pitch −90° from over the middle).
        fn on_the_moon_the_report_gives_its_ground_and_gravity(f: FaceFrame) {
            let m = map();
            let &(i, def, _) = m.grids.iter().find(|g| g.1.name == "dwell:moon").unwrap();
            let t = dwell_worldgen::grids::CrateredTerrain::new(def, SEED).unwrap();
            let s = t.spawn_point(f.face);
            let feet = f.point([s[0], s[1] + 30.0, s[2]]);
            let r = moon_report(&m, SEED, viewer(f, i, feet)).unwrap();
            assert_eq!(r.grid, "moon");
            assert!((r.gravity_now - r.moon_gravity).abs() < 1e-9);
            let g = r.ground_m.unwrap();
            assert!((g - 30.0).abs() < 1.0, "30 m over the spawn's ground: {g}");
            assert!(r.distance < 1_000.0, "by the centre: {}", r.distance);
            assert!(r.look.1 < -45.0, "the centre below: {:?}", r.look);
        }
    );

    dwell_world::both_faces!(
        /// The approach point sits `margin` outside the capture sphere over the spawn, on face `f`'s
        /// side of the moon's midplane.
        fn the_approach_is_just_outside_the_capture_sphere(f: FaceFrame) {
            let m = map();
            let p = approach_point(&m, SEED, f, 1_500.0).unwrap();
            let d = len(sub(p, GRID_CENTRE));
            assert!((d - 41_500.0).abs() < 1.0, "{d}");
            assert_eq!(FaceFrame::at_height(p[1]), f);
        }
    );
}
