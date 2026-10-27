//! The water's reflections of land (docs/WATER_RENDERING.md §8; W5's screen-space reflections
//! brought forward): the reflected ray marched on screen through the scene's view depth, and what
//! the reflection shows: the scene where the ray hits it, faded at the screen's edges and the ray's
//! reach into the sky (with the clouds) where it doesn't. The WGSL twin is `ssr_trace` and
//! `reflected` in shaders.rs; the tunables are dwell-look's `REFLECTIONS` (data).

/// The camera as the shader has it (the frame's basis and half-angle tangents) and the screen in
/// pixels.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
    /// tan of the half angles (x, y).
    pub tan: [f32; 2],
    /// The screen (pixels).
    pub size: [f32; 2],
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl View {
    /// A point relative to the eye on screen (pixels, y down) and its view depth.
    pub fn to_screen(&self, rel: [f32; 3]) -> ([f32; 2], f32) {
        let d = dot(rel, self.forward);
        let nx = dot(rel, self.right) / (d * self.tan[0]);
        let ny = dot(rel, self.up) / (d * self.tan[1]);
        (
            [
                (nx * 0.5 + 0.5) * self.size[0],
                (0.5 - ny * 0.5) * self.size[1],
            ],
            d,
        )
    }

    /// The ray through a pixel, scaled to view depth 1 (the shader's `view_ray`).
    pub fn ray(&self, px: [f32; 2]) -> [f32; 3] {
        let nx = px[0] / self.size[0] * 2.0 - 1.0;
        let ny = 1.0 - px[1] / self.size[1] * 2.0;
        std::array::from_fn(|i| {
            self.forward[i] + nx * self.tan[0] * self.right[i] + ny * self.tan[1] * self.up[i]
        })
    }
}

/// How far over the reflecting surface (m, along the face's up) the scene at a side's exit pixel
/// must stand to stand in for what lies past the side.
pub const SIDE_LAND_M: f32 = 0.25;

/// One tier's march (dwell-look `reflection_uniforms`).
#[derive(Clone, Copy, Debug)]
pub struct Ssr {
    pub steps: u32,
    pub refine: u32,
    /// The ray's reach (m), and where hits start fading out along it.
    pub reach: f32,
    pub fade_from: f32,
    /// The edge fade (share of the screen height) and the hit thickness (m, plus `thickness_rel`
    /// of the surface's depth).
    pub edge_fade: f32,
    pub thickness: f32,
    pub thickness_rel: f32,
}

/// Where the reflected ray meets the scene on screen (pixels), and how much of the reflection it
/// gives (0–1: faded at the screen's edges and the end of the reach).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub px: [f32; 2],
    pub weight: f32,
}

