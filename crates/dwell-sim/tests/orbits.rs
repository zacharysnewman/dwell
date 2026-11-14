//! Orbits by seed through the server (docs/proposals/REALISTIC_WORLD.md §8, O2): a new world with a
//! Sky preset rolls its orbit from its seed and keeps it in its settings (its clock, its moon's
//! phase, `CELESTIAL`); a world without one is today's sky, its settings as before.

mod common;

use common::*;
use dwell_look::orbits::Orbit;
use dwell_sim::entropy::SeededEntropy;
use dwell_sim::server::{Server, ServerConfig};

fn server(seed: u64, sky: Option<&str>) -> Server {
    Server::new(
        ServerConfig {
            world_seed: seed,
            sky: sky.map(Into::into),
            ..flat()
        },
        Box::new(SeededEntropy(1)),
    )
}

#[test]
fn a_world_without_a_sky_preset_keeps_todays_sky_and_writes_no_orbit() {
    let s = server(7, None).world_settings();
    assert_eq!(s.orbit, None);
    assert_eq!(s.moon_phase, None);
    assert_eq!(Orbit::for_world(&s), Orbit::today());
    assert_eq!(
        dwell_look::orbits::day_night_for(&s),
        dwell_look::daynight::DAY_NIGHT
    );
}

#[test]
fn a_new_world_with_a_sky_preset_rolls_its_seeds_orbit_and_keeps_it() {
    for preset in ["default", "day_varies", "extreme"] {
        let s = server(1234, Some(preset)).world_settings();
        let want = Orbit::roll_preset(1234, preset).unwrap();
        let o = s.orbit.expect("the orbit is saved");
        assert_eq!(o.pole_deg, want.pole_deg as f32, "{preset}");
        assert_eq!(s.clock.day_s, want.day_s as f32);
        assert_eq!(s.clock.night_s, want.night_s as f32);
        assert_eq!(s.moon_phase, Some(want.moon_phase as f32));
        // Same seed, same sky; another seed, another.
        assert_eq!(server(1234, Some(preset)).world_settings(), s);
        assert_ne!(server(99, Some(preset)).world_settings().orbit, Some(o));
    }
    // A name not in orbits.json is the default preset.
    assert_eq!(
        server(5, Some("nope")).world_settings(),
        server(5, Some("default")).world_settings()
    );
}
