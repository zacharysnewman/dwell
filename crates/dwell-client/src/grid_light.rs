//! A grid's light rig (`grids.json` `light`; BUGS.md #145; owner, 2026-11-13: "It needs some
//! directional lighting, let's make it from the earth and moon reflections" / "Sun should also be
//! glowing generally 'due to the lava'"): on a grid with a `light`, the sun's light is kept or not
//! (`sunlit`), each body it lists lights it from its true direction from the viewer with the
//! sunlight it reflects (its `albedo` × `intensity` × the share of its disc lit as the viewer sees
//! it × (its angular radius / `refDeg`)²), the hemisphere gets at least the rig's `ambient`, and a
//! `glow` from the ground (the sun's lava) lights surfaces from below and around. One rule for
//! every grid and body: nothing here names one. Home has no rig (its sky's keyframes, unchanged).

use dwell_look::daynight::{DayState, RigLight};
use dwell_world::GridFrame;
use dwell_worldgen::grids::LightRig;

type V3 = [f64; 3];

fn lin(c: V3) -> V3 {
    c.map(dwell_look::srgb_to_linear)
}

fn unit(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    v.map(|x| x / l)
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The share of a body's disc lit as seen from the viewer: toward it `toward` (unit, world), the
/// sun's direction `sun` (unit, world, the sky's: far enough to be the same from every body).
/// 1 full (the sun behind the viewer), 0 new.
pub fn lit_share(toward: V3, sun: V3) -> f64 {
    (1.0 - dot(toward, sun)).clamp(0.0, 2.0) / 2.0
}

/// The rig's lights for an eye at `eye` (world) on a grid drawn in `drawn`'s frame, the sun toward
/// `sun` (world): each listed body's light toward its centre and radius (`body`: world point, m;
/// `None` for one not known, left dark), turned into the drawn frame.
pub fn rig_lights(
    rig: &LightRig,
    eye: V3,
    drawn: &GridFrame,
    sun: V3,
    body: impl Fn(&str) -> Option<(V3, f64)>,
) -> [RigLight; 2] {
    let mut out = [RigLight::default(); 2];
    for (slot, b) in out.iter_mut().zip(&rig.bodies) {
        let Some((centre, radius)) = body(&b.grid) else {
            continue;
        };
        let to = [centre[0] - eye[0], centre[1] - eye[1], centre[2] - eye[2]];
        let d = dot(to, to).sqrt().max(1.0);
        let toward = unit(to);
        let size = (radius / d).atan().to_degrees() / b.ref_deg;
        let k = b.intensity * lit_share(toward, sun) * size * size;
        let c = lin(b.albedo);
        *slot = RigLight {
            direction: unit(drawn.vector_to_grid(toward)),
            light: c.map(|v| v * k),
        };
    }
    out
}

/// `day` (already turned onto the grid, `grid_view::day_on_grid`) lit by `rig`: the sun kept or
/// not, the lights, the ambient floor and the glow from the ground.
pub fn apply(day: &mut DayState, rig: &LightRig, lights: [RigLight; 2]) {
    day.sun_off = !rig.sunlit;
    day.rig = lights;
    let floor = lin(rig.ambient).map(|v| v * rig.ambient_min);
    day.hemi_floor = floor;
    // The glow lights from below (the hemisphere's ground) and around (some of it in its sky).
    day.hemi = rig.glow.map(|(c, k)| {
        let g = lin(c).map(|v| v * k);
        (
            std::array::from_fn(|i| floor[i] + GLOW_AROUND * g[i]),
            std::array::from_fn(|i| floor[i] + g[i]),
        )
    });
}

/// How much of a ground glow reaches surfaces facing up (the hemisphere's sky), of what reaches
/// those facing down: the glow is all round, brightest from below.
pub const GLOW_AROUND: f64 = 0.4;

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_look::daynight::DAY_NIGHT;
    use dwell_world::FaceFrame;
    use dwell_worldgen::grids::grid;

    /// The sun at `sun_deg` round the sky's equator, the bodies placed in a line: home at the
    /// origin (50 km), the moon 600 km along +x (12.5 km), the sun grid 950 km along +z.
    fn bodies(name: &str) -> Option<(V3, f64)> {
        match name {
            "dwell:home" => Some(([0.0; 3], 50_000.0)),
            "dwell:moon" => Some(([600_000.0, 0.0, 0.0], 12_500.0)),
            _ => None,
        }
    }

    /// The light on a surface facing `n` (face-local, through `f`) from the packed day: its rig
    /// lights' N·L and the hemisphere (the shader's `lambert_lit`, without the sun and moon).
    fn rig_irradiance(day: &DayState, f: FaceFrame, n: V3) -> V3 {
        let u = day.pack(f, 1.0, 0.0, 0.001);
        let v = |i: usize| [0, 1, 2].map(|k| f64::from(u[i * 4 + k]));
        let n = f.vector(n);
        let mut e = [0.0; 3];
        for (dir, light) in [(24, 25), (26, 27)] {
            let k = dot(n, v(dir)).max(0.0);
            for c in 0..3 {
                e[c] += k * v(light)[c];
            }
        }
        let (sky, ground) = if f.is_b() {
            (v(15), v(16))
        } else {
            (v(11), v(12))
        };
        let up = dot(n, f.vector([0.0, 1.0, 0.0])) * 0.5 + 0.5;
        for c in 0..3 {
            e[c] += ground[c] + (sky[c] - ground[c]) * up;
        }
        e
    }

    fn luma(c: V3) -> f64 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    dwell_world::both_faces!(
        /// On the sun (BUGS.md #145), standing on face `f` with home and the moon in its sky: the
        /// ground's lit side faces them (a surface facing home gets home's light, one facing away
        /// none of it), home the brighter; the sun lights nothing; and the strength follows each
        /// body's phase (from the sun every body is full; a viewer off the sun's line sees less).
        fn on_the_sun_the_ground_is_lit_from_home_and_the_moon(f: FaceFrame) {
            let def = grid("dwell:sun").unwrap();
            let rig = def.light.as_ref().expect("the sun's rig");
            assert!(!rig.sunlit);
            // The sun grid 950 km along +z, home and the moon as `bodies`; the sun toward +z.
            let drawn = GridFrame {
                origin: [0.0, 0.0, 950_000.0],
                ..GridFrame::HOME
            };
            let eye = drawn.to_world(f.point([0.0, 120.0, 0.0]));
            let sun = [0.0, 0.0, 1.0];
            let lights = rig_lights(rig, eye, &drawn, sun, bodies);
            let mut day = crate::grid_view::day_on_grid(
                &DAY_NIGHT.state(0.0, Some(0.5), None),
                &drawn,
                dwell_worldgen::grids::grids()
                    .iter()
                    .position(|g| g.name == "dwell:sun")
                    .unwrap() as u8,
            );
            assert!(day.on_sun, "on the sun: its disc not drawn");
            assert_eq!(day.pack(f, 1.0, 0.0, 0.001)[22 * 4 + 2], 1.0);
            apply(&mut day, rig, lights);
            let to_home = unit([-eye[0], -eye[1], -eye[2]]);
            assert!(
                dot(lights[0].direction, to_home) > 0.999,
                "home's light from home"
            );
            let to_moon = unit([600_000.0 - eye[0], -eye[1], -eye[2]]);
            assert!(
                dot(lights[1].direction, to_moon) > 0.999,
                "the moon's from the moon"
            );
            assert!(
                luma(lights[0].light) > luma(lights[1].light),
                "home the brighter"
            );
            let home_side = rig_irradiance(&day, f, f.vector(to_home));
            let away = rig_irradiance(&day, f, f.vector(to_home.map(|v| -v)));
            assert!(
                luma(home_side) > 2.0 * luma(away),
                "the lit side faces home: {home_side:?} vs {away:?}"
            );
            // The sun lights nothing on the sun: both faces' sun light packed zero.
            let u = day.pack(f, 1.0, 0.0, 0.001);
            assert_eq!(&u[9 * 4..9 * 4 + 3], &[0.0; 3]);
            assert_eq!(&u[13 * 4..13 * 4 + 3], &[0.0; 3]);
            // The phase: home seen from a viewer with the sun behind home is new (dark), with the
            // sun behind the viewer full, by the lit share.
            assert!(
                (lit_share(to_home, [0.0, 0.0, 1.0]) - 1.0).abs() < 0.01,
                "full"
            );
            assert!(lit_share(to_home, [0.0, 0.0, -1.0]) < 0.01, "new");
            let quarter = rig_lights(rig, eye, &drawn, [1.0, 0.0, 0.0], bodies);
            let ratio = luma(quarter[0].light) / luma(lights[0].light);
            assert!(
                (ratio - 0.5).abs() < 0.02,
                "a half-lit home gives half: {ratio}"
            );
        }
    );

    dwell_world::both_faces!(
        /// The sun's lava glow (owner: "Sun should also be glowing generally 'due to the lava'"):
        /// on the sun, ground facing away from every body still reads warm, its light over the
        /// rig's floor and red over blue; brightest facing down (the lava below). On the moon and
        /// home, no such glow.
        fn the_sun_glows_warm_from_below_and_the_moon_and_home_do_not(f: FaceFrame) {
            let sun = grid("dwell:sun").unwrap().light.clone().unwrap();
            let mut day = DAY_NIGHT.state(0.0, Some(0.0), None);
            apply(&mut day, &sun, [RigLight::default(); 2]);
            let up = rig_irradiance(&day, f, [0.0, 1.0, 0.0]);
            let down = rig_irradiance(&day, f, [0.0, -1.0, 0.0]);
            let floor = lin(sun.ambient).map(|v| v * sun.ambient_min);
            assert!(luma(up) > luma(floor), "over the floor: {up:?}");
            assert!(up[0] > 2.0 * up[2] && up[0] > up[1], "red-shifted: {up:?}");
            assert!(luma(down) > luma(up), "brightest from below");
            let moon = grid("dwell:moon").unwrap().light.clone().unwrap();
            assert!(moon.glow.is_none() && moon.sunlit);
            let mut on_moon = DAY_NIGHT.state(0.0, Some(0.0), None);
            apply(&mut on_moon, &moon, [RigLight::default(); 2]);
            assert!(on_moon.hemi.is_none(), "no glow on the moon");
            let m = rig_irradiance(&on_moon, f, [0.0, 1.0, 0.0]);
            assert!(
                m[0] < 1.5 * m[2].max(1e-6),
                "not red-shifted on the moon: {m:?}"
            );
            assert!(
                grid("dwell:home").unwrap().light.is_none(),
                "home has no rig"
            );
        }
    );

    dwell_world::both_faces!(
        /// Home's lighting is unchanged (BUGS.md #145): its day packs every old slot as before,
        /// the rig's lights zero, at every hour.
        fn homes_lighting_is_unchanged(f: FaceFrame) {
            for k in 0..24 {
                let day = DAY_NIGHT.state(0.0, Some(f64::from(k) / 24.0), None);
                let home = crate::grid_view::day_on_grid(&day, &GridFrame::HOME, 0);
                assert_eq!(home, day);
                let u = home.pack(f, 1.0, 0.0, 0.001);
                assert!(u[24 * 4..].iter().all(|&v| v == 0.0), "no rig lights");
                assert_eq!(u[22 * 4 + 2], 0.0, "not on the sun");
            }
        }
    );

    dwell_world::both_faces!(
        /// On the moon the sun stays the main light (`sunlit`), home's reflection a faint fill
        /// from home's direction, following home's phase as the moon sees it.
        fn on_the_moon_home_is_a_faint_fill(f: FaceFrame) {
            let rig = grid("dwell:moon").unwrap().light.clone().unwrap();
            let drawn = GridFrame {
                origin: [600_000.0, 0.0, 0.0],
                ..GridFrame::HOME
            };
            let eye = drawn.to_world(f.point([0.0, 80.0, 0.0]));
            let full = rig_lights(&rig, eye, &drawn, [1.0, 0.0, 0.0], bodies);
            let half = rig_lights(&rig, eye, &drawn, [0.0, 1.0, 0.0], bodies);
            let new = rig_lights(&rig, eye, &drawn, [-1.0, 0.0, 0.0], bodies);
            assert!(luma(full[0].light) > 0.0 && luma(new[0].light) < 1e-4 * luma(full[0].light));
            assert!(((luma(half[0].light) / luma(full[0].light)) - 0.5).abs() < 0.02);
            let day = DAY_NIGHT.state(0.0, Some(0.5), None);
            let sun_light = day.pack(f, 1.0, 0.0, 0.001);
            let sun = (0..3)
                .map(|k| f64::from(sun_light[9 * 4 + k]))
                .fold(0.0, f64::max);
            assert!(
                luma(full[0].light) < 0.1 * sun,
                "a faint fill beside the sun: {:?} vs {sun}",
                full[0].light
            );
        }
    );
}
