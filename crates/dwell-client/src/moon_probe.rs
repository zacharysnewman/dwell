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
    let &(index, def, _) = map.grids.iter().find(|g| g.1.name == "dwell:moon")?;
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
        .then(|| dwell_worldgen::grids::CrateredTerrain::new(def, world_seed))
        .flatten()
        .map(|t| {
            let local = f.point(viewer.feet);
            local[1] - t.height(f.face, local[0], local[2])
        });
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

/// Where the dev tools put a player before a short flight into the moon's capture (the bench's
/// `flyTo`): `margin_m` outside the capture sphere, over the moon's spawn on its face `f` (the
/// grid-local point), so flying straight at the moon's centre is captured within a few seconds of
/// far flight and descending from there lands by the spawn.
pub fn approach_point(map: &TravelMap, world_seed: u64, f: FaceFrame, margin_m: f64) -> Option<V3> {
    let &(_, def, _) = map.grids.iter().find(|g| g.1.name == "dwell:moon")?;
    let terrain = dwell_worldgen::grids::CrateredTerrain::new(def, world_seed)?;
    let s = terrain.spawn_point(f.face);
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
    let &(_, def, _) = map.grids.iter().find(|g| g.1.name == "dwell:moon")?;
    let terrain = dwell_worldgen::grids::CrateredTerrain::new(def, world_seed)?;
    let s = terrain.spawn_point(f.face);
    Some(f.point([s[0], s[1] + up_m, s[2]]))
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