/// The reflected ray `r` (unit) from the surface point `p` (relative to the eye), marched on
/// screen through `depth_at` (the scene's view depth at a pixel; 0 where nothing is drawn).
pub fn trace(
    view: &View,
    p: [f32; 3],
    r: [f32; 3],
    ssr: &Ssr,
    depth_at: impl Fn([f32; 2]) -> f32,
) -> Option<Hit> {
    let d0 = dot(p, view.forward);
    let toward = dot(r, view.forward);
    let mut len = ssr.reach;
    // A ray back toward the eye ends in front of it.
    if toward < 0.0 {
        len = len.min(0.9 * d0 / -toward);
    }
    if ssr.steps == 0 || len <= 0.0 {
        return None;
    }
    let (a, da) = view.to_screen(p);
    let (b, db) = view.to_screen([p[0] + r[0] * len, p[1] + r[1] * len, p[2] + r[2] * len]);
    let delta = [b[0] - a[0], b[1] - a[1]];
    let t_end = screen_clip(a, delta, view.size);
    // View depth runs hyperbolically along the screen: 1/depth is linear.
    let (k0, k1) = (1.0 / da, 1.0 / db);
    let depth_at_t = |t: f32| 1.0 / (k0 + (k1 - k0) * t);
    let px_at = |t: f32| [a[0] + delta[0] * t, a[1] + delta[1] * t];
    let (mut prev_t, mut prev_d) = (0.0, da);
    for i in 1..=ssr.steps {
        let t = t_end * i as f32 / ssr.steps as f32;
        let d = depth_at_t(t);
        if hits(ssr, depth_at(px_at(t)), d, prev_d) {
            let (mut lo, mut hi) = (prev_t, t);
            for _ in 0..ssr.refine {
                let mid = 0.5 * (lo + hi);
                let sm = depth_at(px_at(mid));
                if sm > 0.0 && depth_at_t(mid) >= sm {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let px = px_at(hi);
            // How far along the ray in 3D (view depth is linear along it), at the scene's depth
            // where it was hit: the march's own depth at the end of a step can lie far beyond it,
            // since a long reach squeezes kilometres into the last pixels (docs/BUGS.md 6).
            let dh = depth_at(px).clamp(da.min(db), da.max(db));
            let u = if (db - da).abs() > 1e-3 * da {
                (dh - da) / (db - da)
            } else {
                hi
            };
            let fade = 1.0 - smoothstep(ssr.fade_from, ssr.reach, u * len);
            return Some(Hit {
                px,
                weight: edge_weight(view, px, ssr.edge_fade) * fade,
            });
        }
        prev_t = t;
        prev_d = d;
    }
    // Out through a side with no hit: what lies past it is unknown. Where the scene at the exit
    // pixel stands over the surface (land: a mountain running off the screen), it stands in, so the
    // reflection goes on to the edge instead of cutting to the sky's white haze (docs/BUGS.md 9).
    // The floor seen through open water lies under the surface: a miss, else it drew streaks.
    let exit = px_at(t_end);
    let side = exit[0] <= 0.5 + 1e-3 || exit[0] >= view.size[0] - 0.5 - 1e-3;
    if t_end < 1.0 && side {
        let depth = depth_at(exit);
        let ray = view.ray(exit);
        if depth > 0.0 && ray[1] * depth > p[1] + SIDE_LAND_M {
            let fade = 1.0 - smoothstep(ssr.fade_from, ssr.reach, t_end * len);
            return Some(Hit {
                px: exit,
                weight: edge_weight(view, exit, ssr.edge_fade) * fade,
            });
        }
    }
    None
}

/// A march step at view depth `d` (from `prev`) meets the scene at depth `scene`: it is behind it,
/// and the surface lies within the step (else the ray passed behind something in front of it).
fn hits(ssr: &Ssr, scene: f32, d: f32, prev: f32) -> bool {
    scene > 0.0 && d >= scene && scene >= prev.min(d) - (ssr.thickness + ssr.thickness_rel * scene)
}

/// The share of the screen segment from `a` along `delta` that stays on screen.
fn screen_clip(a: [f32; 2], delta: [f32; 2], size: [f32; 2]) -> f32 {
    let mut t_end = 1.0f32;
    for k in 0..2 {
        if delta[k] > 1e-3 {
            t_end = t_end.min((size[k] - 0.5 - a[k]) / delta[k]);
        } else if delta[k] < -1e-3 {
            t_end = t_end.min((0.5 - a[k]) / delta[k]);
        }
    }
    t_end.max(0.0)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a hit at `px` survives the screen-edge fade: within `edge_fade` of the screen
/// height from the top or bottom edge, the reflection fades into the sky, so it doesn't pop there.
/// Not at the sides: fading a hit there into the sky drew a white haze down both sides
/// (docs/BUGS.md 6).
pub fn edge_weight(view: &View, px: [f32; 2], edge_fade: f32) -> f32 {
    let e = px[1].min(view.size[1] - px[1]);
    smoothstep(0.0, 1.0, e / (edge_fade * view.size[1]))
}

/// What the surface reflects along its ray: the scene at the hit (`scene_at`: linear colour and
/// coverage), weighted by the hit's fade and coverage, over `sky` (the sky with its clouds).
pub fn reflected(
    sky: [f32; 3],
    hit: Option<Hit>,
    scene_at: impl Fn([f32; 2]) -> [f32; 4],
) -> [f32; 3] {
    let Some(hit) = hit else {
        return sky;
    };
    let c = scene_at(hit.px);
    if c[3] <= 0.0 {
        return sky;
    }
    let w = hit.weight * c[3].min(1.0);
    [0, 1, 2].map(|k| sky[k] + (c[k] / c[3] - sky[k]) * w)
}

/// The constants the WGSL twin takes from the data (the per-tier values ride in the frame
/// uniform's `reflect` and `reflect_sky`).
pub fn wgsl() -> String {
    format!(
        "const SSR_THICKNESS_REL: f32 = {:?};\nconst SSR_RIPPLE: f32 = {:?};\n",
        dwell_look::reflections::REFLECTIONS.ssr.thickness_rel as f32,
        dwell_look::reflections::REFLECTIONS.ssr.ripple_scale as f32
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(v: [f32; 3]) -> [f32; 3] {
        let l = dot(v, v).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    }
    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    /// A 1280×720 view from the origin looking along +z, pitched `down` (slope) toward the water.
    fn view(down: f32) -> View {
        let forward = norm([0.0, -down, 1.0]);
        let right = norm(cross([0.0, 1.0, 0.0], forward));
        let up = cross(forward, right);
        let tan_y = (37.5f32).to_radians().tan();
        View {
            right,
            up,
            forward,
            tan: [tan_y * 16.0 / 9.0, tan_y],
            size: [1280.0, 720.0],
        }
    }

    /// The view ray through a pixel (view depth 1), as the shader's `view_ray`.
    fn ray(v: &View, px: [f32; 2]) -> [f32; 3] {
        let nx = px[0] / v.size[0] * 2.0 - 1.0;
        let ny = 1.0 - px[1] / v.size[1] * 2.0;
        let k = |i: usize| v.forward[i] + nx * v.tan[0] * v.right[i] + ny * v.tan[1] * v.up[i];
        [k(0), k(1), k(2)]
    }

    /// A lake: the sea floor at y = −6 seen through the water (water writes no depth), and a bank
    /// (a wall facing the eye) at z = `wall_z` rising to `wall_top`. The scene's view depth at a pixel.
    fn lake(v: View, wall_z: f32, wall_top: f32) -> impl Fn([f32; 2]) -> f32 {
        move |px| {
            let d = ray(&v, px);
            let mut best = f32::INFINITY;
            // The ray scaled to view depth 1: a hit at parameter s has view depth s.
            if d[2] > 0.0 {
                let s = wall_z / d[2];
                let y = d[1] * s;
                if (-6.0..=wall_top).contains(&y) {
                    best = best.min(s);
                }
            }
            if d[1] < 0.0 {
                let s = -6.0 / d[1];
                best = best.min(s);
            }
            if best.is_finite() { best } else { 0.0 }
        }
    }

    const MEDIUM: Ssr = Ssr {
        steps: 12,
        refine: 4,
        reach: 600.0,
        fade_from: 420.0,
        edge_fade: 0.12,
        thickness: 1.5,
        thickness_rel: 0.03,
    };

    /// The water point the eye sees at view depth ~`z` straight ahead, and its mirror ray.
    fn surface(_v: &View, z: f32) -> ([f32; 3], [f32; 3]) {
        let p = [0.0, -2.0, z];
        let d = norm(p);
        (p, [d[0], -d[1], d[2]])
    }

    const SKY: [f32; 3] = [0.2, 0.4, 0.9];
    const BANK: [f32; 4] = [0.6, 0.3, 0.1, 1.0];

    #[test]
    fn a_reflected_ray_over_terrain_shows_the_scene_not_the_sky() {
        let v = view(0.12);
        let depth = lake(v, 40.0, 10.0);
        let (p, r) = surface(&v, 20.0);
        let hit = trace(&v, p, r, &MEDIUM, &depth).expect("the bank is hit");
        // It lands on the bank: the wall at z = 40 where the mirror ray crosses it (y = 0).
        let at = ray(&v, hit.px);
        let s = 40.0 / at[2];
        assert!((at[1] * s).abs() < 0.3, "hit at y {}", at[1] * s);
        assert!(
            hit.weight > 0.9,
            "a hit mid-screen is whole: {}",
            hit.weight
        );
        let c = reflected(SKY, Some(hit), |_| BANK);
        for k in 0..3 {
            assert!(
                (c[k] - BANK[k]).abs() < 0.1,
                "the bank's colour, not the sky's: {c:?}"
            );
        }
    }

    #[test]
    fn a_ray_that_clears_the_bank_reflects_the_sky() {
        let v = view(0.12);
        // A low bank: the mirror ray crosses z = 40 at y = 0, a metre above it.
        let depth = lake(v, 40.0, -1.0);
        let (p, r) = surface(&v, 20.0);
        let hit = trace(&v, p, r, &MEDIUM, &depth);
        assert_eq!(hit, None);
        assert_eq!(reflected(SKY, hit, |_| BANK), SKY);
    }

    #[test]
    fn the_water_does_not_reflect_its_own_floor_and_rays_toward_the_eye_stay_in_front() {
        // No bank at all: only the floor seen through the water, which the ray stays above.
        let v = view(0.3);
        let depth = lake(v, 1e6, -100.0);
        for z in [5.0, 20.0, 80.0] {
            let (p, r) = surface(&v, z);
            assert_eq!(trace(&v, p, r, &MEDIUM, &depth), None, "z {z}");
        }
        // A ray back toward the eye ends in front of it (no division by a crossed depth).
        let (p, _) = surface(&v, 20.0);
        let back = norm([0.0, 0.3, -1.0]);
        assert_eq!(trace(&v, p, back, &MEDIUM, &depth), None);
    }

    #[test]
    fn hits_fade_toward_the_screen_edge_and_none_without_steps() {
        let v = view(0.12);
        let depth = lake(v, 40.0, 10.0);
        let (p, r) = surface(&v, 20.0);
        let mid = trace(&v, p, r, &MEDIUM, &depth).expect("hit");
        // The same hit taken as if the screen edge were just above it.
        let near_edge = edge_weight(&v, [mid.px[0], 4.0], MEDIUM.edge_fade);
        assert!(near_edge < 0.2 && edge_weight(&v, mid.px, MEDIUM.edge_fade) > 0.9);
        let none = Ssr { steps: 0, ..MEDIUM };
        assert_eq!(trace(&v, p, r, &none, &depth), None);
    }

    #[test]
    fn the_sides_keep_the_scene_not_a_fade_into_the_sky() {
        // docs/BUGS.md 6: a hit near a side faded into the sky, and a ray leaving through a side
        // with no hit took the sky: a white haze down both sides of the screen.
        let v = view(0.12);
        let depth = lake(v, 40.0, 10.0);
        let (p, r) = surface(&v, 20.0);
        let mid = trace(&v, p, r, &MEDIUM, &depth).expect("hit");
        assert!(
            edge_weight(&v, [4.0, mid.px[1]], MEDIUM.edge_fade) > 0.9,
            "a hit at the left edge keeps the scene"
        );
        assert!(
            edge_weight(&v, [v.size[0] - 4.0, mid.px[1]], MEDIUM.edge_fade) > 0.9,
            "and at the right"
        );
        // Open water to the side: the pixels there show the sea floor through the water (water
        // writes no depth). A ray leaving through the side is a miss (the sky), not that floor's
        // colour at its exit pixel: an edge clamp drew it as streaks fanning along the sides.
        let floor = lake(v, 1e6, -100.0);
        let (q, _) = surface(&v, 5.0);
        let left = norm([-1.0, 0.01, 0.05]);
        assert_eq!(trace(&v, q, left, &MEDIUM, &floor), None);
        // docs/BUGS.md 9: land past the left side (a mountain running off the screen) is met by
        // no step; the scene at the exit pixel stands over the surface, so it stands in there.
        let land = lake(v, 1e6, 3000.0);
        let mountain = |px: [f32; 2]| {
            let d = ray(&v, px);
            if px[0] < 2.0 && d[1] > -0.01 {
                900.0
            } else {
                land(px)
            }
        };
        let (q, _) = surface(&v, 20.0);
        let left = norm([-1.0, 0.05, 0.3]);
        let hit = trace(&v, q, left, &MEDIUM, mountain).expect("the edge's land stands in");
        assert!(hit.px[0] <= 1.0 && hit.weight > 0.5, "{hit:?}");
    }

    /// A tier's march from the data (as `reflection_uniforms` gives the shader).
    fn tier_ssr(level: usize) -> Ssr {
        let t = &dwell_look::reflections::REFLECTIONS;
        let [m, s] = dwell_look::reflections::reflection_uniforms(t, level, 8, true);
        Ssr {
            steps: m[0] as u32,
            refine: m[1] as u32,
            reach: m[2],
            fade_from: m[3],
            edge_fade: s[2],
            thickness: s[3],
            thickness_rel: t.ssr.thickness_rel as f32,
        }
    }

    #[test]
    fn far_mountains_are_reflected_and_near_banks_still_are() {
        // The owner (2026-10-26): tall mountains farther away didn't show in the water. A range
        // 12 km off, 1.5 km tall, seen over the water 200 m ahead: Medium and High reflect it.
        let v = view(0.12);
        for level in [1, 2] {
            let ssr = tier_ssr(level);
            let far = lake(v, 12_000.0, 1_500.0);
            let (p, r) = surface(&v, 200.0);
            let hit = trace(&v, p, r, &ssr, &far);
            assert!(
                hit.is_some_and(|h| h.weight > 0.5),
                "tier {level}: the range is reflected: {hit:?}"
            );
            // The longer reach still finds a bank close by.
            let near = lake(v, 40.0, 10.0);
            let (p, r) = surface(&v, 20.0);
            let hit = trace(&v, p, r, &ssr, &near).expect("the near bank");
            let at = ray(&v, hit.px);
            assert!((at[1] * 40.0 / at[2]).abs() < 0.5, "tier {level}: {hit:?}");
        }
    }

    #[test]
    fn the_shaders_side_margin_is_this_one() {
        let wgsl = crate::render::shaders::source(1);
        assert!(wgsl.contains(&format!("const SIDE_LAND_M: f32 = {SIDE_LAND_M:?};")));
    }
}
